use std::{
    collections::HashSet,
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
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{parse_usage_from_bytes, DebitHold, TokenUsage},
    error::{reqwest_status, AppError, AppResult},
    id::DbId,
    provider::adapters::adapter_for_endpoint,
    relay::{
        describe_upstream_http_failure, forward_openai, read_upstream_error_body,
        selector::{SelectedUpstream, UpstreamProtocol},
    },
    task::{billing as task_billing, upstream},
    AppState,
};

use super::upstream::{NewUpstreamTask, UpstreamTask, UpstreamTaskType, UsageSummary};

const STATUS_QUEUED: &str = "queued";
const STATUS_IN_PROGRESS: &str = "in_progress";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";
const ASSET_URL_TTL_SECONDS: u64 = 3600;

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
    response: Value,
    #[serde(default)]
    image_format: NeogateImageResponseFormat,
    #[serde(default)]
    assets: Vec<NeogateResponseAsset>,
    #[serde(default)]
    error: Option<Value>,
    #[serde(default)]
    cancel_requested: bool,
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
    let request: Value = serde_json::from_slice(&request_body)?;
    let size = image_tool_string(&request, "size")
        .unwrap_or("")
        .to_string();
    let quality = image_tool_string(&request, "quality")
        .unwrap_or("")
        .to_string();
    let output_format = image_tool_string(&request, "output_format")
        .unwrap_or("")
        .to_string();
    let upstream_request: Value = serde_json::from_slice(&upstream_request_body)?;
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
        upstream_request: Some(upstream_request),
        response: response.clone(),
        image_format,
        assets: Vec::new(),
        error: None,
        cancel_requested: false,
    };
    upstream::insert_task(
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
    .await?;
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
        if update_metadata(
            &state.db.pool,
            task.id,
            STATUS_CANCELLED,
            true,
            metadata(&task)?,
            None,
            Some(STATUS_QUEUED),
        )
        .await?
        {
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
                    &state.db.pool,
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
            &state.db.pool,
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

    let mut metadata = metadata(&task)?;
    let response_model = metadata
        .response
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if metadata.cancel_requested || task.status == STATUS_CANCELLED {
        if set_terminal_status(&state.db.pool, task.id, STATUS_CANCELLED, None, metadata).await? {
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

    if !update_metadata(
        &state.db.pool,
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
    let body = if let Some(upstream_request) = &metadata.upstream_request {
        Bytes::from(serde_json::to_vec(upstream_request)?)
    } else {
        adapter_for_endpoint(&upstream.provider, &upstream.base_url)
            .prepare_response_image_generation_request(Bytes::from(serde_json::to_vec(
                &metadata.request,
            )?))?
            .ok_or_else(|| {
                AppError::BadRequest(
                    "provider adapter does not translate response image generation".to_string(),
                )
            })?
            .body
    };
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
            metadata.response = response;
            metadata.assets = assets;
            metadata.error = None;
            if set_terminal_status(&state.db.pool, task.id, STATUS_COMPLETED, usage, metadata)
                .await?
            {
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
                upstream_path = "/v1/images/generations",
                "async image task failed"
            );
            metadata.error = Some(json!({
                "code": "neogate_response_failed",
                "message": err.to_string(),
            }));
            metadata.response = response_json(
                &task.upstream_task_id,
                task.model.as_deref().unwrap_or(""),
                STATUS_FAILED,
                Vec::new(),
                metadata.error.clone(),
                None,
            );
            if set_terminal_status(&state.db.pool, task.id, STATUS_FAILED, None, metadata).await? {
                task_billing::release_task_hold_by_id(state, task.id, "failed neogate response")
                    .await?;
            }
        }
    }
    Ok(())
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
            let exists: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM task_upstream WHERE task_type = 'neogate_response' AND upstream_task_id = $1)",
            )
            .bind(&response_id)
            .fetch_one(&state.db.pool)
            .await?;
            if !exists {
                remove_managed_tree(&task_entry.path()).await?;
                deleted += 1;
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
    let request: Value = serde_json::from_slice(&body)?;
    let image_model = request.get("model").and_then(Value::as_str).unwrap_or("");
    let size = request.get("size").and_then(Value::as_str).unwrap_or("");
    let quality = request.get("quality").and_then(Value::as_str).unwrap_or("");
    let output_format = request
        .get("output_format")
        .and_then(Value::as_str)
        .unwrap_or("");
    let requested_image_count = request.get("n").and_then(Value::as_i64).unwrap_or(1);
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
        upstream_path = "/v1/images/generations",
        response_model,
        image_model = %task.model.as_deref().unwrap_or(image_model),
        upstream_image_model = %task.upstream_model.as_deref().unwrap_or(image_model),
        size,
        quality,
        output_format,
        image_count = requested_image_count,
        request_bytes = body.len(),
        "sending async image task to upstream"
    );
    let response = match forward_openai(
        state,
        upstream,
        UpstreamProtocol::Openai,
        body.clone(),
        "/v1/images/generations",
    )
    .await
    {
        Ok(response) => response,
        Err(err) => {
            tracing::warn!(
                task_id = task.id,
                response_id = %task.upstream_task_id,
                provider = %upstream.provider,
                channel_id = upstream.channel_id,
                channel_endpoint_id = upstream.channel_endpoint_id,
                upstream_path = "/v1/images/generations",
                elapsed_ms = started.elapsed().as_millis() as u64,
                error = %err,
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
            upstream_path = "/v1/images/generations",
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
    let value: Value = serde_json::from_slice(&response_body)?;
    let images = value
        .get("data")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if images.is_empty() {
        return Err(AppError::BadRequest(
            "image generation response did not include data".to_string(),
        ));
    }

    let (mime, extension) = image_output_type(&request);
    let mut assets = Vec::new();
    for (index, image) in images.iter().enumerate() {
        let result = image
            .get("b64_json")
            .or_else(|| image.get("result"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::BadRequest(
                    "image generation response did not include base64 image data".to_string(),
                )
            })?;
        let revised_prompt = image
            .get("revised_prompt")
            .and_then(Value::as_str)
            .map(str::to_string);
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
        upstream_path = "/v1/images/generations",
        upstream_status = status.as_u16(),
        content_type = %content_type,
        elapsed_ms = started.elapsed().as_millis() as u64,
        response_bytes = response_body.len(),
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

fn image_output_type(request: &Value) -> (&'static str, &'static str) {
    match request.get("output_format").and_then(Value::as_str) {
        Some("jpeg" | "jpg") => ("image/jpeg", "jpg"),
        Some("webp") => ("image/webp", "webp"),
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
    pool: &PgPool,
    task_id: DbId,
    status: &str,
    terminal: bool,
    mut metadata: NeogateResponseMetadata,
    usage: Option<TokenUsage>,
    expected_status: Option<&str>,
) -> AppResult<bool> {
    metadata.response["status"] = Value::String(status.to_string());
    let usage_summary = UsageSummary::value_from_usage(usage)?;
    let next_poll_at = (!terminal && status == STATUS_QUEUED).then(Utc::now);
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
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

fn asset_expiration(completed_at: chrono::DateTime<Utc>) -> chrono::DateTime<Utc> {
    completed_at
        + chrono::Duration::from_std(super::ASSET_RETENTION)
            .expect("asset retention must fit in chrono duration")
}

async fn set_terminal_status(
    pool: &PgPool,
    task_id: DbId,
    status: &str,
    usage: Option<TokenUsage>,
    metadata: NeogateResponseMetadata,
) -> AppResult<bool> {
    update_metadata(pool, task_id, status, true, metadata, usage, None).await
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
    fn detects_image_output_content_type() {
        assert_eq!(
            image_output_type(&json!({"output_format": "jpeg"})),
            ("image/jpeg", "jpg")
        );
        assert_eq!(
            image_output_type(&json!({"output_format": "webp"})),
            ("image/webp", "webp")
        );
        assert_eq!(image_output_type(&json!({})), ("image/png", "png"));
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
