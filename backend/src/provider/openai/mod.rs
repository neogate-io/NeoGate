mod assets;
mod audio;
mod background;
mod images;
mod multipart;
mod realtime;
mod videos;

pub(crate) use assets::{openai_asset_detail, openai_assets_create};
pub(crate) use audio::openai_audio_transcriptions;
pub(crate) use background::response_terminal;
pub(crate) use realtime::openai_realtime;
pub(crate) use videos::{video_status_text, video_terminal};

use std::{sync::Arc, time::Instant};

use axum::{
    extract::{OriginalUri, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode, Uri},
    response::Response,
};
use bytes::Bytes;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    error::{reqwest_status, AppError, AppResult},
    provider::adapters::{adapter_for_endpoint, AdapterResponseMode, RelayRoute},
    task::{jobs, upstream as upstream_task},
    AppState,
};

use crate::billing::DebitHold;
use crate::relay::raw_upstream_response;
use crate::relay::{
    bridge, describe_upstream_http_failure, finish_relay, finish_task_json_response,
    forward_anthropic, forward_openai_bound, forward_prepared_openai, log_upstream_http_failure,
    prepare_relay_body, read_upstream_error_body, record_upstream_http_failure,
    record_upstream_transport_failure_for_failover, reserve_credit, respond_upstream_http_failure,
    rewrite_relay_body_model,
    selector::{
        AttemptedUpstream, ModelCooldown, SelectedUpstream, SelectionConstraints, UpstreamProtocol,
    },
    should_failover_upstream_failure, BodyKind, ChannelAffinityKey, PreparedRelayBody, RelayBody,
    RelayContext, UpstreamFailureKind,
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
const OPENAI_RESPONSES_COMPACT_PROTOCOLS: [UpstreamProtocol; 2] =
    [UpstreamProtocol::OpenAiOauth, UpstreamProtocol::Openai];

fn project_model_request_context(
    body: &Bytes,
) -> Option<crate::project::models::ProjectModelRequestContext> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .map(|value| crate::project::models::ProjectModelRequestContext::from_value(&value))
}

fn content_type_header(headers: &HeaderMap) -> AppResult<(HeaderValue, String)> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let text = content_type
        .to_str()
        .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?
        .to_string();
    Ok((content_type, text))
}

fn json_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn required_json_string_field(value: &Value, key: &str) -> AppResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| AppError::BadRequest(format!("{key} is required")))
}

fn positive_i64_text(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok().filter(|value| *value > 0)
}

fn positive_i64_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_str().and_then(positive_i64_text))
            .filter(|value| *value > 0)
    })
}

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
        RelayRoute::ChatCompletions,
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
        RelayRoute::Embeddings,
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
        RelayRoute::Moderations,
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
        RelayRoute::Responses,
        BodyKind::OpenaiResponses,
    )
    .await
}

pub(crate) async fn openai_responses_compact(
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
        RelayRoute::ResponsesCompact,
        BodyKind::OpenaiResponsesCompact,
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

pub(crate) async fn openai_videos(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    body: RelayBody,
) -> AppResult<Response> {
    videos::openai_videos(state, auth, headers, body).await
}

pub(crate) async fn openai_video(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    path: Path<String>,
) -> AppResult<Response> {
    videos::openai_video(state, auth, path).await
}

pub(crate) async fn openai_video_content(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    path: Path<String>,
) -> AppResult<Response> {
    videos::openai_video_content(state, auth, path).await
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
        .map_or_else(|| uri.path(), |value| value.as_str());
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
    route: RelayRoute,
    body_kind: BodyKind,
) -> AppResult<Response> {
    let path = route.path();
    let PreparedRelayBody {
        body,
        meta,
        output_tokens,
    } = prepare_relay_body(body, body_kind, state.billing.default_output_tokens())?;
    let routing_context = project_model_request_context(&body);
    let resolved = crate::project::models::resolve_project_model_with_context(
        &state.db.pool,
        auth.project_id,
        &meta.model,
        routing_context,
    )
    .await?;
    let upstream_body = if resolved.target_model == meta.model {
        body.clone()
    } else {
        rewrite_relay_body_model(body.clone(), body_kind, &resolved.target_model)?
    };
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
            background::CreateBackgroundResponseRequest {
                state,
                auth,
                body: upstream_body,
                external_model: resolved.external_model,
                target_model: resolved.target_model,
                target_channel_id: resolved.target_channel_id,
                routing: resolved.routing,
                output_tokens,
                request_params: meta.request_params,
                channel_affinity_key: meta.channel_affinity_key,
            },
        )
        .await;
    }
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let user_key_model_credit_account =
        auth.model_credit_account(&resolved.external_model).cloned();
    let channel_affinity_key = meta.channel_affinity_key.clone();
    let relay_trace_id = Uuid::new_v4();

    let mut model_unavailable_reroutes = 0;
    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();
    let mut reuse_upstream: Option<(UpstreamProtocol, SelectedUpstream)> = None;
    let mut reuse_hold: Option<DebitHold> = None;
    let mut responses_downgraded = false;
    let mut relay_attempt_counter = 0i32;
    loop {
        let started = Instant::now();
        let reused = reuse_upstream.is_some();
        let (protocol, mut upstream) = match reuse_upstream.take() {
            Some(prev) => prev,
            None => {
                select_upstream_excluding(
                    &state,
                    path,
                    &resolved.target_model,
                    resolved.target_channel_id,
                    channel_affinity_key.as_ref(),
                    &attempted_upstreams,
                )
                .await?
            }
        };
        relay_attempt_counter += 1;
        // 路径 B：已学习到该 (endpoint, model) 不支持 /v1/responses → 覆写为 chat 降级。
        // reuse 路径的 upstream 已是降级版（responses_chat_fallback=true），此处不重复命中。
        if !upstream.responses_chat_fallback
            && route == RelayRoute::Responses
            && matches!(
                protocol,
                UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
            )
            && state
                .selector
                .responses_unsupported(upstream.channel_endpoint_id, &resolved.target_model)
                .await
        {
            tracing::debug!(
                provider = %upstream.provider,
                channel_id = upstream.channel_id,
                channel_name = %upstream.channel_name,
                channel_endpoint_id = upstream.channel_endpoint_id,
                protocol = protocol.as_str(),
                model = %meta.model,
                target_model = %resolved.target_model,
                path,
                "downgrading responses to chat fallback (learned unsupported for this model)"
            );
            upstream.responses_chat_fallback = true;
        }
        if !reused {
            attempted_upstreams.push(AttemptedUpstream::from(&upstream));
        }
        let price = state
            .billing
            .price_for(
                &state.db.pool,
                upstream.channel_id,
                &resolved.target_model,
                &auth.user_group,
            )
            .await?;
        let hold = if reused {
            reuse_hold
                .take()
                .expect("reuse hold is set alongside reuse_upstream")
        } else {
            reserve_credit(
                &state,
                &auth,
                user_key_model_credit_account.as_ref(),
                &upstream_body,
                output_tokens,
                &price,
            )
            .await?
        };
        let mut ctx = RelayContext {
            state: Arc::clone(&state),
            auth: auth.clone(),
            upstream,
            protocol,
            method: "POST",
            path,
            model: resolved.target_model.clone(),
            external_model: resolved.external_model.clone(),
            upstream_model: resolved.target_model.clone(),
            routing: resolved.routing.clone(),
            streamed: meta.stream,
            price,
            hold,
            user_key_model_credit_account: user_key_model_credit_account.clone(),
            started,
            channel_affinity_key: channel_affinity_key.clone(),
            relay_trace_id,
            relay_attempt: relay_attempt_counter,
            relay_final: false,
            request_body_bytes: upstream_body.len(),
            request_input_tokens_estimate: crate::billing::estimate_input_tokens(&upstream_body),
            request_params: meta.request_params.clone(),
            request_permit: None,
            upstream_request_path: None,
            upstream_response_mode: None,
        };
        let mut adapter_response_mode = AdapterResponseMode::Passthrough;
        let response = match protocol {
            UpstreamProtocol::Anthropic if path == "/v1/chat/completions" => {
                let body = bridge::openai_chat_to_anthropic_messages(upstream_body.clone())?;
                forward_anthropic(&state, &HeaderMap::new(), &ctx.upstream, body).await
            }
            UpstreamProtocol::Anthropic if path == "/v1/responses" => {
                let body = bridge::openai_response_to_anthropic_messages(upstream_body.clone())?;
                forward_anthropic(&state, &HeaderMap::new(), &ctx.upstream, body).await
            }
            UpstreamProtocol::Anthropic => Err(AppError::BadRequest(format!(
                "Anthropic fallback is not supported for {path}"
            ))),
            UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth => {
                let adapter = adapter_for_endpoint(
                    &ctx.upstream.provider,
                    &ctx.upstream.base_url,
                    ctx.upstream.adapter_hint.as_deref(),
                );
                let prepared = adapter.prepare_openai_request(
                    &ctx.upstream,
                    protocol,
                    route,
                    upstream_body.clone(),
                    &headers,
                    meta.stream,
                )?;
                ctx.upstream_request_path = Some(prepared.log_path.clone());
                ctx.upstream_response_mode = Some(prepared.response_mode.as_str());
                adapter_response_mode = prepared.response_mode;
                forward_prepared_openai(&state, &ctx.upstream, protocol, &headers, prepared).await
            }
        };

        match response {
            Ok(upstream_response) => {
                let status = reqwest_status(upstream_response.status());
                if status.is_success() {
                    mark_credential_model_available(&ctx).await?;
                    ctx.mark_final_with_permit(&mut request_permit);
                    return finish_openai_relay_success(
                        ctx,
                        status,
                        upstream_response,
                        adapter_response_mode,
                    )
                    .await;
                }

                let body = read_upstream_error_body(upstream_response).await;
                let failure = describe_upstream_http_failure(status, &body);
                // 路径 C：responses 路由首次收到「模型不可用」类错误 → 学习该 (endpoint, model)
                // 不支持 responses，并就地降级为 chat 重试同一 upstream（不重新 select、不写
                // ModelBlockKey/channel_model，避免误伤该模型的 chat 路径）。chat 重试若再失败，
                // 落入下方 model-unavailable 逻辑自然区分「不支持 responses 形态」与「model 真不存在」。
                if route == RelayRoute::Responses
                    && matches!(
                        ctx.protocol,
                        UpstreamProtocol::Openai | UpstreamProtocol::OpenAiOauth
                    )
                    && failure.kind == UpstreamFailureKind::ModelUnavailable
                    && !responses_downgraded
                    && !ctx
                        .state
                        .selector
                        .responses_unsupported(ctx.upstream.channel_endpoint_id, &ctx.model)
                        .await
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
                        "upstream rejected responses for this model; downgrading to chat retry"
                    );
                    let mut fallback = ctx.upstream.clone();
                    fallback.responses_chat_fallback = true;
                    reuse_upstream = Some((ctx.protocol, fallback));
                    reuse_hold = Some(ctx.hold.clone());
                    responses_downgraded = true;
                    continue;
                }
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
                        ctx.mark_final_with_permit(&mut request_permit);
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
                        ctx.mark_final_with_permit(&mut request_permit);
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

                if should_failover_upstream_failure(
                    &ctx,
                    &attempted_upstreams,
                    failure.failoverable(),
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
                        "failoverable upstream http failure; trying another upstream"
                    );
                    continue;
                }

                ctx.mark_final_with_permit(&mut request_permit);
                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => {
                let retryable = err.retryable();
                if should_failover_upstream_failure(
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
                ctx.mark_final_with_permit(&mut request_permit);
                return finish_relay(ctx, Err(err)).await;
            }
        }
    }
}

async fn finish_openai_relay_success(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
    adapter_response_mode: AdapterResponseMode,
) -> AppResult<Response> {
    match (ctx.protocol, ctx.path, adapter_response_mode) {
        (UpstreamProtocol::Anthropic, "/v1/chat/completions", _) => {
            bridge::finish_anthropic_as_openai_chat(ctx, status, upstream_response).await
        }
        (UpstreamProtocol::Anthropic, "/v1/responses", _) => {
            bridge::finish_anthropic_as_openai_response(ctx, status, upstream_response).await
        }
        (
            UpstreamProtocol::Openai,
            "/v1/responses",
            AdapterResponseMode::OpenAiChatAsOpenAiResponse,
        ) => bridge::finish_openai_chat_as_openai_response(ctx, status, upstream_response).await,
        (UpstreamProtocol::Openai, "/v1/responses", AdapterResponseMode::Passthrough) => {
            bridge::finish_openai_response_with_reasoning_normalization(
                ctx,
                status,
                upstream_response,
            )
            .await
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
        .is_some_and(|pairs| {
            pairs
                .iter()
                .any(|(key, value)| key == "stream" && value == "true")
        })
}

async fn select_upstream_excluding(
    state: &AppState,
    path: &'static str,
    model: &str,
    target_channel_id: Option<i64>,
    affinity_key: Option<&ChannelAffinityKey>,
    attempted: &[AttemptedUpstream],
) -> AppResult<(UpstreamProtocol, SelectedUpstream)> {
    let protocols = match path {
        "/v1/chat/completions" => &OPENAI_CHAT_PROTOCOLS[..],
        "/v1/responses" => &OPENAI_RESPONSES_PROTOCOLS[..],
        "/v1/responses/compact" => &OPENAI_RESPONSES_COMPACT_PROTOCOLS[..],
        _ => &OPENAI_PROTOCOLS[..],
    };
    let excluded_endpoint_ids = if path == "/v1/responses" {
        state
            .selector
            .responses_unsupported_endpoint_ids(&state.db.pool, protocols, model)
            .await?
    } else {
        Vec::new()
    };

    if let Some(channel_id) = target_channel_id {
        let selected = state
            .selector
            .select_bound_channel_protocols(
                &state.db.pool,
                &state.secrets,
                protocols,
                model,
                channel_id,
                SelectionConstraints {
                    attempted,
                    excluded_endpoint_ids: &excluded_endpoint_ids,
                    ..SelectionConstraints::default()
                },
            )
            .await;
        if selected.is_ok() || excluded_endpoint_ids.is_empty() {
            return selected;
        }
        return state
            .selector
            .select_bound_channel_protocols(
                &state.db.pool,
                &state.secrets,
                protocols,
                model,
                channel_id,
                SelectionConstraints {
                    attempted,
                    ..SelectionConstraints::default()
                },
            )
            .await;
    }

    let selected = state
        .selector
        .select_with_affinity_excluding_protocols(
            &state.db.pool,
            &state.secrets,
            &state.channel_affinity,
            protocols,
            model,
            SelectionConstraints {
                affinity_key,
                attempted,
                excluded_endpoint_ids: &excluded_endpoint_ids,
            },
        )
        .await;
    if selected.is_ok() || excluded_endpoint_ids.is_empty() {
        return selected;
    }
    state
        .selector
        .select_with_affinity_excluding_protocols(
            &state.db.pool,
            &state.secrets,
            &state.channel_affinity,
            protocols,
            model,
            SelectionConstraints {
                affinity_key,
                attempted,
                excluded_endpoint_ids: &[],
            },
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
    ) && failure.kind == UpstreamFailureKind::ModelUnavailable
}

#[cfg(test)]
mod tests;
