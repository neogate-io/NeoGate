mod body;
mod credential;
mod models;
mod request;
pub mod selector;
mod streaming;
mod upstream;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
    routing::{get, post},
    Router,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::UserAuth,
    billing::{
        estimate_input_tokens, estimated_cost_micro_usd, parse_usage_from_bytes, BillingCharge,
        DebitHold, Price, TokenUsage,
    },
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    AppState,
};

use self::selector::{SelectedUpstream, UpstreamProtocol};
use self::streaming::RelayContext;
use crate::task::{
    billing as task_billing, results::AnthropicResultsUsageParser, upstream as upstream_task,
};
use crate::usage::{KeyFailure, UsageInsert};
use body::RelayBody;
pub use credential::CredentialModelRecorder;
use models::{list_anthropic_models, list_openai_models};
use request::{prepare_relay_body, BodyKind, PreparedRelayBody};
pub(crate) use upstream::upstream_url;
use upstream::{
    forward_anthropic, forward_openai, log_relay_upstream_failure, relay_upstream_error,
};
pub(crate) use upstream::{forward_anthropic_bound, forward_openai_bound};
use upstream_task::{NewUpstreamTask, UpstreamTask, UpstreamTaskType};

const MODEL_UNAVAILABLE_MAX_REROUTES: usize = 3;
const MODEL_UNAVAILABLE_BLOCK_HOURS: i64 = 12;
const UPSTREAM_ERROR_BODY_LOG_LIMIT: usize = 1000;
const UPSTREAM_ERROR_BODY_READ_LIMIT: usize = 64 * 1024;

#[derive(Debug, Deserialize)]
struct AnthropicBatchListQuery {
    limit: Option<i64>,
    after_id: Option<String>,
    before_id: Option<String>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/models", get(list_openai_models))
        .route("/v1/chat/completions", post(openai_chat_completions))
        .route("/v1/responses", post(openai_responses))
        .route("/v1/responses/{response_id}", get(openai_response))
        .route(
            "/v1/responses/{response_id}/cancel",
            post(cancel_openai_response),
        )
        .route("/anthropic/v1/messages/models", get(list_anthropic_models))
        .route("/v1/messages", post(anthropic_messages))
        .route("/anthropic/v1/messages", post(anthropic_messages))
        .route(
            "/v1/messages/batches",
            post(create_anthropic_message_batch).get(list_anthropic_message_batches),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}",
            get(anthropic_message_batch).delete(delete_anthropic_message_batch),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}/cancel",
            post(cancel_anthropic_message_batch),
        )
        .route(
            "/v1/messages/batches/{message_batch_id}/results",
            get(anthropic_message_batch_results),
        )
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

async fn openai_response(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(response_id): Path<String>,
    OriginalUri(uri): OriginalUri,
) -> AppResult<Response> {
    let (task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::OpenAiResponse,
        &response_id,
    )
    .await?;
    let path = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| uri.path());
    let response = forward_openai_bound(&state, &upstream, Method::GET, path, None).await?;
    if openai_response_query_streams(path) {
        return finish_openai_stream_response(state, auth, task, upstream, response);
    }
    finish_task_json_response(state, auth, task, response).await
}

async fn cancel_openai_response(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(response_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::OpenAiResponse,
        &response_id,
    )
    .await?;
    let path = format!("/v1/responses/{response_id}/cancel");
    let response = forward_openai_bound(&state, &upstream, Method::POST, &path, None).await?;
    finish_task_json_response(state, auth, task, response).await
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
    let user_key_model_credit_account = auth.model_credit_account(&meta.model).cloned();
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
    let hold = reserve_credit(
        &state,
        &auth,
        user_key_model_credit_account.as_ref(),
        &body,
        output_tokens,
        &price,
    )
    .await?;
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
            user_key_model_credit_account,
            started,
        },
        response,
    )
    .await
}

async fn create_anthropic_message_batch(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    let model = anthropic_batch_model(&body)?;
    let request_count = anthropic_batch_request_count(&body)?;
    auth.ensure_model_allowed(&model)?;
    let user_key_model_credit_account = auth.model_credit_account(&model).cloned();
    let upstream = state
        .selector
        .select(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Anthropic,
            &model,
        )
        .await?;
    ensure_key_backed_async_upstream(&upstream)?;
    let price = state
        .billing
        .price_for(&state.db.pool, &upstream.provider, &model, &auth.user_group)
        .await?;
    let hold = reserve_credit(
        &state,
        &auth,
        user_key_model_credit_account.as_ref(),
        &body,
        state
            .billing
            .default_output_tokens()
            .saturating_mul(request_count),
        &price,
    )
    .await?;
    let response = match forward_anthropic_bound(
        &state,
        &headers,
        &upstream,
        Method::POST,
        "/v1/messages/batches",
        Some(body),
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            release_empty_hold(
                &state,
                hold,
                "anthropic batch create upstream request error",
            )
            .await;
            return Err(err);
        }
    };
    finish_anthropic_batch_create(state, auth, upstream, model, hold, response).await
}

async fn list_anthropic_message_batches(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Query(query): Query<AnthropicBatchListQuery>,
) -> AppResult<Response> {
    let requested_limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let tasks = upstream_task::list_tasks_for_auth(
        &state.db.pool,
        &auth,
        UpstreamTaskType::AnthropicMessageBatch,
        requested_limit + 1,
        query.after_id.as_deref(),
        query.before_id.as_deref(),
    )
    .await?;
    let has_more = tasks.len() as i64 > requested_limit;
    let data = tasks
        .into_iter()
        .take(requested_limit as usize)
        .map(|task| {
            json!({
                "id": task.upstream_task_id,
                "type": "message_batch",
                "processing_status": task.status,
                "request_counts": task_upstream_counts(&task),
                "created_at": task_created_at(&task),
            })
        })
        .collect::<Vec<_>>();
    let first_id = data
        .first()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let last_id = data
        .last()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    response_from_bytes(
        StatusCode::OK,
        HeaderValue::from_static("application/json"),
        Bytes::from(
            json!({
                "data": data,
                "first_id": first_id,
                "last_id": last_id,
                "has_more": has_more
            })
            .to_string(),
        ),
    )
}

async fn anthropic_message_batch(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    Path(message_batch_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::AnthropicMessageBatch,
        &message_batch_id,
    )
    .await?;
    let path = format!("/v1/messages/batches/{message_batch_id}");
    let response =
        forward_anthropic_bound(&state, &headers, &upstream, Method::GET, &path, None).await?;
    finish_task_json_response(state, auth, task, response).await
}

async fn cancel_anthropic_message_batch(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    Path(message_batch_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::AnthropicMessageBatch,
        &message_batch_id,
    )
    .await?;
    let path = format!("/v1/messages/batches/{message_batch_id}/cancel");
    let response =
        forward_anthropic_bound(&state, &headers, &upstream, Method::POST, &path, None).await?;
    finish_task_json_response(state, auth, task, response).await
}

async fn delete_anthropic_message_batch(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    Path(message_batch_id): Path<String>,
) -> AppResult<Response> {
    let (_task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::AnthropicMessageBatch,
        &message_batch_id,
    )
    .await?;
    let path = format!("/v1/messages/batches/{message_batch_id}");
    let response =
        forward_anthropic_bound(&state, &headers, &upstream, Method::DELETE, &path, None).await?;
    raw_upstream_response(response).await
}

async fn anthropic_message_batch_results(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    Path(message_batch_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) = upstream_task::fetch_task_for_auth(
        &state,
        &auth,
        UpstreamTaskType::AnthropicMessageBatch,
        &message_batch_id,
    )
    .await?;
    let path = format!("/v1/messages/batches/{message_batch_id}/results");
    let response =
        forward_anthropic_bound(&state, &headers, &upstream, Method::GET, &path, None).await?;
    finish_anthropic_results_response(state, auth, task, response).await
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
    if matches!(body_kind, BodyKind::OpenaiResponses) && meta.background {
        if meta.store == Some(false) {
            return Err(AppError::BadRequest(
                "background responses require store=true".to_string(),
            ));
        }
        if meta.stream {
            return Err(AppError::BadRequest(
                "create-time streaming for background responses is not supported; retrieve with stream=true to resume".to_string(),
            ));
        }
        return create_openai_background_response(state, auth, body, meta.model, output_tokens)
            .await;
    }
    let user_key_model_credit_account = auth.model_credit_account(&meta.model).cloned();

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
        let hold = reserve_credit(
            &state,
            &auth,
            user_key_model_credit_account.as_ref(),
            &body,
            output_tokens,
            &price,
        )
        .await?;
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
            user_key_model_credit_account: user_key_model_credit_account.clone(),
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

async fn create_openai_background_response(
    state: Arc<AppState>,
    auth: UserAuth,
    body: Bytes,
    model: String,
    output_tokens: i64,
) -> AppResult<Response> {
    let started = Instant::now();
    let upstream = state
        .selector
        .select(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            &model,
        )
        .await?;
    ensure_key_backed_async_upstream(&upstream)?;
    let price = state
        .billing
        .price_for(&state.db.pool, &upstream.provider, &model, &auth.user_group)
        .await?;
    let user_key_model_credit_account = auth.model_credit_account(&model).cloned();
    let hold = reserve_credit(
        &state,
        &auth,
        user_key_model_credit_account.as_ref(),
        &body,
        output_tokens,
        &price,
    )
    .await?;
    let response = forward_openai(
        &state,
        &upstream,
        UpstreamProtocol::Openai,
        body,
        "/v1/responses",
    )
    .await;
    let ctx = RelayContext {
        state: Arc::clone(&state),
        auth: auth.clone(),
        upstream: upstream.clone(),
        protocol: UpstreamProtocol::Openai,
        path: "/v1/responses",
        model: model.clone(),
        streamed: false,
        price,
        hold: hold.clone(),
        user_key_model_credit_account,
        started,
    };

    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return handle_upstream_http_error(ctx, status, upstream_response).await;
    }

    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = match upstream_response.bytes().await {
        Ok(body) => body,
        Err(err) => {
            release_empty_hold(&state, hold, "openai background response body read error").await;
            return Err(err.into());
        }
    };
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            release_empty_hold(&state, hold, "openai background response parse error").await;
            return Err(err.into());
        }
    };
    let response_id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("upstream response is missing id".to_string()));
    let response_id = match response_id {
        Ok(response_id) => response_id,
        Err(err) => {
            release_empty_hold(&state, hold, "openai background response missing id").await;
            return Err(err);
        }
    };
    let status_text = value
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("queued");
    let terminal = openai_response_terminal(status_text);
    if let Err(err) = upstream_task::insert_task(
        &state.db.pool,
        NewUpstreamTask {
            task_type: UpstreamTaskType::OpenAiResponse,
            upstream_task_id: response_id,
            auth: &auth,
            protocol: UpstreamProtocol::Openai,
            upstream: &upstream,
            model: Some(&model),
            status: status_text,
            terminal,
            hold: &hold,
            upstream_metadata: value.clone(),
        },
        state.config.task_upstream_poll_interval,
        state.config.task_upstream_retention,
    )
    .await
    {
        release_empty_hold(&state, hold, "openai background task insert error").await;
        return Err(err);
    }
    if terminal {
        let usage = parse_usage_from_bytes(&body, false);
        task_billing::finalize_for_auth(
            &state,
            &auth,
            response_id,
            UpstreamTaskType::OpenAiResponse,
            usage,
            terminal,
        )
        .await?;
    }
    response_from_bytes(status, content_type, body)
}

async fn finish_anthropic_batch_create(
    state: Arc<AppState>,
    auth: UserAuth,
    upstream: SelectedUpstream,
    model: String,
    hold: DebitHold,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = match upstream_response.bytes().await {
        Ok(body) => body,
        Err(err) => {
            release_empty_hold(&state, hold, "anthropic batch create body read error").await;
            return Err(err.into());
        }
    };
    if !status.is_success() {
        release_empty_hold(&state, hold, "anthropic batch create upstream error").await;
        return response_from_bytes(status, content_type, body);
    }
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            release_empty_hold(&state, hold, "anthropic batch create parse error").await;
            return Err(err.into());
        }
    };
    let batch_id = value
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("upstream batch response is missing id".to_string()));
    let batch_id = match batch_id {
        Ok(batch_id) => batch_id.to_string(),
        Err(err) => {
            release_empty_hold(&state, hold, "anthropic batch create missing id").await;
            return Err(err);
        }
    };
    let status_text = value
        .get("processing_status")
        .and_then(Value::as_str)
        .unwrap_or("in_progress")
        .to_string();
    if let Err(err) = upstream_task::insert_task(
        &state.db.pool,
        NewUpstreamTask {
            task_type: UpstreamTaskType::AnthropicMessageBatch,
            upstream_task_id: &batch_id,
            auth: &auth,
            protocol: UpstreamProtocol::Anthropic,
            upstream: &upstream,
            model: Some(&model),
            status: &status_text,
            terminal: anthropic_batch_terminal(&status_text),
            hold: &hold,
            upstream_metadata: value,
        },
        state.config.task_upstream_poll_interval,
        state.config.task_upstream_retention,
    )
    .await
    {
        release_empty_hold(&state, hold, "anthropic batch task insert error").await;
        return Err(err);
    }
    response_from_bytes(status, content_type, body)
}

async fn finish_task_json_response(
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
                task.id,
                task.task_type,
                &task.upstream_task_id,
                &status_text,
                terminal,
                value,
                usage,
                state.config.task_upstream_poll_interval,
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

fn finish_openai_stream_response(
    state: Arc<AppState>,
    auth: UserAuth,
    task: UpstreamTask,
    upstream: SelectedUpstream,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("text/event-stream"));
    let relay = OpenAiTaskStreamRelay {
        state: Some(state),
        auth: Some(auth),
        task: Some(task),
        upstream: Some(upstream),
        status,
        stream: upstream_response.bytes_stream().boxed(),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(futures_util::stream::unfold(
            Some(relay),
            |relay| async move {
                let mut relay = relay?;
                match relay.stream.next().await {
                    Some(Ok(chunk)) => Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay))),
                    Some(Err(err)) => Some((Err(std::io::Error::other(err)), None)),
                    None => {
                        relay.finish().await;
                        None
                    }
                }
            },
        )))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

async fn finish_anthropic_results_response(
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
        .unwrap_or_else(|| HeaderValue::from_static("application/x-jsonlines"));
    let relay = AnthropicResultsRelay {
        state: Some(state),
        auth: Some(auth),
        task: Some(task),
        status,
        usage_parser: AnthropicResultsUsageParser::default(),
        stream: upstream_response.bytes_stream().boxed(),
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(futures_util::stream::unfold(
            Some(relay),
            |relay| async move {
                let mut relay = relay?;
                match relay.stream.next().await {
                    Some(Ok(chunk)) => {
                        if relay.status.is_success() {
                            relay.usage_parser.observe(&chunk);
                        }
                        Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay)))
                    }
                    Some(Err(err)) => Some((Err(std::io::Error::other(err)), None)),
                    None => {
                        relay.finish().await;
                        None
                    }
                }
            },
        )))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

async fn raw_upstream_response(upstream_response: reqwest::Response) -> AppResult<Response> {
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

fn response_from_bytes(
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

fn task_status_from_value(
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
            let terminal = openai_response_terminal(&status);
            (status, terminal)
        }
        UpstreamTaskType::AnthropicMessageBatch => {
            let status = value
                .get("processing_status")
                .and_then(Value::as_str)
                .unwrap_or(&task.status)
                .to_string();
            let terminal = anthropic_batch_terminal(&status);
            (status, terminal)
        }
    }
}

fn openai_response_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "canceled" | "incomplete"
    )
}

fn anthropic_batch_terminal(status: &str) -> bool {
    matches!(status, "ended" | "canceled" | "cancelled" | "expired")
}

fn anthropic_batch_model(body: &[u8]) -> AppResult<String> {
    let value: Value = serde_json::from_slice(body)?;
    let requests = value
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("requests is required".to_string()))?;
    if requests.is_empty() {
        return Err(AppError::BadRequest(
            "requests must not be empty".to_string(),
        ));
    }
    let mut model = None;
    for (index, request) in requests.iter().enumerate() {
        let item_model = request
            .get("params")
            .and_then(|params| params.get("model"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::BadRequest(format!("requests[{index}].params.model is required"))
            })?;
        if let Some(model) = model {
            if model != item_model {
                return Err(AppError::BadRequest(
                    "message batches must use a single model".to_string(),
                ));
            }
        } else {
            model = Some(item_model);
        }
    }
    let model = model.expect("non-empty requests set model");
    Ok(model.to_string())
}

fn anthropic_batch_request_count(body: &[u8]) -> AppResult<i64> {
    let value: Value = serde_json::from_slice(body)?;
    let count = value
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("requests is required".to_string()))?
        .len();
    i64::try_from(count)
        .map_err(|_| AppError::BadRequest("too many batch requests".to_string()))
        .map(|count| count.max(1))
}

struct AnthropicResultsRelay {
    state: Option<Arc<AppState>>,
    auth: Option<UserAuth>,
    task: Option<UpstreamTask>,
    status: StatusCode,
    usage_parser: AnthropicResultsUsageParser,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
}

struct OpenAiTaskStreamRelay {
    state: Option<Arc<AppState>>,
    auth: Option<UserAuth>,
    task: Option<UpstreamTask>,
    upstream: Option<SelectedUpstream>,
    status: StatusCode,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
}

impl OpenAiTaskStreamRelay {
    async fn finish(mut self) {
        let (Some(state), Some(auth), Some(task), Some(upstream)) = (
            self.state.take(),
            self.auth.take(),
            self.task.take(),
            self.upstream.take(),
        ) else {
            return;
        };
        if !self.status.is_success() {
            return;
        }
        if let Err(err) = refresh_openai_task_after_stream(&state, &auth, &task, &upstream).await {
            tracing::warn!("failed to refresh openai response after stream resume: {err}");
        }
    }
}

impl Drop for OpenAiTaskStreamRelay {
    fn drop(&mut self) {
        if self.state.is_some() || self.auth.is_some() || self.task.is_some() {
            tracing::warn!("openai response stream ended before completion; skipping task refresh");
        }
    }
}

async fn refresh_openai_task_after_stream(
    state: &AppState,
    auth: &UserAuth,
    task: &UpstreamTask,
    upstream: &SelectedUpstream,
) -> AppResult<()> {
    let path = format!("/v1/responses/{}", task.upstream_task_id);
    let response = forward_openai_bound(state, upstream, Method::GET, &path, None).await?;
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return Ok(());
    }
    let body = response.bytes().await?;
    let value: Value = serde_json::from_slice(&body)?;
    let (status_text, terminal) = task_status_from_value(task.task_type, &value, task);
    let usage = parse_usage_from_bytes(&body, false);
    upstream_task::update_task_from_upstream_value(
        &state.db.pool,
        task.id,
        task.task_type,
        &task.upstream_task_id,
        &status_text,
        terminal,
        value,
        usage,
        state.config.task_upstream_poll_interval,
    )
    .await?;
    if terminal {
        task_billing::finalize_for_auth(
            state,
            auth,
            &task.upstream_task_id,
            task.task_type,
            usage,
            true,
        )
        .await?;
    }
    Ok(())
}

impl AnthropicResultsRelay {
    async fn finish(mut self) {
        let (Some(state), Some(auth), Some(task)) =
            (self.state.take(), self.auth.take(), self.task.take())
        else {
            return;
        };
        if !self.status.is_success() {
            return;
        }
        let usage = std::mem::take(&mut self.usage_parser).finish();
        if usage.is_some() || task.terminal {
            if let Err(err) = task_billing::finalize_for_auth(
                &state,
                &auth,
                &task.upstream_task_id,
                task.task_type,
                usage,
                true,
            )
            .await
            {
                tracing::warn!("failed to finalize anthropic batch results billing: {err}");
            }
        }
    }
}

impl Drop for AnthropicResultsRelay {
    fn drop(&mut self) {
        if self.state.is_some() || self.auth.is_some() || self.task.is_some() {
            tracing::warn!("anthropic batch results stream ended before completion; skipping partial billing settlement");
        }
    }
}

fn task_upstream_counts(task: &UpstreamTask) -> Value {
    task.upstream_metadata
        .get("request_counts")
        .cloned()
        .unwrap_or_else(|| {
            json!({
                "processing": 0,
                "succeeded": 0,
                "errored": 0,
                "canceled": 0,
                "expired": 0
            })
        })
}

fn task_created_at(task: &UpstreamTask) -> String {
    task.upstream_metadata
        .get("created_at")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| task.created_at.to_rfc3339())
}

fn openai_response_query_streams(path: &str) -> bool {
    path.split_once('?')
        .and_then(|(_, query)| serde_urlencoded::from_str::<Vec<(String, String)>>(query).ok())
        .map(|pairs| {
            pairs
                .iter()
                .any(|(key, value)| key == "stream" && value == "true")
        })
        .unwrap_or(false)
}

fn ensure_key_backed_async_upstream(upstream: &SelectedUpstream) -> AppResult<()> {
    if upstream.channel_key_id.is_none() || upstream.credential_id.is_some() {
        return Err(AppError::BadRequest(
            "async tasks require a key-backed upstream channel".to_string(),
        ));
    }
    Ok(())
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
        .header("x-neogate-error-type", failure.error_type)
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
    user_key_model_credit_account: Option<&crate::billing::CreditAccountId>,
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
            user_key_model_credit_account,
            &auth.user_key_credit_account,
            &auth.user_credit_account,
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

    #[test]
    fn openai_response_terminal_statuses_are_detected() {
        assert!(openai_response_terminal("completed"));
        assert!(openai_response_terminal("failed"));
        assert!(openai_response_terminal("cancelled"));
        assert!(!openai_response_terminal("in_progress"));
    }

    #[test]
    fn anthropic_batch_model_reads_first_request_params() {
        let body = Bytes::from_static(
            br#"{"requests":[{"custom_id":"one","params":{"model":"claude-sonnet"}}]}"#,
        );

        assert_eq!(anthropic_batch_model(&body).unwrap(), "claude-sonnet");
    }

    #[test]
    fn anthropic_batch_model_rejects_mixed_models() {
        let body = Bytes::from_static(
            br#"{"requests":[{"params":{"model":"claude-a"}},{"params":{"model":"claude-b"}}]}"#,
        );

        assert!(anthropic_batch_model(&body).is_err());
    }

    #[test]
    fn openai_response_query_streams_detects_true_flag() {
        assert!(openai_response_query_streams(
            "/v1/responses/resp_123?starting_after=10&stream=true"
        ));
        assert!(!openai_response_query_streams(
            "/v1/responses/resp_123?stream=false"
        ));
    }

    #[test]
    fn anthropic_results_usage_sums_successful_messages() {
        let body = br#"{"custom_id":"one","result":{"type":"succeeded","message":{"usage":{"input_tokens":7,"output_tokens":3}}}}
{"custom_id":"two","result":{"type":"succeeded","message":{"usage":{"input_tokens":5,"output_tokens":2}}}}
"#;

        let usage = crate::task::results::anthropic_results_usage(body).unwrap();

        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 5);
    }
}
