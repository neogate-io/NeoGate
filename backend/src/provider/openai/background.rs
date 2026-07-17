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
    billing::{parse_usage_from_bytes, BillingMeter},
    error::{reqwest_status, AppError, AppResult},
    project::models::UsageRoutingSnapshot,
    provider::adapters::adapter_for_endpoint,
    relay::{
        ensure_key_backed_async_upstream, finish_relay, forward_openai, forward_openai_bound,
        handle_upstream_http_error, release_empty_hold, reserve_billable_credit, reserve_credit,
        response_from_bytes,
        selector::{SelectedUpstream, SelectionConstraints, UpstreamProtocol},
        task_status_from_value, ChannelAffinityKey, RelayContext, RelayRequestParams,
    },
    task::{billing as task_billing, jobs, upstream as upstream_task},
    AppState,
};

use crate::task::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType};

pub(super) struct CreateBackgroundResponseRequest {
    pub(super) state: Arc<AppState>,
    pub(super) auth: UserAuth,
    pub(super) body: Bytes,
    pub(super) external_model: String,
    pub(super) target_model: String,
    pub(super) target_channel_id: Option<i64>,
    pub(super) routing: Option<UsageRoutingSnapshot>,
    pub(super) output_tokens: i64,
    pub(super) request_params: RelayRequestParams,
    pub(super) channel_affinity_key: Option<ChannelAffinityKey>,
}

pub(super) async fn create_background_response(
    req: CreateBackgroundResponseRequest,
) -> AppResult<Response> {
    let CreateBackgroundResponseRequest {
        state,
        auth,
        body,
        external_model,
        target_model,
        target_channel_id,
        routing,
        output_tokens,
        request_params,
        channel_affinity_key,
    } = req;
    let started = Instant::now();
    let prepared = jobs::prepare_request_body(body)?;
    let upstream = if let Some(channel_id) = target_channel_id {
        state
            .selector
            .select_bound_channel_protocols(
                &state.db.pool,
                &state.secrets,
                &[UpstreamProtocol::Openai],
                &target_model,
                channel_id,
                SelectionConstraints::default(),
            )
            .await?
            .1
    } else {
        state
            .selector
            .select_with_affinity(
                &state.db.pool,
                &state.secrets,
                &state.channel_affinity,
                UpstreamProtocol::Openai,
                &target_model,
                SelectionConstraints {
                    affinity_key: channel_affinity_key.as_ref(),
                    attempted: &[],
                    excluded_endpoint_ids: &[],
                },
            )
            .await?
    };
    ensure_key_backed_async_upstream(&upstream)?;
    let adapter = adapter_for_endpoint(&upstream.provider, &upstream.base_url);
    let translates_image_generation = adapter.capabilities().translates_response_image_generation
        && prepared.has_image_generation_tool;
    if translates_image_generation {
        let image_request = adapter
            .prepare_response_image_generation_request(prepared.body.clone())?
            .ok_or_else(|| {
                AppError::BadRequest(
                    "provider adapter did not prepare image generation request".to_string(),
                )
            })?;
        let image_resolved = crate::project::models::resolve_project_model(
            &state.db.pool,
            auth.project_id,
            &image_request.model,
        )
        .await?;
        let image_upstream = if let Some(channel_id) = image_resolved.target_channel_id {
            state
                .selector
                .select_bound_channel_protocols(
                    &state.db.pool,
                    &state.secrets,
                    &[UpstreamProtocol::Openai],
                    &image_resolved.target_model,
                    channel_id,
                    SelectionConstraints::default(),
                )
                .await?
                .1
        } else {
            state
                .selector
                .select_with_affinity(
                    &state.db.pool,
                    &state.secrets,
                    &state.channel_affinity,
                    UpstreamProtocol::Openai,
                    &image_resolved.target_model,
                    SelectionConstraints::default(),
                )
                .await?
        };
        ensure_key_backed_async_upstream(&image_upstream)?;
        let image_price = state
            .billing
            .price_for(
                &state.db.pool,
                image_upstream.channel_id,
                &image_resolved.target_model,
                &auth.user_group,
            )
            .await?;
        let image_credit_account = auth
            .model_credit_account(&image_resolved.external_model)
            .cloned();
        let image_count = serde_json::from_slice::<Value>(&image_request.body)
            .ok()
            .and_then(|value| value.get("n").and_then(Value::as_i64))
            .filter(|count| *count > 0)
            .unwrap_or(1);
        let hold = if image_price.billing_meter == BillingMeter::Image {
            reserve_billable_credit(
                &state,
                &auth,
                image_credit_account.as_ref(),
                image_count.saturating_mul(
                    image_price
                        .unit_price_micros
                        .ok_or_else(|| {
                            AppError::BadRequest(
                                "unit price is required for image billing".to_string(),
                            )
                        })?
                        .max(0),
                ),
            )
            .await?
        } else {
            reserve_credit(
                &state,
                &auth,
                image_credit_account.as_ref(),
                &image_request.body,
                state.billing.default_output_tokens(),
                &image_price,
            )
            .await?
        };
        let response = match jobs::create(
            &state,
            &auth,
            jobs::CreateNeogateResponse {
                upstream: &image_upstream,
                response_model: &target_model,
                image_model: &image_resolved.external_model,
                upstream_image_model: &image_resolved.target_model,
                request_body: prepared.body.clone(),
                upstream_request_body: image_request.body,
                image_format: prepared.image_format,
                hold: &hold,
            },
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
    let price = state
        .billing
        .price_for(
            &state.db.pool,
            upstream.channel_id,
            &target_model,
            &auth.user_group,
        )
        .await?;
    let user_key_model_credit_account = auth.model_credit_account(&external_model).cloned();
    let hold = reserve_credit(
        &state,
        &auth,
        user_key_model_credit_account.as_ref(),
        &prepared.body,
        output_tokens,
        &price,
    )
    .await?;
    if prepared.image_format.requires_neogate_asset_url() {
        release_empty_hold(&state, hold, "unsupported neogate image_format").await;
        return Err(AppError::BadRequest(
            "image_format=url or both is only supported for NeoGate async image tasks".to_string(),
        ));
    }
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let request_body_bytes = prepared.body.len();
    let request_input_tokens_estimate = crate::billing::estimate_input_tokens(&prepared.body);
    let response = forward_openai(
        &state,
        &upstream,
        UpstreamProtocol::Openai,
        prepared.body,
        "/v1/responses",
    )
    .await;
    let mut ctx = RelayContext {
        state: Arc::clone(&state),
        auth: auth.clone(),
        upstream: upstream.clone(),
        protocol: UpstreamProtocol::Openai,
        path: "/v1/responses",
        model: target_model.clone(),
        external_model: external_model.clone(),
        upstream_model: target_model.clone(),
        routing,
        streamed: false,
        price,
        hold: hold.clone(),
        user_key_model_credit_account,
        started,
        channel_affinity_key,
        relay_trace_id: Uuid::new_v4(),
        relay_attempt: 1,
        relay_final: true,
        request_body_bytes,
        request_input_tokens_estimate,
        request_params,
        request_permit: request_permit.take(),
        upstream_request_path: Some("/v1/responses".to_string()),
        upstream_response_mode: None,
    };

    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = reqwest_status(upstream_response.status());
    if !status.is_success() {
        return handle_upstream_http_error(ctx, status, upstream_response).await;
    }

    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| axum::http::HeaderValue::from_static("application/json"));
    let body = match upstream_response.bytes().await {
        Ok(body) => {
            ctx.release_request_permit();
            body
        }
        Err(err) => {
            ctx.release_request_permit();
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
            model: Some(&external_model),
            upstream_model: Some(&target_model),
            status: status_text,
            terminal,
            hold: &hold,
            upstream_metadata: value.clone(),
        },
        crate::task::POLL_INTERVAL,
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
    let status = reqwest_status(upstream_response.status());
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
    let status = reqwest_status(response.status());
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
            poll_interval: crate::task::POLL_INTERVAL,
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
