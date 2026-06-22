mod images;

use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::parse_usage_from_bytes,
    error::{AppError, AppResult},
    task::{billing as task_billing, jobs, upstream as upstream_task},
    AppState,
};

use crate::relay::{ensure_key_backed_async_upstream, raw_upstream_response, response_from_bytes};
use crate::task::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType};
use crate::{
    provider::newapi,
    relay::{
        bridge, describe_upstream_http_failure, finish_relay, finish_task_json_response,
        forward_anthropic, forward_openai, forward_openai_bound, handle_upstream_http_error,
        log_upstream_http_failure, prepare_relay_body, read_upstream_error_body,
        record_upstream_http_failure, record_upstream_transport_failure_for_failover,
        release_empty_hold, reserve_credit, respond_upstream_http_failure,
        selector::{AttemptedUpstream, ModelCooldown, SelectedUpstream, UpstreamProtocol},
        should_failover_retryable_upstream_failure, task_status_from_value, BodyKind,
        ChannelAffinityKey, PreparedRelayBody, RelayBody, RelayContext,
    },
};

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
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    openai_chat_completion_response(state, auth, body).await
}

pub(crate) async fn openai_chat_completion_response(
    state: Arc<AppState>,
    auth: UserAuth,
    body: Bytes,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
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
    relay_openai(state, auth, body, "/v1/embeddings", BodyKind::OpenaiJson).await
}

pub(crate) async fn openai_moderations(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(state, auth, body, "/v1/moderations", BodyKind::OpenaiJson).await
}

pub(crate) async fn openai_responses(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai(
        state,
        auth,
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
        return finish_stream_response(state, auth, task, upstream, response);
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
        return create_background_response(
            state,
            auth,
            body,
            meta.model,
            output_tokens,
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
            UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth => {
                forward_openai(&state, &ctx.upstream, protocol, body.clone(), path).await
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
    if ctx.protocol != UpstreamProtocol::Anthropic {
        return finish_relay(ctx, Ok(upstream_response)).await;
    }
    match ctx.path {
        "/v1/chat/completions" => {
            bridge::finish_anthropic_as_openai_chat(ctx, status, upstream_response).await
        }
        "/v1/responses" => {
            bridge::finish_anthropic_as_openai_response(ctx, status, upstream_response).await
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

async fn create_background_response(
    state: Arc<AppState>,
    auth: UserAuth,
    body: Bytes,
    model: String,
    output_tokens: i64,
    channel_affinity_key: Option<ChannelAffinityKey>,
) -> AppResult<Response> {
    let started = Instant::now();
    let prepared = jobs::prepare_request_body(body)?;
    let upstream = state
        .selector
        .select_with_affinity(
            &state.db.pool,
            &state.secrets,
            &state.channel_affinity,
            UpstreamProtocol::Openai,
            &model,
            channel_affinity_key.as_ref(),
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
        &prepared.body,
        output_tokens,
        &price,
    )
    .await?;
    if newapi::is_newapi_provider(&upstream.provider) && prepared.has_image_generation_tool {
        let response = match jobs::create(
            &state,
            &auth,
            &upstream,
            &model,
            prepared.body.clone(),
            prepared.image_format,
            &hold,
        )
        .await
        {
            Ok(response) => response,
            Err(err) => {
                release_empty_hold(&state, hold, "neogate response create error").await;
                return Err(err);
            }
        };
        if let Some(key) = channel_affinity_key.clone() {
            state.channel_affinity.insert(key, (&upstream).into());
        }
        return jobs::response(response).await;
    }
    if prepared.image_format.requires_neogate_asset_url() {
        release_empty_hold(&state, hold, "unsupported neogate image_format").await;
        return Err(AppError::BadRequest(
            "image_format=url or both is only supported for NeoGate async image tasks".to_string(),
        ));
    }
    let response = forward_openai(
        &state,
        &upstream,
        UpstreamProtocol::Openai,
        prepared.body,
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
        channel_affinity_key,
        relay_trace_id: Uuid::new_v4(),
        relay_attempt: 1,
        relay_final: true,
        _image_sync_permit: None,
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
        state.config.task.upstream_poll_interval,
        state.config.task.upstream_retention,
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
            poll_interval: state.config.task.upstream_poll_interval,
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

pub(crate) fn response_terminal(status: &str) -> bool {
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
