mod credential;
mod models;
mod request;
pub mod selector;
mod streaming;
mod upstream;
pub(crate) mod usage;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{FromRequest, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use serde_json::{json, Value};

use crate::{
    auth::UserAuth,
    billing::{
        estimate_input_tokens, estimated_cost_micro_usd, BillingCharge, DebitHold, Price,
        TokenUsage,
    },
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    AppState,
};

use self::selector::{SelectedUpstream, UpstreamProtocol};
use self::streaming::RelayContext;
pub use credential::CredentialModelRecorder;
use models::{list_anthropic_models, list_openai_models};
use request::{prepare_relay_body, BodyKind, PreparedRelayBody};
pub(crate) use upstream::upstream_url;
use upstream::{
    forward_anthropic, forward_openai, log_relay_upstream_failure, relay_upstream_error,
};
pub use usage::{ActivityRecorder, UsageDailyRecorder, UsageRecorder};
use usage::{KeyFailure, UsageInsert};

const MODEL_UNAVAILABLE_MAX_REROUTES: usize = 3;
const MODEL_UNAVAILABLE_BLOCK_HOURS: i64 = 12;
const UPSTREAM_ERROR_BODY_LOG_LIMIT: usize = 1000;
const UPSTREAM_ERROR_BODY_READ_LIMIT: usize = 64 * 1024;

struct RelayBody(Bytes);

impl FromRequest<Arc<AppState>> for RelayBody {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &Arc<AppState>) -> Result<Self, Self::Rejection> {
        let method = req.method().clone();
        let path = req.uri().path().to_string();
        let content_length = req
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());

        match Bytes::from_request(req, state).await {
            Ok(body) => Ok(Self(body)),
            Err(rejection) => {
                let status = rejection.status();
                let message = rejection.body_text();
                tracing::warn!(
                    %method,
                    %path,
                    status = status.as_u16(),
                    ?content_length,
                    relay_body_limit_bytes = state.config.relay_body_limit_bytes,
                    rejection = %message,
                    "relay request body rejected"
                );
                if status == StatusCode::PAYLOAD_TOO_LARGE {
                    return Err(AppError::PayloadTooLarge(format!(
                        "request body exceeds {} bytes",
                        state.config.relay_body_limit_bytes
                    )));
                }
                Err(AppError::BadRequest(message))
            }
        }
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/models", get(list_openai_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
        .route("/v1/responses", post(openai_responses))
        .route("/anthropic/v1/messages/models", get(list_anthropic_models))
        .route("/anthropic/v1/messages", post(anthropic_messages))
}

async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(state, auth, body, "/v1/chat/completions").await
}

async fn openai_responses(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(state, auth, body, "/v1/responses").await
}

async fn anthropic_messages(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    let PreparedRelayBody {
        body,
        meta,
        output_tokens,
    } = prepare_relay_body(
        body,
        BodyKind::Anthropic,
        state.billing.default_output_tokens(),
    )?;
    auth.ensure_model_allowed(&meta.model)?;
    let started = Instant::now();
    let upstream = state
        .selector
        .select(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Anthropic,
            &meta.model,
        )
        .await?;
    let price = state
        .billing
        .price_for(
            &state.db.pool,
            &upstream.provider,
            &meta.model,
            &auth.user_group,
        )
        .await?;
    let hold = reserve_credit(&state, &auth, &body, output_tokens, &price).await?;
    let response = forward_anthropic(&state, &headers, &upstream, body).await;
    finish_relay(
        RelayContext {
            state,
            auth,
            upstream,
            protocol: UpstreamProtocol::Anthropic,
            path: "/v1/messages",
            model: meta.model,
            streamed: meta.stream,
            price,
            hold,
            started,
        },
        response,
    )
    .await
}

async fn relay_openai(
    state: Arc<AppState>,
    auth: UserAuth,
    body: Bytes,
    path: &'static str,
) -> AppResult<Response> {
    let body_kind = if path == "/v1/responses" {
        BodyKind::OpenaiResponses
    } else {
        BodyKind::OpenaiChat
    };
    let PreparedRelayBody {
        body,
        meta,
        output_tokens,
    } = prepare_relay_body(body, body_kind, state.billing.default_output_tokens())?;
    auth.ensure_model_allowed(&meta.model)?;

    let mut model_unavailable_reroutes = 0;
    loop {
        let started = Instant::now();
        let (protocol, upstream) = select_openai_upstream(&state, path, &meta.model).await?;
        let price = state
            .billing
            .price_for(
                &state.db.pool,
                &upstream.provider,
                &meta.model,
                &auth.user_group,
            )
            .await?;
        let hold = reserve_credit(&state, &auth, &body, output_tokens, &price).await?;
        let ctx = RelayContext {
            state: Arc::clone(&state),
            auth: auth.clone(),
            upstream,
            protocol,
            path,
            model: meta.model.clone(),
            streamed: meta.stream,
            price,
            hold,
            started,
        };
        let response = forward_openai(&state, &ctx.upstream, protocol, body.clone(), path).await;

        match response {
            Ok(upstream_response) => {
                let status = StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                if status.is_success() {
                    mark_credential_model_available(&ctx);
                    return finish_relay(ctx, Ok(upstream_response)).await;
                }

                let body = read_upstream_error_body(upstream_response).await;
                let failure = describe_upstream_http_failure(status, &body);
                if should_retry_after_model_unavailable(&ctx, &failure) {
                    log_upstream_http_failure(&ctx, status, &failure);
                    mark_credential_model_unavailable(&ctx, status, &failure).await;
                    if model_unavailable_reroutes >= MODEL_UNAVAILABLE_MAX_REROUTES {
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
                            max_reroutes = MODEL_UNAVAILABLE_MAX_REROUTES,
                            "upstream model unavailable reroute limit reached"
                        );
                        return respond_upstream_http_failure(ctx, status, failure).await;
                    }
                    model_unavailable_reroutes += 1;
                    record_upstream_http_failure(
                        &ctx,
                        status,
                        &failure,
                        "upstream model unavailable",
                    )
                    .await;
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
                        reroute_attempt = model_unavailable_reroutes,
                        max_reroutes = MODEL_UNAVAILABLE_MAX_REROUTES,
                        blocked_hours = MODEL_UNAVAILABLE_BLOCK_HOURS,
                        "temporarily blocked upstream model for this credential/key; retrying another upstream"
                    );
                    continue;
                }

                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => return finish_relay(ctx, Err(err)).await,
        }
    }
}

async fn select_openai_upstream(
    state: &AppState,
    path: &'static str,
    model: &str,
) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
    if path == "/v1/responses" {
        match state
            .selector
            .select(
                &state.db.pool,
                &state.secrets,
                UpstreamProtocol::OpenAiOauth,
                model,
            )
            .await
        {
            Ok(upstream) => return Ok((UpstreamProtocol::OpenAiOauth, upstream)),
            Err(AppError::UpstreamUnavailable(_)) => {}
            Err(err) => return Err(err),
        }
    }

    let upstream = state
        .selector
        .select(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            model,
        )
        .await?;
    Ok((UpstreamProtocol::Openai, upstream))
}

async fn finish_relay(
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

struct UpstreamHttpFailure {
    error_type: &'static str,
    user_message: &'static str,
    summary: String,
    detail: String,
    relay_status: StatusCode,
    retryable: bool,
}

async fn handle_upstream_http_error(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let body = read_upstream_error_body(upstream_response).await;
    let failure = describe_upstream_http_failure(status, &body);
    respond_upstream_http_failure(ctx, status, failure).await
}

async fn respond_upstream_http_failure(
    ctx: RelayContext,
    status: StatusCode,
    failure: UpstreamHttpFailure,
) -> AppResult<Response> {
    log_upstream_http_failure(&ctx, status, &failure);
    record_upstream_http_failure(&ctx, status, &failure, "upstream error").await;

    let payload = json!({
        "error": {
            "message": failure.user_message,
            "type": failure.error_type,
            "upstream": ctx.upstream.provider,
            "upstream_status": status.as_u16(),
            "retryable": failure.retryable,
        }
    });
    let mut builder = Response::builder()
        .status(failure.relay_status)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-moligate-error-type", failure.error_type)
        .header(
            "x-moligate-retryable",
            if failure.retryable { "true" } else { "false" },
        );
    if let Ok(value) = HeaderValue::from_str(&ctx.upstream.provider) {
        builder = builder.header("x-moligate-upstream-provider", value);
    }
    if let Ok(value) = HeaderValue::from_str(&status.as_u16().to_string()) {
        builder = builder.header("x-moligate-upstream-status", value);
    }
    builder
        .body(Body::from(payload.to_string()))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

fn log_upstream_http_failure(
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

async fn record_upstream_http_failure(
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

async fn mark_credential_model_unavailable(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &UpstreamHttpFailure,
) {
    let unavailable_until = model_unavailable_until();
    ctx.state
        .selector
        .mark_credential_model_unavailable(
            &ctx.upstream,
            ctx.protocol,
            &ctx.model,
            unavailable_until,
        )
        .await;
    if let Some(credential_id) = ctx.upstream.credential_id {
        ctx.state
            .credential_models
            .record_unavailable(
                credential_id,
                ctx.upstream.channel_endpoint_id,
                &ctx.model,
                unavailable_until,
                &failure.summary,
                status.as_u16() as i32,
            )
            .await;
    }
}

fn mark_credential_model_available(ctx: &RelayContext) {
    if let Some(credential_id) = ctx.upstream.credential_id {
        ctx.state.credential_models.record_available(
            credential_id,
            ctx.upstream.channel_endpoint_id,
            &ctx.model,
        );
    }
}

fn should_retry_after_model_unavailable(ctx: &RelayContext, failure: &UpstreamHttpFailure) -> bool {
    matches!(
        ctx.protocol,
        UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
    ) && failure.error_type == "upstream_model_unavailable"
}

fn model_unavailable_until() -> chrono::DateTime<Utc> {
    Utc::now() + ChronoDuration::hours(MODEL_UNAVAILABLE_BLOCK_HOURS)
}

async fn read_upstream_error_body(upstream_response: reqwest::Response) -> Bytes {
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

fn describe_upstream_http_failure(status: StatusCode, body: &[u8]) -> UpstreamHttpFailure {
    let detail = upstream_error_detail(body);
    let lowered = detail.to_ascii_lowercase();
    if is_quota_or_balance_error(&lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_quota_exhausted",
            "upstream quota exhausted",
            "The upstream provider account has insufficient balance or quota. Please switch to another channel or contact the service administrator.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_rate_limit_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_rate_limited",
            "upstream rate limited",
            "The upstream provider is rate limited. Please retry later or switch to another channel.",
            StatusCode::BAD_GATEWAY,
            true,
        );
    }

    if is_auth_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_authentication_failed",
            "upstream authentication failed",
            "The upstream provider rejected the channel credentials. Please switch to another channel or contact the service administrator.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_model_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_model_unavailable",
            "upstream model unavailable",
            "The upstream provider does not have the requested model available on this channel. Please use another model or switch channels.",
            StatusCode::BAD_GATEWAY,
            false,
        );
    }

    if is_context_length_error(status, &lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_context_length_exceeded",
            "upstream context length exceeded",
            "The request is too large for the upstream model context window. Please shorten the input and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    if is_safety_error(&lowered) {
        return upstream_http_failure(
            status,
            detail,
            "upstream_content_rejected",
            "upstream content rejected",
            "The upstream provider rejected the request content. Please revise the request and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    if status.is_server_error() || status.as_u16() == 529 {
        return upstream_http_failure(
            status,
            detail,
            "upstream_server_error",
            "upstream server error",
            "The upstream provider is temporarily unavailable. Please retry later or switch to another channel.",
            StatusCode::BAD_GATEWAY,
            true,
        );
    }

    if status == StatusCode::BAD_REQUEST {
        return upstream_http_failure(
            status,
            detail,
            "upstream_bad_request",
            "upstream bad request",
            "The upstream provider rejected the request format or parameters. Please check the request and retry.",
            StatusCode::BAD_REQUEST,
            false,
        );
    }

    upstream_http_failure(
        status,
        detail,
        "upstream_http_error",
        "upstream http error",
        "The upstream provider rejected the request. Please retry later or switch to another channel.",
        StatusCode::BAD_GATEWAY,
        false,
    )
}

fn upstream_http_failure(
    status: StatusCode,
    detail: String,
    error_type: &'static str,
    summary_prefix: &'static str,
    user_message: &'static str,
    relay_status: StatusCode,
    retryable: bool,
) -> UpstreamHttpFailure {
    UpstreamHttpFailure {
        error_type,
        user_message,
        summary: format!("{summary_prefix}: status {}; {detail}", status.as_u16()),
        detail,
        relay_status,
        retryable,
    }
}

fn upstream_error_detail(body: &[u8]) -> String {
    let raw = String::from_utf8_lossy(body);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "empty upstream error body".to_string();
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = json_error_field(&value, "message") {
            let mut parts = vec![message];
            if let Some(error_type) = json_error_field(&value, "type") {
                parts.push(format!("type={error_type}"));
            }
            if let Some(code) = json_error_field(&value, "code") {
                parts.push(format!("code={code}"));
            }
            return truncate_for_log(&parts.join("; "));
        }
    }

    truncate_for_log(trimmed)
}

fn json_error_field(value: &Value, field: &str) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get(field))
        .or_else(|| value.get(field))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
}

fn is_quota_or_balance_error(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "insufficient_quota",
            "insufficient quota",
            "exceeded your current quota",
            "quota exceeded",
            "insufficient balance",
            "insufficient credit",
            "not enough credits",
            "credit balance",
            "billing hard limit",
            "billing",
            "余额",
            "额度",
            "欠费",
        ],
    )
}

fn is_rate_limit_error(status: StatusCode, lowered: &str) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS
        || contains_any(
            lowered,
            &[
                "rate_limit_exceeded",
                "rate limit",
                "too many requests",
                "requests per minute",
                "tokens per minute",
                "overloaded_error",
                "overloaded",
                "请求过于频繁",
                "限流",
            ],
        )
}

fn is_auth_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        || contains_any(
            lowered,
            &[
                "invalid_api_key",
                "incorrect api key",
                "invalid api key",
                "expired api key",
                "authentication",
                "unauthorized",
                "permission denied",
                "forbidden",
                "access denied",
                "无效的 api key",
                "未授权",
                "无权限",
            ],
        )
}

fn is_model_error(status: StatusCode, lowered: &str) -> bool {
    status == StatusCode::NOT_FOUND
        || contains_any(
            lowered,
            &[
                "model_not_found",
                "model not found",
                "model_not_available",
                "model is not available",
                "does not exist",
                "doesn't exist",
                "not supported",
                "unsupported model",
                "no such model",
                "模型不存在",
                "模型不可用",
                "不支持的模型",
            ],
        )
}

fn is_context_length_error(status: StatusCode, lowered: &str) -> bool {
    matches!(status, StatusCode::PAYLOAD_TOO_LARGE)
        || contains_any(
            lowered,
            &[
                "context_length_exceeded",
                "maximum context length",
                "context window",
                "too many tokens",
                "input is too long",
                "prompt is too long",
                "tokens exceeds",
                "上下文",
                "输入过长",
                "token 超",
            ],
        )
}

fn is_safety_error(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "content_policy_violation",
            "content policy",
            "safety",
            "moderation",
            "blocked",
            "sensitive content",
            "unsafe content",
            "内容安全",
            "安全策略",
            "敏感内容",
        ],
    )
}

fn contains_any(value: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| value.contains(needle))
}

fn truncate_for_log(value: &str) -> String {
    value.chars().take(UPSTREAM_ERROR_BODY_LOG_LIMIT).collect()
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

async fn reserve_credit(
    state: &AppState,
    auth: &UserAuth,
    body: &[u8],
    output_tokens: i64,
    price: &Price,
) -> AppResult<DebitHold> {
    let input_tokens = estimate_input_tokens(body);
    let estimated = estimated_cost_micro_usd(input_tokens, output_tokens, price);
    state
        .billing
        .reserve(
            &state.db.pool,
            auth.user_id,
            auth.user_key_id,
            &auth.user_key_wallet,
            &auth.user_wallet,
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
    fn upstream_http_failure_detects_openai_quota_errors() {
        let body = br#"{
            "error": {
                "message": "You exceeded your current quota, please check your plan and billing details.",
                "type": "insufficient_quota",
                "code": "insufficient_quota"
            }
        }"#;

        let failure = describe_upstream_http_failure(StatusCode::FORBIDDEN, body);

        assert_eq!(failure.error_type, "upstream_quota_exhausted");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
        assert!(failure.summary.contains("insufficient_quota"));
    }

    #[test]
    fn upstream_http_failure_detects_chinese_balance_errors() {
        let failure =
            describe_upstream_http_failure(StatusCode::FORBIDDEN, "账户余额不足".as_bytes());

        assert_eq!(failure.error_type, "upstream_quota_exhausted");
        assert!(!failure.retryable);
    }

    #[test]
    fn upstream_http_failure_classifies_plain_rate_limit() {
        let failure =
            describe_upstream_http_failure(StatusCode::TOO_MANY_REQUESTS, b"too many requests");

        assert_eq!(failure.error_type, "upstream_rate_limited");
        assert!(failure.retryable);
    }

    #[test]
    fn upstream_http_failure_classifies_authentication_errors() {
        let body =
            br#"{"error":{"message":"Incorrect API key provided","type":"invalid_api_key"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::UNAUTHORIZED, body);

        assert_eq!(failure.error_type, "upstream_authentication_failed");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
    }

    #[test]
    fn upstream_http_failure_classifies_model_errors() {
        let body =
            br#"{"error":{"message":"The model `gpt-x` does not exist","code":"model_not_found"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::NOT_FOUND, body);

        assert_eq!(failure.error_type, "upstream_model_unavailable");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(!failure.retryable);
    }

    #[test]
    fn upstream_http_failure_classifies_context_errors() {
        let body =
            br#"{"error":{"message":"This model's maximum context length is 128000 tokens"}}"#;

        let failure = describe_upstream_http_failure(StatusCode::BAD_REQUEST, body);

        assert_eq!(failure.error_type, "upstream_context_length_exceeded");
        assert_eq!(failure.relay_status, StatusCode::BAD_REQUEST);
        assert!(!failure.retryable);
    }

    #[test]
    fn upstream_http_failure_classifies_server_errors() {
        let failure =
            describe_upstream_http_failure(StatusCode::INTERNAL_SERVER_ERROR, b"backend failed");

        assert_eq!(failure.error_type, "upstream_server_error");
        assert_eq!(failure.relay_status, StatusCode::BAD_GATEWAY);
        assert!(failure.retryable);
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
}
