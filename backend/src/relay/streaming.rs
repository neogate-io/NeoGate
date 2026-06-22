use std::{sync::Arc, time::Instant};

use axum::{body::Body, http::StatusCode};
use bytes::Bytes;
use futures_util::StreamExt;

use crate::{
    auth::UserAuth,
    billing::{
        parse_usage_from_bytes, parse_usage_from_sse_data, BillableUsage, BillingAccounts,
        BillingCharge, CreditAccountId, DebitHold, Price, SettleRequest, TokenUsage,
    },
    AppState,
};
use uuid::Uuid;

use super::{
    enqueue_relay_usage, key_failure_from_context,
    limit::ImageSyncPermit,
    release_empty_hold,
    selector::{SelectedUpstream, UpstreamProtocol},
    usage_from_context, ChannelAffinityKey,
};

pub(crate) struct RelayContext {
    pub(crate) state: Arc<AppState>,
    pub(crate) auth: UserAuth,
    pub(crate) upstream: SelectedUpstream,
    pub(crate) protocol: UpstreamProtocol,
    pub(crate) path: &'static str,
    pub(crate) model: String,
    pub(crate) streamed: bool,
    pub(crate) price: Price,
    pub(crate) hold: DebitHold,
    pub(crate) user_key_model_credit_account: Option<CreditAccountId>,
    pub(crate) started: Instant,
    pub(crate) channel_affinity_key: Option<ChannelAffinityKey>,
    pub(crate) relay_trace_id: Uuid,
    pub(crate) relay_attempt: i32,
    pub(crate) relay_final: bool,
    pub(crate) _image_sync_permit: Option<ImageSyncPermit>,
}

pub(crate) fn body(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> Body {
    let content_length = upstream_response.content_length();
    let usage_buffer_limit_bytes = ctx.state.config.relay.usage_buffer_limit_bytes;
    body_from_stream(
        ctx,
        status,
        content_length,
        usage_buffer_limit_bytes,
        upstream_response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|err| err.to_string()))
            .boxed(),
    )
}

pub(crate) fn body_from_bytes(ctx: RelayContext, status: StatusCode, bytes: Bytes) -> Body {
    let usage_buffer_limit_bytes = ctx.state.config.relay.usage_buffer_limit_bytes;
    body_from_stream(
        ctx,
        status,
        Some(bytes.len() as u64),
        usage_buffer_limit_bytes,
        futures_util::stream::once(async move { Ok(bytes) }).boxed(),
    )
}

pub(crate) fn body_from_stream(
    ctx: RelayContext,
    status: StatusCode,
    content_length: Option<u64>,
    usage_buffer_limit_bytes: usize,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, String>>,
) -> Body {
    let streamed = ctx.streamed;
    let path = ctx.path;
    let relay = StreamingRelay {
        ctx: Some(ctx),
        status,
        stream,
        usage: ResponseUsageParser::for_response(
            status,
            streamed,
            path,
            content_length,
            usage_buffer_limit_bytes,
        ),
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
                Some(Err(summary)) => {
                    if relay.should_ignore_successful_stream_error(&summary) {
                        relay.finish_trailing_stream_error(summary).await;
                        None
                    } else {
                        relay.finish_stream_error(summary.clone()).await;
                        Some((Err(std::io::Error::other(summary)), None))
                    }
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
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, String>>,
    usage: ResponseUsageParser,
    first_response_ms: Option<i64>,
}

impl StreamingRelay {
    fn should_ignore_successful_stream_error(&self, summary: &str) -> bool {
        self.status.is_success() && self.usage.stream_complete() && is_body_decode_error(summary)
    }

    async fn finish_stream_success(mut self) {
        let ctx = self.ctx.take().expect("stream context finalized once");
        let token_usage = self.usage.finish();
        let billing = if self.status.is_success() {
            record_channel_affinity(&ctx);
            settle_successful_hold(&ctx, token_usage, "streamed relay").await
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

    async fn finish_trailing_stream_error(self, summary: String) {
        if let Some(ctx) = self.ctx.as_ref() {
            tracing::debug!(
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
                "ignored trailing upstream stream read error after completed response"
            );
        }
        self.finish_stream_success().await;
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
        let token_usage = self.usage.finish();
        let billing = if self.status.is_success() {
            settle_successful_hold(&ctx, token_usage, "successful stream error").await
        } else {
            release_empty_hold(&ctx.state, ctx.hold.clone(), "stream error").await;
            None
        };
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
            token_usage,
            billing,
        );
        enqueue_relay_usage(&ctx.state, usage, failure).await;
    }
}

async fn settle_successful_hold(
    ctx: &RelayContext,
    token_usage: Option<TokenUsage>,
    context: &str,
) -> Option<BillingCharge> {
    match ctx
        .state
        .billing
        .settle(
            &ctx.state.db.pool,
            SettleRequest {
                accounts: BillingAccounts {
                    user_id: ctx.auth.user_id,
                    project_id: ctx.auth.project_id,
                    user_key_id: ctx.auth.user_key_id,
                    user_key_model_credit_account: ctx.user_key_model_credit_account.as_ref(),
                    user_key_credit_account: &ctx.auth.user_key_credit_account,
                    project_credit_account: &ctx.auth.project_credit_account,
                },
                hold: ctx.hold.clone(),
                usage: token_usage.map(BillableUsage::token),
                price: &ctx.price,
            },
        )
        .await
    {
        Ok(billing) => Some(billing),
        Err(err) => {
            tracing::warn!("failed to settle {context} hold: {err}");
            None
        }
    }
}

fn should_cooldown_key_for_stream_error(status: StatusCode) -> bool {
    !status.is_success()
}

fn is_body_decode_error(summary: &str) -> bool {
    summary.contains("error decoding response body")
}

impl Drop for StreamingRelay {
    fn drop(&mut self) {
        let Some(ctx) = self.ctx.take() else {
            return;
        };
        let status = self.status;
        let token_usage = self.usage.finish();
        let first_response_ms = self.first_response_ms;

        tokio::spawn(async move {
            tracing::info!(
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
            let billing = if status.is_success() {
                settle_successful_hold(&ctx, token_usage, "dropped successful stream").await
            } else {
                release_empty_hold(&ctx.state, ctx.hold.clone(), "dropped stream").await;
                None
            };
            let failure = if status.is_success() {
                None
            } else {
                key_failure_from_context(&ctx, "upstream error".to_string()).await
            };
            let usage = usage_from_context(
                &ctx,
                Some(status.as_u16() as i32),
                Some("downstream stream closed before completion".to_string()),
                first_response_ms,
                token_usage,
                billing,
            );
            enqueue_relay_usage(&ctx.state, usage, failure).await;
        });
    }
}

fn record_channel_affinity(ctx: &RelayContext) {
    let Some(key) = ctx.channel_affinity_key.clone() else {
        return;
    };
    ctx.state
        .channel_affinity
        .insert(key, (&ctx.upstream).into());
}

enum ResponseUsageParser {
    Sse(StreamUsageParser),
    Json {
        buffer: Option<Vec<u8>>,
        limit_bytes: usize,
    },
    Disabled,
}

impl ResponseUsageParser {
    fn for_response(
        status: StatusCode,
        streamed: bool,
        path: &str,
        content_length: Option<u64>,
        limit_bytes: usize,
    ) -> Self {
        if !status.is_success() || path.starts_with("/v1/images/") {
            Self::Disabled
        } else if streamed {
            Self::Sse(StreamUsageParser::new(limit_bytes))
        } else {
            Self::Json {
                buffer: Some(Vec::with_capacity(json_usage_buffer_capacity(
                    content_length,
                    limit_bytes,
                ))),
                limit_bytes,
            }
        }
    }

    fn observe(&mut self, chunk: &[u8]) {
        match self {
            Self::Sse(parser) => parser.observe(chunk),
            Self::Json {
                buffer,
                limit_bytes,
            } => {
                if let Some(bytes) = buffer {
                    if bytes.len().saturating_add(chunk.len()) <= *limit_bytes {
                        bytes.extend_from_slice(chunk);
                    } else {
                        tracing::warn!(
                            limit_bytes,
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
            Self::Json { buffer, .. } => buffer
                .as_deref()
                .and_then(|bytes| parse_usage_from_bytes(bytes, false)),
            Self::Disabled => None,
        }
    }

    fn stream_complete(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.completed)
    }
}

fn json_usage_buffer_capacity(content_length: Option<u64>, limit_bytes: usize) -> usize {
    content_length
        .and_then(|length| usize::try_from(length).ok())
        .map(|length| length.min(limit_bytes))
        .unwrap_or(0)
}

struct StreamUsageParser {
    buffered: Vec<u8>,
    latest: Option<TokenUsage>,
    completed: bool,
    skipping_oversized_line: bool,
    limit_bytes: usize,
}

impl StreamUsageParser {
    fn new(limit_bytes: usize) -> Self {
        Self {
            buffered: Vec::new(),
            latest: None,
            completed: false,
            skipping_oversized_line: false,
            limit_bytes,
        }
    }

    fn observe(&mut self, chunk: &[u8]) {
        if self.skipping_oversized_line {
            if let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') {
                self.skipping_oversized_line = false;
                self.observe(&chunk[offset + 1..]);
            }
            return;
        }
        if self.buffered.len().saturating_add(chunk.len()) > self.limit_bytes {
            tracing::debug!(
                limit_bytes = self.limit_bytes,
                "streamed relay response line exceeded usage parse buffer; skipping oversized line"
            );
            self.buffered.clear();
            if let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') {
                self.observe(&chunk[offset + 1..]);
            } else {
                self.skipping_oversized_line = true;
            }
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
            let (usage, completed) = Self::parse_line(line);
            self.observe_parsed_line(usage, completed);
            consumed = line_end + 1;
        }
        if consumed == self.buffered.len() {
            self.buffered.clear();
        } else if consumed > 0 {
            self.buffered.drain(..consumed);
        }
    }

    fn observe_line(&mut self, line: &[u8]) {
        let (usage, completed) = Self::parse_line(line);
        self.observe_parsed_line(usage, completed);
    }

    fn observe_parsed_line(&mut self, usage: Option<TokenUsage>, completed: bool) {
        if let Some(usage) = usage {
            self.latest = Some(usage);
        }
        if completed {
            self.completed = true;
        }
    }

    fn parse_line(line: &[u8]) -> (Option<TokenUsage>, bool) {
        let Ok(line) = std::str::from_utf8(line) else {
            return (None, false);
        };
        if line.strip_prefix("event:").map(str::trim) == Some("message_stop") {
            return (None, true);
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return (None, false);
        };
        if data.is_empty() {
            return (None, false);
        }
        if data == "[DONE]" {
            return (None, true);
        }
        (
            parse_usage_from_sse_data(data),
            data.contains("message_stop") && sse_data_type_is(data, "message_stop"),
        )
    }

    fn finish(&mut self) -> Option<TokenUsage> {
        if self.skipping_oversized_line {
            return self.latest;
        }
        if !self.buffered.is_empty() {
            let line = std::mem::take(&mut self.buffered);
            self.observe_line(&line);
        }
        self.latest
    }
}

fn sse_data_type_is(data: &str, expected: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .and_then(|value| {
            value
                .get("type")
                .and_then(|type_| type_.as_str())
                .map(|type_| type_ == expected)
        })
        .unwrap_or(false)
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
    fn body_decode_errors_are_identified() {
        assert!(is_body_decode_error("error decoding response body"));
    }

    #[test]
    fn stream_usage_parser_caps_unterminated_buffer() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(&vec![b'a'; 1025]);

        assert!(parser.skipping_oversized_line);
        assert!(parser.buffered.is_empty());
        assert!(parser.finish().is_none());
    }

    #[test]
    fn stream_usage_parser_keeps_latest_before_cap() {
        let mut parser = StreamUsageParser::new(1024);
        parser.observe(
            br#"data: {"usage":{"input_tokens":10,"output_tokens":3}}
"#,
        );

        parser.observe(&vec![b'a'; 1025]);

        let usage = parser.finish().expect("latest usage should be retained");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
    }

    #[test]
    fn stream_usage_parser_recovers_after_oversized_line() {
        let mut parser = StreamUsageParser::new(128);

        parser.observe(b"data: ");
        parser.observe(&vec![b'a'; 256]);
        parser.observe(
            br#"
data: {"usage":{"input_tokens":10,"output_tokens":3}}
"#,
        );

        let usage = parser
            .finish()
            .expect("usage after oversized line should be parsed");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
    }

    #[test]
    fn stream_usage_parser_detects_openai_done() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(b"data: [DONE]\n");

        assert!(parser.completed);
    }

    #[test]
    fn stream_usage_parser_detects_anthropic_message_stop() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: message_stop
data: {"type":"message_stop"}
"#,
        );

        assert!(parser.completed);
    }
}
