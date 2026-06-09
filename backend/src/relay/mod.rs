mod anthropic;
mod body;
mod credential;
mod error;
mod models;
mod openai;
mod request;
pub mod selector;
mod streaming;
mod upstream;

use std::sync::Arc;

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

use self::selector::SelectedUpstream;
use self::streaming::RelayContext;
use crate::task::{billing as task_billing, upstream as upstream_task};
use crate::usage::{KeyFailure, UsageInsert};
pub use credential::CredentialModelRecorder;
use error::{describe_upstream_http_failure, UpstreamHttpFailure};
use models::{list_anthropic_models, list_openai_models};
pub(crate) use upstream::upstream_url;
pub(in crate::relay) use upstream::{
    forward_anthropic, forward_openai, log_relay_upstream_failure, relay_upstream_error,
};
pub(crate) use upstream::{forward_anthropic_bound, forward_openai_bound};
use upstream_task::{UpstreamTask, UpstreamTaskType};

const UPSTREAM_ERROR_BODY_READ_LIMIT: usize = 64 * 1024;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/models", get(list_openai_models))
        .route(
            "/v1/chat/completions",
            post(openai::openai_chat_completions),
        )
        .route("/v1/responses", post(openai::openai_responses))
        .route("/v1/responses/{response_id}", get(openai::openai_response))
        .route(
            "/v1/responses/{response_id}/cancel",
            post(openai::cancel_openai_response),
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

pub(in crate::relay) async fn finish_task_json_response(
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
                    poll_interval: state.config.task_upstream_poll_interval,
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

pub(in crate::relay) async fn raw_upstream_response(
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

pub(in crate::relay) fn response_from_bytes(
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

pub(in crate::relay) fn task_status_from_value(
    task_type: UpstreamTaskType,
    value: &Value,
    task: &UpstreamTask,
) -> (String, bool) {
    match task_type {
        UpstreamTaskType::OpenAiResponse => {
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

pub(in crate::relay) fn ensure_key_backed_async_upstream(
    upstream: &SelectedUpstream,
) -> AppResult<()> {
    if upstream.channel_key_id.is_none() || upstream.credential_id.is_some() {
        return Err(AppError::BadRequest(
            "async tasks require a key-backed upstream channel".to_string(),
        ));
    }
    Ok(())
}

pub(in crate::relay) async fn finish_relay(
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
        Err(err) => {
            let err = relay_upstream_error(&ctx, err);
            let summary = err.to_string();
            log_relay_upstream_failure(&ctx, &err);
            let usage = usage_from_context(&ctx, None, Some(summary.clone()), None, None, None);
            let failure = key_failure_from_context(&ctx, summary);
            release_empty_hold(&ctx.state, ctx.hold.clone(), "failed relay").await;
            enqueue_relay_usage(&ctx.state, usage, failure).await;
            Err(err)
        }
    }
}

pub(in crate::relay) async fn handle_upstream_http_error(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let body = read_upstream_error_body(upstream_response).await;
    let failure = describe_upstream_http_failure(status, &body);
    respond_upstream_http_failure(ctx, status, failure).await
}

pub(in crate::relay) async fn respond_upstream_http_failure(
    ctx: RelayContext,
    status: StatusCode,
    failure: UpstreamHttpFailure,
) -> AppResult<Response> {
    log_upstream_http_failure(&ctx, status, &failure);
    record_upstream_http_failure(&ctx, status, &failure, "upstream error").await;

    let payload = json!({
        "error": {
            "message": failure.user_message,
            "code": failure.error_type,
            "upstream": ctx.upstream.provider,
            "upstream_status": status.as_u16(),
            "retryable": failure.retryable,
        }
    });
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
        .body(Body::from(payload.to_string()))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

pub(in crate::relay) fn log_upstream_http_failure(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
) {
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
        upstream_status = status.as_u16(),
        upstream_error_type = failure.error_type,
        retryable = failure.retryable,
        latency_ms = ctx.started.elapsed().as_millis() as i64,
        upstream_error = %failure.detail,
        "upstream returned error response"
    );
}

pub(in crate::relay) async fn record_upstream_http_failure(
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
    let key_failure = key_failure_from_context(ctx, failure.summary.clone());
    release_empty_hold(&ctx.state, ctx.hold.clone(), release_context).await;
    enqueue_relay_usage(&ctx.state, usage, key_failure).await;
}

pub(in crate::relay) async fn read_upstream_error_body(
    upstream_response: reqwest::Response,
) -> Bytes {
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

pub(in crate::relay) fn usage_from_context(
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
    UsageInsert {
        user_id: ctx.auth.user_id,
        user_key_id: ctx.auth.user_key_id,
        channel_id: ctx.upstream.channel_id,
        channel_key_id: ctx.upstream.channel_key_id,
        credential_id: ctx.upstream.credential_id,
        provider: ctx.upstream.provider.clone(),
        model: Some(ctx.model.clone()),
        status_code,
        streamed: ctx.streamed,
        latency_ms,
        first_response_ms,
        output_tokens_per_second,
        error_summary,
        token_usage,
        billing,
    }
}

pub(in crate::relay) fn key_failure_from_context(
    ctx: &RelayContext,
    error: String,
) -> Option<KeyFailure> {
    ctx.upstream
        .channel_key_id
        .map(|channel_key_id| KeyFailure {
            channel_key_id,
            cooldown_until: key_cooldown_until(&ctx.state),
            error,
        })
}

pub(in crate::relay) async fn release_empty_hold(state: &AppState, hold: DebitHold, context: &str) {
    if let Err(err) = state.billing.release_hold(&state.db.pool, hold).await {
        tracing::warn!("failed to release {context} hold: {err}");
    }
}

pub(in crate::relay) async fn reserve_credit(
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
                user_key_id: auth.user_key_id,
                user_key_model_credit_account,
                user_key_credit_account: &auth.user_key_credit_account,
                user_credit_account: &auth.user_credit_account,
            },
            estimated,
        )
        .await
}

fn key_cooldown_until(state: &AppState) -> chrono::DateTime<Utc> {
    let cooldown = ChronoDuration::from_std(state.config.key_cooldown)
        .unwrap_or_else(|_| ChronoDuration::seconds(60));
    Utc::now() + cooldown
}

async fn enqueue_relay_usage(state: &AppState, item: UsageInsert, failure: Option<KeyFailure>) {
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
