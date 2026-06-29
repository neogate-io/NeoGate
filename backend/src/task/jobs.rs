use std::path::{Path, PathBuf};

use axum::{
    body::Body,
    http::{header, HeaderValue, StatusCode},
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::Bytes;
use chrono::Utc;
use futures_util::StreamExt;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use tokio::{fs, io::AsyncWriteExt};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{parse_usage_from_sse_data, DebitHold, TokenUsage},
    error::{AppError, AppResult},
    id::DbId,
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
const SSE_BUFFER_LIMIT_BYTES: usize = 32 * 1024 * 1024;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NeogateResponseMetadata {
    request: Value,
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
}

struct NeogateResponseResult {
    response: Value,
    assets: Vec<NeogateResponseAsset>,
    usage: Option<TokenUsage>,
}

#[derive(Default)]
struct NeogateResponseSseCollector {
    buffered: Vec<u8>,
    completed: Option<Value>,
    usage: Option<TokenUsage>,
}

pub(crate) fn has_image_generation_tool(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    value
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .any(|tool| tool.get("type").and_then(Value::as_str) == Some("image_generation"))
        })
        .unwrap_or(false)
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
    upstream: &SelectedUpstream,
    model: &str,
    request_body: Bytes,
    image_format: NeogateImageResponseFormat,
    hold: &DebitHold,
) -> AppResult<Value> {
    let response_id = format!("resp_{}", Uuid::new_v4().simple());
    let request: Value = serde_json::from_slice(&request_body)?;
    let response = response_json(&response_id, model, STATUS_QUEUED, Vec::new(), None, None);
    let metadata = NeogateResponseMetadata {
        request,
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
            model: Some(model),
            upstream_model: Some(model),
            status: STATUS_QUEUED,
            terminal: false,
            hold,
            upstream_metadata: serde_json::to_value(metadata)?,
        },
        state.config.task.upstream_poll_interval,
        state.config.response_assets.retention,
    )
    .await?;
    mark_due_now(&state.db.pool, &response_id).await?;
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
        return Ok(());
    }

    let mut metadata = metadata(&task)?;
    if metadata.cancel_requested || task.status == STATUS_CANCELLED {
        if set_terminal_status(&state.db.pool, task.id, STATUS_CANCELLED, None, metadata).await? {
            task_billing::release_task_hold_by_id(state, task.id, "cancelled neogate response")
                .await?;
        }
        return Ok(());
    }

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
    let body = Bytes::from(serde_json::to_vec(&stream_request(&metadata.request)?)?);
    let result = run_streamed_response(state, &task, &upstream, body).await;
    match result {
        Ok(result) => {
            metadata.response = result.response;
            metadata.assets = result.assets;
            metadata.error = None;
            if set_terminal_status(
                &state.db.pool,
                task.id,
                STATUS_COMPLETED,
                result.usage,
                metadata,
            )
            .await?
            {
                let updated = upstream::fetch_task(
                    &state.db.pool,
                    task.user_key_id,
                    UpstreamTaskType::NeogateResponse,
                    &task.upstream_task_id,
                )
                .await?;
                task_billing::finalize_polled(state, updated, upstream, result.usage).await?;
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
                "neogate async response failed"
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

pub(crate) async fn cleanup_expired_assets(state: &AppState, limit: i64) -> AppResult<()> {
    let rows = sqlx::query(
        r#"
        SELECT id, upstream_metadata
        FROM task_upstream
        WHERE task_type = 'neogate_response'
          AND terminal = TRUE
          AND expires_at IS NOT NULL
          AND expires_at <= now()
        ORDER BY expires_at ASC, id ASC
        LIMIT $1
        "#,
    )
    .bind(limit.max(1))
    .fetch_all(&state.db.pool)
    .await?;

    for row in rows {
        let value: Value = row.try_get("upstream_metadata")?;
        let Ok(metadata) = serde_json::from_value::<NeogateResponseMetadata>(value) else {
            continue;
        };
        for asset in metadata.assets {
            let path = asset_path(&state.config.response_assets.dir, &asset.path)?;
            if let Err(err) = fs::remove_file(path).await {
                if err.kind() != std::io::ErrorKind::NotFound {
                    tracing::warn!("failed to remove expired neogate response asset: {err}");
                }
            }
        }
    }
    Ok(())
}

async fn run_streamed_response(
    state: &AppState,
    task: &UpstreamTask,
    upstream: &SelectedUpstream,
    body: Bytes,
) -> AppResult<NeogateResponseResult> {
    let response = forward_openai(
        state,
        upstream,
        UpstreamProtocol::Openai,
        body,
        "/v1/responses",
    )
    .await?;
    if !response.status().is_success() {
        let status = StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = read_upstream_error_body(response).await;
        let failure = describe_upstream_http_failure(status, &body);
        return Err(AppError::BadRequest(failure.summary));
    }
    let mut collector = NeogateResponseSseCollector::default();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        collector.observe(&chunk?)?;
    }
    let completed = collector.completed.ok_or_else(|| {
        AppError::BadRequest("upstream stream ended without completion".to_string())
    })?;
    let outputs = completed
        .get("response")
        .and_then(|response| response.get("output"))
        .or_else(|| completed.get("output"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut assets = Vec::new();
    for (index, output) in outputs.iter().enumerate() {
        if output.get("type").and_then(Value::as_str) != Some("image_generation_call") {
            continue;
        }
        let Some(result) = output.get("result").and_then(Value::as_str) else {
            continue;
        };
        assets.push(save_image_asset(state, task, index, result).await?);
    }
    if assets.is_empty() {
        return Err(AppError::BadRequest(
            "completed response did not include image_generation_call result".to_string(),
        ));
    }
    let mut response = completed
        .get("response")
        .cloned()
        .unwrap_or_else(|| completed.clone());
    response["id"] = Value::String(task.upstream_task_id.clone());
    response["status"] = Value::String(STATUS_COMPLETED.to_string());
    response["background"] = Value::Bool(true);
    response["output"] = Value::Array(outputs_without_results(&outputs));
    Ok(NeogateResponseResult {
        response,
        assets,
        usage: collector.usage,
    })
}

impl NeogateResponseSseCollector {
    fn observe(&mut self, chunk: &[u8]) -> AppResult<()> {
        if self.buffered.len().saturating_add(chunk.len()) > SSE_BUFFER_LIMIT_BYTES {
            return Err(AppError::BadRequest(
                "upstream response stream event exceeded buffer limit".to_string(),
            ));
        }
        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .windows(2)
            .position(|window| window == b"\n\n")
        {
            let end = consumed + offset;
            let event = String::from_utf8_lossy(&self.buffered[consumed..end]).to_string();
            self.observe_event(&event)?;
            consumed = end + 2;
        }
        if consumed > 0 {
            self.buffered.drain(..consumed);
        }
        Ok(())
    }

    fn observe_event(&mut self, event: &str) -> AppResult<()> {
        let mut data_lines = Vec::new();
        for line in event.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            data_lines.push(data);
        }
        if data_lines.is_empty() {
            return Ok(());
        }
        let data = data_lines.join("\n");
        if let Some(usage) = parse_usage_from_sse_data(&data) {
            self.usage = Some(usage);
        }
        let value: Value = serde_json::from_str(&data)?;
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        if event_type == "response.completed" {
            self.completed = Some(value);
        }
        Ok(())
    }
}

fn stream_request(request: &Value) -> AppResult<Value> {
    let mut value = request.clone();
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be an object".to_string()))?;
    object.insert("stream".to_string(), Value::Bool(true));
    object.insert("store".to_string(), Value::Bool(true));
    object.remove("background");
    object.remove("image_format");
    Ok(value)
}

async fn save_image_asset(
    state: &AppState,
    task: &UpstreamTask,
    index: usize,
    b64: &str,
) -> AppResult<NeogateResponseAsset> {
    let bytes = STANDARD
        .decode(b64)
        .map_err(|err| AppError::BadRequest(format!("invalid image result base64: {err}")))?;
    let relative = format!(
        "responses/{}/{}/{}.png",
        Utc::now().format("%Y-%m-%d"),
        task.upstream_task_id,
        index
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
        mime: "image/png".to_string(),
        sha256,
        bytes: bytes.len(),
        index,
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
    let ttl = state
        .config
        .response_assets
        .retention
        .as_secs()
        .clamp(1, ASSET_URL_TTL_SECONDS);
    let expires = Utc::now().timestamp() + ttl as i64;
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

fn outputs_without_results(outputs: &[Value]) -> Vec<Value> {
    outputs
        .iter()
        .enumerate()
        .filter_map(|(index, output)| {
            if output.get("type").and_then(Value::as_str) != Some("image_generation_call") {
                return None;
            }
            Some(json!({
                "id": output
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("ig_{index}")),
                "type": "image_generation_call",
                "status": output
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("completed"),
            }))
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
    let usage_summary = usage
        .map(UsageSummary::from_usage)
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or_else(|| json!({}));
    let next_poll_at = (!terminal && status == STATUS_QUEUED).then(Utc::now);
    let result = sqlx::query(
        r#"
        UPDATE task_upstream
        SET status = $2,
            terminal = $3,
            upstream_metadata = $4,
            usage_summary = CASE WHEN $5::JSONB = '{}'::JSONB THEN usage_summary ELSE $5 END,
            last_polled_at = now(),
            next_poll_at = $6,
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
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
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
            br#"{"model":"gpt","image_format":"url","tools":[{"type":"image_generation"}]}"#,
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
            br#"{"model":"gpt","tools":[{"type":"image_generation"}]}"#,
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
    fn stream_request_removes_background_and_enables_stream() {
        let request = json!({
            "model": "gpt",
            "background": true,
            "store": true,
            "image_format": "url",
            "tools": [{"type": "image_generation"}]
        });
        let value = stream_request(&request).unwrap();
        assert_eq!(value["stream"], true);
        assert_eq!(value["store"], true);
        assert!(value.get("background").is_none());
        assert!(value.get("image_format").is_none());
    }

    #[test]
    fn collector_reads_completed_response_usage() {
        let mut collector = NeogateResponseSseCollector::default();
        collector
            .observe(br#"event: response.completed
data: {"type":"response.completed","response":{"output":[{"type":"image_generation_call","result":"QUJD"}],"usage":{"input_tokens":1,"output_tokens":2}}}

"#)
            .unwrap();
        assert!(collector.completed.is_some());
        assert_eq!(collector.usage.unwrap().input_tokens, 1);
    }

    #[test]
    fn collector_reads_multiline_sse_data() {
        let mut collector = NeogateResponseSseCollector::default();
        collector
            .observe(
                br#"event: response.completed
data: {"type":"response.completed",
data: "response":{"output":[{"type":"image_generation_call","result":"QUJD"}]}}

"#,
            )
            .unwrap();

        assert!(collector.completed.is_some());
    }

    #[test]
    fn collector_allows_large_image_result_event() {
        let mut collector = NeogateResponseSseCollector::default();
        let result = "A".repeat(3 * 1024 * 1024);
        let event = format!(
            "event: response.completed\ndata: {{\"type\":\"response.completed\",\"response\":{{\"output\":[{{\"type\":\"image_generation_call\",\"result\":\"{result}\"}}]}}}}\n\n"
        );

        for chunk in event.as_bytes().chunks(8192) {
            collector.observe(chunk).unwrap();
        }

        assert!(collector.completed.is_some());
    }
}
