use std::{
    collections::HashSet,
    error::Error as StdError,
    path::{Path, PathBuf},
    time::{Instant, SystemTime},
};

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde::{de::IgnoredAny, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{parse_usage_from_bytes, DebitHold, TokenUsage},
    config::UPSTREAM_TIMEOUT,
    error::{reqwest_status, AppError, AppResult, UpstreamErrorKind},
    id::DbId,
    provider::adapters::adapter_for_endpoint,
    relay::{
        describe_upstream_http_failure, forward_openai_with_content_type, read_upstream_error_body,
        selector::{SelectedUpstream, UpstreamProtocol},
    },
    task::{billing as task_billing, spool, upstream},
    AppState,
};

use super::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType, UsageSummary};

const STATUS_QUEUED: &str = "queued";
const STATUS_IN_PROGRESS: &str = "in_progress";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";
const ASSET_URL_TTL_SECONDS: u64 = 3600;
const IMAGE_TASK_LEASE_MARGIN: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const MAX_IMAGE_TASK_ATTEMPTS: u32 = 3;
const MAX_IMAGE_EDIT_INPUT_BYTES: usize = 50 * 1024 * 1024;
const IMAGE_REQUEST_BODY_MUST_BE_OBJECT: &str = "image request body must be an object";

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum NeogateImageResponseFormat {
    #[default]
    Base64,
    Url,
    Both,
}

impl NeogateImageResponseFormat {
    fn from_value(value: &Value) -> AppResult<Self> {
        match value.as_str() {
            Some("base64") => Ok(Self::Base64),
            Some("url") => Ok(Self::Url),
            Some("both") => Ok(Self::Both),
            Some(_) => Err(AppError::BadRequest(
                "image_format must be one of base64, url, or both".to_string(),
            )),
            None => Err(AppError::BadRequest(
                "image_format must be a string".to_string(),
            )),
        }
    }

    pub(crate) fn requires_neogate_asset_url(self) -> bool {
        matches!(self, Self::Url | Self::Both)
    }
}

#[derive(Debug)]
pub(crate) struct PreparedNeogateResponseRequest {
    pub(crate) body: Bytes,
    pub(crate) image_format: NeogateImageResponseFormat,
    pub(crate) has_image_generation_tool: bool,
}

pub(crate) struct CreateNeogateResponse<'a> {
    pub(crate) upstream: &'a SelectedUpstream,
    pub(crate) response_model: &'a str,
    pub(crate) image_model: &'a str,
    pub(crate) upstream_image_model: &'a str,
    pub(crate) request_body: Bytes,
    pub(crate) upstream_request_body: Bytes,
    pub(crate) image_format: NeogateImageResponseFormat,
    pub(crate) hold: &'a DebitHold,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NeogateResponseMetadata {
    request: Value,
    #[serde(default)]
    upstream_request: Option<Value>,
    #[serde(default)]
    request_spool: Option<spool::Spool>,
    response: Value,
    #[serde(default)]
    image_format: NeogateImageResponseFormat,
    #[serde(default)]
    assets: Vec<NeogateResponseAsset>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    cancel_requested: bool,
    #[serde(default)]
    attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NeogateResponseAsset {
    path: String,
    mime: String,
    sha256: String,
    bytes: usize,
    index: usize,
    #[serde(default)]
    revised_prompt: Option<String>,
}

struct NeogateResponseResult {
    response: Value,
    assets: Vec<NeogateResponseAsset>,
    usage: Option<TokenUsage>,
}

#[derive(Deserialize)]
struct ImageRequestSummary {
    #[serde(default)]
    model: String,
    #[serde(default)]
    size: String,
    #[serde(default)]
    quality: String,
    #[serde(default)]
    output_format: String,
    #[serde(default = "default_image_count")]
    n: i64,
    #[serde(default)]
    images: Option<Vec<IgnoredAny>>,
}

struct PreparedImageUpstreamRequest {
    body: Bytes,
    content_type: HeaderValue,
    path: &'static str,
}

#[derive(Deserialize)]
struct ImageGenerationResponse {
    #[serde(default)]
    data: Vec<ImageGenerationOutput>,
}

#[derive(Deserialize)]
struct ImageGenerationOutput {
    b64_json: Option<String>,
    result: Option<String>,
    revised_prompt: Option<String>,
}

fn default_image_count() -> i64 {
    1
}

pub(crate) fn has_image_generation_tool(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    value
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|tool| tool.get("type").and_then(Value::as_str) == Some("image_generation"))
        })
}

fn image_tool_string<'a>(request: &'a Value, field: &str) -> Option<&'a str> {
    request
        .get("tools")
        .and_then(Value::as_array)?
        .iter()
        .find(|tool| tool.get("type").and_then(Value::as_str) == Some("image_generation"))?
        .get(field)
        .and_then(Value::as_str)
}

pub(crate) fn prepare_request_body(body: Bytes) -> AppResult<PreparedNeogateResponseRequest> {
    let mut request: Value = serde_json::from_slice(&body)?;
    let image_format = request
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be an object".to_string()))?
        .remove("image_format")
        .map(|value| NeogateImageResponseFormat::from_value(&value))
        .transpose()?
        .unwrap_or_default();
    let sanitized = Bytes::from(serde_json::to_vec(&request)?);
    Ok(PreparedNeogateResponseRequest {
        has_image_generation_tool: has_image_generation_tool(&sanitized),
        body: sanitized,
        image_format,
    })
}

pub(crate) async fn create(
    state: &AppState,
    auth: &UserAuth,
    task: CreateNeogateResponse<'_>,
) -> AppResult<Value> {
    let CreateNeogateResponse {
        upstream,
        response_model,
        image_model,
        upstream_image_model,
        request_body,
        upstream_request_body,
        image_format,
        hold,
    } = task;
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let mut request: Value = serde_json::from_slice(&request_body)?;
    compact_data_url_images(&mut request);
    let request_spool = spool::save(
        &state.config.response_assets.dir,
        &response_id,
        &upstream_request_body,
    )
    .await?;
    let size = image_tool_string(&request, "size")
        .unwrap_or("")
        .to_string();
    let quality = image_tool_string(&request, "quality")
        .unwrap_or("")
        .to_string();
    let output_format = image_tool_string(&request, "output_format")
        .unwrap_or("")
        .to_string();
    let response = response_json(
        &response_id,
        response_model,
        STATUS_QUEUED,
        Vec::new(),
        None,
        None,
    );
    let metadata = NeogateResponseMetadata {
        request,
        upstream_request: None,
        request_spool: Some(request_spool.clone()),
        response: response.clone(),
        image_format,
        assets: Vec::new(),
        error: None,
        cancel_requested: false,
        attempts: 0,
    };
    if let Err(err) = upstream::insert_task(
        &state.db.pool,
        NewUpstreamTask {
            task_type: UpstreamTaskType::NeogateResponse,
            upstream_task_id: &response_id,
            auth,
            protocol: UpstreamProtocol::Openai,
            upstream,
            model: Some(image_model),
            upstream_model: Some(upstream_image_model),
            status: STATUS_QUEUED,
            terminal: false,
            hold,
            upstream_metadata: serde_json::to_value(metadata)?,
        },
        super::POLL_INTERVAL,
        state.config.task.upstream_retention,
    )
    .await
    {
        spool::remove(&state.config.response_assets.dir, &request_spool.path).await;
        return Err(err);
    }
    mark_due_now(&state.db.pool, &response_id).await?;
    tracing::info!(
        response_id = %response_id,
        user_id = auth.user_id,
        project_id = auth.project_id,
        user_key_id = auth.user_key_id,
        provider = %upstream.provider,
        channel_id = upstream.channel_id,
        channel_endpoint_id = upstream.channel_endpoint_id,
        channel_key_id = ?upstream.channel_key_id,
        credential_id = ?upstream.credential_id,
        response_model,
        image_model,
        upstream_image_model,
        size = %size,
        quality = %quality,
        output_format = %output_format,
        ?image_format,
        status = STATUS_QUEUED,
        "created async image task"
    );
    state.task_wakeup.notify_one();
    Ok(response)
}

pub(crate) async fn response_for_task(state: &AppState, task: &UpstreamTask) -> AppResult<Value> {
    let metadata = metadata(task)?;
    let mut response = metadata.response;
    response["status"] = Value::String(task.status.clone());
    response["id"] = Value::String(task.upstream_task_id.clone());
    if task.status == STATUS_COMPLETED {
        match outputs_from_assets(
            state,
            &task.upstream_task_id,
            metadata.image_format,
            &metadata.assets,
        )
        .await
        {
            Ok(output) => response["output"] = Value::Array(output),
            Err(err) => {
                response["status"] = Value::String(STATUS_FAILED.to_string());
                response["error"] = json!({
                    "code": "neogate_response_asset_missing",
                    "message": err.to_string(),
                });
            }
        }
    }
    if let Some(usage) = task_usage_summary(&state.db.pool, task.id).await? {
        response["usage"] = serde_json::to_value(usage)?;
    }
    if let Some(error) = metadata.error {
        response["error"] = error;
    }
    Ok(response)
}

pub(crate) async fn response(task_response: Value) -> AppResult<Response> {
    Response::builder()
        .status(200)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&task_response)?))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

pub(crate) async fn cancel(state: &AppState, task: UpstreamTask) -> AppResult<Value> {
    if task.terminal {
        return response_for_task(state, &task).await;
    }
    tracing::info!(
        task_id = task.id,
        response_id = %task.upstream_task_id,
        user_id = task.user_id,
        project_id = task.project_id,
        provider = %task.provider,
        channel_id = task.channel_id,
        current_status = %task.status,
        "async image task cancellation requested"
    );
    if task.status == STATUS_QUEUED {
        let mut queued_metadata = metadata(&task)?;
        let request_spool = queued_metadata.request_spool.take();
        if update_metadata(
            state,
            task.id,
            STATUS_CANCELLED,
            true,
            queued_metadata,
            None,
            Some(STATUS_QUEUED),
        )
        .await?
        {
            if let Some(spool) = request_spool {
                spool::remove(&state.config.response_assets.dir, &spool.path).await;
            }
            task_billing::release_task_hold_by_id(state, task.id, "cancelled neogate response")
                .await?;
        } else {
            let current = upstream::fetch_task(
                &state.db.pool,
                task.user_key_id,
                UpstreamTaskType::NeogateResponse,
                &task.upstream_task_id,
            )
            .await?;
            if !current.terminal {
                let mut metadata = metadata(&current)?;
                metadata.cancel_requested = true;
                let _ = update_metadata(
                    state,
                    current.id,
                    current.status.as_str(),
                    false,
                    metadata,
                    None,
                    None,
                )
                .await?;
            }
        }
    } else {
        let mut metadata = metadata(&task)?;
        metadata.cancel_requested = true;
        let _ = update_metadata(
            state,
            task.id,
            task.status.as_str(),
            false,
            metadata,
            None,
            None,
        )
        .await?;
    }
    let task = upstream::fetch_task(
        &state.db.pool,
        task.user_key_id,
        UpstreamTaskType::NeogateResponse,
        &task.upstream_task_id,
    )
    .await?;
    response_for_task(state, &task).await
}

pub(crate) async fn run(state: &AppState, task: UpstreamTask) -> AppResult<()> {
    let task = upstream::fetch_task(
        &state.db.pool,
        task.user_key_id,
        UpstreamTaskType::NeogateResponse,
        &task.upstream_task_id,
    )
    .await?;
    if task.terminal {
        tracing::debug!(
            task_id = task.id,
            response_id = %task.upstream_task_id,
            status = %task.status,
            "skipping terminal async image task"
        );
        return Ok(());
    }

    let mut metadata = match metadata(&task) {
        Ok(metadata) => metadata,
        Err(err) => {
            fail_malformed_response_task(
                state,
                task.id,
                &task.upstream_task_id,
                task.model.as_deref().unwrap_or(""),
                &task.status,
                &task.upstream_metadata,
                &err.to_string(),
            )
            .await?;
            return Ok(());
        }
    };
    let response_model = metadata
        .response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if metadata.cancel_requested || task.status == STATUS_CANCELLED {
        let request_spool = metadata.request_spool.take();
        if set_terminal_status(state, task.id, STATUS_CANCELLED, None, metadata).await? {
            if let Some(spool) = request_spool {
                spool::remove(&state.config.response_assets.dir, &spool.path).await;
            }
            task_billing::release_task_hold_by_id(state, task.id, "cancelled neogate response")
                .await?;
            tracing::info!(
                task_id = task.id,
                response_id = %task.upstream_task_id,
                provider = %task.provider,
                channel_id = task.channel_id,
                status = STATUS_CANCELLED,
                "cancelled async image task"
            );
        }
        return Ok(());
    }

    if metadata.attempts >= MAX_IMAGE_TASK_ATTEMPTS {
        let request_spool = metadata.request_spool.take();
        if fail_response_task(
            state,
            task.id,
            metadata,
            "neogate_response_attempts_exhausted",
            "async image request exceeded the retry limit".to_string(),
            Some(task.status.as_str()),
            "exhausted async image request retries",
        )
        .await?
        {
            if let Some(spool) = request_spool {
                spool::remove(&state.config.response_assets.dir, &spool.path).await;
            }
        }
        return Ok(());
    }

    tracing::info!(
        task_id = task.id,
        response_id = %task.upstream_task_id,
        user_id = task.user_id,
        project_id = task.project_id,
        user_key_id = task.user_key_id,
        provider = %task.provider,
        channel_id = task.channel_id,
        channel_endpoint_id = task.channel_endpoint_id,
        channel_key_id = ?task.channel_key_id,
        credential_id = ?task.credential_id,
        response_model = %response_model,
        image_model = %task.model.as_deref().unwrap_or(""),
        upstream_image_model = %task.upstream_model.as_deref().unwrap_or(""),
        current_status = %task.status,
        "starting async image task"
    );

    let _request_permit = state.user_request_limiter.try_acquire(task.user_id).await?;
    metadata.attempts += 1;

    if !update_metadata(
        state,
        task.id,
        STATUS_IN_PROGRESS,
        false,
        metadata.clone(),
        None,
        Some(task.status.as_str()),
    )
    .await?
    {
        return Ok(());
    }

    let upstream = task
        .selected_upstream(&state.db.pool, &state.secrets)
        .await?;

    let body_result = if let Some(upstream_request) = &metadata.upstream_request {
        serde_json::to_vec(upstream_request)
            .map(Bytes::from)
            .map_err(Into::into)
    } else if let Some(spool) = metadata.request_spool.clone() {
        spool::read(&state.config.response_assets.dir, &spool).await
    } else {
        adapter_for_endpoint(
            &upstream.provider,
            &upstream.base_url,
            upstream.adapter_hint.as_deref(),
        )
        .prepare_response_image_generation_request(Bytes::from(serde_json::to_vec(
            &metadata.request,
        )?))?
        .ok_or_else(|| {
            AppError::BadRequest(
                "provider adapter does not translate response image generation".to_string(),
            )
        })
        .map(|request| request.body)
    };
    let body = match body_result {
        Ok(body) => body,
        Err(err) => {
            let request_spool = metadata.request_spool.take();
            if fail_response_task(
                state,
                task.id,
                metadata,
                "neogate_response_request_unavailable",
                err.to_string(),
                None,
                "unavailable async image request",
            )
            .await?
            {
                if let Some(spool) = request_spool {
                    spool::remove(&state.config.response_assets.dir, &spool.path).await;
                }
            }
            return Ok(());
        }
    };
    let upstream_path = image_generation_upstream_path_from_body(&body);
    let result = run_image_generation(state, &task, &upstream, &response_model, body).await;
    match result {
        Ok(result) => {
            let NeogateResponseResult {
                response,
                assets,
                usage,
            } = result;
            let image_count = assets.len();
            let asset_bytes = assets.iter().map(|asset| asset.bytes).sum::<usize>();
            let request_spool = metadata.request_spool.take();
            metadata.response = response;
            metadata.assets = assets;
            metadata.error = None;
            if set_terminal_status(state, task.id, STATUS_COMPLETED, usage, metadata).await? {
                if let Some(spool) = request_spool {
                    spool::remove(&state.config.response_assets.dir, &spool.path).await;
                }
                let updated = upstream::fetch_task(
                    &state.db.pool,
                    task.user_key_id,
                    UpstreamTaskType::NeogateResponse,
                    &task.upstream_task_id,
                )
                .await?;
                task_billing::finalize_polled(state, updated, upstream, usage).await?;
                tracing::info!(
                    task_id = task.id,
                    response_id = %task.upstream_task_id,
                    provider = %task.provider,
                    channel_id = task.channel_id,
                    channel_endpoint_id = task.channel_endpoint_id,
                    response_model = %response_model,
                    image_model = %task.model.as_deref().unwrap_or(""),
                    upstream_image_model = %task.upstream_model.as_deref().unwrap_or(""),
                    status = STATUS_COMPLETED,
                    image_count,
                    asset_bytes,
                    input_tokens = usage.map_or(0, |usage| usage.input_tokens),
                    output_tokens = usage.map_or(0, |usage| usage.output_tokens),
                    "completed async image task"
                );
            }
        }
        Err(err) => {
            let diagnostics = image_task_error_diagnostics(&err);
            tracing::warn!(
                task_id = task.id,
                response_id = %task.upstream_task_id,
                user_id = task.user_id,
                project_id = task.project_id,
                user_key_id = task.user_key_id,
                channel_id = task.channel_id,
                channel_endpoint_id = task.channel_endpoint_id,
                channel_key_id = ?task.channel_key_id,
                credential_id = ?task.credential_id,
                provider = %task.provider,
                model = %task.model.as_deref().unwrap_or(""),
                upstream = %task.upstream_base_url,
                error = %err,
                error_debug = %diagnostics.debug,
                error_kind = diagnostics.kind,
                retryable = diagnostics.retryable,
                is_timeout = diagnostics.is_timeout,
                is_connect = diagnostics.is_connect,
                error_url = ?diagnostics.url,
                source_chain = ?diagnostics.source_chain,
                upstream_path,
                "async image task failed"
            );
            let request_spool = metadata.request_spool.take();
            if fail_response_task(
                state,
                task.id,
                metadata,
                "neogate_response_failed",
                err.to_string(),
                None,
                "failed neogate response",
            )
            .await?
            {
                if let Some(spool) = request_spool {
                    spool::remove(&state.config.response_assets.dir, &spool.path).await;
                }
            }
        }
    }
    Ok(())
}

fn compact_data_url_images(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                compact_data_url_images(item);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image") {
                if let Some(image_url) = object.get_mut("image_url") {
                    if image_url
                        .as_str()
                        .is_some_and(|value| value.starts_with("data:"))
                    {
                        *image_url = Value::String("[data URL stored separately]".to_string());
                    }
                }
            } else {
                for item in object.values_mut() {
                    compact_data_url_images(item);
                }
            }
        }
        _ => {}
    }
}

pub(crate) async fn fail_stale_request_spool_tasks(state: &AppState, limit: i64) -> AppResult<u64> {
    let stale_before = Utc::now()
        - chrono::Duration::from_std(super::REQUEST_SPOOL_TTL)
            .unwrap_or_else(|_| chrono::Duration::hours(1));
    let rows = sqlx::query(
        r#"
        SELECT id, upstream_task_id, model, status, upstream_metadata
        FROM task_upstream
        WHERE task_type = 'neogate_response'
          AND status = 'queued'
          AND terminal = FALSE
          AND created_at <= $1
        ORDER BY created_at ASC, id ASC
        LIMIT $2
        "#,
    )
    .bind(stale_before)
    .bind(limit.max(1))
    .fetch_all(&state.db.pool)
    .await?;

    let mut failed = 0;
    for row in rows {
        let task_id: DbId = row.try_get("id")?;
        let response_id: String = row.try_get("upstream_task_id")?;
        let model: Option<String> = row.try_get("model")?;
        let status: String = row.try_get("status")?;
        let value: Value = row.try_get("upstream_metadata")?;
        let mut metadata = match serde_json::from_value::<NeogateResponseMetadata>(value.clone()) {
            Ok(metadata) => metadata,
            Err(err) => {
                if fail_malformed_response_task(
                    state,
                    task_id,
                    &response_id,
                    model.as_deref().unwrap_or(""),
                    &status,
                    &value,
                    &err.to_string(),
                )
                .await?
                {
                    failed += 1;
                }
                continue;
            }
        };
        let Some(spool) = metadata.request_spool.take() else {
            continue;
        };
        if fail_response_task(
            state,
            task_id,
            metadata,
            "neogate_response_request_expired",
            "async image request expired before a worker started it".to_string(),
            Some(STATUS_QUEUED),
            "expired async image request",
        )
        .await?
        {
            spool::remove(&state.config.response_assets.dir, &spool.path).await;
            failed += 1;
            tracing::warn!(task_id, response_id, "expired queued async image request");
        }
    }
    Ok(failed)
}

pub(crate) async fn cleanup_expired_assets(state: &AppState, limit: i64) -> AppResult<u64> {
    let rows = sqlx::query(
        r#"
        SELECT id, upstream_task_id, upstream_metadata
        FROM task_upstream
        WHERE task_type = 'neogate_response'
          AND terminal = TRUE
          AND billing_status IN ('settled', 'released', 'failed')
          AND expires_at IS NOT NULL
          AND expires_at <= now()
        ORDER BY expires_at ASC, id ASC
        LIMIT $1
        "#,
    )
    .bind(limit.max(1))
    .fetch_all(&state.db.pool)
    .await?;

    let mut deleted = 0;
    for row in rows {
        let task_id: DbId = row.try_get("id")?;
        let response_id: String = row.try_get("upstream_task_id")?;
        let value: Value = row.try_get("upstream_metadata")?;
        let metadata = match serde_json::from_value::<NeogateResponseMetadata>(value) {
            Ok(metadata) => metadata,
            Err(err) => {
                tracing::warn!(task_id, response_id, %err, "cannot clean malformed response asset metadata");
                deleted += delete_expired_response_task(&state.db.pool, task_id).await?;
                continue;
            }
        };
        if let Err(err) = remove_asset_directories(
            &state.config.response_assets.dir,
            &response_id,
            &metadata.assets,
        )
        .await
        {
            tracing::warn!(task_id, response_id, %err, "failed to remove expired response assets");
            continue;
        }
        let row_deleted = delete_expired_response_task(&state.db.pool, task_id).await?;
        if row_deleted > 0 {
            deleted += row_deleted;
            tracing::info!(
                task_id,
                response_id,
                "deleted expired response assets and metadata"
            );
        }
    }
    Ok(deleted)
}

async fn delete_expired_response_task(pool: &PgPool, task_id: DbId) -> AppResult<u64> {
    let result = sqlx::query(
        r#"
        DELETE FROM task_upstream
        WHERE id = $1
          AND task_type = 'neogate_response'
          AND terminal = TRUE
          AND billing_status IN ('settled', 'released', 'failed')
          AND expires_at IS NOT NULL
          AND expires_at <= now()
        "#,
    )
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

async fn remove_asset_directories(
    root: &Path,
    response_id: &str,
    assets: &[NeogateResponseAsset],
) -> AppResult<()> {
    let mut directories = HashSet::new();
    for asset in assets {
        directories.insert(asset_directories(root, response_id, &asset.path)?);
    }
    for (date_dir, task_dir) in directories {
        ensure_directory_is_not_symlink(&root.join("responses")).await?;
        ensure_directory_is_not_symlink(&date_dir).await?;
        remove_managed_tree(&task_dir).await?;
        remove_empty_date_dir(&date_dir).await?;
    }
    Ok(())
}

async fn ensure_directory_is_not_symlink(path: &Path) -> AppResult<()> {
    let metadata = match fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AppError::BadRequest(
            "invalid neogate response asset directory".to_string(),
        ));
    }
    Ok(())
}

fn asset_directories(
    root: &Path,
    response_id: &str,
    relative: &str,
) -> AppResult<(PathBuf, PathBuf)> {
    let components = Path::new(relative)
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| AppError::BadRequest("invalid neogate response asset path".to_string()))?;
    if components.len() != 4
        || components[0] != "responses"
        || chrono::NaiveDate::parse_from_str(&components[1], "%Y-%m-%d").is_err()
        || components[2] != response_id
        || components[3].is_empty()
    {
        return Err(AppError::BadRequest(
            "invalid neogate response asset path".to_string(),
        ));
    }
    let date_dir = root.join(&components[0]).join(&components[1]);
    let task_dir = date_dir.join(&components[2]);
    Ok((date_dir, task_dir))
}

async fn remove_managed_tree(path: &Path) -> AppResult<()> {
    let metadata = match fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path).await?;
    } else {
        fs::remove_dir_all(path).await?;
    }
    Ok(())
}

async fn remove_empty_date_dir(path: &Path) -> AppResult<()> {
    let ds_store = path.join(".DS_Store");
    match fs::remove_file(ds_store).await {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    let mut entries = match fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };
    if entries.next_entry().await?.is_none() {
        match fs::remove_dir(path).await {
            Ok(()) => {}
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}

pub(crate) async fn cleanup_orphaned_asset_directories(state: &AppState) -> AppResult<u64> {
    let responses_dir = state.config.response_assets.dir.join("responses");
    let mut date_entries = match fs::read_dir(&responses_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(err) => return Err(err.into()),
    };
    let mut deleted = 0;
    while let Some(date_entry) = date_entries.next_entry().await? {
        let date_name = date_entry.file_name().to_string_lossy().to_string();
        if chrono::NaiveDate::parse_from_str(&date_name, "%Y-%m-%d").is_err()
            || !date_entry.file_type().await?.is_dir()
        {
            continue;
        }
        let date_dir = date_entry.path();
        // 收集所有候选目录（过期的 resp_* 目录）后批量查询，避免 N+1 问题。
        let mut candidates: Vec<(std::path::PathBuf, String)> = Vec::new();
        let mut task_entries = fs::read_dir(&date_dir).await?;
        while let Some(task_entry) = task_entries.next_entry().await? {
            let response_id = task_entry.file_name().to_string_lossy().to_string();
            if !response_id.starts_with("resp_") {
                continue;
            }
            let metadata = task_entry.metadata().await?;
            if !older_than_asset_retention(metadata.modified().ok()) {
                continue;
            }
            candidates.push((task_entry.path(), response_id));
        }
        if !candidates.is_empty() {
            // 单次批量查询替代逐个 SELECT EXISTS
            let response_ids: Vec<&str> = candidates.iter().map(|(_, id)| id.as_str()).collect();
            let existing: std::collections::HashSet<String> = sqlx::query_scalar(
                "SELECT upstream_task_id
                 FROM task_upstream
                 WHERE task_type = 'neogate_response'
                   AND upstream_task_id = ANY($1::TEXT[])",
            )
            .bind(&response_ids[..])
            .fetch_all(&state.db.pool)
            .await?
            .into_iter()
            .collect();
            for (path, response_id) in candidates {
                if !existing.contains(&response_id) {
                    remove_managed_tree(&path).await?;
                    deleted += 1;
                }
            }
        }
        remove_empty_date_dir(&date_dir).await?;
    }
    Ok(deleted)
}

fn older_than_asset_retention(modified: Option<SystemTime>) -> bool {
    modified
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= super::ASSET_RETENTION)
}

async fn run_image_generation(
    state: &AppState,
    task: &UpstreamTask,
    upstream: &SelectedUpstream,
    response_model: &str,
    body: Bytes,
) -> AppResult<NeogateResponseResult> {
    let request: ImageRequestSummary = serde_json::from_slice(&body)?;
    let prepared = prepare_image_upstream_request(
        body,
        task.upstream_model.as_deref().unwrap_or(&request.model),
    )?;
    let upstream_path = prepared.path;
    let started = Instant::now();
    tracing::info!(
        task_id = task.id,
        response_id = %task.upstream_task_id,
        provider = %upstream.provider,
        channel_id = upstream.channel_id,
        channel_endpoint_id = upstream.channel_endpoint_id,
        channel_key_id = ?upstream.channel_key_id,
        credential_id = ?upstream.credential_id,
        upstream = %upstream.base_url,
        upstream_path,
        response_model,
        image_model = %task.model.as_deref().unwrap_or(&request.model),
        upstream_image_model = %task.upstream_model.as_deref().unwrap_or(&request.model),
        size = %request.size,
        quality = %request.quality,
        output_format = %request.output_format,
        image_count = request.n,
        request_bytes = prepared.body.len(),
        "sending async image task to upstream"
    );
    let response = match forward_openai_with_content_type(
        state,
        upstream,
        UpstreamProtocol::Openai,
        prepared.body,
        upstream_path,
        prepared.content_type,
        false,
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            let diagnostics = image_task_error_diagnostics(&err);
            tracing::warn!(
                task_id = task.id,
                response_id = %task.upstream_task_id,
                provider = %upstream.provider,
                channel_id = upstream.channel_id,
                channel_endpoint_id = upstream.channel_endpoint_id,
                upstream_path,
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %err,
                error_debug = %diagnostics.debug,
                error_kind = diagnostics.kind,
                retryable = diagnostics.retryable,
                is_timeout = diagnostics.is_timeout,
                is_connect = diagnostics.is_connect,
                error_url = ?diagnostics.url,
                source_chain = ?diagnostics.source_chain,
                "async image upstream request failed"
            );
            return Err(err);
        }
    };
    let status = reqwest_status(response.status());
    if !response.status().is_success() {
        let body = read_upstream_error_body(response).await;
        let failure = describe_upstream_http_failure(status, &body);
        tracing::warn!(
            task_id = task.id,
            response_id = %task.upstream_task_id,
            provider = %upstream.provider,
            channel_id = upstream.channel_id,
            channel_endpoint_id = upstream.channel_endpoint_id,
            upstream_path,
            upstream_status = status.as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            upstream_error = %failure.summary,
            "async image upstream returned error"
        );
        return Err(AppError::BadRequest(failure.summary));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let response_body = response.bytes().await?;
    let usage = parse_usage_from_bytes(&response_body, false);
    let response_bytes = response_body.len();
    let value: ImageGenerationResponse = serde_json::from_slice(&response_body)?;
    drop(response_body);
    let images = value.data;
    if images.is_empty() {
        return Err(AppError::BadRequest(
            "image generation response did not include data".to_string(),
        ));
    }

    let (mime, extension) = image_output_type(&request.output_format);
    let mut assets = Vec::new();
    for (index, image) in images.into_iter().enumerate() {
        let ImageGenerationOutput {
            b64_json,
            result,
            revised_prompt,
        } = image;
        let result = b64_json.as_deref().or(result.as_deref()).ok_or_else(|| {
            AppError::BadRequest(
                "image generation response did not include base64 image data".to_string(),
            )
        })?;
        assets.push(
            save_image_asset(state, task, index, result, mime, extension, revised_prompt).await?,
        );
    }

    tracing::info!(
        task_id = task.id,
        response_id = %task.upstream_task_id,
        provider = %upstream.provider,
        channel_id = upstream.channel_id,
        channel_endpoint_id = upstream.channel_endpoint_id,
        upstream_path,
        upstream_status = status.as_u16(),
        content_type = %content_type,
        elapsed_ms = started.elapsed().as_millis() as u64,
        response_bytes,
        image_count = assets.len(),
        asset_bytes = assets.iter().map(|asset| asset.bytes).sum::<usize>(),
        input_tokens = usage.map_or(0, |usage| usage.input_tokens),
        output_tokens = usage.map_or(0, |usage| usage.output_tokens),
        "async image upstream response"
    );

    let response = response_json(
        &task.upstream_task_id,
        response_model,
        STATUS_COMPLETED,
        asset_output_metadata(&assets),
        None,
        None,
    );
    Ok(NeogateResponseResult {
        response,
        assets,
        usage,
    })
}

fn prepare_image_upstream_request(
    body: Bytes,
    upstream_model: &str,
) -> AppResult<PreparedImageUpstreamRequest> {
    let mut request: Value = serde_json::from_slice(&body)?;
    let object = request
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest(IMAGE_REQUEST_BODY_MUST_BE_OBJECT.to_string()))?;
    object.insert(
        "model".to_string(),
        Value::String(upstream_model.to_string()),
    );
    if upstream_model == "gpt-image-2" || upstream_model.starts_with("gpt-image-2-") {
        object.remove("input_fidelity");
    }
    validate_image_api_request(object)?;

    if !object.contains_key("images") {
        return Ok(PreparedImageUpstreamRequest {
            body: Bytes::from(serde_json::to_vec(&image_api_json_request(
                &request, false,
            )?)?),
            content_type: HeaderValue::from_static("application/json"),
            path: "/v1/images/generations",
        });
    }

    if !has_only_embedded_image_data_urls(object) {
        return Ok(PreparedImageUpstreamRequest {
            body: Bytes::from(serde_json::to_vec(&image_api_json_request(
                &request, true,
            )?)?),
            content_type: HeaderValue::from_static("application/json"),
            path: "/v1/images/edits",
        });
    }

    let (body, boundary) = image_edit_multipart_body(&request)?;
    let content_type = HeaderValue::from_str(&format!("multipart/form-data; boundary={boundary}"))
        .map_err(|err| AppError::BadRequest(format!("invalid multipart content type: {err}")))?;
    Ok(PreparedImageUpstreamRequest {
        body,
        content_type,
        path: "/v1/images/edits",
    })
}

fn validate_image_api_request(object: &serde_json::Map<String, Value>) -> AppResult<()> {
    for field in ["model", "prompt"] {
        if object
            .get(field)
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        {
            return Err(AppError::BadRequest(format!(
                "image API request requires a non-empty {field}"
            )));
        }
    }

    let Some(images) = object.get("images") else {
        return Ok(());
    };
    let images = images
        .as_array()
        .filter(|images| !images.is_empty())
        .ok_or_else(|| {
            AppError::BadRequest("image edit requires at least one image".to_string())
        })?;
    if images.len() > 16 {
        return Err(AppError::BadRequest(
            "image edit supports at most 16 input images".to_string(),
        ));
    }
    for image in images {
        validate_image_reference(image, "image")?;
    }
    if let Some(mask) = object.get("mask") {
        validate_image_reference(mask, "mask")?;
    }
    Ok(())
}

fn validate_image_reference(value: &Value, label: &str) -> AppResult<()> {
    let reference = value
        .as_object()
        .ok_or_else(|| AppError::BadRequest(format!("{label} reference must be an object")))?;
    let has_image_url = reference
        .get("image_url")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());
    let has_file_id = reference
        .get("file_id")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty());
    if has_image_url == has_file_id {
        return Err(AppError::BadRequest(format!(
            "{label} reference must provide exactly one of image_url or file_id"
        )));
    }
    Ok(())
}

fn image_api_json_request(request: &Value, edit: bool) -> AppResult<Value> {
    let object = request
        .as_object()
        .ok_or_else(|| AppError::BadRequest(IMAGE_REQUEST_BODY_MUST_BE_OBJECT.to_string()))?;
    let mut output = serde_json::Map::new();
    let fields: &[&str] = if edit {
        &[
            "model",
            "prompt",
            "images",
            "mask",
            "background",
            "input_fidelity",
            "moderation",
            "n",
            "output_compression",
            "output_format",
            "quality",
            "size",
        ]
    } else {
        &[
            "model",
            "prompt",
            "background",
            "moderation",
            "n",
            "output_compression",
            "output_format",
            "quality",
            "size",
        ]
    };
    for &field in fields {
        if let Some(value) = object.get(field) {
            output.insert(field.to_string(), value.clone());
        }
    }
    Ok(Value::Object(output))
}

fn has_only_embedded_image_data_urls(object: &serde_json::Map<String, Value>) -> bool {
    let images_are_embedded =
        object
            .get("images")
            .and_then(Value::as_array)
            .is_some_and(|images| {
                !images.is_empty()
                    && images.iter().all(|image| {
                        image
                            .get("image_url")
                            .and_then(Value::as_str)
                            .is_some_and(|value| value.starts_with("data:image/"))
                    })
            });
    let mask_is_embedded = object.get("mask").is_none_or(|mask| {
        mask.get("image_url")
            .and_then(Value::as_str)
            .is_some_and(|value| value.starts_with("data:image/"))
    });
    images_are_embedded && mask_is_embedded
}

fn image_edit_multipart_body(request: &Value) -> AppResult<(Bytes, String)> {
    let object = request
        .as_object()
        .ok_or_else(|| AppError::BadRequest(IMAGE_REQUEST_BODY_MUST_BE_OBJECT.to_string()))?;
    let images = object
        .get("images")
        .and_then(Value::as_array)
        .filter(|images| !images.is_empty())
        .ok_or_else(|| {
            AppError::BadRequest("image edit requires at least one image".to_string())
        })?;
    let boundary = format!("----neogate-{}", Uuid::new_v4().simple());
    let mut output = Vec::new();

    for field in [
        "model",
        "prompt",
        "size",
        "quality",
        "output_format",
        "output_compression",
        "background",
        "input_fidelity",
        "moderation",
        "n",
    ] {
        let Some(value) = object.get(field) else {
            continue;
        };
        let value = match value {
            Value::String(value) => value.clone(),
            Value::Bool(value) => value.to_string(),
            Value::Number(value) => value.to_string(),
            _ => continue,
        };
        append_multipart_text(&mut output, &boundary, field, &value);
    }

    let field_name = if images.len() == 1 {
        "image"
    } else {
        "image[]"
    };
    for (index, image) in images.iter().enumerate() {
        let image_url = image
            .get("image_url")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::BadRequest(
                    "Responses image edits require data URL input images for multipart forwarding"
                        .to_string(),
                )
            })?;
        let (mime, extension, bytes) = decode_image_data_url(image_url)?;
        append_multipart_file(
            &mut output,
            &boundary,
            field_name,
            &format!("image_{}.{}", index + 1, extension),
            mime,
            &bytes,
        );
    }
    if let Some(mask_url) = object
        .get("mask")
        .and_then(|mask| mask.get("image_url"))
        .and_then(Value::as_str)
    {
        let (mime, extension, bytes) = decode_image_data_url(mask_url)?;
        append_multipart_file(
            &mut output,
            &boundary,
            "mask",
            &format!("mask.{extension}"),
            mime,
            &bytes,
        );
    }
    output.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok((Bytes::from(output), boundary))
}

fn append_multipart_text(output: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    output.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    output.extend_from_slice(value.as_bytes());
    output.extend_from_slice(b"\r\n");
}

fn append_multipart_file(
    output: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) {
    output.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    output.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    output.extend_from_slice(format!("Content-Type: {mime}\r\n\r\n").as_bytes());
    output.extend_from_slice(bytes);
    output.extend_from_slice(b"\r\n");
}

fn decode_image_data_url(value: &str) -> AppResult<(&'static str, &'static str, Vec<u8>)> {
    let (metadata, encoded) = value
        .split_once(',')
        .ok_or_else(|| AppError::BadRequest("input image must be a base64 data URL".to_string()))?;
    let mime = metadata
        .strip_prefix("data:")
        .and_then(|metadata| metadata.strip_suffix(";base64"))
        .ok_or_else(|| AppError::BadRequest("input image must be a base64 data URL".to_string()))?;
    let (mime, extension) = match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => ("image/jpeg", "jpg"),
        "image/png" => ("image/png", "png"),
        "image/webp" => ("image/webp", "webp"),
        _ => {
            return Err(AppError::BadRequest(format!(
                "unsupported input image content type: {mime}"
            )))
        }
    };
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|err| AppError::BadRequest(format!("invalid input image base64: {err}")))?;
    if bytes.len() >= MAX_IMAGE_EDIT_INPUT_BYTES {
        return Err(AppError::BadRequest(
            "input images and masks must each be smaller than 50MB".to_string(),
        ));
    }
    Ok((mime, extension, bytes))
}

fn image_generation_upstream_path(request: &ImageRequestSummary) -> &'static str {
    if request.images.is_some() {
        "/v1/images/edits"
    } else {
        "/v1/images/generations"
    }
}

fn image_generation_upstream_path_from_body(body: &[u8]) -> &'static str {
    serde_json::from_slice::<ImageRequestSummary>(body)
        .as_ref()
        .map(image_generation_upstream_path)
        .unwrap_or("unknown")
}

struct ImageTaskErrorDiagnostics {
    debug: String,
    kind: &'static str,
    retryable: bool,
    is_timeout: bool,
    is_connect: bool,
    url: Option<String>,
    source_chain: Vec<String>,
}

fn image_task_error_diagnostics(err: &AppError) -> ImageTaskErrorDiagnostics {
    let (kind, is_timeout, is_connect, url) = match err {
        AppError::Reqwest(source) => {
            let kind = UpstreamErrorKind::from_reqwest(source);
            (
                kind.type_code(),
                source.is_timeout(),
                source.is_connect(),
                source.url().map(ToString::to_string),
            )
        }
        AppError::UpstreamRequest(source) => (
            source.kind.type_code(),
            source.kind == UpstreamErrorKind::Timeout,
            source.kind == UpstreamErrorKind::Connect,
            None,
        ),
        _ => ("application_error", false, false, None),
    };
    let mut source_chain = Vec::new();
    let mut source = StdError::source(err);
    while let Some(current) = source {
        source_chain.push(current.to_string());
        source = current.source();
    }

    ImageTaskErrorDiagnostics {
        debug: format!("{err:?}"),
        kind,
        retryable: err.retryable(),
        is_timeout,
        is_connect,
        url,
        source_chain,
    }
}

fn image_output_type(output_format: &str) -> (&'static str, &'static str) {
    match output_format {
        "jpeg" | "jpg" => ("image/jpeg", "jpg"),
        "webp" => ("image/webp", "webp"),
        _ => ("image/png", "png"),
    }
}

async fn save_image_asset(
    state: &AppState,
    task: &UpstreamTask,
    index: usize,
    b64: &str,
    mime: &str,
    extension: &str,
    revised_prompt: Option<String>,
) -> AppResult<NeogateResponseAsset> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|err| AppError::BadRequest(format!("invalid image result base64: {err}")))?;
    let relative = format!(
        "responses/{}/{}/{}.{}",
        Utc::now().format("%Y-%m-%d"),
        task.upstream_task_id,
        index,
        extension,
    );
    let path = asset_path(&state.config.response_assets.dir, &relative)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut file = fs::File::create(&path).await?;
    file.write_all(&bytes).await?;
    file.flush().await?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    Ok(NeogateResponseAsset {
        path: relative,
        mime: mime.to_string(),
        sha256,
        bytes: bytes.len(),
        index,
        revised_prompt,
    })
}

async fn outputs_from_assets(
    state: &AppState,
    response_id: &str,
    image_format: NeogateImageResponseFormat,
    assets: &[NeogateResponseAsset],
) -> AppResult<Vec<Value>> {
    let mut outputs = Vec::with_capacity(assets.len());
    for asset in assets {
        let path = asset_path(&state.config.response_assets.dir, &asset.path)?;
        let mut output = json!({
            "id": format!("ig_{}", asset.index),
            "type": "image_generation_call",
            "status": "completed",
        });
        if let Some(revised_prompt) = &asset.revised_prompt {
            output["revised_prompt"] = Value::String(revised_prompt.clone());
        }
        if matches!(
            image_format,
            NeogateImageResponseFormat::Base64 | NeogateImageResponseFormat::Both
        ) {
            let bytes = fs::read(&path).await.map_err(|err| {
                AppError::BadRequest(format!("neogate response image asset is missing: {err}"))
            })?;
            output["result"] = Value::String(STANDARD.encode(bytes));
        } else {
            fs::metadata(&path).await.map_err(|err| {
                AppError::BadRequest(format!("neogate response image asset is missing: {err}"))
            })?;
        }
        if image_format.requires_neogate_asset_url() {
            output["url"] = Value::String(signed_asset_url(state, response_id, asset.index));
        }
        outputs.push(output);
    }
    Ok(outputs)
}

pub(crate) async fn asset_response(
    state: &AppState,
    response_id: &str,
    index: usize,
    expires: i64,
    sig: &str,
) -> AppResult<Response> {
    verify_asset_signature(state, response_id, index, expires, sig)?;
    if Utc::now().timestamp() > expires {
        return Err(AppError::Unauthorized);
    }

    let row = sqlx::query(
        r#"
        SELECT upstream_metadata
        FROM task_upstream
        WHERE task_type = 'neogate_response' AND upstream_task_id = $1
        "#,
    )
    .bind(response_id)
    .fetch_optional(&state.db.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::NotFound);
    };
    let value: Value = row.try_get("upstream_metadata")?;
    let metadata: NeogateResponseMetadata = serde_json::from_value(value)?;
    let asset = metadata
        .assets
        .iter()
        .find(|asset| asset.index == index)
        .ok_or(AppError::NotFound)?;
    let path = asset_path(&state.config.response_assets.dir, &asset.path)?;
    let bytes = fs::read(path).await.map_err(|err| {
        AppError::BadRequest(format!("neogate response image asset is missing: {err}"))
    })?;
    let content_type = HeaderValue::from_str(&asset.mime)
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    let max_age = (expires - Utc::now().timestamp()).max(0);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, format!("private, max-age={max_age}"))
        .body(Body::from(bytes))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

fn signed_asset_url(state: &AppState, response_id: &str, index: usize) -> String {
    let expires = Utc::now().timestamp() + ASSET_URL_TTL_SECONDS as i64;
    let sig = asset_signature(state, response_id, index, expires);
    let path = format!("/v1/responses/{response_id}/assets/{index}?expires={expires}&sig={sig}");
    state
        .config
        .public_base_url
        .as_deref()
        .map(|base| format!("{}{}", base.trim_end_matches('/'), path))
        .unwrap_or(path)
}

fn verify_asset_signature(
    state: &AppState,
    response_id: &str,
    index: usize,
    expires: i64,
    sig: &str,
) -> AppResult<()> {
    let provided = hex::decode(sig).map_err(|_| AppError::Unauthorized)?;
    let mut mac =
        <HmacSha256 as KeyInit>::new_from_slice(state.config.upstream_secret_key.as_bytes())
            .expect("hmac accepts any key length");
    mac.update(&asset_signature_payload(response_id, index, expires));
    mac.verify_slice(&provided)
        .map_err(|_| AppError::Unauthorized)?;
    Ok(())
}

fn asset_signature(state: &AppState, response_id: &str, index: usize, expires: i64) -> String {
    hex::encode(asset_signature_bytes(state, response_id, index, expires))
}

fn asset_signature_bytes(
    state: &AppState,
    response_id: &str,
    index: usize,
    expires: i64,
) -> Vec<u8> {
    let mut mac =
        <HmacSha256 as KeyInit>::new_from_slice(state.config.upstream_secret_key.as_bytes())
            .expect("hmac accepts any key length");
    mac.update(&asset_signature_payload(response_id, index, expires));
    mac.finalize().into_bytes().to_vec()
}

fn asset_signature_payload(response_id: &str, index: usize, expires: i64) -> Vec<u8> {
    format!("neogate_asset.v1.{response_id}.{index}.{expires}").into_bytes()
}

fn asset_output_metadata(assets: &[NeogateResponseAsset]) -> Vec<Value> {
    assets
        .iter()
        .map(|asset| {
            let mut output = json!({
                "id": format!("ig_{}", asset.index),
                "type": "image_generation_call",
                "status": "completed",
            });
            if let Some(revised_prompt) = &asset.revised_prompt {
                output["revised_prompt"] = Value::String(revised_prompt.clone());
            }
            output
        })
        .collect()
}

fn response_json(
    response_id: &str,
    model: &str,
    status: &str,
    output: Vec<Value>,
    error: Option<Value>,
    usage: Option<UsageSummary>,
) -> Value {
    json!({
        "id": response_id,
        "object": "response",
        "created_at": Utc::now().timestamp(),
        "status": status,
        "background": true,
        "model": model,
        "output": output,
        "error": error,
        "usage": usage,
    })
}

fn metadata(task: &UpstreamTask) -> AppResult<NeogateResponseMetadata> {
    serde_json::from_value(task.upstream_metadata.clone()).map_err(Into::into)
}

async fn mark_due_now(pool: &PgPool, response_id: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE task_upstream SET next_poll_at = now(), updated_at = now()
         WHERE task_type = 'neogate_response' AND upstream_task_id = $1 AND terminal = FALSE",
    )
    .bind(response_id)
    .execute(pool)
    .await?;
    Ok(())
}

async fn update_metadata(
    state: &AppState,
    task_id: DbId,
    status: &str,
    terminal: bool,
    mut metadata: NeogateResponseMetadata,
    usage: Option<TokenUsage>,
    expected_status: Option<&str>,
) -> AppResult<bool> {
    metadata.response["status"] = Value::String(status.to_string());
    let usage_summary = UsageSummary::value_from_usage(usage)?;
    let next_poll_at = next_poll_at_for_status(status, terminal, Utc::now(), UPSTREAM_TIMEOUT);
    let expires_at = terminal.then(|| asset_expiration(Utc::now()));
    let result = sqlx::query(
        r#"
        UPDATE task_upstream
        SET status = $2,
            terminal = $3,
            upstream_metadata = $4,
            usage_summary = CASE WHEN $5::JSONB = '{}'::JSONB THEN usage_summary ELSE $5 END,
            last_polled_at = now(),
            next_poll_at = $6,
            expires_at = COALESCE($8, expires_at),
            updated_at = now()
        WHERE id = $1
          AND task_type = 'neogate_response'
          AND terminal = FALSE
          AND ($7::TEXT IS NULL OR status = $7)
        "#,
    )
    .bind(task_id)
    .bind(status)
    .bind(terminal)
    .bind(serde_json::to_value(metadata)?)
    .bind(usage_summary)
    .bind(next_poll_at)
    .bind(expected_status)
    .bind(expires_at)
    .execute(&state.db.pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

fn next_poll_at_for_status(
    status: &str,
    terminal: bool,
    now: chrono::DateTime<Utc>,
    upstream_timeout: std::time::Duration,
) -> Option<chrono::DateTime<Utc>> {
    if terminal {
        return None;
    }
    match status {
        STATUS_QUEUED => Some(now),
        STATUS_IN_PROGRESS => Some(
            now + chrono::Duration::from_std(image_task_lease(upstream_timeout))
                .unwrap_or_else(|_| chrono::Duration::minutes(15)),
        ),
        _ => None,
    }
}

fn image_task_lease(upstream_timeout: std::time::Duration) -> std::time::Duration {
    upstream_timeout
        .checked_add(IMAGE_TASK_LEASE_MARGIN)
        .unwrap_or(upstream_timeout)
}

fn asset_expiration(completed_at: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    completed_at
        + chrono::Duration::from_std(super::ASSET_RETENTION)
            .expect("asset retention must fit in chrono duration")
}

async fn set_terminal_status(
    state: &AppState,
    task_id: DbId,
    status: &str,
    usage: Option<TokenUsage>,
    metadata: NeogateResponseMetadata,
) -> AppResult<bool> {
    update_metadata(state, task_id, status, true, metadata, usage, None).await
}

async fn fail_malformed_response_task(
    state: &AppState,
    task_id: DbId,
    response_id: &str,
    fallback_model: &str,
    expected_status: &str,
    value: &Value,
    parse_error: &str,
) -> AppResult<bool> {
    let model = value
        .get("response")
        .and_then(|response| response.get("model"))
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    let request_spool = value
        .get("request_spool")
        .cloned()
        .and_then(|value| serde_json::from_value::<spool::Spool>(value).ok());
    let metadata = NeogateResponseMetadata {
        request: json!({}),
        upstream_request: None,
        request_spool: None,
        response: response_json(response_id, model, expected_status, Vec::new(), None, None),
        image_format: NeogateImageResponseFormat::default(),
        assets: Vec::new(),
        error: None,
        cancel_requested: false,
        attempts: 0,
    };
    let updated = fail_response_task(
        state,
        task_id,
        metadata,
        "neogate_response_metadata_invalid",
        "stored async image task metadata is invalid".to_string(),
        Some(expected_status),
        "invalid neogate response metadata",
    )
    .await?;
    if updated {
        if let Some(spool) = request_spool {
            spool::remove(&state.config.response_assets.dir, &spool.path).await;
        }
        tracing::warn!(
            task_id,
            response_id,
            error = parse_error,
            "failed async image task with malformed response metadata"
        );
    }
    Ok(updated)
}

async fn fail_response_task(
    state: &AppState,
    task_id: DbId,
    mut metadata: NeogateResponseMetadata,
    code: &str,
    message: String,
    expected_status: Option<&str>,
    billing_context: &str,
) -> AppResult<bool> {
    let response_id = metadata
        .response
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let model = metadata
        .response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    metadata.error = Some(json!({ "code": code, "message": message }));
    metadata.response = response_json(
        &response_id,
        &model,
        STATUS_FAILED,
        Vec::new(),
        metadata.error.clone(),
        None,
    );
    let updated = update_metadata(
        state,
        task_id,
        STATUS_FAILED,
        true,
        metadata,
        None,
        expected_status,
    )
    .await?;
    if updated {
        task_billing::release_task_hold_by_id(state, task_id, billing_context).await?;
    }
    Ok(updated)
}

async fn task_usage_summary(pool: &PgPool, task_id: DbId) -> AppResult<Option<UsageSummary>> {
    let row = sqlx::query("SELECT usage_summary FROM task_upstream WHERE id = $1")
        .bind(task_id)
        .fetch_optional(pool)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let value: Value = row.try_get("usage_summary")?;
    if value == json!({}) {
        return Ok(None);
    }
    serde_json::from_value(value).map(Some).map_err(Into::into)
}

fn asset_path(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.is_absolute()
        || relative_path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AppError::BadRequest(
            "invalid neogate response asset path".to_string(),
        ));
    }
    Ok(root.join(relative_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_image_generation_tool() {
        assert!(has_image_generation_tool(
            br#"{"model":"gpt","tools":[{"type":"image_generation"}]}"#
        ));
        assert!(!has_image_generation_tool(
            br#"{"model":"gpt","tools":[{"type":"web_search_preview"}]}"#
        ));
    }

    #[test]
    fn prepare_request_body_extracts_image_format_extension() {
        let prepared = prepare_request_body(Bytes::from_static(
            br#"{"model":"gpt","input":"draw","image_format":"url","tools":[{"type":"image_generation"}]}"#,
        ))
        .unwrap();
        assert_eq!(prepared.image_format, NeogateImageResponseFormat::Url);
        assert!(prepared.has_image_generation_tool);
        let sanitized: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert!(sanitized.get("image_format").is_none());
        assert_eq!(sanitized["model"], "gpt");
    }

    #[test]
    fn prepare_request_body_defaults_to_base64_and_rejects_invalid_format() {
        let prepared = prepare_request_body(Bytes::from_static(
            br#"{"model":"gpt","input":"draw","tools":[{"type":"image_generation"}]}"#,
        ))
        .unwrap();
        assert_eq!(prepared.image_format, NeogateImageResponseFormat::Base64);

        let err = prepare_request_body(Bytes::from_static(
            br#"{"model":"gpt","image_format":"file"}"#,
        ))
        .unwrap_err();
        assert!(err.to_string().contains("image_format must be one of"));
    }

    #[test]
    fn compacts_only_embedded_image_data_urls() {
        let mut request = json!({
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_image", "image_url": "data:image/png;base64,abc"},
                    {"type": "input_image", "image_url": "https://example.com/image.png"}
                ]
            }]
        });

        compact_data_url_images(&mut request);

        assert_eq!(
            request["input"][0]["content"][0]["image_url"],
            "[data URL stored separately]"
        );
        assert_eq!(
            request["input"][0]["content"][1]["image_url"],
            "https://example.com/image.png"
        );
    }

    #[test]
    fn detects_image_output_content_type() {
        assert_eq!(image_output_type("jpeg"), ("image/jpeg", "jpg"));
        assert_eq!(image_output_type("webp"), ("image/webp", "webp"));
        assert_eq!(image_output_type(""), ("image/png", "png"));
    }

    #[test]
    fn selects_image_edit_path_when_reference_images_are_present() {
        let edit: ImageRequestSummary = serde_json::from_value(json!({
            "images": [{"image_url": "data:image/png;base64,AAAA"}]
        }))
        .unwrap();
        let generation: ImageRequestSummary =
            serde_json::from_value(json!({"prompt": "Draw a teapot."})).unwrap();
        assert_eq!(image_generation_upstream_path(&edit), "/v1/images/edits");
        assert_eq!(
            image_generation_upstream_path(&generation),
            "/v1/images/generations"
        );
        assert_eq!(
            image_generation_upstream_path_from_body(
                br#"{"images":[{"image_url":"data:image/png;base64,AAAA"}]}"#
            ),
            "/v1/images/edits"
        );
        assert_eq!(
            image_generation_upstream_path_from_body(b"not json"),
            "unknown"
        );
    }

    #[test]
    fn prepares_response_image_edit_as_multipart() {
        let prepared = prepare_image_upstream_request(
            Bytes::from_static(
                br#"{"model":"image-alias","prompt":"Cut out the dog.","images":[{"image_url":"data:image/jpeg;base64,AAECAw=="}],"size":"1024x1536","background":"transparent","input_fidelity":"high","output_format":"png"}"#,
            ),
            "gpt-image-1.5",
        )
        .unwrap();

        assert_eq!(prepared.path, "/v1/images/edits");
        let content_type = prepared.content_type.to_str().unwrap();
        assert!(content_type.starts_with("multipart/form-data; boundary=----neogate-"));
        let body = prepared.body.as_ref();
        for expected in [
            b"name=\"model\"\r\n\r\ngpt-image-1.5\r\n".as_slice(),
            b"name=\"prompt\"\r\n\r\nCut out the dog.\r\n".as_slice(),
            b"name=\"size\"\r\n\r\n1024x1536\r\n".as_slice(),
            b"name=\"background\"\r\n\r\ntransparent\r\n".as_slice(),
            b"name=\"input_fidelity\"\r\n\r\nhigh\r\n".as_slice(),
            b"name=\"output_format\"\r\n\r\npng\r\n".as_slice(),
            b"name=\"image\"; filename=\"image_1.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n\x00\x01\x02\x03\r\n"
                .as_slice(),
        ] {
            assert!(
                body.windows(expected.len()).any(|window| window == expected),
                "multipart body is missing {:?}",
                String::from_utf8_lossy(expected)
            );
        }
        assert!(!body
            .windows(b"name=\"images\"".len())
            .any(|window| { window == b"name=\"images\"" }));
    }

    #[test]
    fn keeps_response_image_generation_as_json() {
        let prepared = prepare_image_upstream_request(
            Bytes::from_static(
                br#"{"model":"image-alias","prompt":"Draw a teapot.","quality":"high","stream":true,"partial_images":2,"unknown":"discard"}"#,
            ),
            "gpt-image-2",
        )
        .unwrap();

        assert_eq!(prepared.path, "/v1/images/generations");
        assert_eq!(
            prepared.content_type,
            HeaderValue::from_static("application/json")
        );
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["model"], "gpt-image-2");
        assert_eq!(body["prompt"], "Draw a teapot.");
        assert_eq!(body["quality"], "high");
        assert!(body.get("unknown").is_none());
        assert!(body.get("images").is_none());
        assert!(body.get("stream").is_none());
        assert!(body.get("partial_images").is_none());
    }

    #[test]
    fn keeps_remote_and_file_edit_references_in_official_json_shape() {
        for image in [
            json!({"image_url": "https://example.com/input.png"}),
            json!({"file_id": "file-input"}),
        ] {
            let request = json!({
                "model": "image-alias",
                "prompt": "Edit the image.",
                "images": [image],
                "input_fidelity": "high",
                "action": "edit"
            });
            let prepared = prepare_image_upstream_request(
                Bytes::from(serde_json::to_vec(&request).unwrap()),
                "gpt-image-1.5",
            )
            .unwrap();

            assert_eq!(prepared.path, "/v1/images/edits");
            assert_eq!(
                prepared.content_type,
                HeaderValue::from_static("application/json")
            );
            let body: Value = serde_json::from_slice(&prepared.body).unwrap();
            assert_eq!(body["model"], "gpt-image-1.5");
            assert_eq!(body["images"][0], request["images"][0]);
            assert_eq!(body["input_fidelity"], "high");
            assert!(body.get("action").is_none());
        }
    }

    #[test]
    fn rejects_invalid_image_api_requests_before_forwarding() {
        for request in [
            json!({"model": "gpt-image-2", "prompt": ""}),
            json!({"model": "gpt-image-2", "prompt": "edit", "images": []}),
            json!({
                "model": "gpt-image-2",
                "prompt": "edit",
                "images": [{"image_url": "https://example.com/a.png", "file_id": "file-a"}]
            }),
        ] {
            assert!(prepare_image_upstream_request(
                Bytes::from(serde_json::to_vec(&request).unwrap()),
                "gpt-image-2",
            )
            .is_err());
        }
    }

    #[test]
    fn omits_configurable_input_fidelity_for_gpt_image_2() {
        let request = json!({
            "model": "image-alias",
            "prompt": "Edit the image.",
            "images": [{"file_id": "file-input"}],
            "input_fidelity": "high"
        });

        let prepared = prepare_image_upstream_request(
            Bytes::from(serde_json::to_vec(&request).unwrap()),
            "gpt-image-2",
        )
        .unwrap();
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(body.get("input_fidelity").is_none());
    }

    #[test]
    fn converts_embedded_response_mask_to_multipart_file() {
        let request = json!({
            "model": "gpt-image-2",
            "prompt": "Edit the masked area.",
            "images": [{"image_url": "data:image/png;base64,AA=="}],
            "mask": {"image_url": "data:image/png;base64,AQ=="}
        });

        let prepared = prepare_image_upstream_request(
            Bytes::from(serde_json::to_vec(&request).unwrap()),
            "gpt-image-2",
        )
        .unwrap();

        assert!(prepared
            .content_type
            .to_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary="));
        assert!(prepared
            .body
            .windows(b"name=\"mask\"; filename=\"mask.png\"".len())
            .any(|window| window == b"name=\"mask\"; filename=\"mask.png\""));
    }

    #[test]
    fn uses_image_array_parts_for_multiple_edit_inputs() {
        let request = json!({
            "model": "gpt-image-2",
            "prompt": "Combine the references.",
            "images": [
                {"image_url": "data:image/png;base64,AA=="},
                {"image_url": "data:image/webp;base64,AQ=="}
            ]
        });

        let (body, _) = image_edit_multipart_body(&request).unwrap();
        assert_eq!(
            body.windows(b"name=\"image[]\"".len())
                .filter(|window| *window == b"name=\"image[]\"")
                .count(),
            2
        );
        assert!(!body
            .windows(b"name=\"image\";".len())
            .any(|window| window == b"name=\"image\";"));
    }

    #[test]
    fn parses_image_response_outputs_without_preserving_unknown_fields() {
        let response: ImageGenerationResponse = serde_json::from_value(json!({
            "data": [
                {"b64_json": "AAAA", "revised_prompt": "first", "ignored": "large"},
                {"result": "BBBB"}
            ],
            "usage": {"total_tokens": 10}
        }))
        .unwrap();

        assert_eq!(response.data.len(), 2);
        assert_eq!(response.data[0].b64_json.as_deref(), Some("AAAA"));
        assert_eq!(response.data[0].revised_prompt.as_deref(), Some("first"));
        assert_eq!(response.data[1].result.as_deref(), Some("BBBB"));
    }

    #[test]
    fn asset_expiration_is_three_days_after_completion() {
        let completed_at = Utc::now();
        assert_eq!(
            asset_expiration(completed_at) - completed_at,
            chrono::Duration::days(3)
        );
    }

    #[test]
    fn task_poll_schedule_keeps_an_in_progress_lease() {
        let now = Utc::now();
        assert_eq!(
            next_poll_at_for_status(
                STATUS_QUEUED,
                false,
                now,
                std::time::Duration::from_secs(600),
            ),
            Some(now)
        );
        assert_eq!(
            next_poll_at_for_status(
                STATUS_IN_PROGRESS,
                false,
                now,
                std::time::Duration::from_secs(600),
            ),
            Some(now + chrono::Duration::minutes(15))
        );
        assert_eq!(
            next_poll_at_for_status(
                STATUS_COMPLETED,
                true,
                now,
                std::time::Duration::from_secs(600),
            ),
            None
        );
        assert_eq!(
            image_task_lease(std::time::Duration::from_secs(20 * 60)),
            std::time::Duration::from_secs(25 * 60)
        );
    }

    #[test]
    fn asset_directory_validation_rejects_other_tasks_and_parent_paths() {
        let root = Path::new("/tmp/neogate-assets");
        assert!(asset_directories(
            root,
            "resp_expected",
            "responses/2026-07-15/resp_other/0.png"
        )
        .is_err());
        assert!(asset_directories(
            root,
            "resp_expected",
            "responses/2026-07-15/resp_expected/../0.png"
        )
        .is_err());
        assert!(asset_directories(root, "resp_expected", "/tmp/0.png").is_err());
    }

    #[tokio::test]
    async fn removes_task_directory_and_empty_date_directory_idempotently() {
        let root = std::env::temp_dir().join(format!(
            "neogate-asset-cleanup-test-{}",
            Uuid::new_v4().simple()
        ));
        let task_dir = root.join("responses").join("2026-07-15").join("resp_test");
        fs::create_dir_all(&task_dir).await.unwrap();
        fs::write(task_dir.join("0.png"), b"image").await.unwrap();
        fs::write(task_dir.parent().unwrap().join(".DS_Store"), b"")
            .await
            .unwrap();
        let assets = vec![test_asset("responses/2026-07-15/resp_test/0.png")];

        remove_asset_directories(&root, "resp_test", &assets)
            .await
            .unwrap();
        remove_asset_directories(&root, "resp_test", &assets)
            .await
            .unwrap();

        assert!(!task_dir.exists());
        assert!(!task_dir.parent().unwrap().exists());
        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn keeps_date_directory_while_a_sibling_task_exists() {
        let root = std::env::temp_dir().join(format!(
            "neogate-asset-cleanup-test-{}",
            Uuid::new_v4().simple()
        ));
        let date_dir = root.join("responses").join("2026-07-15");
        let task_dir = date_dir.join("resp_test");
        let sibling_dir = date_dir.join("resp_sibling");
        fs::create_dir_all(&task_dir).await.unwrap();
        fs::create_dir_all(&sibling_dir).await.unwrap();
        fs::write(task_dir.join("0.png"), b"image").await.unwrap();
        fs::write(sibling_dir.join("0.png"), b"image")
            .await
            .unwrap();
        let assets = vec![test_asset("responses/2026-07-15/resp_test/0.png")];

        remove_asset_directories(&root, "resp_test", &assets)
            .await
            .unwrap();

        assert!(!task_dir.exists());
        assert!(date_dir.exists());
        assert!(sibling_dir.exists());
        let _ = fs::remove_dir_all(root).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn does_not_follow_symlinked_asset_date_directory() {
        use std::os::unix::fs::symlink;

        let test_id = Uuid::new_v4().simple();
        let root = std::env::temp_dir().join(format!("neogate-asset-cleanup-test-{test_id}"));
        let outside = std::env::temp_dir().join(format!("neogate-asset-cleanup-outside-{test_id}"));
        let outside_task = outside.join("resp_test");
        fs::create_dir_all(&outside_task).await.unwrap();
        fs::write(outside_task.join("0.png"), b"image")
            .await
            .unwrap();
        fs::create_dir_all(root.join("responses")).await.unwrap();
        symlink(&outside, root.join("responses").join("2026-07-15")).unwrap();
        let assets = vec![test_asset("responses/2026-07-15/resp_test/0.png")];

        let result = remove_asset_directories(&root, "resp_test", &assets).await;

        assert!(result.is_err());
        assert!(outside_task.join("0.png").exists());
        let _ = fs::remove_dir_all(root).await;
        let _ = fs::remove_dir_all(outside).await;
    }

    fn test_asset(path: &str) -> NeogateResponseAsset {
        NeogateResponseAsset {
            path: path.to_string(),
            mime: "image/png".to_string(),
            sha256: String::new(),
            bytes: 0,
            index: 0,
            revised_prompt: None,
        }
    }
}
