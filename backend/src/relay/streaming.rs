use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    body::Body,
    http::{HeaderMap, StatusCode},
};
use bytes::Bytes;
use futures_util::StreamExt;

use crate::{
    auth::UserAuth,
    billing::{
        parse_usage_from_bytes, parse_usage_from_sse_data, BillableUsage, BillingAccounts,
        BillingCharge, CreditAccountId, DebitHold, Price, SettleRequest, TokenUsage,
    },
    config::{STREAM_IDLE_TIMEOUT, STREAM_KEEP_ALIVE_INTERVAL},
    project::models::UsageRoutingSnapshot,
    AppState,
};
use serde_json::Value;
use uuid::Uuid;

use super::{
    enqueue_relay_usage, is_model_error_text, key_failure_from_context,
    limit::UserRequestPermit,
    release_empty_hold,
    selector::{SelectedUpstream, UpstreamProtocol},
    usage_from_context, ChannelAffinityKey, RelayRequestParams,
};

const MISSING_TERMINAL_STREAM_ERROR: &str = "upstream stream ended before terminal SSE event";
const DOWNSTREAM_STREAM_ERROR_MESSAGE: &str =
    "The upstream response stream ended unexpectedly. Please retry the request.";
const DOWNSTREAM_STREAM_KEEP_ALIVE: Bytes = Bytes::from_static(b": PING\n\n");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamFinishDisposition {
    Success,
    HttpError,
    SseError,
    MissingTerminal,
}

enum StreamPoll {
    Upstream(Option<Result<Bytes, String>>),
    KeepAlive,
    IdleTimeout,
}

pub(crate) struct RelayContext {
    pub(crate) state: Arc<AppState>,
    pub(crate) auth: UserAuth,
    pub(crate) upstream: SelectedUpstream,
    pub(crate) protocol: UpstreamProtocol,
    pub(crate) path: &'static str,
    pub(crate) model: String,
    pub(crate) external_model: String,
    pub(crate) upstream_model: String,
    pub(crate) routing: Option<UsageRoutingSnapshot>,
    pub(crate) streamed: bool,
    pub(crate) price: Price,
    pub(crate) hold: DebitHold,
    pub(crate) user_key_model_credit_account: Option<CreditAccountId>,
    pub(crate) started: Instant,
    pub(crate) channel_affinity_key: Option<ChannelAffinityKey>,
    pub(crate) relay_trace_id: Uuid,
    pub(crate) relay_attempt: i32,
    pub(crate) relay_final: bool,
    /// Payload metrics are estimates for diagnostics only; they do not expose request content.
    pub(crate) request_body_bytes: usize,
    pub(crate) request_input_tokens_estimate: i64,
    pub(crate) request_params: RelayRequestParams,
    pub(crate) request_permit: Option<UserRequestPermit>,
    pub(crate) upstream_request_path: Option<String>,
    pub(crate) upstream_response_mode: Option<&'static str>,
}

impl RelayContext {
    pub(crate) fn mark_final_with_permit(
        &mut self,
        request_permit: &mut Option<UserRequestPermit>,
    ) {
        self.relay_final = true;
        self.request_permit = request_permit.take();
    }

    pub(crate) fn release_request_permit(&mut self) {
        self.request_permit.take();
    }
}

#[derive(Default)]
pub(crate) struct UpstreamResponseMetadata {
    request_id: Option<String>,
    trace_id: Option<String>,
    cf_ray: Option<String>,
    server_timing: Option<String>,
}

impl UpstreamResponseMetadata {
    pub(crate) fn from_headers(headers: &HeaderMap) -> Self {
        Self {
            request_id: first_header_value(
                headers,
                &["x-request-id", "request-id", "openai-request-id"],
                256,
            ),
            trace_id: first_header_value(headers, &["x-trace-id", "trace-id"], 256),
            cf_ray: first_header_value(headers, &["cf-ray"], 256),
            server_timing: first_header_value(headers, &["server-timing"], 500),
        }
    }
}

fn first_header_value(headers: &HeaderMap, names: &[&str], limit: usize) -> Option<String> {
    names.iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_for_log(value, limit))
    })
}

pub(crate) fn body(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> Body {
    let content_length = upstream_response.content_length();
    let response_metadata = UpstreamResponseMetadata::from_headers(upstream_response.headers());
    let usage_buffer_limit_bytes = ctx.state.config.relay.usage_buffer_limit_bytes;
    body_from_stream(
        ctx,
        status,
        content_length,
        usage_buffer_limit_bytes,
        response_metadata,
        upstream_response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|err| err.to_string()))
            .boxed(),
    )
}

pub(crate) fn body_from_bytes(mut ctx: RelayContext, status: StatusCode, bytes: Bytes) -> Body {
    let usage_buffer_limit_bytes = ctx.state.config.relay.usage_buffer_limit_bytes;
    ctx.release_request_permit();
    body_from_stream(
        ctx,
        status,
        Some(bytes.len() as u64),
        usage_buffer_limit_bytes,
        UpstreamResponseMetadata::default(),
        futures_util::stream::once(async move { Ok(bytes) }).boxed(),
    )
}

pub(crate) fn body_from_stream(
    ctx: RelayContext,
    status: StatusCode,
    content_length: Option<u64>,
    usage_buffer_limit_bytes: usize,
    response_metadata: UpstreamResponseMetadata,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, String>>,
) -> Body {
    let streamed = ctx.streamed;
    let path = ctx.path;
    let sse_rewriter = (status.is_success() && ctx.streamed).then(|| {
        SseStreamRewriter::new(
            (ctx.external_model != ctx.model).then(|| ctx.external_model.clone()),
            ctx.path,
            usage_buffer_limit_bytes,
        )
    });
    let upstream_path = ctx.upstream_request_path.as_deref().unwrap_or(ctx.path);
    let response_mode = ctx.upstream_response_mode.unwrap_or("passthrough");
    let stream_idle_timeout = ctx.streamed.then_some(STREAM_IDLE_TIMEOUT);
    let stream_keep_alive_interval = ctx.streamed.then_some(STREAM_KEEP_ALIVE_INTERVAL);
    let now = tokio::time::Instant::now();
    tracing::debug!(
        relay_trace_id = %ctx.relay_trace_id,
        relay_attempt = ctx.relay_attempt,
        relay_final = ctx.relay_final,
        provider = %ctx.upstream.provider,
        channel_id = ctx.upstream.channel_id,
        channel_name = %ctx.upstream.channel_name,
        channel_endpoint_id = ctx.upstream.channel_endpoint_id,
        channel_key_id = ?ctx.upstream.channel_key_id,
        credential_id = ?ctx.upstream.credential_id,
        protocol = ctx.protocol.as_str(),
        model = %ctx.model,
        external_model = %ctx.external_model,
        upstream_model = %ctx.upstream_model,
        path = ctx.path,
        upstream_path,
        response_mode,
        responses_chat_fallback = response_mode == "openai_chat_as_openai_response",
        base_url = %ctx.upstream.base_url,
        status = status.as_u16(),
        streamed = ctx.streamed,
        content_length,
        usage_buffer_limit_bytes,
        stream_idle_timeout_seconds = ?stream_idle_timeout.map(|timeout| timeout.as_secs()),
        stream_keep_alive_interval_seconds = ?stream_keep_alive_interval.map(|interval| interval.as_secs()),
        "starting relay response body stream"
    );
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
        sse_rewriter,
        response_metadata,
        stream_idle_timeout,
        upstream_idle_deadline: stream_idle_timeout.map(|timeout| now + timeout),
        downstream_keep_alive_interval: stream_keep_alive_interval,
        downstream_keep_alive_deadline: stream_keep_alive_interval.map(|interval| now + interval),
        keep_alive_frames_sent: 0,
        first_response_ms: None,
        last_chunk_ms: None,
        chunks_sent: 0,
        bytes_sent: 0,
        largest_chunk_bytes: 0,
    };

    Body::from_stream(futures_util::stream::unfold(
        Some(relay),
        |relay| async move {
            let mut relay = relay?;
            let next = match (
                relay.upstream_idle_deadline,
                relay.downstream_keep_alive_deadline,
            ) {
                (Some(idle_deadline), Some(keep_alive_deadline)) => tokio::select! {
                    upstream = relay.stream.next() => StreamPoll::Upstream(upstream),
                    () = tokio::time::sleep_until(keep_alive_deadline) => StreamPoll::KeepAlive,
                    () = tokio::time::sleep_until(idle_deadline) => StreamPoll::IdleTimeout,
                },
                _ => StreamPoll::Upstream(relay.stream.next().await),
            };
            match next {
                StreamPoll::IdleTimeout => {
                    let timeout = relay
                        .stream_idle_timeout
                        .expect("stream timeout is present when timeout fires");
                    let summary = format!(
                        "upstream response stream was idle for {} seconds",
                        timeout.as_secs()
                    );
                    let downstream_error = relay.downstream_stream_error_frame();
                    relay.finish_stream_error(summary.clone()).await;
                    downstream_error.map_or_else(
                        || {
                            Some((
                                Err(std::io::Error::new(std::io::ErrorKind::TimedOut, summary)),
                                None,
                            ))
                        },
                        |frame| Some((Ok::<Bytes, std::io::Error>(frame), None)),
                    )
                }
                StreamPoll::KeepAlive => {
                    relay.observe_keep_alive();
                    Some((
                        Ok::<Bytes, std::io::Error>(DOWNSTREAM_STREAM_KEEP_ALIVE.clone()),
                        Some(relay),
                    ))
                }
                StreamPoll::Upstream(Some(Ok(chunk))) => {
                    relay.observe_upstream_activity();
                    let rewritten = relay.rewrite_sse_chunk(chunk);
                    let chunk = rewritten.chunk;
                    if chunk.is_empty() {
                        return Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay)));
                    }
                    relay.observe_chunk(&chunk);
                    relay.usage.observe(&chunk);
                    if rewritten.bare_error {
                        relay.log_normalized_bare_stream_error();
                        let _ = relay.finish_stream_success().await;
                        Some((Ok::<Bytes, std::io::Error>(chunk), None))
                    } else {
                        Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay)))
                    }
                }
                StreamPoll::Upstream(Some(Err(summary))) => {
                    if relay.should_ignore_successful_stream_error(&summary) {
                        relay.finish_trailing_stream_error(summary).await;
                        None
                    } else {
                        let downstream_error = relay.downstream_stream_error_frame();
                        relay.finish_stream_error(summary.clone()).await;
                        downstream_error.map_or_else(
                            || Some((Err(std::io::Error::other(summary)), None)),
                            |frame| Some((Ok::<Bytes, std::io::Error>(frame), None)),
                        )
                    }
                }
                StreamPoll::Upstream(None) => {
                    if let Some(rewritten) = relay.finish_sse_rewrite() {
                        let chunk = rewritten.chunk;
                        relay.observe_chunk(&chunk);
                        relay.usage.observe(&chunk);
                        if rewritten.bare_error {
                            relay.log_normalized_bare_stream_error();
                            let _ = relay.finish_stream_success().await;
                            return Some((Ok::<Bytes, std::io::Error>(chunk), None));
                        }
                        return Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay)));
                    }
                    relay
                        .finish_stream_success()
                        .await
                        .map(|frame| (Ok::<Bytes, std::io::Error>(frame), None))
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
    sse_rewriter: Option<SseStreamRewriter>,
    response_metadata: UpstreamResponseMetadata,
    stream_idle_timeout: Option<Duration>,
    upstream_idle_deadline: Option<tokio::time::Instant>,
    downstream_keep_alive_interval: Option<Duration>,
    downstream_keep_alive_deadline: Option<tokio::time::Instant>,
    keep_alive_frames_sent: u64,
    first_response_ms: Option<i64>,
    last_chunk_ms: Option<i64>,
    chunks_sent: u64,
    bytes_sent: u64,
    largest_chunk_bytes: usize,
}

impl StreamingRelay {
    fn observe_upstream_activity(&mut self) {
        self.upstream_idle_deadline = self
            .stream_idle_timeout
            .map(|timeout| tokio::time::Instant::now() + timeout);
    }

    fn observe_keep_alive(&mut self) {
        self.keep_alive_frames_sent = self.keep_alive_frames_sent.saturating_add(1);
        self.downstream_keep_alive_deadline = self
            .downstream_keep_alive_interval
            .map(|interval| tokio::time::Instant::now() + interval);

        if let Some(ctx) = self.ctx.as_ref() {
            tracing::trace!(
                relay_trace_id = %ctx.relay_trace_id,
                relay_attempt = ctx.relay_attempt,
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                path = ctx.path,
                keep_alive_frames_sent = self.keep_alive_frames_sent,
                "sent downstream SSE keep-alive"
            );
        }
    }

    fn rewrite_sse_chunk(&mut self, chunk: Bytes) -> SseRewriteOutput {
        match self.sse_rewriter.as_mut() {
            Some(rewriter) => rewriter.rewrite_chunk(chunk),
            None => SseRewriteOutput {
                chunk,
                bare_error: false,
            },
        }
    }

    fn finish_sse_rewrite(&mut self) -> Option<SseRewriteOutput> {
        self.sse_rewriter
            .as_mut()
            .and_then(SseStreamRewriter::finish)
    }

    fn log_normalized_bare_stream_error(&self) {
        let Some(ctx) = self.ctx.as_ref() else {
            return;
        };
        let stream_error = self.usage.stream_error_summary();
        tracing::warn!(
            relay_trace_id = %ctx.relay_trace_id,
            relay_attempt = ctx.relay_attempt,
            relay_final = ctx.relay_final,
            provider = %ctx.upstream.provider,
            channel_id = ctx.upstream.channel_id,
            channel_name = %ctx.upstream.channel_name,
            channel_endpoint_id = ctx.upstream.channel_endpoint_id,
            channel_key_id = ?ctx.upstream.channel_key_id,
            credential_id = ?ctx.upstream.credential_id,
            protocol = ctx.protocol.as_str(),
            model = %ctx.model,
            path = ctx.path,
            sse_error_type = ?stream_error.as_ref().and_then(|error| error.error_type.as_deref()),
            sse_error_code = ?stream_error.as_ref().and_then(|error| error.error_code.as_deref()),
            sse_error_message = ?stream_error.as_ref().and_then(|error| error.error_message.as_deref()),
            "normalized bare upstream JSON error into downstream SSE error event"
        );
    }

    fn observe_chunk(&mut self, chunk: &Bytes) {
        if chunk.is_empty() {
            return;
        }
        let Some(ctx) = self.ctx.as_ref() else {
            return;
        };
        let elapsed_ms = ctx.started.elapsed().as_millis() as i64;
        let previous_chunk_ms = self.last_chunk_ms;
        self.first_response_ms.get_or_insert(elapsed_ms);
        self.last_chunk_ms = Some(elapsed_ms);
        self.chunks_sent = self.chunks_sent.saturating_add(1);
        self.bytes_sent = self.bytes_sent.saturating_add(chunk.len() as u64);
        self.largest_chunk_bytes = self.largest_chunk_bytes.max(chunk.len());
        let idle_ms = previous_chunk_ms.map_or(elapsed_ms, |last| elapsed_ms.saturating_sub(last));

        if self.chunks_sent == 1 {
            tracing::debug!(
                relay_trace_id = %ctx.relay_trace_id,
                relay_attempt = ctx.relay_attempt,
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                path = ctx.path,
                status = self.status.as_u16(),
                streamed = ctx.streamed,
                chunk_bytes = chunk.len(),
                bytes_sent = self.bytes_sent,
                first_response_ms = self.first_response_ms,
                idle_ms,
                "relay stream sent first response chunk"
            );
        } else if self.chunks_sent.is_multiple_of(256) {
            tracing::debug!(
                relay_trace_id = %ctx.relay_trace_id,
                relay_attempt = ctx.relay_attempt,
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                path = ctx.path,
                status = self.status.as_u16(),
                streamed = ctx.streamed,
                chunks_sent = self.chunks_sent,
                bytes_sent = self.bytes_sent,
                largest_chunk_bytes = self.largest_chunk_bytes,
                last_chunk_ms = self.last_chunk_ms,
                idle_ms,
                "relay stream progress"
            );
        }

        tracing::trace!(
            relay_trace_id = %ctx.relay_trace_id,
            relay_attempt = ctx.relay_attempt,
            provider = %ctx.upstream.provider,
            channel_id = ctx.upstream.channel_id,
            channel_endpoint_id = ctx.upstream.channel_endpoint_id,
            protocol = ctx.protocol.as_str(),
            model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
            path = ctx.path,
            status = self.status.as_u16(),
            streamed = ctx.streamed,
            chunk_bytes = chunk.len(),
            chunks_sent = self.chunks_sent,
            bytes_sent = self.bytes_sent,
            idle_ms,
            "relay stream chunk"
        );
    }

    fn should_ignore_successful_stream_error(&self, summary: &str) -> bool {
        self.status.is_success() && self.usage.response_complete() && is_body_decode_error(summary)
    }

    fn downstream_stream_error_frame(&self) -> Option<Bytes> {
        let ctx = self.ctx.as_ref()?;
        (self.status.is_success() && ctx.streamed && !self.usage.response_complete())
            .then(|| downstream_stream_error_frame(ctx.path))
    }

    async fn finish_stream_success(mut self) -> Option<Bytes> {
        let mut ctx = self.ctx.take().expect("stream context finalized once");
        let token_usage = self.usage.finish();
        let stream_complete = self.usage.response_complete();
        let stream_failed = self.usage.response_failed();
        let missing_terminal =
            streamed_success_missing_terminal(self.status, ctx.streamed, stream_complete);
        let disposition = stream_finish_disposition(self.status, stream_failed, missing_terminal);
        let stream_error = self.usage.stream_error_summary();
        let stream_error_summary = stream_error
            .as_ref()
            .map(StreamErrorSummary::to_error_summary)
            .unwrap_or_else(|| "upstream stream ended with SSE error event".to_string());
        ctx.release_request_permit();
        let upstream_path = ctx.upstream_request_path.as_deref().unwrap_or(ctx.path);
        let response_mode = ctx.upstream_response_mode.unwrap_or("passthrough");
        tracing::debug!(
            relay_trace_id = %ctx.relay_trace_id,
            relay_attempt = ctx.relay_attempt,
            relay_final = ctx.relay_final,
            provider = %ctx.upstream.provider,
            channel_id = ctx.upstream.channel_id,
            channel_name = %ctx.upstream.channel_name,
            channel_endpoint_id = ctx.upstream.channel_endpoint_id,
            channel_key_id = ?ctx.upstream.channel_key_id,
            credential_id = ?ctx.upstream.credential_id,
            protocol = ctx.protocol.as_str(),
            model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
            path = ctx.path,
            base_url = %ctx.upstream.base_url,
            status = self.status.as_u16(),
            streamed = ctx.streamed,
            stream_complete,
            first_response_ms = self.first_response_ms,
            last_chunk_ms = self.last_chunk_ms,
            chunks_sent = self.chunks_sent,
            bytes_sent = self.bytes_sent,
            largest_chunk_bytes = self.largest_chunk_bytes,
            keep_alive_frames_sent = self.keep_alive_frames_sent,
            latency_ms = ctx.started.elapsed().as_millis() as i64,
            "relay response body stream completed"
        );
        if missing_terminal {
            tracing::warn!(
                relay_trace_id = %ctx.relay_trace_id,
                relay_attempt = ctx.relay_attempt,
                relay_final = ctx.relay_final,
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                path = ctx.path,
                base_url = %ctx.upstream.base_url,
                status = self.status.as_u16(),
                first_response_ms = self.first_response_ms,
                last_chunk_ms = self.last_chunk_ms,
                chunks_sent = self.chunks_sent,
                bytes_sent = self.bytes_sent,
                largest_chunk_bytes = self.largest_chunk_bytes,
                keep_alive_frames_sent = self.keep_alive_frames_sent,
                latency_ms = ctx.started.elapsed().as_millis() as i64,
                last_signal = ?self.usage.last_signal_summary(),
                previous_signal = ?self.usage.previous_signal_summary(),
                upstream_request_id = ?self.response_metadata.request_id,
                upstream_trace_id = ?self.response_metadata.trace_id,
                upstream_cf_ray = ?self.response_metadata.cf_ray,
                upstream_server_timing = ?self.response_metadata.server_timing,
                "upstream stream ended before terminal SSE event"
            );
        }
        if stream_failed {
            tracing::warn!(
                relay_trace_id = %ctx.relay_trace_id,
                relay_attempt = ctx.relay_attempt,
                relay_final = ctx.relay_final,
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                path = ctx.path,
                upstream_path,
                response_mode,
                responses_chat_fallback = response_mode == "openai_chat_as_openai_response",
                base_url = %ctx.upstream.base_url,
                status = self.status.as_u16(),
                request_body_bytes = ctx.request_body_bytes,
                request_input_tokens_estimate = ctx.request_input_tokens_estimate,
                request_max_tokens = ?ctx.request_params.max_tokens,
                request_reasoning_effort = ?ctx.request_params.reasoning_effort,
                request_tool_count = ?ctx.request_params.tool_count,
                request_tool_choice = ?ctx.request_params.tool_choice,
                first_response_ms = self.first_response_ms,
                last_chunk_ms = self.last_chunk_ms,
                chunks_sent = self.chunks_sent,
                bytes_sent = self.bytes_sent,
                largest_chunk_bytes = self.largest_chunk_bytes,
                keep_alive_frames_sent = self.keep_alive_frames_sent,
                latency_ms = ctx.started.elapsed().as_millis() as i64,
                last_signal = ?self.usage.last_signal_summary(),
                previous_signal = ?self.usage.previous_signal_summary(),
                upstream_request_id = ?self.response_metadata.request_id,
                upstream_trace_id = ?self.response_metadata.trace_id,
                upstream_cf_ray = ?self.response_metadata.cf_ray,
                upstream_server_timing = ?self.response_metadata.server_timing,
                observed_input_tokens = ?token_usage.map(|usage| usage.input_tokens),
                observed_output_tokens = ?token_usage.map(|usage| usage.output_tokens),
                observed_reasoning_output_tokens = ?token_usage.and_then(|usage| usage.reasoning_output_tokens),
                sse_response_id = ?stream_error.as_ref().and_then(|error| error.response_id.as_deref()),
                sse_request_id = ?stream_error.as_ref().and_then(|error| error.request_id.as_deref()),
                sse_error_type = ?stream_error.as_ref().and_then(|error| error.error_type.as_deref()),
                sse_error_code = ?stream_error.as_ref().and_then(|error| error.error_code.as_deref()),
                sse_error_message = ?stream_error.as_ref().and_then(|error| error.error_message.as_deref()),
                sse_error_raw = ?stream_error.as_ref().and_then(|error| error.raw.as_deref()),
                "upstream stream ended with SSE error event"
            );
        }
        if stream_failed {
            if let Some(summary) = stream_error.as_ref() {
                // 与非流式 400 路径（mod.rs 路径 C）对称：上游在流式 responses 里返回
                // 「模型不可用」类 SSE error 时，学习该 (endpoint, model) 不支持
                // /v1/responses，使下一次请求在转发前降级到 chat。仅当本次以 responses
                // 路由 + openai/openai_oauth 协议 + 未降级地走原生 responses 发出
                // （responses_chat_fallback == false）时才标记，避免误伤已降级的 chat 路径。
                if ctx.path == "/v1/responses"
                    && matches!(
                        ctx.protocol,
                        UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
                    )
                    && !ctx.upstream.responses_chat_fallback
                    && !ctx
                        .state
                        .selector
                        .responses_unsupported(ctx.upstream.channel_endpoint_id, &ctx.model)
                        .await
                    && is_model_error_text(&sse_error_lowered(summary))
                {
                    let until = chrono::Utc::now()
                        + chrono::Duration::seconds(
                            ctx.state.config.relay.responses_support_block_seconds,
                        );
                    ctx.state
                        .selector
                        .mark_responses_unsupported(
                            ctx.upstream.channel_endpoint_id,
                            &ctx.model,
                            until,
                        )
                        .await;
                    tracing::warn!(
                        provider = %ctx.upstream.provider,
                        channel_id = ctx.upstream.channel_id,
                        channel_name = %ctx.upstream.channel_name,
                        channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                        protocol = ctx.protocol.as_str(),
                        model = %ctx.model,
                        path = ctx.path,
                        sse_error_code = ?summary.error_code,
                        sse_error_message = ?summary.error_message,
                        "upstream rejected responses for this model via SSE error; downgrading to chat on next request",
                    );
                }
            }
        }
        let billing = match disposition {
            StreamFinishDisposition::Success => {
                record_channel_affinity(&ctx).await;
                settle_successful_hold(&ctx, token_usage, "streamed relay").await
            }
            StreamFinishDisposition::HttpError => {
                release_empty_hold(&ctx.state, ctx.hold.clone(), "upstream error").await;
                None
            }
            StreamFinishDisposition::SseError => {
                release_empty_hold(&ctx.state, ctx.hold.clone(), "stream SSE error").await;
                None
            }
            StreamFinishDisposition::MissingTerminal => {
                release_empty_hold(&ctx.state, ctx.hold.clone(), "incomplete stream").await;
                None
            }
        };
        let error_summary = match disposition {
            StreamFinishDisposition::Success => None,
            StreamFinishDisposition::HttpError => Some("upstream error".to_string()),
            StreamFinishDisposition::SseError => Some(stream_error_summary.clone()),
            StreamFinishDisposition::MissingTerminal => {
                Some(MISSING_TERMINAL_STREAM_ERROR.to_string())
            }
        };
        let failure = match disposition {
            StreamFinishDisposition::Success => None,
            StreamFinishDisposition::HttpError => {
                key_failure_from_context(&ctx, "upstream error".to_string()).await
            }
            StreamFinishDisposition::SseError => {
                key_failure_from_context(&ctx, stream_error_summary).await
            }
            StreamFinishDisposition::MissingTerminal => {
                key_failure_from_context(&ctx, MISSING_TERMINAL_STREAM_ERROR.to_string()).await
            }
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
        (disposition == StreamFinishDisposition::MissingTerminal)
            .then(|| downstream_stream_error_frame(ctx.path))
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
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                path = ctx.path,
                base_url = %ctx.upstream.base_url,
                status = self.status.as_u16(),
                streamed = ctx.streamed,
                first_response_ms = self.first_response_ms,
                last_chunk_ms = self.last_chunk_ms,
                chunks_sent = self.chunks_sent,
                bytes_sent = self.bytes_sent,
                largest_chunk_bytes = self.largest_chunk_bytes,
                keep_alive_frames_sent = self.keep_alive_frames_sent,
                latency_ms = ctx.started.elapsed().as_millis() as i64,
                error = %summary,
                "ignored trailing upstream stream read error after completed response"
            );
        }
        let _ = self.finish_stream_success().await;
    }

    async fn finish_stream_error(mut self, summary: String) {
        let Some(mut ctx) = self.ctx.take() else {
            return;
        };
        ctx.release_request_permit();
        let token_usage = self.usage.finish();
        let stream_complete = self.usage.response_complete();
        let stream_failed = stream_body_error_is_failure(self.status, stream_complete);
        tracing::warn!(
            provider = %ctx.upstream.provider,
            channel_id = ctx.upstream.channel_id,
            channel_name = %ctx.upstream.channel_name,
            channel_endpoint_id = ctx.upstream.channel_endpoint_id,
            channel_key_id = ?ctx.upstream.channel_key_id,
            credential_id = ?ctx.upstream.credential_id,
            protocol = ctx.protocol.as_str(),
            model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
            path = ctx.path,
            base_url = %ctx.upstream.base_url,
            status = self.status.as_u16(),
            streamed = ctx.streamed,
            stream_complete,
            first_response_ms = self.first_response_ms,
            last_chunk_ms = self.last_chunk_ms,
            chunks_sent = self.chunks_sent,
            bytes_sent = self.bytes_sent,
            largest_chunk_bytes = self.largest_chunk_bytes,
            keep_alive_frames_sent = self.keep_alive_frames_sent,
            latency_ms = ctx.started.elapsed().as_millis() as i64,
            last_signal = ?self.usage.last_signal_summary(),
            previous_signal = ?self.usage.previous_signal_summary(),
            upstream_request_id = ?self.response_metadata.request_id,
            upstream_trace_id = ?self.response_metadata.trace_id,
            upstream_cf_ray = ?self.response_metadata.cf_ray,
            upstream_server_timing = ?self.response_metadata.server_timing,
            error = %summary,
            "upstream stream failed while relaying response body"
        );
        let billing = if !stream_failed {
            settle_successful_hold(&ctx, token_usage, "successful stream error").await
        } else {
            release_empty_hold(&ctx.state, ctx.hold.clone(), "stream error").await;
            None
        };
        let failure = if stream_failed {
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

struct SseRewriteOutput {
    chunk: Bytes,
    bare_error: bool,
}

enum SseLineRewrite {
    Line(Vec<u8>),
    BareError(Bytes),
}

struct SseStreamRewriter {
    buffered: Vec<u8>,
    external_model: Option<String>,
    path: &'static str,
    limit_bytes: usize,
    skipping_oversized_line: bool,
    finished: bool,
}

impl SseStreamRewriter {
    fn new(external_model: Option<String>, path: &'static str, limit_bytes: usize) -> Self {
        Self {
            buffered: Vec::new(),
            external_model,
            path,
            limit_bytes,
            skipping_oversized_line: false,
            finished: false,
        }
    }

    fn rewrite_chunk(&mut self, chunk: Bytes) -> SseRewriteOutput {
        if chunk.is_empty() || self.finished {
            return SseRewriteOutput {
                chunk: Bytes::new(),
                bare_error: false,
            };
        }
        let mut output = Vec::with_capacity(self.buffered.len().saturating_add(chunk.len()));
        let chunk = if self.skipping_oversized_line {
            let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') else {
                return SseRewriteOutput {
                    chunk,
                    bare_error: false,
                };
            };
            output.extend_from_slice(&chunk[..=offset]);
            self.skipping_oversized_line = false;
            chunk.slice(offset + 1..)
        } else {
            chunk
        };
        self.buffered.extend_from_slice(&chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let line_end = consumed + offset;
            match self.rewrite_line(&self.buffered[consumed..line_end]) {
                SseLineRewrite::Line(line) => {
                    output.extend_from_slice(&line);
                    output.push(b'\n');
                }
                SseLineRewrite::BareError(frame) => {
                    output.extend_from_slice(&frame);
                    self.buffered.clear();
                    self.finished = true;
                    return SseRewriteOutput {
                        chunk: Bytes::from(output),
                        bare_error: true,
                    };
                }
            }
            consumed = line_end + 1;
        }
        if consumed == self.buffered.len() {
            self.buffered.clear();
        } else if consumed > 0 {
            self.buffered.drain(..consumed);
        }

        if self.buffered.len() > self.limit_bytes {
            output.extend_from_slice(&self.buffered);
            self.buffered.clear();
            self.skipping_oversized_line = true;
            return SseRewriteOutput {
                chunk: Bytes::from(output),
                bare_error: false,
            };
        }

        if let Some(frame) = normalize_bare_sse_error(&self.buffered, self.path) {
            output.extend_from_slice(&frame);
            self.buffered.clear();
            self.finished = true;
            return SseRewriteOutput {
                chunk: Bytes::from(output),
                bare_error: true,
            };
        }

        SseRewriteOutput {
            chunk: Bytes::from(output),
            bare_error: false,
        }
    }

    fn finish(&mut self) -> Option<SseRewriteOutput> {
        if self.finished {
            return None;
        }
        self.finished = true;
        if self.buffered.is_empty() {
            return None;
        }
        let line = std::mem::take(&mut self.buffered);
        Some(match self.rewrite_line(&line) {
            SseLineRewrite::Line(line) => SseRewriteOutput {
                chunk: Bytes::from(line),
                bare_error: false,
            },
            SseLineRewrite::BareError(frame) => SseRewriteOutput {
                chunk: frame,
                bare_error: true,
            },
        })
    }

    fn rewrite_line(&self, line: &[u8]) -> SseLineRewrite {
        let (line, cr) = match line.strip_suffix(b"\r") {
            Some(line) => (line, true),
            None => (line, false),
        };
        if let Some(frame) = normalize_bare_sse_error(line, self.path) {
            return SseLineRewrite::BareError(frame);
        }
        let Some(external_model) = self.external_model.as_deref() else {
            let mut output = line.to_vec();
            if cr {
                output.push(b'\r');
            }
            return SseLineRewrite::Line(output);
        };
        let Some(rewritten) = rewrite_sse_data_model(line, external_model) else {
            let mut output = line.to_vec();
            if cr {
                output.push(b'\r');
            }
            return SseLineRewrite::Line(output);
        };
        let mut output = rewritten.into_bytes();
        if cr {
            output.push(b'\r');
        }
        SseLineRewrite::Line(output)
    }
}

fn rewrite_sse_data_model(line: &[u8], external_model: &str) -> Option<String> {
    let line = std::str::from_utf8(line).ok()?;
    let rest = line.strip_prefix("data:")?;
    let leading_len = rest.len() - rest.trim_start().len();
    let leading = &rest[..leading_len];
    let data = rest[leading_len..].trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let mut value = serde_json::from_str::<Value>(data).ok()?;
    let object = value.as_object_mut()?;
    if !object.contains_key("model") {
        return None;
    }
    object.insert(
        "model".to_string(),
        Value::String(external_model.to_string()),
    );
    serde_json::to_string(&value)
        .ok()
        .map(|json| format!("data:{leading}{json}"))
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

fn stream_body_error_is_failure(status: StatusCode, complete: bool) -> bool {
    !status.is_success() || !complete
}

fn stream_finish_disposition(
    status: StatusCode,
    stream_failed: bool,
    missing_terminal: bool,
) -> StreamFinishDisposition {
    if !status.is_success() {
        StreamFinishDisposition::HttpError
    } else if stream_failed {
        StreamFinishDisposition::SseError
    } else if missing_terminal {
        StreamFinishDisposition::MissingTerminal
    } else {
        StreamFinishDisposition::Success
    }
}

fn streamed_success_missing_terminal(status: StatusCode, streamed: bool, complete: bool) -> bool {
    status.is_success() && streamed && !complete
}

fn is_body_decode_error(summary: &str) -> bool {
    summary.contains("error decoding response body")
}

fn downstream_stream_error_frame(path: &str) -> Bytes {
    let data = match path {
        "/v1/messages" => serde_json::json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
            },
        }),
        "/v1/responses" => serde_json::json!({
            "type": "error",
            "code": "server_error",
            "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
            "param": null,
        }),
        _ => serde_json::json!({
            "error": {
                "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
                "type": "server_error",
                "param": null,
                "code": "upstream_stream_error",
            },
        }),
    };
    encode_downstream_stream_error(path, data)
}

fn normalize_bare_sse_error(line: &[u8], path: &str) -> Option<Bytes> {
    let line = std::str::from_utf8(line).ok()?.trim();
    if line.is_empty()
        || line.starts_with("data:")
        || line.starts_with("event:")
        || line.starts_with(':')
    {
        return None;
    }
    let mut data = serde_json::from_str::<Value>(line).ok()?;
    let object = data.as_object_mut()?;
    let type_name = object.get("type").and_then(Value::as_str);
    let nested_error = object.get("error").and_then(Value::as_object);
    let nested_error_has_details = nested_error.is_some_and(|error| {
        ["message", "type", "code"]
            .iter()
            .any(|field| error.get(*field).is_some_and(Value::is_string))
    });
    let typed_error = type_name == Some("error");
    let nested_error_envelope =
        nested_error_has_details && matches!(type_name, None | Some("error"));
    let top_level_error = typed_error
        && (object.get("message").is_some_and(Value::is_string)
            || object.get("code").is_some_and(Value::is_string));
    let valid_for_path = match path {
        "/v1/messages" => nested_error_envelope,
        "/v1/responses" => top_level_error,
        _ => nested_error_envelope,
    };
    if !valid_for_path {
        return None;
    }
    if path == "/v1/messages" {
        object
            .entry("type".to_string())
            .or_insert_with(|| Value::String("error".to_string()));
    }
    Some(encode_downstream_stream_error(path, data))
}

fn encode_downstream_stream_error(path: &str, data: Value) -> Bytes {
    let data = serde_json::to_string(&data).expect("stream error payload is serializable");
    if matches!(path, "/v1/messages" | "/v1/responses") {
        Bytes::from(format!("event: error\ndata: {data}\n\n"))
    } else {
        Bytes::from(format!("data: {data}\n\n"))
    }
}

impl Drop for StreamingRelay {
    fn drop(&mut self) {
        let Some(mut ctx) = self.ctx.take() else {
            return;
        };
        ctx.release_request_permit();
        let status = self.status;
        let stream_complete = self.usage.response_complete();
        let token_usage = self.usage.finish();
        let first_response_ms = self.first_response_ms;
        let last_chunk_ms = self.last_chunk_ms;
        let chunks_sent = self.chunks_sent;
        let bytes_sent = self.bytes_sent;
        let largest_chunk_bytes = self.largest_chunk_bytes;
        let keep_alive_frames_sent = self.keep_alive_frames_sent;

        tokio::spawn(async move {
            if stream_complete {
                tracing::debug!(
                    relay_trace_id = %ctx.relay_trace_id,
                    relay_attempt = ctx.relay_attempt,
                    relay_final = ctx.relay_final,
                    provider = %ctx.upstream.provider,
                    channel_id = ctx.upstream.channel_id,
                    channel_name = %ctx.upstream.channel_name,
                    channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                    channel_key_id = ?ctx.upstream.channel_key_id,
                    credential_id = ?ctx.upstream.credential_id,
                    protocol = ctx.protocol.as_str(),
                    model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                    path = ctx.path,
                    base_url = %ctx.upstream.base_url,
                    status = status.as_u16(),
                    streamed = ctx.streamed,
                    first_response_ms,
                    last_chunk_ms,
                    chunks_sent,
                    bytes_sent,
                    largest_chunk_bytes,
                    keep_alive_frames_sent,
                    latency_ms = ctx.started.elapsed().as_millis() as i64,
                    "downstream client closed relay stream after completed response"
                );
            } else {
                tracing::warn!(
                    relay_trace_id = %ctx.relay_trace_id,
                    relay_attempt = ctx.relay_attempt,
                    relay_final = ctx.relay_final,
                    provider = %ctx.upstream.provider,
                    channel_id = ctx.upstream.channel_id,
                    channel_name = %ctx.upstream.channel_name,
                    channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                    channel_key_id = ?ctx.upstream.channel_key_id,
                    credential_id = ?ctx.upstream.credential_id,
                    protocol = ctx.protocol.as_str(),
                    model = %ctx.model,
                external_model = %ctx.external_model,
                upstream_model = %ctx.upstream_model,
                    path = ctx.path,
                    base_url = %ctx.upstream.base_url,
                    status = status.as_u16(),
                    streamed = ctx.streamed,
                    first_response_ms,
                    last_chunk_ms,
                    chunks_sent,
                    bytes_sent,
                    largest_chunk_bytes,
                    keep_alive_frames_sent,
                    latency_ms = ctx.started.elapsed().as_millis() as i64,
                    "downstream client closed relay stream before completion"
                );
            }
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
            let error_summary = (!stream_complete)
                .then(|| "downstream stream closed before completion".to_string());
            let usage = usage_from_context(
                &ctx,
                Some(status.as_u16() as i32),
                error_summary,
                first_response_ms,
                token_usage,
                billing,
            );
            enqueue_relay_usage(&ctx.state, usage, failure).await;
        });
    }
}

async fn record_channel_affinity(ctx: &RelayContext) {
    let Some(key) = ctx.channel_affinity_key.clone() else {
        return;
    };
    ctx.state
        .channel_affinity
        .insert(key, (&ctx.upstream).into())
        .await;
}

enum ResponseUsageParser {
    Sse(Box<StreamUsageParser>),
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
            Self::Sse(Box::new(StreamUsageParser::new(limit_bytes)))
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

    fn response_complete(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.completed)
            || matches!(self, Self::Json { buffer: Some(bytes), .. } if json_body_is_complete(bytes))
    }

    fn response_failed(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.failed)
    }

    fn stream_error_summary(&self) -> Option<StreamErrorSummary> {
        match self {
            Self::Sse(parser) => parser.last_error.clone(),
            Self::Json { .. } | Self::Disabled => None,
        }
    }

    /// Returns a short human-readable summary of the last observed stream
    /// signal so we can include it in the "stream ended before terminal SSE
    /// event" warning. Helps distinguish an upstream that emitted an
    /// unrecognized terminal type from one that simply hung up mid-stream.
    fn last_signal_summary(&self) -> Option<String> {
        match self {
            Self::Sse(parser) => {
                if parser.saw_done {
                    return Some("data:[DONE]".to_string());
                }
                signal_summary(parser.last_event.as_deref(), parser.last_type.as_deref())
            }
            Self::Json {
                buffer: Some(bytes),
                ..
            } => json_body_is_complete(bytes)
                .then(|| "json-body-complete".to_string())
                .or(Some("json-body-incomplete".to_string())),
            Self::Json { buffer: None, .. } => Some("json-buffer-overflow".to_string()),
            Self::Disabled => None,
        }
    }

    fn previous_signal_summary(&self) -> Option<String> {
        match self {
            Self::Sse(parser) => signal_summary(
                parser.previous_event.as_deref(),
                parser.previous_type.as_deref(),
            ),
            Self::Json { .. } | Self::Disabled => None,
        }
    }
}

fn signal_summary(event: Option<&str>, data_type: Option<&str>) -> Option<String> {
    match (event, data_type) {
        (Some(event), Some(data_type)) => Some(format!("event:{event} data_type:{data_type}")),
        (Some(event), None) => Some(format!("event:{event}")),
        (None, Some(data_type)) => Some(format!("data_type:{data_type}")),
        (None, None) => None,
    }
}

fn json_usage_buffer_capacity(content_length: Option<u64>, limit_bytes: usize) -> usize {
    content_length
        .and_then(|length| usize::try_from(length).ok())
        .map_or(0, |length| length.min(limit_bytes))
}

fn json_body_is_complete(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok()
}

#[derive(Default)]
struct ParsedLine {
    usage: Option<TokenUsage>,
    event: Option<String>,
    data: Option<String>,
    data_type: Option<String>,
    completed: bool,
    failed: bool,
    done: bool,
}

pub(crate) struct StreamUsageParser {
    buffered: Vec<u8>,
    latest: Option<TokenUsage>,
    completed: bool,
    skipping_oversized_line: bool,
    limit_bytes: usize,
    last_event: Option<String>,
    last_type: Option<String>,
    previous_event: Option<String>,
    previous_type: Option<String>,
    saw_done: bool,
    failed: bool,
    last_error: Option<StreamErrorSummary>,
}

impl StreamUsageParser {
    pub(crate) fn new(limit_bytes: usize) -> Self {
        Self {
            buffered: Vec::new(),
            latest: None,
            completed: false,
            skipping_oversized_line: false,
            limit_bytes,
            last_event: None,
            last_type: None,
            previous_event: None,
            previous_type: None,
            saw_done: false,
            failed: false,
            last_error: None,
        }
    }

    pub(crate) fn observe(&mut self, chunk: &[u8]) {
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
            let parsed = Self::parse_line(line);
            self.observe_parsed_line(parsed);
            consumed = line_end + 1;
        }
        if consumed == self.buffered.len() {
            self.buffered.clear();
        } else if consumed > 0 {
            self.buffered.drain(..consumed);
        }
    }

    fn observe_line(&mut self, line: &[u8]) {
        let parsed = Self::parse_line(line);
        self.observe_parsed_line(parsed);
    }

    fn observe_parsed_line(&mut self, parsed: ParsedLine) {
        if let Some(usage) = parsed.usage {
            match &mut self.latest {
                Some(latest) => merge_token_usage(latest, usage),
                None => self.latest = Some(usage),
            }
        }
        if parsed.failed && !self.failed {
            self.previous_event.clone_from(&self.last_event);
            self.previous_type.clone_from(&self.last_type);
        }
        if let Some(event) = parsed.event {
            self.last_event = Some(event);
        }
        if let Some(data_type) = parsed.data_type {
            self.last_type = Some(data_type);
        }
        if parsed.done {
            self.saw_done = true;
        }
        if parsed.completed {
            self.completed = true;
        }
        if parsed.failed {
            self.failed = true;
        }
        if let Some(data) = parsed.data.as_deref() {
            let is_error_data = parsed.failed || self.last_event.as_deref() == Some("error");
            if is_error_data {
                self.last_error = Some(StreamErrorSummary::from_sse_data(data));
            }
        }
    }

    fn parse_line(line: &[u8]) -> ParsedLine {
        let Ok(line) = std::str::from_utf8(line) else {
            return ParsedLine::default();
        };
        if let Some(event) = line.strip_prefix("event:").map(str::trim) {
            return ParsedLine {
                event: Some(event.to_string()),
                completed: stream_event_is_terminal(event),
                failed: stream_event_is_failure(event),
                ..Default::default()
            };
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return ParsedLine::default();
        };
        if data.is_empty() {
            return ParsedLine::default();
        }
        if data == "[DONE]" {
            return ParsedLine {
                done: true,
                completed: true,
                ..Default::default()
            };
        }
        ParsedLine {
            usage: parse_usage_from_sse_data(data),
            data: Some(data.to_string()),
            data_type: sse_data_type_name(data),
            completed: sse_data_has_terminal_type(data),
            failed: sse_data_has_failure_type(data),
            ..Default::default()
        }
    }

    pub(crate) fn finish(&mut self) -> Option<TokenUsage> {
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

fn merge_token_usage(current: &mut TokenUsage, incoming: TokenUsage) {
    // SSE providers may put cache details in an early event and final output usage later.
    // Merge fields so the later partial update cannot discard previously reported usage.
    current.input_tokens = current.input_tokens.max(incoming.input_tokens);
    current.output_tokens = current.output_tokens.max(incoming.output_tokens);
    current.cached_input_tokens = incoming.cached_input_tokens.or(current.cached_input_tokens);
    current.cache_creation_input_tokens = incoming
        .cache_creation_input_tokens
        .or(current.cache_creation_input_tokens);
    current.cache_creation_input_tokens_5m = incoming
        .cache_creation_input_tokens_5m
        .or(current.cache_creation_input_tokens_5m);
    current.cache_creation_input_tokens_1h = incoming
        .cache_creation_input_tokens_1h
        .or(current.cache_creation_input_tokens_1h);
    current.reasoning_output_tokens = incoming
        .reasoning_output_tokens
        .or(current.reasoning_output_tokens);
    current.audio_input_tokens = incoming.audio_input_tokens.or(current.audio_input_tokens);
    current.audio_output_tokens = incoming.audio_output_tokens.or(current.audio_output_tokens);
}

fn stream_event_is_terminal(event: &str) -> bool {
    matches!(
        event,
        "message_stop"
            | "response.completed"
            | "response.incomplete"
            | "response.failed"
            | "response.cancelled"
            | "error"
    )
}

fn stream_event_is_failure(event: &str) -> bool {
    matches!(event, "error" | "response.failed")
}

fn sse_data_has_terminal_type(data: &str) -> bool {
    data.contains("message_stop") && sse_data_type_is(data, "message_stop")
        || data.contains("response.completed") && sse_data_type_is(data, "response.completed")
        || data.contains("response.incomplete") && sse_data_type_is(data, "response.incomplete")
        || data.contains("response.failed") && sse_data_type_is(data, "response.failed")
        || data.contains("response.cancelled") && sse_data_type_is(data, "response.cancelled")
        || data.contains("\"error\"") && sse_data_type_is(data, "error")
}

fn sse_data_has_failure_type(data: &str) -> bool {
    data.contains("response.failed") && sse_data_type_is(data, "response.failed")
        || data.contains("\"error\"") && sse_data_type_is(data, "error")
}

fn sse_data_type_name(data: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    value
        .get("type")
        .and_then(|type_| type_.as_str())
        .map(ToString::to_string)
}

fn sse_data_type_is(data: &str, expected: &str) -> bool {
    sse_data_type_name(data).is_some_and(|type_| type_ == expected)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StreamErrorSummary {
    response_id: Option<String>,
    request_id: Option<String>,
    error_type: Option<String>,
    error_code: Option<String>,
    error_message: Option<String>,
    raw: Option<String>,
}

impl StreamErrorSummary {
    fn from_sse_data(data: &str) -> Self {
        let raw = Some(truncate_for_log(data, 1000));
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Self {
                response_id: None,
                request_id: None,
                error_type: None,
                error_code: None,
                error_message: Some(truncate_for_log(data, 500)),
                raw,
            };
        };
        let response_error = value
            .get("response")
            .and_then(|response| response.get("error"));
        let response_id = value
            .get("response")
            .and_then(|response| string_field(response, "id"))
            .or_else(|| string_field(&value, "response_id"))
            .map(|value| truncate_for_log(&value, 256));
        let request_id = string_field(&value, "request_id")
            .or_else(|| string_field(&value, "request-id"))
            .map(|value| truncate_for_log(&value, 256));
        let error = response_error
            .or_else(|| value.get("error"))
            .unwrap_or(&value);
        let error_type = string_field(error, "type")
            .or_else(|| string_field(&value, "type"))
            .map(|value| truncate_for_log(&value, 128));
        let error_code = string_field(error, "code")
            .or_else(|| string_field(&value, "code"))
            .map(|value| truncate_for_log(&value, 128));
        let error_message = string_field(error, "message")
            .or_else(|| string_field(&value, "message"))
            .or_else(|| string_field(error, "msg"))
            .or_else(|| string_field(&value, "msg"))
            .map(|value| truncate_for_log(&value, 500));

        Self {
            response_id,
            request_id,
            error_type,
            error_code,
            error_message,
            raw,
        }
    }

    fn to_error_summary(&self) -> String {
        let mut summary = String::from("upstream stream ended with SSE error event");
        if let Some(code) = self.error_code.as_deref() {
            summary.push_str(" code=");
            summary.push_str(code);
        }
        if let Some(error_type) = self.error_type.as_deref() {
            summary.push_str(" type=");
            summary.push_str(error_type);
        }
        if let Some(message) = self.error_message.as_deref() {
            summary.push_str(": ");
            summary.push_str(message);
        }
        summary
    }
}

/// 把 SSE error 的 code/type/message 拼成小写字符串，供 `is_model_error_text` 做关键词
/// 匹配。code 和 type 也参与拼接，因为有些上游把 `model_not_found` 放在 code 而非
/// message 里。
fn sse_error_lowered(summary: &StreamErrorSummary) -> String {
    [
        summary.error_code.as_deref(),
        summary.error_type.as_deref(),
        summary.error_message.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    })
}

fn truncate_for_log(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= limit {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_error_data(frame: &Bytes) -> Value {
        let frame = std::str::from_utf8(frame).expect("stream error frame is utf-8");
        let data = frame
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("stream error frame has data");
        serde_json::from_str(data).expect("stream error data is json")
    }

    #[test]
    fn anthropic_incomplete_stream_uses_terminal_error_event() {
        let frame = downstream_stream_error_frame("/v1/messages");
        let text = std::str::from_utf8(&frame).expect("utf-8 frame");
        let data = stream_error_data(&frame);
        let mut parser = StreamUsageParser::new(1024);
        parser.observe(&frame);

        assert!(text.starts_with("event: error\n"));
        assert!(text.ends_with("\n\n"));
        assert!(parser.completed);
        assert!(parser.failed);
        assert_eq!(data.get("type").and_then(Value::as_str), Some("error"));
        assert_eq!(
            data.pointer("/error/type").and_then(Value::as_str),
            Some("api_error")
        );
        assert_eq!(
            data.pointer("/error/message").and_then(Value::as_str),
            Some(DOWNSTREAM_STREAM_ERROR_MESSAGE)
        );
    }

    #[test]
    fn responses_incomplete_stream_uses_terminal_error_event() {
        let frame = downstream_stream_error_frame("/v1/responses");
        let text = std::str::from_utf8(&frame).expect("utf-8 frame");
        let data = stream_error_data(&frame);

        assert!(text.starts_with("event: error\n"));
        assert_eq!(data.get("type").and_then(Value::as_str), Some("error"));
        assert_eq!(
            data.get("code").and_then(Value::as_str),
            Some("server_error")
        );
    }

    #[test]
    fn chat_incomplete_stream_uses_openai_error_data() {
        let frame = downstream_stream_error_frame("/v1/chat/completions");
        let text = std::str::from_utf8(&frame).expect("utf-8 frame");
        let data = stream_error_data(&frame);

        assert!(!text.starts_with("event:"));
        assert_eq!(
            data.pointer("/error/type").and_then(Value::as_str),
            Some("server_error")
        );
        assert_eq!(
            data.pointer("/error/code").and_then(Value::as_str),
            Some("upstream_stream_error")
        );
    }

    #[test]
    fn completed_successful_stream_body_errors_do_not_fail_the_key() {
        assert!(!stream_body_error_is_failure(StatusCode::OK, true));
    }

    #[test]
    fn incomplete_successful_stream_body_errors_fail_the_key() {
        assert!(stream_body_error_is_failure(StatusCode::OK, false));
    }

    #[test]
    fn upstream_error_stream_body_errors_fail_the_key() {
        assert!(stream_body_error_is_failure(
            StatusCode::TOO_MANY_REQUESTS,
            false
        ));
    }

    #[test]
    fn stream_finish_disposition_rejects_missing_terminal() {
        assert_eq!(
            stream_finish_disposition(StatusCode::OK, false, true),
            StreamFinishDisposition::MissingTerminal
        );
        assert_eq!(
            stream_finish_disposition(StatusCode::OK, false, false),
            StreamFinishDisposition::Success
        );
    }

    #[test]
    fn stream_finish_disposition_prioritizes_explicit_errors() {
        assert_eq!(
            stream_finish_disposition(StatusCode::BAD_GATEWAY, false, true),
            StreamFinishDisposition::HttpError
        );
        assert_eq!(
            stream_finish_disposition(StatusCode::OK, true, true),
            StreamFinishDisposition::SseError
        );
    }

    #[test]
    fn streamed_success_without_terminal_is_not_complete() {
        assert!(streamed_success_missing_terminal(
            StatusCode::OK,
            true,
            false
        ));
        assert!(!streamed_success_missing_terminal(
            StatusCode::OK,
            true,
            true
        ));
        assert!(!streamed_success_missing_terminal(
            StatusCode::OK,
            false,
            false
        ));
    }

    #[test]
    fn body_decode_errors_are_identified() {
        assert!(is_body_decode_error("error decoding response body"));
    }

    #[test]
    fn complete_json_body_marks_non_streamed_response_complete() {
        let mut parser = ResponseUsageParser::for_response(
            StatusCode::OK,
            false,
            "/v1/chat/completions",
            None,
            1024,
        );

        parser.observe(
            br#"{"choices":[{"message":{"role":"assistant","content":"OK"}}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}"#,
        );

        assert!(parser.response_complete());
    }

    #[test]
    fn partial_json_body_is_not_complete() {
        let mut parser = ResponseUsageParser::for_response(
            StatusCode::OK,
            false,
            "/v1/chat/completions",
            None,
            1024,
        );

        parser.observe(br#"{"choices":["#);

        assert!(!parser.response_complete());
    }

    #[test]
    fn sse_model_rewriter_rewrites_top_level_model() {
        let mut rewriter = SseStreamRewriter::new(
            Some("company-chat".to_string()),
            "/v1/chat/completions",
            1024,
        );

        let output = rewriter.rewrite_chunk(Bytes::from_static(
            br#"event: message
data: {"id":"1","model":"gpt-4o-mini","choices":[]}
data: [DONE]
"#,
        ));

        assert_eq!(
            std::str::from_utf8(&output.chunk).unwrap(),
            "event: message\ndata: {\"choices\":[],\"id\":\"1\",\"model\":\"company-chat\"}\ndata: [DONE]\n"
        );
        assert!(!output.bare_error);
        assert!(rewriter.finish().is_none());
    }

    #[test]
    fn sse_model_rewriter_buffers_partial_lines() {
        let mut rewriter =
            SseStreamRewriter::new(Some("project-model".to_string()), "/v1/messages", 1024);

        assert!(rewriter
            .rewrite_chunk(Bytes::from_static(br#"data: {"model":"#))
            .chunk
            .is_empty());
        let output = rewriter.rewrite_chunk(Bytes::from_static(br#""real-model"}"#));
        assert!(output.chunk.is_empty());
        let trailing = rewriter.finish().expect("unterminated line is flushed");

        assert_eq!(
            std::str::from_utf8(&trailing.chunk).unwrap(),
            "data: {\"model\":\"project-model\"}"
        );
        assert!(!trailing.bare_error);
    }

    #[test]
    fn sse_model_rewriter_preserves_downstream_keep_alive_comments() {
        let mut rewriter =
            SseStreamRewriter::new(Some("project-model".to_string()), "/v1/messages", 1024);

        let output = rewriter.rewrite_chunk(DOWNSTREAM_STREAM_KEEP_ALIVE.clone());

        assert_eq!(output.chunk, DOWNSTREAM_STREAM_KEEP_ALIVE);
        assert!(!output.bare_error);
        assert!(rewriter.finish().is_none());
    }

    #[test]
    fn sse_rewriter_normalizes_bare_anthropic_error() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 1024);

        let output = rewriter.rewrite_chunk(Bytes::from_static(
            br#"{"type":"error","error":{"type":"api_error","message":"upstream stream idle for 30s"}}"#,
        ));
        let data = stream_error_data(&output.chunk);

        assert!(output.bare_error);
        assert!(std::str::from_utf8(&output.chunk)
            .expect("utf-8 frame")
            .starts_with("event: error\n"));
        assert_eq!(
            data.pointer("/error/message").and_then(Value::as_str),
            Some("upstream stream idle for 30s")
        );
        assert!(rewriter.finish().is_none());
    }

    #[test]
    fn sse_rewriter_normalizes_bare_error_split_across_chunks() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 1024);

        let first = rewriter.rewrite_chunk(Bytes::from_static(
            br#"{"type":"error","error":{"type":"api_error","message":"upstream "#,
        ));
        let second = rewriter.rewrite_chunk(Bytes::from_static(br#"idle"}}"#));

        assert!(first.chunk.is_empty());
        assert!(!first.bare_error);
        assert!(second.bare_error);
        assert_eq!(
            stream_error_data(&second.chunk)
                .pointer("/error/message")
                .and_then(Value::as_str),
            Some("upstream idle")
        );
    }

    #[test]
    fn sse_rewriter_preserves_events_before_bare_error() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 1024);

        let output = rewriter.rewrite_chunk(Bytes::from_static(
            b"event: ping\ndata: {\"type\":\"ping\"}\n\n{\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"failed\"}}\n",
        ));
        let text = std::str::from_utf8(&output.chunk).expect("utf-8 frame");

        assert!(output.bare_error);
        assert!(text.starts_with("event: ping\ndata: {\"type\":\"ping\"}\n\n"));
        assert!(text.ends_with("event: error\ndata: {\"error\":{\"message\":\"failed\",\"type\":\"api_error\"},\"type\":\"error\"}\n\n"));
    }

    #[test]
    fn sse_rewriter_does_not_normalize_non_error_json() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 1024);

        let output = rewriter.rewrite_chunk(Bytes::from_static(
            br#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}
"#,
        ));

        assert!(!output.bare_error);
        assert_eq!(
            output.chunk,
            Bytes::from_static(
                br#"{"type":"message_delta","delta":{"stop_reason":"end_turn"}}
"#
            )
        );
    }

    #[test]
    fn sse_rewriter_does_not_normalize_error_metadata_on_non_error_type() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 1024);

        let output = rewriter.rewrite_chunk(Bytes::from_static(
            b"{\"type\":\"message_delta\",\"error\":{\"message\":\"metadata only\"}}\n",
        ));

        assert!(!output.bare_error);
        assert_eq!(
            output.chunk,
            Bytes::from_static(
                b"{\"type\":\"message_delta\",\"error\":{\"message\":\"metadata only\"}}\n"
            )
        );
    }

    #[test]
    fn sse_rewriter_recovers_after_oversized_line() {
        let mut rewriter = SseStreamRewriter::new(None, "/v1/messages", 16);

        let oversized = rewriter.rewrite_chunk(Bytes::from_static(b"abcdefghijklmnopq"));
        let error = rewriter.rewrite_chunk(Bytes::from_static(
            b"rst\n{\"type\":\"error\",\"error\":{\"message\":\"failed\"}}\n",
        ));

        assert_eq!(oversized.chunk, Bytes::from_static(b"abcdefghijklmnopq"));
        assert!(!oversized.bare_error);
        assert!(error.bare_error);
        assert!(std::str::from_utf8(&error.chunk)
            .expect("utf-8 frame")
            .starts_with("rst\nevent: error\n"));
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
    fn stream_usage_parser_preserves_cache_details_from_earlier_events() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"data: {"message":{"usage":{"input_tokens":100,"output_tokens":0,"cache_read_input_tokens":80,"cache_creation":{"ephemeral_5m_input_tokens":12}}}}
data: {"usage":{"input_tokens":0,"output_tokens":20}}
"#,
        );

        let usage = parser.finish().expect("merged usage should be retained");
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cached_input_tokens, Some(80));
        assert_eq!(usage.cache_creation_input_tokens, Some(12));
        assert_eq!(usage.cache_creation_input_tokens_5m, Some(12));
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
    fn stream_usage_parser_detects_openai_responses_completed_event() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: response.completed
data: {"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":3}}}
"#,
        );

        assert!(parser.completed);
    }

    #[test]
    fn stream_usage_parser_detects_openai_responses_completed_data() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"data: {"type":"response.completed","response":{"usage":{"input_tokens":10,"output_tokens":3}}}
"#,
        );

        assert!(parser.completed);
    }

    #[test]
    fn stream_usage_parser_detects_sse_error_event_as_failed_terminal() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: error
data: {"error":{"code":"InvalidParameter","message":"model does not support responses","type":"invalid_request_error"}}
"#,
        );

        assert!(parser.completed);
        assert!(parser.failed);
        assert_eq!(parser.last_event.as_deref(), Some("error"));
        assert_eq!(parser.last_type.as_deref(), None);
        let error = parser.last_error.as_ref().expect("error summary");
        assert_eq!(error.error_code.as_deref(), Some("InvalidParameter"));
        assert_eq!(
            error.error_message.as_deref(),
            Some("model does not support responses")
        );
        assert_eq!(error.error_type.as_deref(), Some("invalid_request_error"));
    }

    #[test]
    fn stream_usage_parser_detects_error_data_type_as_failed_terminal() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"data: {"type":"error","error":{"message":"model does not support responses"}}
"#,
        );

        assert!(parser.completed);
        assert!(parser.failed);
        assert_eq!(parser.last_type.as_deref(), Some("error"));
    }

    #[test]
    fn stream_error_summary_includes_code_type_and_message() {
        let error = StreamErrorSummary::from_sse_data(
            r#"{"type":"error","code":"unsupported_parameter","message":"tools is not supported"}"#,
        );

        assert_eq!(
            error.to_error_summary(),
            "upstream stream ended with SSE error event code=unsupported_parameter type=error: tools is not supported"
        );
    }

    #[test]
    fn stream_error_summary_reads_openai_response_failed_nested_error() {
        let error = StreamErrorSummary::from_sse_data(
            r#"{"type":"response.failed","request_id":"req_456","response":{"id":"resp_123","status":"failed","error":{"code":"rate_limit_exceeded","message":"Concurrency limit exceeded for user, please retry later"}}}"#,
        );

        assert_eq!(error.response_id.as_deref(), Some("resp_123"));
        assert_eq!(error.request_id.as_deref(), Some("req_456"));
        assert_eq!(error.error_code.as_deref(), Some("rate_limit_exceeded"));
        assert_eq!(
            error.error_message.as_deref(),
            Some("Concurrency limit exceeded for user, please retry later")
        );
        assert_eq!(error.error_type.as_deref(), Some("response.failed"));
        assert_eq!(
            error.to_error_summary(),
            "upstream stream ended with SSE error event code=rate_limit_exceeded type=response.failed: Concurrency limit exceeded for user, please retry later"
        );
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

    #[test]
    fn stream_usage_parser_treats_anthropic_ping_as_non_terminal() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: ping
data: {"type":"ping"}
"#,
        );

        assert!(!parser.completed);
        assert_eq!(
            signal_summary(parser.last_event.as_deref(), parser.last_type.as_deref()).as_deref(),
            Some("event:ping data_type:ping")
        );
    }

    #[test]
    fn stream_usage_parser_ignores_downstream_keep_alive_comments() {
        let mut parser = StreamUsageParser::new(1024);
        parser.observe(
            br#"event: message_start
data: {"type":"message_start"}
"#,
        );

        parser.observe(&DOWNSTREAM_STREAM_KEEP_ALIVE);

        assert!(!parser.completed);
        assert_eq!(parser.last_event.as_deref(), Some("message_start"));
        assert_eq!(parser.last_type.as_deref(), Some("message_start"));
    }

    #[test]
    fn stream_usage_parser_detects_openai_responses_incomplete_event() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: response.incomplete
data: {"type":"response.incomplete","response":{"status":"incomplete"}}
"#,
        );

        assert!(parser.completed);
        assert_eq!(parser.last_event.as_deref(), Some("response.incomplete"));
        assert_eq!(parser.last_type.as_deref(), Some("response.incomplete"));
    }

    #[test]
    fn stream_usage_parser_detects_openai_responses_incomplete_data_only() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"data: {"type":"response.incomplete","response":{"status":"incomplete"}}
"#,
        );

        assert!(parser.completed);
        assert_eq!(parser.last_type.as_deref(), Some("response.incomplete"));
    }

    #[test]
    fn stream_usage_parser_records_last_signal_without_terminal() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            br#"event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"hi"}
"#,
        );

        assert!(!parser.completed);
        assert_eq!(
            parser.last_event.as_deref(),
            Some("response.output_text.delta")
        );
        assert_eq!(
            parser.last_type.as_deref(),
            Some("response.output_text.delta")
        );
        assert!(!parser.saw_done);
    }

    #[test]
    fn stream_usage_parser_preserves_signal_before_failure() {
        let mut parser = StreamUsageParser::new(2048);

        parser.observe(
            br#"event: response.output_text.delta
data: {"type":"response.output_text.delta","delta":"hi"}
event: response.failed
data: {"type":"response.failed","response":{"id":"resp_123","error":{"code":"upstream_error","message":"failed"}}}
"#,
        );

        assert!(parser.failed);
        assert_eq!(
            signal_summary(
                parser.previous_event.as_deref(),
                parser.previous_type.as_deref()
            )
            .as_deref(),
            Some("event:response.output_text.delta data_type:response.output_text.delta")
        );
    }

    #[test]
    fn upstream_response_metadata_reads_only_diagnostic_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", "req_123".parse().expect("request id"));
        headers.insert("x-trace-id", "trace_456".parse().expect("trace id"));
        headers.insert("cf-ray", "ray_789".parse().expect("cf ray"));
        headers.insert(
            "server-timing",
            "upstream;dur=123".parse().expect("server timing"),
        );
        headers.insert("set-cookie", "secret=value".parse().expect("cookie"));

        let metadata = UpstreamResponseMetadata::from_headers(&headers);

        assert_eq!(metadata.request_id.as_deref(), Some("req_123"));
        assert_eq!(metadata.trace_id.as_deref(), Some("trace_456"));
        assert_eq!(metadata.cf_ray.as_deref(), Some("ray_789"));
        assert_eq!(metadata.server_timing.as_deref(), Some("upstream;dur=123"));
    }

    #[test]
    fn stream_usage_parser_records_done_signal() {
        let mut parser = StreamUsageParser::new(1024);

        parser.observe(
            b"data: [DONE]
",
        );

        assert!(parser.completed);
        assert!(parser.saw_done);
    }

    #[test]
    fn sse_error_lowered_classifies_bailian_model_unsupported() {
        // 复现阿里云百炼 /v1/responses 流式 SSE error：HTTP 200，错误藏在 event:error 里。
        let summary = StreamErrorSummary::from_sse_data(
            r#"{"code":"InvalidParameter","message":"Unsupported model: 'glm-5.2'.","request_id":"dacd5cbe-1f99-9ee3-a542-8bab2014bfc9"}"#,
        );

        let lowered = sse_error_lowered(&summary);
        assert!(lowered.contains("unsupported model"));
        assert!(is_model_error_text(&lowered));
    }

    #[test]
    fn sse_error_lowered_ignores_generic_invalid_parameter_without_model_keyword() {
        // 仅凭 InvalidParameter 这类通用错误码不应判定为模型不可用，避免把参数错误
        // 误学习成 responses 不支持。
        let summary = StreamErrorSummary::from_sse_data(
            r#"{"error":{"code":"InvalidParameter","message":"missing required field: input","type":"invalid_request_error"}}"#,
        );

        let lowered = sse_error_lowered(&summary);
        assert!(!is_model_error_text(&lowered));
    }

    #[test]
    fn sse_error_lowered_matches_model_not_found_in_code() {
        // 有些上游把模型错误放在 code 字段。
        let summary = StreamErrorSummary::from_sse_data(
            r#"{"error":{"code":"model_not_found","message":"qwen3.6-plus","type":"not_found_error"}}"#,
        );

        let lowered = sse_error_lowered(&summary);
        assert!(is_model_error_text(&lowered));
    }
}
