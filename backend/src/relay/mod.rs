mod affinity;
mod body;
pub(crate) mod bridge;
mod credential;
mod error;
mod limit;
mod models;
mod request;
pub mod selector;
mod streaming;
mod upstream;

use std::{fmt::Write as _, sync::Arc};

use async_compression::tokio::bufread::{
    BrotliDecoder, DeflateDecoder, GzipDecoder, ZlibDecoder, ZstdDecoder,
};
use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use serde_json::Value;
use tokio::io::{AsyncReadExt, BufReader};
use uuid::Uuid;

use crate::provider::adapters::{adapter_for_endpoint, RelayRoute};
use crate::provider::{anthropic, openai};
use crate::{
    auth::UserAuth,
    billing::{
        estimate_input_tokens, estimated_cost_micros, parse_usage_from_bytes, BillingAccounts,
        BillingCharge, DebitHold, Price, TokenUsage,
    },
    cache::InvalidationEvent,
    error::{reqwest_status, AppError, AppResult},
    policy, AppState,
};

use self::selector::{AttemptedUpstream, SelectedUpstream};
use crate::task::{billing as task_billing, upstream as upstream_task};
use crate::usage::{KeyFailure, UsageInsert};
pub(crate) use affinity::{ChannelAffinityCache, ChannelAffinityKey};
pub(crate) use body::RelayBody;
pub use credential::CredentialModelRecorder;
pub(crate) use error::{
    describe_upstream_http_failure, is_model_error_text, FailureCooldown, UpstreamFailureKind,
    UpstreamHttpFailure,
};
pub(crate) use limit::UserRequestLimiter;
use models::{list_anthropic_models, list_openai_models, retrieve_openai_model};
pub(crate) use request::{
    prepare_relay_body, rewrite_relay_body_model, safe_log_label, BodyKind, PreparedRelayBody,
    RelayRequestParams,
};
pub(crate) use streaming::{body_from_bytes, body_from_stream, RelayContext, StreamUsageParser};
pub(crate) use upstream::{
    forward_anthropic, forward_openai, forward_openai_with_content_type, forward_prepared_openai,
    log_relay_upstream_failure, relay_upstream_error, upstream_url,
};
pub(crate) use upstream::{forward_anthropic_bound, forward_openai_bound};
use upstream_task::{UpstreamTask, UpstreamTaskType};

const UPSTREAM_ERROR_BODY_READ_LIMIT: usize = 64 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/models", get(list_openai_models))
        .route("/v1/models/{model_id}", get(retrieve_openai_model))
        .route(
            "/v1/chat/completions",
            post(openai::openai_chat_completions),
        )
        .route("/v1/embeddings", post(openai::openai_embeddings))
        .route("/v1/moderations", post(openai::openai_moderations))
        .route(
            "/v1/audio/transcriptions",
            post(openai::openai_audio_transcriptions),
        )
        .route("/v1/responses", post(openai::openai_responses))
        .route(
            "/v1/responses/compact",
            post(openai::openai_responses_compact),
        )
        .route(
            "/anthropic/v1/responses/compact",
            post(openai::openai_responses_compact),
        )
        .route("/v1/responses/{response_id}", get(openai::openai_response))
        .route(
            "/v1/responses/{response_id}/assets/{index}",
            get(openai::openai_response_asset),
        )
        .route(
            "/v1/responses/{response_id}/input_items",
            get(openai::openai_response_input_items),
        )
        .route(
            "/v1/responses/{response_id}/cancel",
            post(openai::cancel_openai_response),
        )
        .route(
            "/v1/images/generations",
            post(openai::openai_image_generations),
        )
        .route("/v1/images/edits", post(openai::openai_image_edits))
        .route(
            "/v1/images/variations",
            post(openai::openai_image_variations),
        )
        .route("/v1/videos", post(openai::openai_videos))
        .route(
            "/v1/videos/{video_id}/content",
            get(openai::openai_video_content),
        )
        .route("/v1/videos/{video_id}", get(openai::openai_video))
        .route(
            "/anthropic",
            get(anthropic_gateway_probe).head(anthropic_gateway_probe),
        )
        .route("/anthropic/v1/messages/models", get(list_anthropic_models))
        .route("/v1/messages", post(anthropic::anthropic_messages))
        .route(
            "/anthropic/v1/messages",
            post(anthropic::anthropic_messages),
        )
        .route(
            "/v1/messages/count_tokens",
            post(anthropic::anthropic_count_tokens),
        )
        .route(
            "/anthropic/v1/messages/count_tokens",
            post(anthropic::anthropic_count_tokens),
        )
        .route(
            "/v1/messages/batches",
            post(anthropic::create_anthropic_message_batch)
                .get(anthropic::list_anthropic_message_batches),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}",
            get(anthropic::anthropic_message_batch)
                .delete(anthropic::delete_anthropic_message_batch),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}/cancel",
            post(anthropic::cancel_anthropic_message_batch),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}/results",
            get(anthropic::anthropic_message_batch_results),
        )
}

async fn anthropic_gateway_probe() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub(crate) async fn finish_task_json_response(
    state: Arc<AppState>,
    auth: UserAuth,
    task: UpstreamTask,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let status = reqwest_status(upstream_response.status());
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = upstream_response.bytes().await?;
    let body = adapter_for_endpoint(
        &task.provider,
        &task.upstream_base_url,
        task.adapter_hint.as_deref(),
    )
    .normalize_response_body(RelayRoute::Videos, body)?;
    tracing::info!(
        task_id = task.id,
        ?task.task_type,
        upstream_task_id = %task.upstream_task_id,
        upstream_status = status.as_u16(),
        upstream_response = %String::from_utf8_lossy(&body),
        "upstream async task bound response"
    );
    if status.is_success() {
        if let Ok(mut value) = serde_json::from_slice::<Value>(&body) {
            if task.task_type == UpstreamTaskType::OpenAiVideo {
                crate::billing::video::copy_neogate_metadata(&task.upstream_metadata, &mut value);
            }
            let (status_text, terminal) = task_status_from_value(task.task_type, &value, &task);
            let usage = parse_usage_from_bytes(&body, false);
            upstream_task::update_task_from_upstream_value(
                &state.db.pool,
                upstream_task::UpstreamTaskUpdate {
                    task_id: task.id,
                    task_type: task.task_type,
                    upstream_task_id: task.upstream_task_id.clone(),
                    status: status_text,
                    terminal,
                    metadata: value,
                    usage,
                    poll_interval: crate::task::POLL_INTERVAL,
                },
            )
            .await?;
            if terminal {
                task_billing::finalize_for_auth(
                    &state,
                    &auth,
                    &task.upstream_task_id,
                    task.task_type,
                    usage,
                    true,
                )
                .await?;
            }
        }
    }
    response_from_bytes(status, content_type, body)
}

pub(crate) async fn raw_upstream_response(
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let status = reqwest_status(upstream_response.status());
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = upstream_response.bytes().await?;
    response_from_bytes(status, content_type, body)
}

pub(crate) fn response_from_bytes(
    status: StatusCode,
    content_type: HeaderValue,
    body: Bytes,
) -> AppResult<Response> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(body))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

pub(crate) fn task_status_from_value(
    task_type: UpstreamTaskType,
    value: &Value,
    task: &UpstreamTask,
) -> (String, bool) {
    match task_type {
        UpstreamTaskType::OpenAiResponse | UpstreamTaskType::NeogateResponse => {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(&task.status)
                .to_string();
            let terminal = openai::response_terminal(&status);
            (status, terminal)
        }
        UpstreamTaskType::OpenAiVideo => {
            let status = openai::video_status_text(value, &task.status);
            let terminal = openai::video_terminal(&status);
            (status, terminal)
        }
        UpstreamTaskType::AudioTranscription => {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(&task.status)
                .to_string();
            let terminal = matches!(status.as_str(), "completed" | "failed");
            (status, terminal)
        }
        UpstreamTaskType::AnthropicMessageBatch => {
            let status = value
                .get("processing_status")
                .and_then(Value::as_str)
                .unwrap_or(&task.status)
                .to_string();
            let terminal = anthropic::batch_terminal(&status);
            (status, terminal)
        }
    }
}

pub(crate) fn ensure_key_backed_async_upstream(upstream: &SelectedUpstream) -> AppResult<()> {
    if upstream.channel_key_id.is_none() || upstream.credential_id.is_some() {
        return Err(AppError::BadRequest(
            "async tasks require a key-backed upstream channel".to_string(),
        ));
    }
    Ok(())
}

pub(crate) async fn finish_relay(
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
) -> AppResult<Response> {
    match response {
        Ok(upstream_response) => {
            let status = reqwest_status(upstream_response.status());
            if !status.is_success() {
                return handle_upstream_http_error(ctx, status, upstream_response).await;
            }
            let content_type = upstream_response
                .headers()
                .get("content-type")
                .cloned()
                .unwrap_or_else(|| {
                    if ctx.streamed {
                        HeaderValue::from_static("text/event-stream")
                    } else {
                        HeaderValue::from_static("application/json")
                    }
                });
            if should_rewrite_response_model(&ctx, &content_type) {
                let body = upstream_response.bytes().await?;
                let body = rewrite_response_model(body, &ctx.external_model)?;
                return Response::builder()
                    .status(status)
                    .header("content-type", content_type)
                    .body(streaming::body_from_bytes(ctx, status, body))
                    .map_err(|err| AppError::BadRequest(err.to_string()));
            }
            Response::builder()
                .status(status)
                .header("content-type", content_type)
                .body(streaming::body(ctx, status, upstream_response))
                .map_err(|err| AppError::BadRequest(err.to_string()))
        }
        Err(err) => finish_relay_error(ctx, err).await,
    }
}

fn should_rewrite_response_model(ctx: &RelayContext, content_type: &HeaderValue) -> bool {
    !ctx.streamed
        && ctx.external_model != ctx.model
        && content_type
            .to_str()
            .is_ok_and(|value| value.to_ascii_lowercase().starts_with("application/json"))
}

fn rewrite_response_model(body: Bytes, external_model: &str) -> AppResult<Bytes> {
    let mut value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => return Ok(body),
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(body);
    };
    if object.contains_key("model") {
        object.insert(
            "model".to_string(),
            Value::String(external_model.to_string()),
        );
        return Ok(Bytes::from(serde_json::to_vec(&value)?));
    }
    Ok(body)
}

async fn finish_relay_error(mut ctx: RelayContext, err: AppError) -> AppResult<Response> {
    let err = relay_upstream_error(&ctx, err);
    log_relay_upstream_failure(&ctx, &err);
    ctx.release_request_permit();
    record_relay_transport_failure(&ctx, err.to_string(), "failed relay").await;
    Err(err)
}

/// 记录传输层或前置失败的 relay 请求：释放预扣额度、记录失败 key 冷却并入队 usage。
/// 仅用于无上游 HTTP 状态码的场景（status_code = None）；有状态码时使用
/// `record_upstream_http_failure`。
pub(crate) async fn record_relay_transport_failure(
    ctx: &RelayContext,
    summary: String,
    release_context: &str,
) {
    let usage = usage_from_context(ctx, None, Some(summary.clone()), None, None, None);
    let failure = key_failure_from_context(ctx, summary).await;
    release_empty_hold(&ctx.state, ctx.hold.clone(), release_context).await;
    enqueue_relay_usage(&ctx.state, usage, failure).await;
}

pub(crate) async fn handle_upstream_http_error(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let body = read_upstream_error_body(upstream_response).await;
    let failure = describe_upstream_http_failure(status, &body);
    respond_upstream_http_failure(ctx, status, failure).await
}

pub(crate) async fn respond_upstream_http_failure(
    mut ctx: RelayContext,
    status: StatusCode,
    failure: UpstreamHttpFailure,
) -> AppResult<Response> {
    let client_message = upstream_http_failure_client_message(&ctx, status, &failure);
    let payload = failure.client_payload(ctx.path, &ctx.upstream.provider, status, client_message);
    let client_response = payload.to_string();
    log_upstream_http_failure(&ctx, status, &failure, Some(&client_response));
    ctx.release_request_permit();
    record_upstream_http_failure(&ctx, status, &failure, "upstream error").await;

    let mut builder = Response::builder()
        .status(failure.relay_status())
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-neogate-error-code", failure.error_code())
        .header(
            "x-neogate-retryable",
            if failure.client_retryable() {
                "true"
            } else {
                "false"
            },
        );
    if let Ok(value) = HeaderValue::from_str(&ctx.upstream.provider) {
        builder = builder.header("x-neogate-upstream-provider", value);
    }
    if let Ok(value) = HeaderValue::from_str(&status.as_u16().to_string()) {
        builder = builder.header("x-neogate-upstream-status", value);
    }
    builder
        .body(Body::from(client_response))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

pub(crate) fn log_upstream_http_failure(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
    client_response: Option<&str>,
) {
    let line = format_upstream_http_failure_log(ctx, status, failure, client_response);
    if client_response.is_some() && failure.relay_status().is_server_error() {
        tracing::error!("{line}");
    } else {
        tracing::warn!("{line}");
    }
}

fn upstream_http_failure_client_message(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
) -> String {
    format!(
        "{} Upstream {} returned {}: {}",
        failure.kind.user_message(),
        ctx.upstream.provider,
        status.as_u16(),
        failure.detail
    )
}

fn format_upstream_http_failure_log(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
    client_response: Option<&str>,
) -> String {
    let upstream_path = ctx.upstream_request_path.as_deref().unwrap_or(ctx.path);
    let response_mode = ctx.upstream_response_mode.unwrap_or("passthrough");
    let mut line = format!(
        "upstream returned error | trace={} channel={}({}) endpoint={} key={} credential={} provider={} protocol={} model={} path={} upstream_path={} response_mode={} upstream={} upstream_status={} relay_status={} latency={}ms streamed={} request_body_bytes={} request_input_tokens_estimate={} error_type={} retryable={} error={} client_response={}",
        short_trace_id(ctx.relay_trace_id),
        ctx.upstream.channel_name,
        ctx.upstream.channel_id,
        ctx.upstream.channel_endpoint_id,
        optional_id(ctx.upstream.channel_key_id),
        optional_id(ctx.upstream.credential_id),
        ctx.upstream.provider,
        ctx.protocol.as_str(),
        ctx.model,
        ctx.path,
        upstream_path,
        response_mode,
        ctx.upstream.base_url,
        status.as_u16(),
        failure.relay_status().as_u16(),
        ctx.started.elapsed().as_millis(),
        ctx.streamed,
        ctx.request_body_bytes,
        ctx.request_input_tokens_estimate,
        failure.error_code(),
        failure.client_retryable(),
        failure.detail,
        client_response.unwrap_or("not_returned_attempting_reroute")
    );
    push_info_request_params(&mut line, &ctx.request_params);
    line
}

pub(crate) async fn record_upstream_http_failure(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
    release_context: &str,
) {
    let usage = usage_from_context(
        ctx,
        Some(status.as_u16() as i32),
        Some(failure.summary.clone()),
        None,
        None,
        None,
    );
    let key_failure =
        key_failure_from_context_with_cooldown(ctx, failure.summary.clone(), failure.cooldown())
            .await;
    release_empty_hold(&ctx.state, ctx.hold.clone(), release_context).await;
    enqueue_relay_usage(&ctx.state, usage, key_failure).await;
}

pub(crate) async fn should_failover_upstream_failure(
    ctx: &RelayContext,
    attempted: &[AttemptedUpstream],
    failoverable: bool,
    failovers: usize,
) -> bool {
    if !failoverable || failovers >= ctx.state.config.relay.max_upstream_failovers {
        return false;
    }
    match ctx
        .state
        .selector
        .has_selectable_upstream_excluding(&ctx.state.db.pool, ctx.protocol, &ctx.model, attempted)
        .await
    {
        Ok(true) => true,
        Ok(false) => {
            tracing::info!(
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                failovers,
                max_failovers = ctx.state.config.relay.max_upstream_failovers,
                "skipping upstream failover because no alternate upstream is selectable"
            );
            false
        }
        Err(err) => {
            tracing::warn!(
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                error = %err,
                "failed to check alternate upstream before failover"
            );
            false
        }
    }
}

pub(crate) async fn record_upstream_transport_failure_for_failover(
    ctx: &RelayContext,
    summary: String,
) {
    record_relay_transport_failure(ctx, summary, "upstream transport failover").await;
}

pub(crate) async fn read_upstream_error_body(upstream_response: reqwest::Response) -> Bytes {
    let status = upstream_response.status();
    let content_length = upstream_response.content_length();
    let headers = upstream_response.headers().clone();
    let content_encoding = header_for_log(&headers, "content-encoding");
    let mut stream = upstream_response.bytes_stream();
    let mut body = Vec::new();
    let mut truncated = false;
    let mut read_err: Option<reqwest::Error> = None;

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if !append_limited_error_body(&mut body, &chunk) {
                    truncated = true;
                    break;
                }
            }
            Err(err) => {
                read_err = Some(err);
                break;
            }
        }
    }

    if let Some(err) = &read_err {
        // reqwest's Kind::Decode Display is a fixed string with no underlying
        // cause, so walk the source chain to surface the real failure (e.g.
        // framing, timeout, or an unexpected gzip stream from an upstream that
        // ignored `accept-encoding: identity`).
        let mut chain = Vec::new();
        let mut cur: Option<&(dyn std::error::Error + 'static)> = Some(err);
        while let Some(e) = cur {
            if let Some(s) = e.source() {
                chain.push(format!("{s}"));
                cur = Some(s);
            } else {
                cur = None;
            }
        }
        tracing::warn!(
            upstream_status = status.as_u16(),
            content_type = header_for_log(&headers, header::CONTENT_TYPE.as_str()),
            content_encoding = %content_encoding,
            transfer_encoding = header_for_log(&headers, "transfer-encoding"),
            content_length,
            bytes_read = body.len(),
            is_decode = err.is_decode(),
            is_body = err.is_body(),
            error = %err,
            source_chain = ?chain,
            "failed to read upstream error body; keeping partial bytes"
        );
    }

    if truncated {
        tracing::warn!(
            limit_bytes = UPSTREAM_ERROR_BODY_READ_LIMIT,
            "upstream error body exceeded read limit; truncating"
        );
    }

    // Some upstreams (e.g. jdcloud) ignore `accept-encoding: identity` and
    // return a compressed error body. Decode it in-memory before handing the
    // bytes to JSON parsing; the error body is bounded by
    // UPSTREAM_ERROR_BODY_READ_LIMIT so a full in-memory decode is cheap.
    let decoded = match content_encoding.as_str() {
        "gzip" | "x-gzip" => decode_all(GzipDecoder::new(BufReader::new(&body[..]))).await,
        "br" => decode_all(BrotliDecoder::new(BufReader::new(&body[..]))).await,
        "zstd" => decode_all(ZstdDecoder::new(BufReader::new(&body[..]))).await,
        "deflate" => {
            // RFC 7230 defines `deflate` as the zlib format, but some servers
            // emit raw deflate instead. Try zlib first, then fall back to raw.
            let out = decode_all(ZlibDecoder::new(BufReader::new(&body[..]))).await;
            if out.is_empty() {
                decode_all(DeflateDecoder::new(BufReader::new(&body[..]))).await
            } else {
                out
            }
        }
        _ => body.clone(),
    };

    if let (Some(err), true) = (&read_err, decoded.is_empty()) {
        // No usable bytes survived; fall back to a synthetic detail so the
        // client still gets a structured error instead of an empty body.
        return Bytes::from(format!("failed to read upstream error body: {err}"));
    }
    Bytes::from(decoded)
}

async fn decode_all<R: AsyncReadExt + Unpin>(mut decoder: R) -> Vec<u8> {
    let mut out = Vec::new();
    match decoder.read_to_end(&mut out).await {
        Ok(_) => out,
        Err(_) => Vec::new(),
    }
}

fn header_for_log(headers: &reqwest::header::HeaderMap, name: &str) -> String {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn append_limited_error_body(body: &mut Vec<u8>, chunk: &[u8]) -> bool {
    let remaining = UPSTREAM_ERROR_BODY_READ_LIMIT.saturating_sub(body.len());
    if chunk.len() <= remaining {
        body.extend_from_slice(chunk);
        true
    } else {
        body.extend_from_slice(&chunk[..remaining]);
        false
    }
}

pub(crate) fn usage_from_context(
    ctx: &RelayContext,
    status_code: Option<i32>,
    error_summary: Option<String>,
    first_response_ms: Option<i64>,
    token_usage: Option<TokenUsage>,
    billing: Option<BillingCharge>,
) -> UsageInsert {
    let latency_ms = ctx.started.elapsed().as_millis() as i64;
    let output_tokens_per_second = token_usage.and_then(|usage| {
        (latency_ms > 0 && usage.output_tokens > 0)
            .then_some((usage.output_tokens as f64 * 1000.0) / latency_ms as f64)
    });
    let billing_meter = billing
        .as_ref()
        .map_or(ctx.price.billing_meter, |billing| billing.billing_meter);
    let billable_units = billing.as_ref().map_or_else(
        || token_usage.map_or(0, |usage| usage.total_tokens().max(0)),
        |billing| billing.billable_units,
    );
    let usage = UsageInsert {
        user_id: ctx.auth.user_id,
        project_id: ctx.auth.project_id,
        user_key_id: ctx.auth.user_key_id,
        channel_id: ctx.upstream.channel_id,
        channel_key_id: ctx.upstream.channel_key_id,
        credential_id: ctx.upstream.credential_id,
        relay_trace_id: Some(ctx.relay_trace_id),
        relay_attempt: ctx.relay_attempt,
        relay_final: ctx.relay_final,
        model: Some(ctx.external_model.clone()),
        upstream_model: Some(ctx.upstream_model.clone()),
        routing_phase: "relay".to_string(),
        routing: ctx.routing.clone(),
        status_code,
        streamed: ctx.streamed,
        latency_ms,
        first_response_ms,
        output_tokens_per_second,
        error_summary,
        token_usage,
        billing_meter,
        billable_units,
        billing,
    };
    log_relay_request_summary(ctx, &usage);
    usage
}

fn log_relay_request_summary(ctx: &RelayContext, usage: &UsageInsert) {
    let token_usage = usage.token_usage;
    let billing = usage.billing.as_ref();
    let cost_micros = billing.map(|billing| billing.cost_micros);
    let input_tokens = billing
        .and_then(|billing| billing.input_tokens)
        .or_else(|| token_usage.map(|usage| usage.input_tokens));
    let output_tokens = billing
        .and_then(|billing| billing.output_tokens)
        .or_else(|| token_usage.map(|usage| usage.output_tokens));
    let total_tokens = billing
        .and_then(|billing| billing.total_tokens)
        .or_else(|| token_usage.map(TokenUsage::total_tokens));
    let cached_input_tokens = token_usage.and_then(|usage| usage.cached_input_tokens);
    let cache_creation_input_tokens =
        token_usage.and_then(|usage| usage.cache_creation_input_tokens);
    let cache_creation_input_tokens_5m =
        token_usage.and_then(|usage| usage.cache_creation_input_tokens_5m);
    let cache_creation_input_tokens_1h =
        token_usage.and_then(|usage| usage.cache_creation_input_tokens_1h);
    let reasoning_output_tokens = token_usage.and_then(|usage| usage.reasoning_output_tokens);
    let audio_input_tokens = token_usage.and_then(|usage| usage.audio_input_tokens);
    let audio_output_tokens = token_usage.and_then(|usage| usage.audio_output_tokens);
    let (uncached_input_tokens, cache_hit_pct) =
        request_cache_metrics(input_tokens, cached_input_tokens);
    let generation_tokens_per_second =
        generation_tokens_per_second(output_tokens, usage.latency_ms, usage.first_response_ms);
    let upstream_path = ctx.upstream_request_path.as_deref().unwrap_or(ctx.path);
    let response_mode = ctx.upstream_response_mode.unwrap_or("passthrough");
    let responses_chat_fallback = response_mode == "openai_chat_as_openai_response";

    let mut info = String::from("relay request");
    push_field(&mut info, "trace", short_trace_id(ctx.relay_trace_id));
    push_field(&mut info, "path", ctx.path);
    push_field(&mut info, "upstream_path", upstream_path);
    push_field(&mut info, "response_mode", response_mode);
    push_field(
        &mut info,
        "responses_chat_fallback",
        responses_chat_fallback,
    );
    push_field(&mut info, "user", usage.user_id);
    push_field(&mut info, "key", usage.user_key_id);
    push_field(&mut info, "channel", usage.channel_id);
    push_opt(
        &mut info,
        "affinity",
        ctx.upstream
            .affinity
            .as_ref()
            .map(|affinity| affinity.status.as_str()),
    );
    push_field(
        &mut info,
        "model",
        usage.model.as_deref().unwrap_or(&ctx.model),
    );
    if ctx.upstream_model != usage.model.as_deref().unwrap_or(&ctx.model) {
        push_field(&mut info, "upstream_model", &ctx.upstream_model);
    }
    push_opt(&mut info, "status", usage.status_code);
    push_field(&mut info, "latency_ms", usage.latency_ms);
    push_opt(&mut info, "first_ms", usage.first_response_ms);
    push_opt(&mut info, "in", input_tokens);
    push_opt(&mut info, "uncached_in", uncached_input_tokens);
    push_opt(&mut info, "cache_read", cached_input_tokens);
    push_opt(&mut info, "cache_write", cache_creation_input_tokens);
    push_opt_f64(&mut info, "cache_hit_pct", cache_hit_pct);
    push_opt(&mut info, "out", output_tokens);
    push_opt(&mut info, "reasoning", reasoning_output_tokens);
    push_opt_f64(&mut info, "gen_tps", generation_tokens_per_second);
    push_opt(&mut info, "cost_micros", cost_micros);
    push_info_request_params(&mut info, &ctx.request_params);
    if usage.relay_attempt > 1 {
        push_field(&mut info, "attempt", usage.relay_attempt);
    }
    if !usage.relay_final {
        push_field(&mut info, "final", false);
    }
    if let Some(error) = usage
        .error_summary
        .as_deref()
        .filter(|error| !error.is_empty())
    {
        push_field(&mut info, "error", error);
    }
    tracing::info!("{info}");

    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }

    let mut detail = String::from("relay request detail");
    push_field(&mut detail, "relay_trace_id", ctx.relay_trace_id);
    push_field(&mut detail, "path", ctx.path);
    push_field(&mut detail, "upstream_path", upstream_path);
    push_field(&mut detail, "response_mode", response_mode);
    push_field(
        &mut detail,
        "responses_chat_fallback",
        responses_chat_fallback,
    );
    push_field(&mut detail, "user_id", usage.user_id);
    push_field(&mut detail, "project_id", usage.project_id);
    push_field(&mut detail, "user_key_id", usage.user_key_id);
    push_field(&mut detail, "provider", &ctx.upstream.provider);
    push_field(&mut detail, "protocol", ctx.protocol.as_str());
    push_field(
        &mut detail,
        "model",
        usage.model.as_deref().unwrap_or(&ctx.model),
    );
    push_field(&mut detail, "external_model", &ctx.external_model);
    push_field(&mut detail, "upstream_model", &ctx.upstream_model);
    push_field(&mut detail, "channel_id", usage.channel_id);
    push_field(&mut detail, "channel_name", &ctx.upstream.channel_name);
    push_opt(
        &mut detail,
        "affinity_status",
        ctx.upstream
            .affinity
            .as_ref()
            .map(|affinity| affinity.status.as_str()),
    );
    push_opt(
        &mut detail,
        "affinity_key_fp",
        ctx.upstream
            .affinity
            .as_ref()
            .map(|affinity| affinity.key_fingerprint.as_str()),
    );
    push_field(
        &mut detail,
        "channel_endpoint_id",
        ctx.upstream.channel_endpoint_id,
    );
    push_opt(&mut detail, "channel_key_id", usage.channel_key_id);
    push_opt(&mut detail, "credential_id", usage.credential_id);
    push_field(&mut detail, "upstream", &ctx.upstream.base_url);
    push_opt(&mut detail, "status", usage.status_code);
    push_field(&mut detail, "streamed", usage.streamed);
    push_field(&mut detail, "relay_attempt", usage.relay_attempt);
    push_field(&mut detail, "relay_final", usage.relay_final);
    push_field(&mut detail, "latency_ms", usage.latency_ms);
    push_opt(&mut detail, "first_response_ms", usage.first_response_ms);
    push_opt_f64(
        &mut detail,
        "output_tokens_per_second",
        usage.output_tokens_per_second,
    );

    push_request_params(&mut detail, &ctx.request_params);
    push_opt(&mut detail, "input_tokens", input_tokens);
    push_opt(&mut detail, "output_tokens", output_tokens);
    push_opt(&mut detail, "total_tokens", total_tokens);
    push_opt(&mut detail, "cached_input_tokens", cached_input_tokens);
    push_opt(
        &mut detail,
        "cache_creation_input_tokens",
        cache_creation_input_tokens,
    );
    push_opt(
        &mut detail,
        "cache_creation_input_tokens_5m",
        cache_creation_input_tokens_5m,
    );
    push_opt(
        &mut detail,
        "cache_creation_input_tokens_1h",
        cache_creation_input_tokens_1h,
    );
    push_opt(
        &mut detail,
        "reasoning_output_tokens",
        reasoning_output_tokens,
    );
    push_opt(&mut detail, "audio_input_tokens", audio_input_tokens);
    push_opt(&mut detail, "audio_output_tokens", audio_output_tokens);
    if billing.is_some() || usage.billable_units > 0 {
        push_field(&mut detail, "billing_meter", usage.billing_meter.as_str());
        push_field(&mut detail, "billable_units", usage.billable_units);
    }
    push_opt(&mut detail, "cost_micros", cost_micros);
    if let Some(status) = billing.map(|billing| billing.status.as_str()) {
        push_field(&mut detail, "billing_status", status);
    }
    if let Some(error) = usage
        .error_summary
        .as_deref()
        .filter(|error| !error.is_empty())
    {
        push_field(&mut detail, "error", error);
    }

    tracing::debug!("{detail}");
}

fn request_cache_metrics(
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
) -> (Option<i64>, Option<f64>) {
    let Some((input_tokens, cached_input_tokens)) = input_tokens.zip(cached_input_tokens) else {
        return (None, None);
    };
    if input_tokens <= 0 {
        return (None, None);
    }

    let cached_input_tokens = cached_input_tokens.clamp(0, input_tokens);
    (
        Some(input_tokens - cached_input_tokens),
        Some((cached_input_tokens as f64 * 100.0) / input_tokens as f64),
    )
}

fn generation_tokens_per_second(
    output_tokens: Option<i64>,
    latency_ms: i64,
    first_response_ms: Option<i64>,
) -> Option<f64> {
    let output_tokens = output_tokens?;
    let generation_ms = latency_ms.saturating_sub(first_response_ms?);
    (output_tokens > 0 && generation_ms > 0)
        .then_some((output_tokens as f64 * 1000.0) / generation_ms as f64)
}

fn push_request_params(line: &mut String, params: &RelayRequestParams) {
    push_opt(line, "request_max_tokens", params.max_tokens);
    push_opt_f64(line, "request_temperature", params.temperature);
    push_opt_f64(line, "request_top_p", params.top_p);
    push_opt_str(
        line,
        "request_reasoning_effort",
        params.reasoning_effort.as_deref(),
    );
    push_opt(
        line,
        "request_reasoning_max_tokens",
        params.reasoning_max_tokens,
    );
    push_opt(line, "request_tool_count", params.tool_count);
    push_opt_str(line, "request_tool_choice", params.tool_choice.as_deref());
    push_opt_str(
        line,
        "request_response_format",
        params.response_format.as_deref(),
    );
    push_opt(
        line,
        "request_parallel_tool_calls",
        params.parallel_tool_calls,
    );
    push_opt(line, "request_store", params.store);
    push_opt(line, "request_background", params.background);
    push_opt(line, "request_image_count", params.image_count);
    push_opt_str(line, "request_image_size", params.image_size.as_deref());
    push_opt_str(
        line,
        "request_image_quality",
        params.image_quality.as_deref(),
    );
    push_opt_str(line, "request_image_style", params.image_style.as_deref());
    push_opt_str(line, "request_video_size", params.video_size.as_deref());
    push_opt(line, "request_video_seconds", params.video_seconds);
}

fn push_info_request_params(line: &mut String, params: &RelayRequestParams) {
    push_opt_str(line, "effort", params.reasoning_effort.as_deref());
    push_opt(line, "tools", params.tool_count);
    push_opt_str(line, "tool_choice", params.tool_choice.as_deref());
}

fn short_trace_id(trace_id: Uuid) -> String {
    trace_id
        .as_hyphenated()
        .to_string()
        .chars()
        .take(8)
        .collect()
}

fn push_field(line: &mut String, key: &str, value: impl std::fmt::Display) {
    let _ = write!(line, " {key}={value}");
}

fn push_opt<T: std::fmt::Display>(line: &mut String, key: &str, value: Option<T>) {
    if let Some(value) = value {
        push_field(line, key, value);
    }
}

fn push_opt_str(line: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        push_field(line, key, value);
    }
}

fn push_opt_f64(line: &mut String, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        let _ = write!(line, " {key}={value:.2}");
    }
}

pub(crate) async fn key_failure_from_context(
    ctx: &RelayContext,
    error: String,
) -> Option<KeyFailure> {
    key_failure_from_context_with_cooldown(ctx, error, FailureCooldown::Default).await
}

async fn key_failure_from_context_with_cooldown(
    ctx: &RelayContext,
    error: String,
    cooldown: FailureCooldown,
) -> Option<KeyFailure> {
    if cooldown == FailureCooldown::None {
        return None;
    }
    let channel_key_id = ctx.upstream.channel_key_id?;
    let attempted = [AttemptedUpstream::from(&ctx.upstream)];
    match ctx
        .state
        .selector
        .has_selectable_upstream_excluding(&ctx.state.db.pool, ctx.protocol, &ctx.model, &attempted)
        .await
    {
        Ok(true) => Some(KeyFailure {
            channel_key_id,
            cooldown_until: key_cooldown_until(&ctx.state, cooldown),
            error,
        }),
        Ok(false) => {
            tracing::info!("{}", format_skipped_key_cooldown_log(ctx, channel_key_id));
            None
        }
        Err(err) => {
            tracing::warn!(
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                error = %err,
                "failed to check alternate upstream channel; applying key cooldown"
            );
            Some(KeyFailure {
                channel_key_id,
                cooldown_until: key_cooldown_until(&ctx.state, cooldown),
                error,
            })
        }
    }
}

fn format_skipped_key_cooldown_log(ctx: &RelayContext, channel_key_id: i64) -> String {
    format!(
        "skipped upstream key cooldown | channel={}({}) endpoint={} key={} credential={} provider={} protocol={} model={} reason=no_alternate_channel_for_model",
        ctx.upstream.channel_name,
        ctx.upstream.channel_id,
        ctx.upstream.channel_endpoint_id,
        channel_key_id,
        optional_id(ctx.upstream.credential_id),
        ctx.upstream.provider,
        ctx.protocol.as_str(),
        ctx.model
    )
}

fn optional_id(id: Option<i64>) -> String {
    id.map_or_else(|| "none".to_string(), |id| id.to_string())
}

pub(crate) async fn release_empty_hold(state: &AppState, hold: DebitHold, context: &str) {
    if let Err(err) = state.billing.release_hold(&state.db.pool, hold).await {
        tracing::warn!("failed to release {context} hold: {err}");
    }
}

pub(crate) async fn reserve_credit(
    state: &AppState,
    auth: &UserAuth,
    user_key_model_credit_account: Option<&crate::billing::CreditAccountId>,
    body: &[u8],
    output_tokens: i64,
    price: &Price,
) -> AppResult<DebitHold> {
    let input_tokens = estimate_input_tokens(body);
    let estimated = estimated_cost_micros(input_tokens, output_tokens, price);
    if !policy::credit_required(state).await? {
        return Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micros: estimated,
            parts: Vec::new(),
            charge_credit: false,
        });
    }
    state
        .billing
        .reserve(
            &state.db.pool,
            BillingAccounts {
                user_id: auth.user_id,
                project_id: auth.project_id,
                user_key_id: auth.user_key_id,
                user_key_model_credit_account,
                user_key_credit_account: &auth.user_key_credit_account,
                project_credit_account: &auth.project_credit_account,
            },
            estimated,
        )
        .await
}

pub(crate) async fn reserve_billable_credit(
    state: &AppState,
    auth: &UserAuth,
    user_key_model_credit_account: Option<&crate::billing::CreditAccountId>,
    estimated_micros: i64,
) -> AppResult<DebitHold> {
    let estimated = estimated_micros.max(0);
    if !policy::credit_required(state).await? {
        return Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micros: estimated,
            parts: Vec::new(),
            charge_credit: false,
        });
    }
    state
        .billing
        .reserve(
            &state.db.pool,
            BillingAccounts {
                user_id: auth.user_id,
                project_id: auth.project_id,
                user_key_id: auth.user_key_id,
                user_key_model_credit_account,
                user_key_credit_account: &auth.user_key_credit_account,
                project_credit_account: &auth.project_credit_account,
            },
            estimated,
        )
        .await
}

fn key_cooldown_until(state: &AppState, policy: FailureCooldown) -> chrono::DateTime<Utc> {
    let duration = match policy {
        FailureCooldown::None => return Utc::now(),
        FailureCooldown::Default => state.config.relay.key_cooldown,
        FailureCooldown::QuotaExhausted => state.config.relay.quota_exhausted_cooldown,
    };
    let cooldown =
        ChronoDuration::from_std(duration).unwrap_or_else(|_| ChronoDuration::seconds(60));
    Utc::now() + cooldown
}

pub(crate) async fn enqueue_relay_usage(
    state: &AppState,
    item: UsageInsert,
    failure: Option<KeyFailure>,
) {
    if let Some(failure) = &failure {
        state
            .cache_invalidator
            .invalidate(
                state,
                InvalidationEvent::ChannelKeyCooldown {
                    id: failure.channel_key_id,
                    cooldown_until: failure.cooldown_until,
                },
            )
            .await;
    }
    if item.billing.is_some() {
        state.billing_outbox.enqueue_or_retry(item);
        return;
    }
    if let Err(err) = state.usage.enqueue(item, failure).await {
        tracing::warn!("failed to enqueue relay usage: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_length_failure_uses_openai_compatible_error_shape() {
        let failure = describe_upstream_http_failure(
            StatusCode::BAD_GATEWAY,
            br#"{"error":{"message":"Your input exceeds the context window"}}"#,
        );
        let payload = failure.client_payload(
            "/v1/responses",
            "newapi",
            StatusCode::BAD_GATEWAY,
            "The request is too large".to_string(),
        );

        assert_eq!(payload["error"]["code"], "context_length_exceeded");
        assert_eq!(payload["error"]["type"], "invalid_request_error");
        assert_eq!(payload["error"]["param"], "input");
        assert!(payload["error"].get("upstream").is_none());
        assert!(payload["error"].get("retryable").is_none());
    }

    #[test]
    fn anthropic_failure_uses_anthropic_error_shape() {
        let failure = describe_upstream_http_failure(
            StatusCode::FORBIDDEN,
            br#"{"error":{"message":"quota exhausted","type":"insufficient_quota"}}"#,
        );
        let payload = failure.client_payload(
            "/v1/messages",
            "moonshot",
            StatusCode::FORBIDDEN,
            "The upstream quota is exhausted".to_string(),
        );

        assert_eq!(payload["type"], "error");
        assert_eq!(payload["error"]["type"], "invalid_request_error");
        assert_eq!(
            payload["error"]["message"],
            "The upstream quota is exhausted"
        );
        assert!(payload["error"].get("code").is_none());
        assert!(payload["error"].get("retryable").is_none());
    }

    #[test]
    fn upstream_error_body_buffer_is_bounded() {
        let mut body = Vec::new();
        let chunk = vec![b'a'; UPSTREAM_ERROR_BODY_READ_LIMIT + 1];

        assert!(!append_limited_error_body(&mut body, &chunk));

        assert_eq!(body.len(), UPSTREAM_ERROR_BODY_READ_LIMIT);
    }

    #[test]
    fn upstream_error_body_buffer_accepts_exact_limit() {
        let mut body = Vec::new();
        let chunk = vec![b'a'; UPSTREAM_ERROR_BODY_READ_LIMIT];

        assert!(append_limited_error_body(&mut body, &chunk));

        assert_eq!(body.len(), UPSTREAM_ERROR_BODY_READ_LIMIT);
    }

    #[tokio::test]
    async fn decode_all_decompresses_gzip_error_body() {
        use async_compression::tokio::write::GzipEncoder;
        use tokio::io::AsyncWriteExt;

        let original = br#"{"error":{"message":"model not found","code":406}}"#;
        let mut encoder = GzipEncoder::new(Vec::new());
        encoder.write_all(original).await.unwrap();
        encoder.shutdown().await.unwrap();
        let compressed: Vec<u8> = encoder.into_inner();

        let decoded = decode_all(GzipDecoder::new(BufReader::new(&compressed[..]))).await;
        assert_eq!(decoded, original);
    }

    #[tokio::test]
    async fn decode_all_returns_empty_for_corrupt_input() {
        let corrupt = b"not a gzip stream at all";
        let decoded = decode_all(GzipDecoder::new(BufReader::new(&corrupt[..]))).await;
        assert!(decoded.is_empty());
    }

    #[test]
    fn derives_cache_metrics_from_input_usage() {
        let (uncached_input, cache_hit_pct) = request_cache_metrics(Some(327_027), Some(325_376));

        assert_eq!(uncached_input, Some(1_651));
        assert!((cache_hit_pct.unwrap() - 99.495).abs() < 0.001);
    }

    #[test]
    fn clamps_invalid_cache_usage_and_omits_missing_values() {
        assert_eq!(
            request_cache_metrics(Some(10), Some(15)),
            (Some(0), Some(100.0))
        );
        assert_eq!(request_cache_metrics(Some(10), None), (None, None));
        assert_eq!(request_cache_metrics(Some(0), Some(0)), (None, None));
    }

    #[test]
    fn calculates_generation_rate_after_first_response() {
        let generation_rate = generation_tokens_per_second(Some(106), 7_864, Some(5_565));
        assert!((generation_rate.unwrap() - 46.107).abs() < 0.001);
        assert_eq!(generation_tokens_per_second(Some(106), 7_864, None), None);
    }
}
