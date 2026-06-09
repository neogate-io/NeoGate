use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{OriginalUri, Path, State},
    http::{header, Method, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;

use crate::{
    auth::UserAuth,
    billing::parse_usage_from_bytes,
    error::{AppError, AppResult},
    task::{billing as task_billing, upstream as upstream_task},
    AppState,
};

use super::request::{prepare_relay_body, BodyKind, PreparedRelayBody};
use super::{
    body::RelayBody,
    error::describe_upstream_http_failure,
    finish_relay, finish_task_json_response, forward_openai, forward_openai_bound,
    handle_upstream_http_error, log_upstream_http_failure, read_upstream_error_body,
    record_upstream_http_failure, release_empty_hold, reserve_credit,
    respond_upstream_http_failure,
    selector::{SelectedUpstream, UpstreamProtocol},
    streaming::RelayContext,
    task_status_from_value,
    upstream_task::{NewUpstreamTask, UpstreamTask, UpstreamTaskType},
};
use super::{ensure_key_backed_async_upstream, response_from_bytes};

const MODEL_UNAVAILABLE_MAX_REROUTES: usize = 3;
const MODEL_UNAVAILABLE_BLOCK_HOURS: i64 = 12;

pub(super) async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(state, auth, body, "/v1/chat/completions").await
}

pub(super) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(state, auth, body, "/v1/responses").await
}

pub(super) async fn openai_response(
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
    if response_query_streams(path) {
        return finish_stream_response(state, auth, task, upstream, response);
    }
    finish_task_json_response(state, auth, task, response).await
}

pub(super) async fn cancel_openai_response(
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
        return create_background_response(state, auth, body, meta.model, output_tokens).await;
    }
    let user_key_model_credit_account = auth.model_credit_account(&meta.model).cloned();

    let mut model_unavailable_reroutes = 0;
    loop {
        let started = Instant::now();
        let (protocol, upstream) = select_upstream(&state, path, &meta.model).await?;
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
                    let blocked = mark_credential_model_unavailable(&ctx, status, &failure).await?;
                    if !blocked {
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
                            "skipping upstream model cooldown because this model has no alternate channel"
                        );
                        return respond_upstream_http_failure(ctx, status, failure).await;
                    }
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

async fn create_background_response(
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
        .unwrap_or_else(|| axum::http::HeaderValue::from_static("application/json"));
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
    let terminal = response_terminal(status_text);
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

fn finish_stream_response(
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
        .unwrap_or_else(|| axum::http::HeaderValue::from_static("text/event-stream"));
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
        if let Err(err) = refresh_task_after_stream(&state, &auth, &task, &upstream).await {
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

async fn refresh_task_after_stream(
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

pub(super) fn response_terminal(status: &str) -> bool {
    matches!(
        status,
        "completed" | "failed" | "cancelled" | "canceled" | "incomplete"
    )
}

fn response_query_streams(path: &str) -> bool {
    path.split_once('?')
        .and_then(|(_, query)| serde_urlencoded::from_str::<Vec<(String, String)>>(query).ok())
        .map(|pairs| {
            pairs
                .iter()
                .any(|(key, value)| key == "stream" && value == "true")
        })
        .unwrap_or(false)
}

async fn select_upstream(
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

async fn mark_credential_model_unavailable(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &super::error::UpstreamHttpFailure,
) -> AppResult<bool> {
    let unavailable_until =
        chrono::Utc::now() + chrono::Duration::hours(MODEL_UNAVAILABLE_BLOCK_HOURS);
    let blocked = ctx
        .state
        .selector
        .mark_credential_model_unavailable(
            &ctx.state.db.pool,
            &ctx.upstream,
            ctx.protocol,
            &ctx.model,
            unavailable_until,
        )
        .await?;
    if !blocked {
        return Ok(false);
    }
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
    Ok(true)
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

fn should_retry_after_model_unavailable(
    ctx: &RelayContext,
    failure: &super::error::UpstreamHttpFailure,
) -> bool {
    matches!(
        ctx.protocol,
        UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
    ) && failure.error_type == "upstream_model_unavailable"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_terminal_statuses_are_detected() {
        assert!(response_terminal("completed"));
        assert!(response_terminal("failed"));
        assert!(response_terminal("cancelled"));
        assert!(!response_terminal("in_progress"));
    }

    #[test]
    fn response_query_streams_detects_true_flag() {
        assert!(response_query_streams(
            "/v1/responses/resp_123?starting_after=10&stream=true"
        ));
        assert!(!response_query_streams(
            "/v1/responses/resp_123?stream=false"
        ));
    }
}
