use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;

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
        describe_upstream_http_failure, finish_relay, finish_task_json_response, forward_openai,
        forward_openai_bound, forward_openai_with_content_type, handle_upstream_http_error,
        log_upstream_http_failure, prepare_relay_body, read_upstream_error_body,
        record_upstream_http_failure, release_empty_hold, reserve_credit,
        respond_upstream_http_failure,
        selector::{ModelCooldown, SelectedUpstream, UpstreamProtocol},
        task_status_from_value, BodyKind, ChannelAffinityKey, PreparedRelayBody, RelayBody,
        RelayContext,
    },
};

const MODEL_UNAVAILABLE_MAX_REROUTES: usize = 3;
const MODEL_UNAVAILABLE_BLOCK_HOURS: i64 = 12;

#[derive(Debug, Clone)]
struct ImageRequestMeta {
    model: String,
    stream: bool,
    content_type: HeaderValue,
}

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
    relay_openai_image(state, auth, headers, body, "/v1/images/generations").await
}

pub(crate) async fn openai_image_edits(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai_image(state, auth, headers, body, "/v1/images/edits").await
}

pub(crate) async fn openai_image_variations(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai_image(state, auth, headers, body, "/v1/images/variations").await
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

    let mut model_unavailable_reroutes = 0;
    loop {
        let started = Instant::now();
        let (protocol, upstream) =
            select_upstream(&state, path, &meta.model, channel_affinity_key.as_ref()).await?;
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
            channel_affinity_key: channel_affinity_key.clone(),
            _image_sync_permit: None,
        };
        let response = forward_openai(&state, &ctx.upstream, protocol, body.clone(), path).await;

        match response {
            Ok(upstream_response) => {
                let status = StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                if status.is_success() {
                    mark_credential_model_available(&ctx).await?;
                    return finish_relay(ctx, Ok(upstream_response)).await;
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

                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => return finish_relay(ctx, Err(err)).await,
        }
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

async fn relay_openai_image(
    state: Arc<AppState>,
    auth: UserAuth,
    headers: HeaderMap,
    body: Bytes,
    path: &'static str,
) -> AppResult<Response> {
    let meta = image_request_meta(path, &headers, &body)?;
    auth.ensure_model_allowed(&meta.model)?;
    let started = Instant::now();
    let upstream = state
        .selector
        .select(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
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
    let user_key_model_credit_account = auth.model_credit_account(&meta.model).cloned();
    let image_sync_permit = Some(
        state
            .image_sync_limiter
            .try_acquire(auth.user_key_id)
            .await?,
    );
    let hold = reserve_credit(
        &state,
        &auth,
        user_key_model_credit_account.as_ref(),
        &body,
        state.billing.default_output_tokens(),
        &price,
    )
    .await?;
    let ctx = RelayContext {
        state: Arc::clone(&state),
        auth,
        upstream: upstream.clone(),
        protocol: UpstreamProtocol::Openai,
        path,
        model: meta.model,
        streamed: meta.stream,
        price,
        hold,
        user_key_model_credit_account,
        started,
        channel_affinity_key: None,
        _image_sync_permit: image_sync_permit,
    };
    let response = forward_openai_with_content_type(
        &state,
        &upstream,
        body.clone(),
        path,
        meta.content_type.clone(),
        meta.stream,
    )
    .await;
    if newapi::should_retry_image_variation(&upstream.provider, path) {
        return finish_newapi_image_variation(
            &state,
            &upstream,
            body,
            meta.content_type,
            meta.stream,
            ctx,
            response,
        )
        .await;
    }
    if newapi::should_wrap_image_stream(&upstream.provider, meta.stream, path) {
        return finish_newapi_image_stream(ctx, response).await;
    }
    finish_relay(ctx, response).await
}

async fn finish_newapi_image_stream(
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
) -> AppResult<Response> {
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
    if newapi::is_event_stream(&content_type) {
        return finish_relay(ctx, Ok(upstream_response)).await;
    }

    tracing::warn!(
        provider = %ctx.upstream.provider,
        channel_id = ctx.upstream.channel_id,
        channel_name = %ctx.upstream.channel_name,
        channel_endpoint_id = ctx.upstream.channel_endpoint_id,
        channel_key_id = ?ctx.upstream.channel_key_id,
        credential_id = ?ctx.upstream.credential_id,
        model = %ctx.model,
        path = ctx.path,
        "NewAPI image stream request returned non-SSE response; relaying upstream body without JSON-to-SSE buffering"
    );
    finish_relay(ctx, Ok(upstream_response)).await
}

async fn finish_newapi_image_variation(
    state: &AppState,
    upstream: &SelectedUpstream,
    body: Bytes,
    content_type: HeaderValue,
    stream: bool,
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
) -> AppResult<Response> {
    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if status != StatusCode::BAD_REQUEST {
        return finish_relay(ctx, Ok(upstream_response)).await;
    }

    let error_body = read_upstream_error_body(upstream_response).await;
    if !newapi::should_retry_variation_as_edit(ctx.path, status, &error_body) {
        let failure = describe_upstream_http_failure(status, &error_body);
        return respond_upstream_http_failure(ctx, status, failure).await;
    }

    tracing::warn!(
        provider = %upstream.provider,
        channel_id = upstream.channel_id,
        channel_name = %upstream.channel_name,
        channel_endpoint_id = upstream.channel_endpoint_id,
        channel_key_id = ?upstream.channel_key_id,
        credential_id = ?upstream.credential_id,
        model = %ctx.model,
        path = ctx.path,
        retry_path = "/v1/images/edits",
        "retrying NewAPI image variation as image edit because upstream dropped multipart model field"
    );

    let retry = newapi::variation_as_edit_request(&body, &content_type)?;
    let response = forward_openai_with_content_type(
        state,
        upstream,
        retry.body,
        retry.path,
        retry.content_type,
        stream,
    )
    .await;
    if stream {
        return finish_newapi_image_stream(ctx, response).await;
    }
    finish_relay(ctx, response).await
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

fn image_request_meta(
    path: &'static str,
    headers: &HeaderMap,
    body: &[u8],
) -> AppResult<ImageRequestMeta> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let content_type_text = content_type
        .to_str()
        .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?;
    let content_type_text = content_type_text.to_string();
    let lower_content_type = content_type_text.to_ascii_lowercase();

    if lower_content_type.starts_with("application/json") {
        if path == "/v1/images/variations" {
            return Err(AppError::BadRequest(
                "image variations require multipart/form-data".to_string(),
            ));
        }
        return json_image_request_meta(body, content_type);
    }

    if lower_content_type.starts_with("multipart/form-data") {
        if path == "/v1/images/generations" {
            return Err(AppError::BadRequest(
                "image generations require application/json".to_string(),
            ));
        }
        return multipart_image_request_meta(body, &content_type_text, content_type);
    }

    Err(AppError::BadRequest(
        "images requests require application/json or multipart/form-data".to_string(),
    ))
}

fn json_image_request_meta(body: &[u8], content_type: HeaderValue) -> AppResult<ImageRequestMeta> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?
        .to_string();
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(ImageRequestMeta {
        model,
        stream,
        content_type,
    })
}

fn multipart_image_request_meta(
    body: &[u8],
    content_type_text: &str,
    content_type: HeaderValue,
) -> AppResult<ImageRequestMeta> {
    let boundary = multipart_boundary(content_type_text)?;
    let mut model = None;
    let mut stream = false;
    for (name, value) in multipart_text_fields(body, &boundary)? {
        match name.as_str() {
            "model" if !value.is_empty() => model = Some(value),
            "stream" => stream = value == "true",
            _ => {}
        }
    }
    let model = model.ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    Ok(ImageRequestMeta {
        model,
        stream,
        content_type,
    })
}

fn multipart_boundary(content_type: &str) -> AppResult<String> {
    for part in content_type.split(';').skip(1) {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("boundary") {
            let boundary = value.trim().trim_matches('"');
            if boundary.is_empty() {
                break;
            }
            return Ok(boundary.to_string());
        }
    }
    Err(AppError::BadRequest(
        "multipart/form-data boundary is required".to_string(),
    ))
}

fn multipart_text_fields(body: &[u8], boundary: &str) -> AppResult<Vec<(String, String)>> {
    let marker = format!("--{boundary}").into_bytes();
    let mut fields = Vec::new();
    let Some(mut cursor) = find_bytes(body, &marker) else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };

    loop {
        cursor += marker.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        cursor = skip_line_break(body, cursor)?;
        let Some(next_marker_offset) = find_bytes(&body[cursor..], &marker) else {
            return Err(AppError::BadRequest("invalid multipart body".to_string()));
        };
        let mut part = &body[cursor..cursor + next_marker_offset];
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        if let Some((name, value)) = multipart_text_field(part)? {
            fields.push((name, value));
        }
        cursor += next_marker_offset;
    }

    Ok(fields)
}

fn skip_line_break(body: &[u8], cursor: usize) -> AppResult<usize> {
    if body.get(cursor..cursor + 2) == Some(b"\r\n") {
        return Ok(cursor + 2);
    }
    if body.get(cursor..cursor + 1) == Some(b"\n") {
        return Ok(cursor + 1);
    }
    Err(AppError::BadRequest("invalid multipart body".to_string()))
}

fn multipart_text_field(part: &[u8]) -> AppResult<Option<(String, String)>> {
    let (headers, value) = if let Some(offset) = find_bytes(part, b"\r\n\r\n") {
        (&part[..offset], &part[offset + 4..])
    } else if let Some(offset) = find_bytes(part, b"\n\n") {
        (&part[..offset], &part[offset + 2..])
    } else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };
    let headers = std::str::from_utf8(headers)
        .map_err(|_| AppError::BadRequest("invalid multipart headers".to_string()))?;
    let Some(disposition) = headers.lines().find(|line| {
        line.to_ascii_lowercase()
            .starts_with("content-disposition:")
    }) else {
        return Ok(None);
    };
    if disposition.contains("filename=") {
        return Ok(None);
    }
    let Some(name) = multipart_disposition_name(disposition) else {
        return Ok(None);
    };
    let value = std::str::from_utf8(value)
        .map_err(|_| AppError::BadRequest("invalid multipart text field".to_string()))?
        .trim()
        .to_string();
    Ok(Some((name, value)))
}

fn multipart_disposition_name(disposition: &str) -> Option<String> {
    let (_, params) = disposition.split_once(':')?;
    for param in params.split(';').skip(1) {
        let (key, value) = param.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case("name") {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn select_upstream(
    state: &AppState,
    path: &'static str,
    model: &str,
    affinity_key: Option<&ChannelAffinityKey>,
) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
    if path == "/v1/responses" {
        match state
            .selector
            .select_with_affinity(
                &state.db.pool,
                &state.secrets,
                &state.channel_affinity,
                UpstreamProtocol::OpenAiOauth,
                model,
                affinity_key,
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
        .select_with_affinity(
            &state.db.pool,
            &state.secrets,
            &state.channel_affinity,
            UpstreamProtocol::Openai,
            model,
            affinity_key,
        )
        .await?;
    Ok((UpstreamProtocol::Openai, upstream))
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
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};

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

    #[test]
    fn response_subresource_path_uses_upstream_id_and_preserves_query() {
        let uri: Uri = "/v1/responses/resp_client/input_items?limit=20&after=item_1"
            .parse()
            .unwrap();

        let path = response_subresource_path("resp_upstream", &uri, "input_items");

        assert_eq!(
            path,
            "/v1/responses/resp_upstream/input_items?limit=20&after=item_1"
        );
    }

    #[test]
    fn parses_json_image_generation_meta() {
        let body = br#"{"model":"gpt-image-1","prompt":"draw","stream":true}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let meta = image_request_meta("/v1/images/generations", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(meta.stream);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("application/json")
        );
    }

    #[test]
    fn parses_json_image_edit_meta() {
        let body = br#"{"model":"gpt-image-2","prompt":"edit","images":[{"image_url":"data:image/png;base64,AAAA"}],"stream":true,"partial_images":2}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-2");
        assert!(meta.stream);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("application/json")
        );
    }

    #[test]
    fn parses_multipart_image_edit_model_without_rewriting_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=----neogate-boundary"),
        );
        let body = b"------neogate-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n------neogate-boundary--\r\n";

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(!meta.stream);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("multipart/form-data; boundary=----neogate-boundary")
        );
    }

    #[test]
    fn parses_multipart_image_edit_stream_flag() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=\"quoted-boundary\""),
        );
        let body = b"--quoted-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n--quoted-boundary\r\nContent-Disposition: form-data; name=\"stream\"\r\n\r\ntrue\r\n--quoted-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n--quoted-boundary--\r\n";

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(meta.stream);
    }

    #[test]
    fn multipart_image_variation_requires_model() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );
        let body = b"--x\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n--x--\r\n";

        let err = image_request_meta("/v1/images/variations", &headers, body).unwrap_err();

        assert!(err.to_string().contains("model is required"));
    }

    #[test]
    fn image_generation_rejects_multipart_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );

        let err = image_request_meta(
            "/v1/images/generations",
            &headers,
            b"--x\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n--x--\r\n",
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("image generations require application/json"));
    }

    #[test]
    fn image_variation_rejects_json_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let err = image_request_meta(
            "/v1/images/variations",
            &headers,
            br#"{"model":"dall-e-2","image":"data:image/png;base64,AAAA"}"#,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("image variations require multipart/form-data"));
    }

    #[test]
    fn multipart_image_request_requires_boundary() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data"),
        );

        let err = image_request_meta("/v1/images/edits", &headers, b"").unwrap_err();

        assert!(err
            .to_string()
            .contains("multipart/form-data boundary is required"));
    }

    #[test]
    fn multipart_image_request_rejects_invalid_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );

        let err = image_request_meta("/v1/images/edits", &headers, b"not multipart").unwrap_err();

        assert!(err.to_string().contains("invalid multipart body"));
    }

    #[test]
    fn image_request_rejects_unsupported_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));

        let err = image_request_meta("/v1/images/generations", &headers, b"model=gpt-image-1")
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("application/json or multipart/form-data"));
    }
}
