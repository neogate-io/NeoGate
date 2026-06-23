use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    error::{AppError, AppResult},
    task::{
        billing as task_billing, results::AnthropicResultsUsageParser, upstream as upstream_task,
    },
    AppState,
};

use crate::relay::{
    bridge, describe_upstream_http_failure, ensure_key_backed_async_upstream, finish_relay,
    finish_task_json_response, forward_anthropic, forward_anthropic_bound, forward_openai,
    log_upstream_http_failure, prepare_relay_body, raw_upstream_response, read_upstream_error_body,
    record_upstream_http_failure, record_upstream_transport_failure_for_failover,
    release_empty_hold, reserve_credit, respond_upstream_http_failure, response_from_bytes,
    selector::{AttemptedUpstream, SelectedUpstream, UpstreamProtocol},
    should_failover_retryable_upstream_failure, BodyKind, PreparedRelayBody, RelayBody,
    RelayContext,
};
use crate::task::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType};

#[derive(Debug, Deserialize)]
pub(crate) struct AnthropicBatchListQuery {
    limit: Option<i64>,
    after_id: Option<String>,
    before_id: Option<String>,
}

const ANTHROPIC_MESSAGE_PROTOCOLS: [UpstreamProtocol; 2] =
    [UpstreamProtocol::Anthropic, UpstreamProtocol::Openai];

pub(crate) async fn anthropic_messages(
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
    let channel_affinity_key = meta.channel_affinity_key.clone();
    let relay_trace_id = Uuid::new_v4();

    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();
    loop {
        let started = Instant::now();
        let (protocol, upstream) = state
            .selector
            .select_with_affinity_excluding_protocols(
                &state.db.pool,
                &state.secrets,
                &state.channel_affinity,
                &ANTHROPIC_MESSAGE_PROTOCOLS,
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
            path: "/v1/messages",
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
            UpstreamProtocol::Anthropic => {
                forward_anthropic(&state, &headers, &ctx.upstream, body.clone()).await
            }
            UpstreamProtocol::Openai => {
                let body = bridge::messages_to_openai_chat(body.clone())?;
                forward_openai(
                    &state,
                    &ctx.upstream,
                    protocol,
                    body,
                    "/v1/chat/completions",
                )
                .await
            }
            UpstreamProtocol::OpenAiOauth => Err(AppError::BadRequest(
                "openai_oauth does not support Anthropic messages".to_string(),
            )),
        };

        match response {
            Ok(upstream_response) => {
                let status = StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                if status.is_success() {
                    ctx.relay_final = true;
                    if protocol == UpstreamProtocol::Openai {
                        return bridge::finish_chat_as_anthropic(ctx, status, upstream_response)
                            .await;
                    }
                    return finish_relay(ctx, Ok(upstream_response)).await;
                }

                let body = read_upstream_error_body(upstream_response).await;
                let failure = describe_upstream_http_failure(status, &body);
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
                    log_anthropic_transport_failover(&ctx, &summary, retryable_failovers);
                    record_upstream_transport_failure_for_failover(&ctx, summary).await;
                    continue;
                }
                ctx.relay_final = true;
                return finish_relay(ctx, Err(err)).await;
            }
        }
    }
}

fn log_anthropic_transport_failover(ctx: &RelayContext, summary: &str, failover_attempt: usize) {
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

pub(crate) async fn create_anthropic_message_batch(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    let model = batch_model(&body)?;
    let request_count = batch_request_count(&body)?;
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
    finish_batch_create(state, auth, upstream, model, hold, response).await
}

pub(crate) async fn list_anthropic_message_batches(
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

pub(crate) async fn anthropic_message_batch(
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

pub(crate) async fn cancel_anthropic_message_batch(
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

pub(crate) async fn delete_anthropic_message_batch(
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

pub(crate) async fn anthropic_message_batch_results(
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
    finish_results_response(state, auth, task, response).await
}

async fn finish_batch_create(
    state: Arc<AppState>,
    auth: UserAuth,
    upstream: SelectedUpstream,
    model: String,
    hold: crate::billing::DebitHold,
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
            terminal: batch_terminal(&status_text),
            hold: &hold,
            upstream_metadata: value,
        },
        state.config.task.upstream_poll_interval,
        state.config.task.upstream_retention,
    )
    .await
    {
        release_empty_hold(&state, hold, "anthropic batch task insert error").await;
        return Err(err);
    }
    response_from_bytes(status, content_type, body)
}

async fn finish_results_response(
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

struct AnthropicResultsRelay {
    state: Option<Arc<AppState>>,
    auth: Option<UserAuth>,
    task: Option<UpstreamTask>,
    status: StatusCode,
    usage_parser: AnthropicResultsUsageParser,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
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

pub(crate) fn batch_terminal(status: &str) -> bool {
    matches!(status, "ended" | "canceled" | "cancelled" | "expired")
}

fn batch_model(body: &[u8]) -> AppResult<String> {
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

fn batch_request_count(body: &[u8]) -> AppResult<i64> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_model_reads_first_request_params() {
        let body = Bytes::from_static(
            br#"{"requests":[{"custom_id":"one","params":{"model":"claude-sonnet"}}]}"#,
        );

        assert_eq!(batch_model(&body).unwrap(), "claude-sonnet");
    }

    #[test]
    fn batch_model_rejects_mixed_models() {
        let body = Bytes::from_static(
            br#"{"requests":[{"params":{"model":"claude-a"}},{"params":{"model":"claude-b"}}]}"#,
        );

        assert!(batch_model(&body).is_err());
    }

    #[test]
    fn batch_terminal_statuses_are_detected() {
        assert!(batch_terminal("ended"));
        assert!(batch_terminal("cancelled"));
        assert!(batch_terminal("expired"));
        assert!(!batch_terminal("in_progress"));
    }

    #[test]
    fn results_usage_sums_successful_messages() {
        let body = br#"{"custom_id":"one","result":{"type":"succeeded","message":{"usage":{"input_tokens":7,"output_tokens":3}}}}
{"custom_id":"two","result":{"type":"succeeded","message":{"usage":{"input_tokens":5,"output_tokens":2}}}}
"#;

        let usage = crate::task::results::anthropic_results_usage(body).unwrap();

        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 5);
    }
}
