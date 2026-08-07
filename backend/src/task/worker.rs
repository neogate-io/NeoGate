use std::{future::Future, sync::Arc};

use axum::http::{HeaderMap, Method};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::{stream, StreamExt};
use serde_json::{json, Value};

use crate::{
    billing::{parse_usage_from_bytes, TokenUsage},
    config::UPSTREAM_TIMEOUT,
    error::{reqwest_status, AppResult},
    provider::adapters::{adapter_for_endpoint, bailian_asr, RelayRoute},
    relay::{
        forward_anthropic_bound, forward_openai_bound, forward_openai_video_task_bound,
        selector::SelectedUpstream,
    },
    AppState,
};

use super::{
    billing as task_billing, jobs,
    results::AnthropicResultsUsageParser,
    spool,
    upstream::{self, UpstreamTask, UpstreamTaskType},
};

const MAX_CONCURRENT_POLLED_TASKS: usize = 8;

pub(crate) fn spawn(state: Arc<AppState>) {
    if !state.config.process_role.runs_background() {
        return;
    }
    let cleanup_state = Arc::clone(&state);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(super::WORKER_TICK_INTERVAL);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                _ = state.task_wakeup.notified() => {}
            }
            if let Err(err) = poll_due_tasks(&state).await {
                tracing::warn!("failed to poll upstream async tasks: {err}");
            }
        }
    });
    tokio::spawn(async move {
        let mut cleanup_ticker = tokio::time::interval(super::CLEANUP_INTERVAL);
        let mut orphan_ticker = tokio::time::interval(super::ORPHAN_CLEANUP_INTERVAL);
        loop {
            tokio::select! {
                _ = cleanup_ticker.tick() => {
                    if let Err(err) = cleanup_expired_tasks(&cleanup_state).await {
                        tracing::warn!("failed to clean expired async tasks: {err}");
                    }
                }
                _ = orphan_ticker.tick() => {
                    match jobs::cleanup_orphaned_asset_directories(&cleanup_state).await {
                        Ok(deleted) if deleted > 0 => {
                            tracing::info!(deleted, "deleted orphaned response asset directories");
                        }
                        Ok(_) => {}
                        Err(err) => {
                            tracing::warn!("failed to clean orphaned response assets: {err}");
                        }
                    }
                }
            }
        }
    });
}

async fn poll_due_tasks(state: &Arc<AppState>) -> AppResult<()> {
    let claim_limit = state
        .config
        .task
        .upstream_poll_batch_size
        .min(MAX_CONCURRENT_POLLED_TASKS as i64);
    let (tasks, claimed_until) =
        upstream::claim_due_tasks(&state.db.pool, claim_limit, super::TASK_CLAIM_TIMEOUT).await?;
    let concurrency = tasks.len().clamp(1, MAX_CONCURRENT_POLLED_TASKS);
    stream::iter(tasks)
        .for_each_concurrent(concurrency, |task| {
            let state = Arc::clone(state);
            async move {
                let task_id = task.id;
                let task_type = task.task_type;
                // tokio::spawn 隔离 panic：防止单个任务 panic unwind 整个
                // for_each_concurrent，导致后台轮询协程静默终止不再处理任何任务。
                // JoinHandle::await 将 panic 转换为 JoinError，不会向外传播。
                let result = tokio::spawn({
                    let state = Arc::clone(&state);
                    async move { poll_task(&state, task).await }
                })
                .await;
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(err)) => {
                        tracing::warn!(
                            task_id,
                            ?task_type,
                            "failed to poll one upstream async task: {err}"
                        );
                    }
                    Err(join_err) => {
                        tracing::error!(
                            task_id,
                            ?task_type,
                            "upstream async task poll panicked; task lease will expire and be retried: {join_err}"
                        );
                    }
                }
                if let Err(err) = upstream::release_task_claim(
                    &state.db.pool,
                    task_id,
                    claimed_until,
                    super::POLL_INTERVAL,
                )
                .await
                {
                    tracing::warn!(
                        task_id,
                        ?task_type,
                        "failed to release upstream async task claim: {err}"
                    );
                }
            }
        })
        .await;
    finalize_terminal_holds(state).await?;
    Ok(())
}

async fn finalize_terminal_holds(state: &Arc<AppState>) -> AppResult<()> {
    let stale_window = ChronoDuration::from_std(super::POLL_INTERVAL)
        .unwrap_or_else(|_| ChronoDuration::seconds(30));
    let stale_before = Utc::now() - stale_window;
    let tasks = upstream::fetch_stale_terminal_held_tasks(
        &state.db.pool,
        stale_before,
        state.config.task.upstream_poll_batch_size,
    )
    .await?;
    for pending in tasks {
        let task = pending.task;
        let upstream = match task.selected_upstream(&state.db.pool, &state.secrets).await {
            Ok(upstream) => upstream,
            Err(err) => {
                if task_billing::permanent_terminal_restore_error(&err) {
                    if let Err(release_err) = task_billing::fail_task_hold_by_id(
                        state,
                        task.id,
                        "terminal task upstream restore failure",
                    )
                    .await
                    {
                        tracing::warn!(
                            task_id = task.id,
                            "failed to abandon unrecoverable terminal task hold: {release_err}"
                        );
                    }
                    continue;
                }
                tracing::warn!(
                    task_id = task.id,
                    "failed to restore terminal task upstream for billing retry: {err}"
                );
                continue;
            }
        };
        if let Err(err) = task_billing::finalize_polled(state, task, upstream, pending.usage).await
        {
            tracing::warn!("failed to retry terminal async task billing: {err}");
        }
    }
    Ok(())
}

async fn cleanup_expired_tasks(state: &Arc<AppState>) -> AppResult<()> {
    let limit = state.config.task.upstream_poll_batch_size;
    let expired_requests = jobs::fail_stale_request_spool_tasks(state, limit).await?;
    let orphaned_request_files = spool::cleanup_orphans(state, limit).await?;
    let mut deleted_assets = 0;
    loop {
        let deleted = jobs::cleanup_expired_assets(state, limit).await?;
        deleted_assets += deleted;
        if deleted == 0 {
            break;
        }
    }
    let mut deleted_tasks = 0;
    loop {
        let deleted = upstream::delete_expired_terminal_tasks(&state.db.pool, limit).await?;
        deleted_tasks += deleted;
        if deleted == 0 {
            break;
        }
    }
    if expired_requests > 0 || orphaned_request_files > 0 || deleted_assets > 0 || deleted_tasks > 0
    {
        tracing::info!(
            expired_requests,
            orphaned_request_files,
            deleted_assets,
            deleted_tasks,
            "cleaned expired async tasks"
        );
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
    if task.task_type == UpstreamTaskType::AudioTranscription {
        return poll_audio_transcription(state, task, upstream).await;
    }
    let poll_response = async {
        let response = match task.task_type {
            UpstreamTaskType::OpenAiResponse => {
                let path = format!("/v1/responses/{}", task.upstream_task_id);
                poll_upstream_response(
                    forward_openai_bound(state, &upstream, Method::GET, &path, None),
                    task.id,
                    task.task_type,
                )
                .await?
            }
            UpstreamTaskType::OpenAiVideo => {
                let path = format!("/v1/videos/{}", task.upstream_task_id);
                poll_upstream_response(
                    forward_openai_video_task_bound(
                        state,
                        &upstream,
                        Method::GET,
                        &path,
                        task.upstream_model.as_deref(),
                    ),
                    task.id,
                    task.task_type,
                )
                .await?
            }
            UpstreamTaskType::AudioTranscription => {
                unreachable!("handled before generic upstream polling")
            }
            UpstreamTaskType::NeogateResponse => unreachable!("handled before upstream polling"),
            UpstreamTaskType::AnthropicMessageBatch => {
                let path = format!("/v1/messages/batches/{}", task.upstream_task_id);
                poll_upstream_response(
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
        let status = reqwest_status(response.status());
        let body = read_response_bytes(response, task.id, task.task_type).await?;
        AppResult::Ok((status, body))
    };
    let (status, body) =
        match tokio::time::timeout(super::TASK_POLL_ATTEMPT_TIMEOUT, poll_response).await {
            Ok(result) => result?,
            Err(_) => {
                tracing::warn!(
                    task_id = task.id,
                    ?task.task_type,
                    timeout_secs = super::TASK_POLL_ATTEMPT_TIMEOUT.as_secs(),
                    "timed out polling upstream async task"
                );
                return Err(crate::error::AppError::UpstreamUnavailable(
                    "upstream async task poll attempt timed out".to_string(),
                ));
            }
        };
    tracing::info!(
        task_id = task.id,
        ?task.task_type,
        upstream_task_id = %task.upstream_task_id,
        provider = %upstream.provider,
        channel_id = upstream.channel_id,
        upstream_status = status.as_u16(),
        upstream_response = %String::from_utf8_lossy(&body),
        "upstream async task poll response"
    );
    if !status.is_success() {
        return Ok(());
    }
    let body = if task.task_type == UpstreamTaskType::OpenAiVideo {
        adapter_for_endpoint(
            &upstream.provider,
            &upstream.base_url,
            upstream.adapter_hint.as_deref(),
        )
        .normalize_response_body(RelayRoute::Videos, body)?
    } else {
        body
    };
    let mut value: Value = serde_json::from_slice(&body)?;
    if task.task_type == UpstreamTaskType::OpenAiVideo {
        crate::billing::video::copy_neogate_metadata(&task.upstream_metadata, &mut value);
    }
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
            metadata: value.clone(),
            usage,
            poll_interval: super::POLL_INTERVAL,
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
        let task = if task.task_type == UpstreamTaskType::OpenAiVideo {
            UpstreamTask {
                status: status_text,
                terminal,
                upstream_metadata: value,
                ..task
            }
        } else {
            task
        };
        task_billing::finalize_polled(state, task, upstream, usage).await?;
    }
    Ok(())
}

async fn poll_audio_transcription(
    state: &Arc<AppState>,
    task: UpstreamTask,
    upstream: SelectedUpstream,
) -> AppResult<()> {
    let local_duration_seconds = task
        .upstream_metadata
        .pointer("/neogate/local_duration_seconds")
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or_else(|| {
            crate::error::AppError::BadRequest(
                "audio transcription task is missing local duration".to_string(),
            )
        })?;
    let model = task.upstream_model.as_deref().ok_or_else(|| {
        crate::error::AppError::BadRequest(
            "audio transcription task is missing upstream model".to_string(),
        )
    })?;
    let poll = match inline_audio_poll_result(&task.upstream_metadata) {
        Some(result) => result,
        None => {
            match tokio::time::timeout(
                super::TASK_POLL_ATTEMPT_TIMEOUT,
                bailian_asr::poll(
                    state,
                    &upstream,
                    model,
                    &task.upstream_task_id,
                    local_duration_seconds,
                ),
            )
            .await
            {
                Ok(result) => result?,
                Err(_) => {
                    tracing::warn!(
                        task_id = task.id,
                        ?task.task_type,
                        timeout_secs = super::TASK_POLL_ATTEMPT_TIMEOUT.as_secs(),
                        "timed out polling upstream audio transcription task"
                    );
                    return Err(crate::error::AppError::UpstreamUnavailable(
                        "upstream audio transcription poll attempt timed out".to_string(),
                    ));
                }
            }
        }
    };
    let mut neogate = task
        .upstream_metadata
        .get("neogate")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if let Some(object) = neogate.as_object_mut() {
        object.remove("inline_result");
    }
    let (status, terminal, result) = match poll {
        bailian_asr::PollResult::Pending { status, request_id } => {
            (status, false, json!({ "request_id": request_id }))
        }
        bailian_asr::PollResult::Completed {
            text,
            duration_seconds,
            duration_source,
            details,
            request_id,
        } => {
            if duration_source == "local_fallback" {
                tracing::warn!(
                    task_id = task.id,
                    upstream_task_id = %task.upstream_task_id,
                    model,
                    channel_id = upstream.channel_id,
                    "Alibaba ASR omitted a valid duration; using local audio duration for billing"
                );
            }
            (
                "completed".to_string(),
                true,
                json!({
                    "text": text,
                    "duration_seconds": duration_seconds,
                    "duration_source": duration_source,
                    "details": details,
                    "request_id": request_id,
                }),
            )
        }
        bailian_asr::PollResult::Failed {
            status,
            message,
            request_id,
        } => (
            "failed".to_string(),
            true,
            json!({
                "upstream_status": status,
                "error": message,
                "request_id": request_id,
            }),
        ),
    };
    let metadata = json!({ "neogate": neogate, "result": result });
    upstream::update_task_from_upstream_value(
        &state.db.pool,
        upstream::UpstreamTaskUpdate {
            task_id: task.id,
            task_type: task.task_type,
            upstream_task_id: task.upstream_task_id.clone(),
            status: status.clone(),
            terminal,
            metadata: metadata.clone(),
            usage: None,
            poll_interval: super::POLL_INTERVAL,
        },
    )
    .await?;
    if terminal {
        task_billing::finalize_polled(
            state,
            UpstreamTask {
                status,
                terminal,
                upstream_metadata: metadata,
                ..task
            },
            upstream,
            None,
        )
        .await?;
    }
    Ok(())
}

fn inline_audio_poll_result(metadata: &Value) -> Option<bailian_asr::PollResult> {
    let result = metadata.pointer("/neogate/inline_result")?;
    let text = result.get("text")?.as_str()?.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let duration_seconds = result
        .get("duration_seconds")?
        .as_f64()
        .filter(|duration| duration.is_finite() && *duration > 0.0)?;
    let duration_source = match result.get("duration_source")?.as_str()? {
        "upstream" => "upstream",
        "local_fallback" => "local_fallback",
        _ => return None,
    };
    let request_id = result
        .get("request_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let details = result
        .get("details")
        .cloned()
        .filter(|value| !value.is_null());
    Some(bailian_asr::PollResult::Completed {
        text,
        duration_seconds,
        duration_source,
        details,
        request_id,
    })
}

async fn poll_upstream_response<F>(
    response: F,
    task_id: i64,
    task_type: UpstreamTaskType,
) -> AppResult<reqwest::Response>
where
    F: Future<Output = AppResult<reqwest::Response>>,
{
    match tokio::time::timeout(UPSTREAM_TIMEOUT, response).await {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                task_id,
                ?task_type,
                timeout_secs = UPSTREAM_TIMEOUT.as_secs(),
                "timed out polling upstream async task response"
            );
            Err(crate::error::AppError::UpstreamUnavailable(
                "upstream async task poll timed out".to_string(),
            ))
        }
    }
}

async fn read_response_bytes(
    response: reqwest::Response,
    task_id: i64,
    task_type: UpstreamTaskType,
) -> AppResult<Bytes> {
    match tokio::time::timeout(UPSTREAM_TIMEOUT, response.bytes()).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            tracing::warn!(
                task_id,
                ?task_type,
                timeout_secs = UPSTREAM_TIMEOUT.as_secs(),
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
        forward_anthropic_bound(state, &HeaderMap::new(), upstream, Method::GET, &path, None),
        task.id,
        task.task_type,
    )
    .await?;
    let status = reqwest_status(response.status());
    if !status.is_success() {
        return Ok(None);
    }
    let mut parser = AnthropicResultsUsageParser::default();
    let mut stream = response.bytes_stream();
    loop {
        let chunk = match tokio::time::timeout(UPSTREAM_TIMEOUT, stream.next()).await {
            Ok(Some(chunk)) => chunk?,
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(
                    task_id = task.id,
                    ?task.task_type,
                    timeout_secs = UPSTREAM_TIMEOUT.as_secs(),
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
        UpstreamTaskType::OpenAiVideo => {
            let status = crate::provider::openai::video_status_text(value, &task.status);
            let terminal = crate::provider::openai::video_terminal(&status);
            (status, terminal)
        }
        UpstreamTaskType::AudioTranscription => {
            unreachable!("handled by the Alibaba ASR poller")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_inline_audio_transcription_result() {
        let metadata = json!({
            "neogate": {
                "inline_result": {
                    "text": "transcript",
                    "duration_seconds": 2.5,
                    "duration_source": "upstream",
                    "request_id": "request-id",
                }
            }
        });

        match inline_audio_poll_result(&metadata).unwrap() {
            bailian_asr::PollResult::Completed {
                text,
                duration_seconds,
                duration_source,
                details,
                request_id,
            } => {
                assert_eq!(text, "transcript");
                assert_eq!(duration_seconds, 2.5);
                assert_eq!(duration_source, "upstream");
                assert!(details.is_none());
                assert_eq!(request_id.as_deref(), Some("request-id"));
            }
            _ => panic!("expected completed inline transcription"),
        }
    }
}
