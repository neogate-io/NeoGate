use std::{sync::Arc, time::Instant};

use axum::{body::Body, http::StatusCode};
use bytes::Bytes;
use futures_util::StreamExt;

use crate::{
    auth::UserAuth,
    billing::{
        parse_usage_from_bytes, parse_usage_from_sse_data, BillingAccounts, CreditAccountId,
        DebitHold, Price, SettleRequest, TokenUsage,
    },
    AppState,
};

use super::{
    enqueue_relay_usage, key_failure_from_context, release_empty_hold,
    selector::{SelectedUpstream, UpstreamProtocol},
    usage_from_context,
};

const MAX_JSON_USAGE_BUFFER_BYTES: usize = 2 * 1024 * 1024;
const MAX_SSE_USAGE_BUFFER_BYTES: usize = 256 * 1024;

pub(super) struct RelayContext {
    pub(super) state: Arc<AppState>,
    pub(super) auth: UserAuth,
    pub(super) upstream: SelectedUpstream,
    pub(super) protocol: UpstreamProtocol,
    pub(super) path: &'static str,
    pub(super) model: String,
    pub(super) streamed: bool,
    pub(super) price: Price,
    pub(super) hold: DebitHold,
    pub(super) user_key_model_credit_account: Option<CreditAccountId>,
    pub(super) started: Instant,
}

pub(super) fn body(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> Body {
    let streamed = ctx.streamed;
    let relay = StreamingRelay {
        ctx: Some(ctx),
        status,
        stream: upstream_response.bytes_stream().boxed(),
        usage: ResponseUsageParser::for_response(status, streamed),
        first_response_ms: None,
    };

    Body::from_stream(futures_util::stream::unfold(
        Some(relay),
        |relay| async move {
            let mut relay = relay?;
            match relay.stream.next().await {
                Some(Ok(chunk)) => {
                    if relay.first_response_ms.is_none() && !chunk.is_empty() {
                        if let Some(ctx) = relay.ctx.as_ref().filter(|ctx| ctx.streamed) {
                            relay.first_response_ms =
                                Some(ctx.started.elapsed().as_millis() as i64);
                        }
                    }
                    relay.usage.observe(&chunk);
                    Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay)))
                }
                Some(Err(err)) => {
                    let summary = err.to_string();
                    relay.finish_stream_error(summary.clone()).await;
                    Some((Err(std::io::Error::other(summary)), None))
                }
                None => {
                    relay.finish_stream_success().await;
                    None
                }
            }
        },
    ))
}

struct StreamingRelay {
    ctx: Option<RelayContext>,
    status: StatusCode,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
    usage: ResponseUsageParser,
    first_response_ms: Option<i64>,
}

impl StreamingRelay {
    async fn finish_stream_success(mut self) {
        let ctx = self.ctx.take().expect("stream context finalized once");
        let token_usage = self.usage.finish();
        let billing = if self.status.is_success() {
            match ctx
                .state
                .billing
                .settle(
                    &ctx.state.db.pool,
                    SettleRequest {
                        accounts: BillingAccounts {
                            user_id: ctx.auth.user_id,
                            user_key_id: ctx.auth.user_key_id,
                            user_key_model_credit_account: ctx
                                .user_key_model_credit_account
                                .as_ref(),
                            user_key_credit_account: &ctx.auth.user_key_credit_account,
                            user_credit_account: &ctx.auth.user_credit_account,
                        },
                        hold: ctx.hold.clone(),
                        usage: token_usage,
                        price: &ctx.price,
                    },
                )
                .await
            {
                Ok(billing) => Some(billing),
                Err(err) => {
                    tracing::warn!("failed to settle streamed relay hold: {err}");
                    None
                }
            }
        } else {
            release_empty_hold(&ctx.state, ctx.hold.clone(), "upstream error").await;
            None
        };
        let error_summary = (!self.status.is_success()).then(|| "upstream error".to_string());
        let failure = if self.status.is_success() {
            None
        } else {
            key_failure_from_context(&ctx, "upstream error".to_string()).await
        };
        let usage = usage_from_context(
            &ctx,
            Some(self.status.as_u16() as i32),
            error_summary,
            self.first_response_ms,
            token_usage,
            billing,
        );
        enqueue_relay_usage(&ctx.state, usage, failure).await;
    }

    async fn finish_stream_error(mut self, summary: String) {
        let Some(ctx) = self.ctx.take() else {
            return;
        };
        tracing::warn!(
            provider = %ctx.upstream.provider,
            channel_id = ctx.upstream.channel_id,
            channel_name = %ctx.upstream.channel_name,
            channel_endpoint_id = ctx.upstream.channel_endpoint_id,
            channel_key_id = ?ctx.upstream.channel_key_id,
            credential_id = ?ctx.upstream.credential_id,
            protocol = ctx.protocol.as_str(),
            model = %ctx.model,
            path = ctx.path,
            base_url = %ctx.upstream.base_url,
            status = self.status.as_u16(),
            streamed = ctx.streamed,
            first_response_ms = self.first_response_ms,
            latency_ms = ctx.started.elapsed().as_millis() as i64,
            error = %summary,
            "upstream stream failed while relaying response body"
        );
        release_empty_hold(&ctx.state, ctx.hold.clone(), "stream error").await;
        let failure = if should_cooldown_key_for_stream_error(self.status) {
            key_failure_from_context(&ctx, summary.clone()).await
        } else {
            None
        };
        let usage = usage_from_context(
            &ctx,
            Some(self.status.as_u16() as i32),
            Some(summary),
            self.first_response_ms,
            None,
            None,
        );
        enqueue_relay_usage(&ctx.state, usage, failure).await;
    }
}

fn should_cooldown_key_for_stream_error(status: StatusCode) -> bool {
    !status.is_success()
}

impl Drop for StreamingRelay {
    fn drop(&mut self) {
        let Some(ctx) = self.ctx.take() else {
            return;
        };
        let status = self.status;

        tokio::spawn(async move {
            tracing::warn!(
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                path = ctx.path,
                base_url = %ctx.upstream.base_url,
                status = status.as_u16(),
                streamed = ctx.streamed,
                latency_ms = ctx.started.elapsed().as_millis() as i64,
                "downstream client closed relay stream before completion"
            );
            release_empty_hold(&ctx.state, ctx.hold.clone(), "dropped stream").await;
            let failure = if status.is_success() {
                None
            } else {
                key_failure_from_context(&ctx, "upstream error".to_string()).await
            };
            let usage = usage_from_context(
                &ctx,
                Some(status.as_u16() as i32),
                Some("downstream stream closed before completion".to_string()),
                None,
                None,
                None,
            );
            enqueue_relay_usage(&ctx.state, usage, failure).await;
        });
    }
}

enum ResponseUsageParser {
    Sse(StreamUsageParser),
    Json(Option<Vec<u8>>),
    Disabled,
}

impl ResponseUsageParser {
    fn for_response(status: StatusCode, streamed: bool) -> Self {
        if !status.is_success() {
            Self::Disabled
        } else if streamed {
            Self::Sse(StreamUsageParser::default())
        } else {
            Self::Json(Some(Vec::new()))
        }
    }

    fn observe(&mut self, chunk: &[u8]) {
        match self {
            Self::Sse(parser) => parser.observe(chunk),
            Self::Json(buffer) => {
                if let Some(bytes) = buffer {
                    if bytes.len().saturating_add(chunk.len()) <= MAX_JSON_USAGE_BUFFER_BYTES {
                        bytes.extend_from_slice(chunk);
                    } else {
                        tracing::warn!(
                            "non-streamed relay response exceeded usage parse buffer; skipping usage parse"
                        );
                        *buffer = None;
                    }
                }
            }
            Self::Disabled => {}
        }
    }

    fn finish(&mut self) -> Option<TokenUsage> {
        match self {
            Self::Sse(parser) => parser.finish(),
            Self::Json(buffer) => buffer
                .as_deref()
                .and_then(|bytes| parse_usage_from_bytes(bytes, false)),
            Self::Disabled => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn successful_stream_body_errors_do_not_cool_down_key() {
        assert!(!should_cooldown_key_for_stream_error(StatusCode::OK));
    }

    #[test]
    fn upstream_error_stream_body_errors_cool_down_key() {
        assert!(should_cooldown_key_for_stream_error(
            StatusCode::TOO_MANY_REQUESTS
        ));
    }

    #[test]
    fn stream_usage_parser_caps_unterminated_buffer() {
        let mut parser = StreamUsageParser::default();

        parser.observe(&vec![b'a'; MAX_SSE_USAGE_BUFFER_BYTES + 1]);

        assert!(parser.disabled);
        assert!(parser.buffered.is_empty());
        assert!(parser.finish().is_none());
    }

    #[test]
    fn stream_usage_parser_keeps_latest_before_cap() {
        let mut parser = StreamUsageParser::default();
        parser.observe(
            br#"data: {"usage":{"input_tokens":10,"output_tokens":3}}
"#,
        );

        parser.observe(&vec![b'a'; MAX_SSE_USAGE_BUFFER_BYTES + 1]);

        let usage = parser.finish().expect("latest usage should be retained");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
    }
}

#[derive(Default)]
struct StreamUsageParser {
    buffered: Vec<u8>,
    latest: Option<TokenUsage>,
    disabled: bool,
}

impl StreamUsageParser {
    fn observe(&mut self, chunk: &[u8]) {
        if self.disabled {
            return;
        }
        if self.buffered.len().saturating_add(chunk.len()) > MAX_SSE_USAGE_BUFFER_BYTES {
            tracing::warn!(
                "streamed relay response exceeded usage parse buffer; skipping usage parse"
            );
            self.buffered.clear();
            self.disabled = true;
            return;
        }

        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let line_end = consumed + offset;
            let mut line = &self.buffered[consumed..line_end];
            if matches!(line.last(), Some(b'\r')) {
                line = &line[..line.len() - 1];
            }
            if let Some(usage) = Self::usage_from_line(line) {
                self.latest = Some(usage);
            }
            consumed = line_end + 1;
        }
        if consumed > 0 {
            self.buffered.drain(..consumed);
        }
    }

    fn usage_from_line(line: &[u8]) -> Option<TokenUsage> {
        let line = std::str::from_utf8(line).ok()?;
        let data = line.strip_prefix("data:")?;
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            return None;
        }
        parse_usage_from_sse_data(data)
    }

    fn finish(&mut self) -> Option<TokenUsage> {
        if self.disabled {
            return self.latest;
        }
        if !self.buffered.is_empty() {
            let line = std::mem::take(&mut self.buffered);
            if let Some(usage) = Self::usage_from_line(&line) {
                self.latest = Some(usage);
            }
        }
        self.latest
    }
}
