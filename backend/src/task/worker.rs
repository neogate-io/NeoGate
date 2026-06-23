use std::{future::Future, sync::Arc};

use axum::http::{HeaderMap, Method, StatusCode};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::{stream, StreamExt};
use serde_json::Value;

use crate::{
    billing::{parse_usage_from_bytes, TokenUsage},
    error::AppResult,
    relay::{forward_anthropic_bound, forward_openai_bound, selector::SelectedUpstream},
    AppState,
};

use super::{
    billing as task_billing, jobs,
    results::AnthropicResultsUsageParser,
    upstream::{self, UpstreamTask, UpstreamTaskType},
};

const MAX_CONCURRENT_POLLED_TASKS: usize = 8;

pub(crate) fn spawn(state: Arc<AppState>) {
    if !state.config.process_role.runs_background() {
        return;
    }
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(state.config.task.upstream_poll_interval);
        loop {
            ticker.tick().await;
            if let Err(err) = poll_due_tasks(&state).await {
                tracing::warn!("failed to poll upstream async tasks: {err}");
            }
        }
    });
}

async fn poll_due_tasks(state: &Arc<AppState>) -> AppResult<()> {
    let tasks = upstream::claim_due_tasks(
        &state.db.pool,
        state.config.task.upstream_poll_batch_size,
        state.config.task.upstream_poll_interval,
    )
    .await?;
    let concurrency = tasks.len().min(MAX_CONCURRENT_POLLED_TASKS).max(1);
    stream::iter(tasks)
        .for_each_concurrent(concurrency, |task| {
            let state = Arc::clone(state);
            async move {
                let task_id = task.id;
                let task_type = task.task_type;
                match poll_task(&state, task).await {
                    Ok(()) => {}
                    Err(err) => {
                        tracing::warn!(
                            task_id,
                            ?task_type,
                            "failed to poll one upstream async task: {err}"
                        );
                    }
                }
            }
        })
        .await;
    release_stale_terminal_holds(state).await?;
    cleanup_expired_tasks(state).await?;
    Ok(())
}

async fn release_stale_terminal_holds(state: &Arc<AppState>) -> AppResult<()> {
    let stale_window =
        ChronoDuration::from_std(state.config.billing.credit_allocation_recovery_after)
            .unwrap_or_else(|_| ChronoDuration::seconds(900));
    let stale_before = Utc::now() - stale_window;
    let tasks = upstream::fetch_stale_terminal_held_tasks(
        &state.db.pool,
        stale_before,
        state.config.task.upstream_poll_batch_size,
    )
    .await?;
    for task in tasks {
        task_billing::release_task_hold_by_id(state, task.id, "stale terminal async task").await?;
    }
    Ok(())
}

async fn cleanup_expired_tasks(state: &Arc<AppState>) -> AppResult<()> {
    jobs::cleanup_expired_assets(state, state.config.task.upstream_poll_batch_size).await?;
    let deleted = upstream::delete_expired_terminal_tasks(
        &state.db.pool,
        state.config.task.upstream_poll_batch_size,
    )
    .await?;
    if deleted > 0 {
        tracing::info!(deleted, "deleted expired upstream async task metadata");
    }
    Ok(())
}

async fn poll_task(state: &Arc<AppState>, task: UpstreamTask) -> AppResult<()> {
    if task.task_type == UpstreamTaskType::NeogateResponse {
        return jobs::run(state, task).await;
    }
    let upstream = task
        .selected_upstream(&state.db.pool, &state.secrets)
        .await?;
    let response = match task.task_type {
        UpstreamTaskType::OpenAiResponse => {
            let path = format!("/v1/responses/{}", task.upstream_task_id);
            poll_upstream_response(
                state,
                forward_openai_bound(state, &upstream, Method::GET, &path, None),
                task.id,
                task.task_type,
            )
            .await?
        }
        UpstreamTaskType::NeogateResponse => unreachable!("handled before upstream polling"),
        UpstreamTaskType::AnthropicMessageBatch => {
            let path = format!("/v1/messages/batches/{}", task.upstream_task_id);
            poll_upstream_response(
                state,
                forward_anthropic_bound(
                    state,
                    &HeaderMap::new(),
                    &upstream,
                    Method::GET,
                    &path,
                    None,
                ),
                task.id,
                task.task_type,
            )
            .await?
        }
    };
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return Ok(());
    }
    let body = read_response_bytes(state, response, task.id, task.task_type).await?;
    let value: Value = serde_json::from_slice(&body)?;
    let (status_text, terminal) = task_status_from_value(&value, &task);
    let mut usage = parse_usage_from_bytes(&body, false);
    upstream::update_task_from_upstream_value(
        &state.db.pool,
        upstream::UpstreamTaskUpdate {
            task_id: task.id,
            task_type: task.task_type,
            upstream_task_id: task.upstream_task_id.clone(),
            status: status_text.clone(),
            terminal,
            metadata: value,
            usage,
            poll_interval: state.config.task.upstream_poll_interval,
        },
    )
    .await?;
    if terminal {
        if usage.is_none()
            && task.task_type == UpstreamTaskType::AnthropicMessageBatch
            && status_text == "ended"
        {
            let Some(results_usage) =
                poll_anthropic_batch_results_usage(state, &task, &upstream).await?
            else {
                return Ok(());
            };
            usage = Some(results_usage);
        }
        task_billing::finalize_polled(state, task, upstream, usage).await?;
    }
    Ok(())
}

async fn poll_upstream_response<F>(
    state: &AppState,
    response: F,
    task_id: i64,
    task_type: UpstreamTaskType,
) -> AppResult<reqwest::Response>
where
    F: Future<Output = AppResult<reqwest::Response>>,
{
    match tokio::time::timeout(state.config.http.upstream_timeout, response).await {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                task_id,
                ?task_type,
                timeout_secs = state.config.http.upstream_timeout.as_secs(),
                "timed out polling upstream async task response"
            );
            Err(crate::error::AppError::UpstreamUnavailable(
                "upstream async task poll timed out".to_string(),
            ))
        }
    }
}

async fn read_response_bytes(
    state: &AppState,
    response: reqwest::Response,
    task_id: i64,
    task_type: UpstreamTaskType,
) -> AppResult<Bytes> {
    match tokio::time::timeout(state.config.http.upstream_timeout, response.bytes()).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            tracing::warn!(
                task_id,
                ?task_type,
                timeout_secs = state.config.http.upstream_timeout.as_secs(),
                "timed out reading upstream async task response body"
            );
            Err(crate::error::AppError::UpstreamUnavailable(
                "upstream async task response body timed out".to_string(),
            ))
        }
    }
}

async fn poll_anthropic_batch_results_usage(
    state: &AppState,
    task: &UpstreamTask,
    upstream: &SelectedUpstream,
) -> AppResult<Option<TokenUsage>> {
    let path = format!("/v1/messages/batches/{}/results", task.upstream_task_id);
    let response = poll_upstream_response(
        state,
        forward_anthropic_bound(state, &HeaderMap::new(), upstream, Method::GET, &path, None),
        task.id,
        task.task_type,
    )
    .await?;
    let status = StatusCode::from_u16(response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return Ok(None);
    }
    let mut parser = AnthropicResultsUsageParser::default();
    let mut stream = response.bytes_stream();
    loop {
        let chunk =
            match tokio::time::timeout(state.config.http.upstream_timeout, stream.next()).await {
                Ok(Some(chunk)) => chunk?,
                Ok(None) => break,
                Err(_) => {
                    tracing::warn!(
                        task_id = task.id,
                        ?task.task_type,
                        timeout_secs = state.config.http.upstream_timeout.as_secs(),
                        "timed out reading anthropic batch results stream"
                    );
                    return Ok(None);
                }
            };
        parser.observe(&chunk);
    }
    Ok(parser.finish())
}

fn task_status_from_value(value: &Value, task: &UpstreamTask) -> (String, bool) {
    match task.task_type {
        UpstreamTaskType::OpenAiResponse => {
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or(&task.status)
                .to_string();
            let terminal = openai_response_terminal(&status);
            (status, terminal)
        }
        UpstreamTaskType::NeogateResponse => unreachable!("handled before upstream polling"),
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
