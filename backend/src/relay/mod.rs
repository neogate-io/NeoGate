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
use serde_json::{json, Value};
use uuid::Uuid;

use crate::provider::{anthropic, openai};
use crate::{
    auth::UserAuth,
    billing::{
        estimate_input_tokens, estimated_cost_micro_usd, parse_usage_from_bytes, BillingAccounts,
        BillingCharge, DebitHold, Price, TokenUsage,
    },
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    policy, AppState,
};

use self::selector::{AttemptedUpstream, SelectedUpstream};
use crate::task::{billing as task_billing, upstream as upstream_task};
use crate::usage::{KeyFailure, UsageInsert};
pub(crate) use affinity::{ChannelAffinityCache, ChannelAffinityKey};
pub(crate) use body::RelayBody;
pub use credential::CredentialModelRecorder;
pub(crate) use error::{describe_upstream_http_failure, UpstreamHttpFailure};
pub(crate) use limit::UserRequestLimiter;
use models::{list_anthropic_models, list_openai_models, retrieve_openai_model};
pub(crate) use request::{prepare_relay_body, BodyKind, PreparedRelayBody, RelayRequestParams};
pub(crate) use streaming::{body_from_bytes, body_from_stream, RelayContext};
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
        .route("/v1/responses", post(openai::openai_responses))
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
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = upstream_response.bytes().await?;
    if status.is_success() {
        if let Ok(value) = serde_json::from_slice::<Value>(&body) {
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
                    poll_interval: state.config.task.upstream_poll_interval,
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
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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
            let status = StatusCode::from_u16(upstream_response.status().as_u16())
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
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
            Response::builder()
                .status(status)
                .header("content-type", content_type)
                .body(streaming::body(ctx, status, upstream_response))
                .map_err(|err| AppError::BadRequest(err.to_string()))
        }
        Err(err) => finish_relay_error(ctx, err).await,
    }
}

async fn finish_relay_error(ctx: RelayContext, err: AppError) -> AppResult<Response> {
    let err = relay_upstream_error(&ctx, err);
    let summary = err.to_string();
    log_relay_upstream_failure(&ctx, &err);
    let usage = usage_from_context(&ctx, None, Some(summary.clone()), None, None, None);
    let failure = key_failure_from_context(&ctx, summary).await;
    release_empty_hold(&ctx.state, ctx.hold.clone(), "failed relay").await;
    enqueue_relay_usage(&ctx.state, usage, failure).await;
    Err(err)
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
    ctx: RelayContext,
    status: StatusCode,
    failure: UpstreamHttpFailure,
) -> AppResult<Response> {
    let client_message = upstream_http_failure_client_message(&ctx, status, &failure);
    let payload = json!({
        "error": {
            "message": client_message,
            "code": failure.error_type,
            "upstream": ctx.upstream.provider,
            "upstream_status": status.as_u16(),
            "upstream_error": failure.detail,
            "retryable": failure.retryable,
        }
    });
    let client_response = payload.to_string();
    log_upstream_http_failure(&ctx, status, &failure, Some(&client_response));
    record_upstream_http_failure(&ctx, status, &failure, "upstream error").await;

    let mut builder = Response::builder()
        .status(failure.relay_status)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-neogate-error-code", failure.error_type)
        .header(
            "x-neogate-retryable",
            if failure.retryable { "true" } else { "false" },
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
    if client_response.is_some() && failure.relay_status.is_server_error() {
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
        failure.user_message,
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
    format!(
        "upstream returned error | channel={}({}) endpoint={} key={} credential={} provider={} protocol={} model={} path={} upstream={} upstream_status={} relay_status={} latency={}ms error_type={} retryable={} error={} client_response={}",
        ctx.upstream.channel_name,
        ctx.upstream.channel_id,
        ctx.upstream.channel_endpoint_id,
        optional_id(ctx.upstream.channel_key_id),
        optional_id(ctx.upstream.credential_id),
        ctx.upstream.provider,
        ctx.protocol.as_str(),
        ctx.model,
        ctx.path,
        ctx.upstream.base_url,
        status.as_u16(),
        failure.relay_status.as_u16(),
        ctx.started.elapsed().as_millis(),
        failure.error_type,
        failure.retryable,
        failure.detail,
        client_response.unwrap_or("not_returned_attempting_reroute")
    )
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
    let key_failure = key_failure_from_context(ctx, failure.summary.clone()).await;
    release_empty_hold(&ctx.state, ctx.hold.clone(), release_context).await;
    enqueue_relay_usage(&ctx.state, usage, key_failure).await;
}

pub(crate) async fn should_failover_retryable_upstream_failure(
    ctx: &RelayContext,
    attempted: &[AttemptedUpstream],
    retryable: bool,
    failovers: usize,
) -> bool {
    if !retryable || failovers >= ctx.state.config.relay.max_upstream_failovers {
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
    let usage = usage_from_context(ctx, None, Some(summary.clone()), None, None, None);
    let failure = key_failure_from_context(ctx, summary).await;
    release_empty_hold(&ctx.state, ctx.hold.clone(), "upstream transport failover").await;
    enqueue_relay_usage(&ctx.state, usage, failure).await;
}

pub(crate) async fn read_upstream_error_body(upstream_response: reqwest::Response) -> Bytes {
    let mut stream = upstream_response.bytes_stream();
    let mut body = Vec::new();
    let mut truncated = false;

    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => {
                if !append_limited_error_body(&mut body, &chunk) {
                    truncated = true;
                    break;
                }
            }
            Err(err) => {
                return Bytes::from(format!("failed to read upstream error body: {err}"));
            }
        }
    }

    if truncated {
        tracing::warn!(
            limit_bytes = UPSTREAM_ERROR_BODY_READ_LIMIT,
            "upstream error body exceeded read limit; truncating"
        );
    }
    Bytes::from(body)
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
        provider: ctx.upstream.provider.clone(),
        model: Some(ctx.model.clone()),
        status_code,
        streamed: ctx.streamed,
        latency_ms,
        first_response_ms,
        output_tokens_per_second,
        error_summary,
        token_usage,
        billing_meter: billing
            .as_ref()
            .map(|billing| billing.billing_meter)
            .unwrap_or(ctx.price.billing_meter),
        billable_units: billing
            .as_ref()
            .map(|billing| billing.billable_units)
            .unwrap_or_else(|| {
                token_usage
                    .map(|usage| usage.total_tokens().max(0))
                    .unwrap_or(0)
            }),
        billing,
    };
    log_relay_request_summary(ctx, &usage);
    usage
}

fn log_relay_request_summary(ctx: &RelayContext, usage: &UsageInsert) {
    let token_usage = usage.token_usage;
    let billing = usage.billing.as_ref();
    let cost_micro_usd = billing.map(|billing| billing.cost_micro_usd);
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

    let mut info = String::from("relay request");
    push_field(&mut info, "trace", short_trace_id(ctx.relay_trace_id));
    push_field(&mut info, "path", ctx.path);
    push_field(&mut info, "user", usage.user_id);
    push_field(&mut info, "key", usage.user_key_id);
    push_field(
        &mut info,
        "model",
        usage.model.as_deref().unwrap_or(&ctx.model),
    );
    push_field(&mut info, "channel", usage.channel_id);
    push_opt(&mut info, "status", usage.status_code);
    push_field(&mut info, "latency_ms", usage.latency_ms);
    push_opt(&mut info, "first_ms", usage.first_response_ms);
    push_opt(&mut info, "in", input_tokens);
    push_opt(&mut info, "out", output_tokens);
    push_opt(&mut info, "cached", cached_input_tokens);
    push_opt(&mut info, "reasoning", reasoning_output_tokens);
    push_opt(&mut info, "cost", cost_micro_usd);
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
    push_field(&mut detail, "user_id", usage.user_id);
    push_field(&mut detail, "project_id", usage.project_id);
    push_field(&mut detail, "user_key_id", usage.user_key_id);
    push_field(&mut detail, "provider", &usage.provider);
    push_field(&mut detail, "protocol", ctx.protocol.as_str());
    push_field(
        &mut detail,
        "model",
        usage.model.as_deref().unwrap_or(&ctx.model),
    );
    push_field(&mut detail, "channel_id", usage.channel_id);
    push_field(&mut detail, "channel_name", &ctx.upstream.channel_name);
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
    push_opt(&mut detail, "cost_micro_usd", cost_micro_usd);
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
            cooldown_until: key_cooldown_until(&ctx.state),
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
                cooldown_until: key_cooldown_until(&ctx.state),
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
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "none".to_string())
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
    let estimated = estimated_cost_micro_usd(input_tokens, output_tokens, price);
    if !policy::credit_required(state).await? {
        return Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micro_usd: estimated,
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
    estimated_micro_usd: i64,
) -> AppResult<DebitHold> {
    let estimated = estimated_micro_usd.max(0);
    if !policy::credit_required(state).await? {
        return Ok(DebitHold {
            transaction_id: Uuid::new_v4(),
            estimated_micro_usd: estimated,
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

fn key_cooldown_until(state: &AppState) -> chrono::DateTime<Utc> {
    let cooldown = ChronoDuration::from_std(state.config.relay.key_cooldown)
        .unwrap_or_else(|_| ChronoDuration::seconds(60));
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
}
