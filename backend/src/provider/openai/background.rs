use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    http::{header, Method, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::parse_usage_from_bytes,
    error::{AppError, AppResult},
    provider::newapi,
    relay::{
        ensure_key_backed_async_upstream, finish_relay, forward_openai, forward_openai_bound,
        handle_upstream_http_error, release_empty_hold, reserve_credit, response_from_bytes,
        selector::{SelectedUpstream, UpstreamProtocol},
        task_status_from_value, ChannelAffinityKey, RelayContext, RelayRequestParams,
    },
    task::{billing as task_billing, jobs, upstream as upstream_task},
    AppState,
};

use crate::task::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType};

pub(super) async fn create_background_response(
    state: Arc<AppState>,
    auth: UserAuth,
    body: Bytes,
    model: String,
    output_tokens: i64,
    request_params: RelayRequestParams,
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
        request_params,
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

pub(super) fn finish_stream_response(
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
