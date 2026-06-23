mod background;
mod images;

pub(crate) use background::response_terminal;

use std::{sync::Arc, time::Instant};

use axum::{
    extract::{OriginalUri, Path, Query, State},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use bytes::Bytes;
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    admin::channel::ResponsesMode,
    auth::UserAuth,
    error::{AppError, AppResult},
    task::{jobs, upstream as upstream_task},
    AppState,
};

use crate::relay::raw_upstream_response;
use crate::relay::{
    bridge, describe_upstream_http_failure, finish_relay, finish_task_json_response,
    forward_anthropic, forward_openai_bound, forward_openai_with_headers,
    log_upstream_http_failure, prepare_relay_body, read_upstream_error_body,
    record_upstream_http_failure, record_upstream_transport_failure_for_failover, reserve_credit,
    respond_upstream_http_failure,
    selector::{AttemptedUpstream, ModelCooldown, SelectedUpstream, UpstreamProtocol},
    should_failover_retryable_upstream_failure, BodyKind, ChannelAffinityKey, PreparedRelayBody,
    RelayBody, RelayContext,
};
use crate::task::upstream::UpstreamTaskType;

const MODEL_UNAVAILABLE_MAX_REROUTES: usize = 3;
const MODEL_UNAVAILABLE_BLOCK_HOURS: i64 = 12;
const OPENAI_CHAT_PROTOCOLS: [UpstreamProtocol; 2] =
    [UpstreamProtocol::Openai, UpstreamProtocol::Anthropic];
const OPENAI_PROTOCOLS: [UpstreamProtocol; 1] = [UpstreamProtocol::Openai];
const OPENAI_RESPONSES_PROTOCOLS: [UpstreamProtocol; 3] = [
    UpstreamProtocol::OpenAiOauth,
    UpstreamProtocol::Openai,
    UpstreamProtocol::Anthropic,
];

#[derive(Debug, Deserialize)]
pub(crate) struct ResponseAssetQuery {
    expires: i64,
    sig: String,
}

pub(crate) async fn openai_chat_completions(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    openai_chat_completion_response(state, auth, headers, body).await
}

pub(crate) async fn openai_chat_completion_response(
    state: Arc<AppState>,
    auth: UserAuth,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
        headers,
        body,
        "/v1/chat/completions",
        BodyKind::OpenaiChat,
    )
    .await
}

pub(crate) async fn openai_embeddings(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
        HeaderMap::new(),
        body,
        "/v1/embeddings",
        BodyKind::OpenaiJson,
    )
    .await
}

pub(crate) async fn openai_moderations(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
        HeaderMap::new(),
        body,
        "/v1/moderations",
        BodyKind::OpenaiJson,
    )
    .await
}

pub(crate) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
        headers,
        body,
        "/v1/responses",
        BodyKind::OpenaiResponses,
    )
    .await
}

pub(crate) async fn openai_image_generations(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    images::relay_openai_image(state, auth, headers, body, "/v1/images/generations").await
}

pub(crate) async fn openai_image_edits(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    images::relay_openai_image(state, auth, headers, body, "/v1/images/edits").await
}

pub(crate) async fn openai_image_variations(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    images::relay_openai_image(state, auth, headers, body, "/v1/images/variations").await
}

pub(crate) async fn openai_response(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(response_id): Path<String>,
    OriginalUri(uri): OriginalUri,
) -> AppResult<Response> {
    match upstream_task::fetch_task(
        &state.db.pool,
        auth.user_key_id,
        UpstreamTaskType::NeogateResponse,
        &response_id,
    )
    .await
    {
        Ok(task) => {
            let response = jobs::response_for_task(&state, &task).await?;
            return jobs::response(response).await;
        }
        Err(AppError::NotFound) => {}
        Err(err) => return Err(err),
    }
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
        return background::finish_stream_response(state, auth, task, upstream, response);
    }
    finish_task_json_response(state, auth, task, response).await
}

pub(crate) async fn openai_response_asset(
    State(state): State<Arc<AppState>>,
    Path((response_id, index)): Path<(String, usize)>,
    Query(query): Query<ResponseAssetQuery>,
) -> AppResult<Response> {
    jobs::asset_response(&state, &response_id, index, query.expires, &query.sig).await
}

pub(crate) async fn openai_response_input_items(
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
    let path = response_subresource_path(&task.upstream_task_id, &uri, "input_items");
    let response = forward_openai_bound(&state, &upstream, Method::GET, &path, None).await?;
    raw_upstream_response(response).await
}

pub(crate) async fn cancel_openai_response(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(response_id): Path<String>,
) -> AppResult<Response> {
    match upstream_task::fetch_task(
        &state.db.pool,
        auth.user_key_id,
        UpstreamTaskType::NeogateResponse,
        &response_id,
    )
    .await
    {
        Ok(task) => {
            let response = jobs::cancel(&state, task).await?;
            return jobs::response(response).await;
        }
        Err(AppError::NotFound) => {}
        Err(err) => return Err(err),
    }
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
    headers: HeaderMap,
    body: Bytes,
    path: &'static str,
    body_kind: BodyKind,
) -> AppResult<Response> {
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
        return background::create_background_response(
            state,
            auth,
            body,
            meta.model,
            output_tokens,
            meta.request_params,
            meta.channel_affinity_key,
        )
        .await;
    }
    let user_key_model_credit_account = auth.model_credit_account(&meta.model).cloned();
    let channel_affinity_key = meta.channel_affinity_key.clone();
    let relay_trace_id = Uuid::new_v4();

    let mut model_unavailable_reroutes = 0;
    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();
    loop {
        let started = Instant::now();
        let (protocol, upstream) = select_upstream_excluding(
            &state,
            path,
            &meta.model,
            channel_affinity_key.as_ref(),
            &attempted_upstreams,
        )
        .await?;
        attempted_upstreams.push(AttemptedUpstream::from(&upstream));
        if path == "/v1/responses" && upstream.responses_mode == ResponsesMode::Disabled {
            tracing::info!(
                provider = %upstream.provider,
                channel_id = upstream.channel_id,
                channel_name = %upstream.channel_name,
                channel_endpoint_id = upstream.channel_endpoint_id,
                protocol = protocol.as_str(),
                model = %meta.model,
                path,
                "skipping upstream because responses API is disabled for this endpoint"
            );
            continue;
        }
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
        let mut ctx = RelayContext {
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
            channel_affinity_key: channel_affinity_key.clone(),
            relay_trace_id,
            relay_attempt: attempted_upstreams.len() as i32,
            relay_final: false,
            request_params: meta.request_params.clone(),
            _image_sync_permit: None,
        };
        let response = match protocol {
            UpstreamProtocol::Anthropic if path == "/v1/chat/completions" => {
                let body = bridge::openai_chat_to_anthropic_messages(body.clone())?;
                forward_anthropic(&state, &HeaderMap::new(), &ctx.upstream, body).await
            }
            UpstreamProtocol::Anthropic if path == "/v1/responses" => {
                let body = bridge::openai_response_to_anthropic_messages(body.clone())?;
                forward_anthropic(&state, &HeaderMap::new(), &ctx.upstream, body).await
            }
            UpstreamProtocol::Anthropic => Err(AppError::BadRequest(format!(
                "Anthropic fallback is not supported for {path}"
            ))),
            UpstreamProtocol::Openai
                if path == "/v1/responses"
                    && ctx.upstream.responses_mode == ResponsesMode::ChatFallback =>
            {
                let body = bridge::openai_response_to_openai_chat(body.clone())?;
                forward_openai_with_headers(
                    &state,
                    &ctx.upstream,
                    protocol,
                    body,
                    "/v1/chat/completions",
                    &headers,
                )
                .await
            }
            UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth => {
                forward_openai_with_headers(
                    &state,
                    &ctx.upstream,
                    protocol,
                    body.clone(),
                    path,
                    &headers,
                )
                .await
            }
        };

        match response {
            Ok(upstream_response) => {
                let status = StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                if status.is_success() {
                    mark_credential_model_available(&ctx).await?;
                    ctx.relay_final = true;
                    return finish_openai_relay_success(ctx, status, upstream_response).await;
                }

                let body = read_upstream_error_body(upstream_response).await;
                let failure = describe_upstream_http_failure(status, &body);
                if should_retry_after_model_unavailable(&ctx, &failure) {
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
                        ctx.relay_final = true;
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
                        ctx.relay_final = true;
                        return respond_upstream_http_failure(ctx, status, failure).await;
                    }
                    model_unavailable_reroutes += 1;
                    log_upstream_http_failure(&ctx, status, &failure, None);
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

                if should_failover_retryable_upstream_failure(
                    &ctx,
                    &attempted_upstreams,
                    failure.retryable,
                    retryable_failovers,
                )
                .await
                {
                    retryable_failovers += 1;
                    log_upstream_http_failure(&ctx, status, &failure, None);
                    record_upstream_http_failure(&ctx, status, &failure, "upstream failover").await;
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
                        upstream_status = status.as_u16(),
                        failover_attempt = retryable_failovers,
                        max_failovers = ctx.state.config.relay.max_upstream_failovers,
                        "retryable upstream http failure; retrying another upstream"
                    );
                    continue;
                }

                ctx.relay_final = true;
                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => {
                let retryable = err.retryable();
                if should_failover_retryable_upstream_failure(
                    &ctx,
                    &attempted_upstreams,
                    retryable,
                    retryable_failovers,
                )
                .await
                {
                    retryable_failovers += 1;
                    let summary = err.to_string();
                    log_relay_transport_failover(&ctx, &summary, retryable_failovers);
                    record_upstream_transport_failure_for_failover(&ctx, summary).await;
                    continue;
                }
                ctx.relay_final = true;
                return finish_relay(ctx, Err(err)).await;
            }
        }
    }
}

async fn finish_openai_relay_success(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    match (ctx.protocol, ctx.path, ctx.upstream.responses_mode) {
        (UpstreamProtocol::Anthropic, "/v1/chat/completions", _) => {
            bridge::finish_anthropic_as_openai_chat(ctx, status, upstream_response).await
        }
        (UpstreamProtocol::Anthropic, "/v1/responses", _) => {
            bridge::finish_anthropic_as_openai_response(ctx, status, upstream_response).await
        }
        (UpstreamProtocol::Openai, "/v1/responses", ResponsesMode::ChatFallback) => {
            bridge::finish_openai_chat_as_openai_response(ctx, status, upstream_response).await
        }
        _ => finish_relay(ctx, Ok(upstream_response)).await,
    }
}

fn response_subresource_path(response_id: &str, uri: &Uri, subresource: &str) -> String {
    let query = uri
        .path_and_query()
        .and_then(|value| value.as_str().split_once('?').map(|(_, query)| query));
    match query {
        Some(query) if !query.is_empty() => {
            format!("/v1/responses/{response_id}/{subresource}?{query}")
        }
        _ => format!("/v1/responses/{response_id}/{subresource}"),
    }
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

async fn select_upstream_excluding(
    state: &AppState,
    path: &'static str,
    model: &str,
    affinity_key: Option<&ChannelAffinityKey>,
    attempted: &[AttemptedUpstream],
) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
    let protocols = match path {
        "/v1/chat/completions" => &OPENAI_CHAT_PROTOCOLS[..],
        "/v1/responses" => &OPENAI_RESPONSES_PROTOCOLS[..],
        _ => &OPENAI_PROTOCOLS[..],
    };

    state
        .selector
        .select_with_affinity_excluding_protocols(
            &state.db.pool,
            &state.secrets,
            &state.channel_affinity,
            protocols,
            model,
            affinity_key,
            attempted,
        )
        .await
}

fn log_relay_transport_failover(ctx: &RelayContext, summary: &str, failover_attempt: usize) {
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
        failover_attempt,
        max_failovers = ctx.state.config.relay.max_upstream_failovers,
        error = %summary,
        "retryable upstream transport failure; retrying another upstream"
    );
}

async fn mark_credential_model_unavailable(
    ctx: &RelayContext,
    status: StatusCode,
    failure: &crate::relay::UpstreamHttpFailure,
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
            ModelCooldown {
                unavailable_until,
                last_error: &failure.summary,
                last_status_code: status.as_u16() as i32,
            },
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

async fn mark_credential_model_available(ctx: &RelayContext) -> AppResult<()> {
    ctx.state
        .selector
        .mark_model_available(&ctx.state.db.pool, &ctx.upstream, &ctx.model)
        .await?;
    if let Some(credential_id) = ctx.upstream.credential_id {
        ctx.state.credential_models.record_available(
            credential_id,
            ctx.upstream.channel_endpoint_id,
            &ctx.model,
        );
    }
    Ok(())
}

fn should_retry_after_model_unavailable(
    ctx: &RelayContext,
    failure: &crate::relay::UpstreamHttpFailure,
) -> bool {
    matches!(
        ctx.protocol,
        UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
    ) && failure.error_type == "upstream_model_unavailable"
}

#[cfg(test)]
mod tests;
