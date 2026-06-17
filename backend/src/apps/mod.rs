mod endpoints;
mod runtime;

use std::{collections::HashMap, sync::Arc};

use axum::{
    routing::{get, post},
    Router,
};
use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use sqlx::Row;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

const WECOM_TOKEN_SECRET_KEY: &str = "token";
const WECOM_AES_SECRET_KEY: &str = "aes_key";
const WECOM_CORP_SECRET_KEY: &str = "corp_secret";
const WEBHOOK_SECRET_KEY: &str = "secret";
pub(crate) const DEFAULT_CONTEXT_TURNS: i32 = 10;
pub(crate) const DEFAULT_MAX_OUTPUT_TOKENS: i32 = 2048;
const APP_BODY_LIMIT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct AppRecord {
    pub id: DbId,
    pub name: String,
    pub description: String,
    pub app_type: String,
    pub status: String,
    pub model: String,
    pub system_prompt: String,
    pub context_turns: i32,
    pub max_output_tokens: i32,
    pub user_key_id: DbId,
    pub user_key_name: String,
    pub project_id: DbId,
    pub project_name: String,
    pub endpoint: Option<AppEndpointRecord>,
    pub today_message_count: i64,
    pub today_cost_micro_usd: i64,
    pub last_active_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppEndpointRecord {
    pub id: DbId,
    pub app_id: DbId,
    pub endpoint_type: String,
    pub name: String,
    pub enabled: bool,
    pub config: Value,
    pub secrets_set: Vec<String>,
    pub callback_url: Option<String>,
    pub invoke_url: Option<String>,
    pub widget_script_url: Option<String>,
    pub last_active_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AppRunLogRecord {
    pub id: DbId,
    pub app_id: Option<DbId>,
    pub endpoint_id: Option<DbId>,
    pub conversation_id: Option<DbId>,
    pub external_user_id: Option<String>,
    pub external_conversation_id: Option<String>,
    pub external_message_id: Option<String>,
    pub trace_id: Option<String>,
    pub app_type: String,
    pub model: Option<String>,
    pub status: String,
    pub status_code: Option<i32>,
    pub latency_ms: i64,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_micro_usd: Option<i64>,
    pub error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct ListAppsQuery {
    pub search: Option<String>,
    pub status: Option<String>,
    pub app_type: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListAppRunLogsQuery {
    pub app_id: Option<DbId>,
    pub endpoint_id: Option<DbId>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertAppRequest {
    pub name: String,
    pub description: Option<String>,
    pub app_type: String,
    pub status: Option<String>,
    pub model: String,
    pub system_prompt: Option<String>,
    pub context_turns: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub user_key_id: DbId,
    pub endpoint: UpsertEndpointRequest,
}

#[derive(Debug, Deserialize)]
pub struct UpdateAppRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    pub context_turns: Option<i32>,
    pub max_output_tokens: Option<i32>,
    pub user_key_id: Option<DbId>,
    pub endpoint: Option<UpsertEndpointRequest>,
}

#[derive(Debug, Deserialize)]
pub struct UpsertEndpointRequest {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<Value>,
    pub secrets: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
pub struct WecomCallbackQuery {
    msg_signature: Option<String>,
    timestamp: Option<String>,
    nonce: Option<String>,
    echostr: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WebhookMessageRequest {
    external_user_id: Option<String>,
    external_conversation_id: Option<String>,
    message_id: Option<String>,
    content: String,
    metadata: Option<Value>,
    trace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WidgetMessageRequest {
    session_id: Option<String>,
    content: String,
    metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct AppMessageResponse {
    pub ok: bool,
    pub conversation_id: DbId,
    pub message: String,
    pub trace_id: String,
    pub duplicate: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct AppRuntime {
    pub(crate) app_id: DbId,
    pub(crate) endpoint_id: DbId,
    pub(crate) app_type: String,
    name: String,
    pub(crate) status: String,
    model: String,
    system_prompt: String,
    context_turns: i32,
    max_output_tokens: i32,
    user_key_id: DbId,
    pub(crate) endpoint_type: String,
    pub(crate) endpoint_enabled: bool,
    endpoint_config: Value,
    endpoint_secrets: Value,
}

#[derive(Debug, Clone)]
struct IncomingAppMessage {
    external_user_id: String,
    external_conversation_id: String,
    external_message_id: Option<String>,
    content: String,
    metadata: Value,
    trace_id: String,
}

struct AppRunOutcome {
    conversation_id: DbId,
    message: String,
    trace_id: String,
    duplicate: bool,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/apps/wecom/{endpoint_id}/callback",
            get(endpoints::wecom_verify).post(endpoints::wecom_message),
        )
        .route(
            "/apps/webhook/{endpoint_id}",
            post(endpoints::webhook_message),
        )
        .route(
            "/apps/widget/{endpoint_id}/messages",
            post(endpoints::widget_message),
        )
        .route("/widget/{endpoint_id}.js", get(endpoints::widget_script))
}

pub(crate) async fn list_app_records(
    state: &AppState,
    query: ListAppsQuery,
) -> AppResult<Vec<AppRecord>> {
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
    let rows = sqlx::query(
        r#"
        SELECT a.id, a.name, a.description, a.app_type, a.status, a.model,
               a.system_prompt, a.context_turns, a.max_output_tokens, a.user_key_id,
               uk.name AS user_key_name, uk.project_id, p.name AS project_name,
               a.last_active_at, a.created_at, a.updated_at,
               COALESCE(today.message_count, 0)::BIGINT AS today_message_count,
               COALESCE(today.cost_micro_usd, 0)::BIGINT AS today_cost_micro_usd
        FROM app a
        JOIN user_key uk ON uk.id = a.user_key_id
        JOIN project p ON p.id = uk.project_id
        LEFT JOIN (
            SELECT app_id, COUNT(*)::BIGINT AS message_count,
                   COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS cost_micro_usd
            FROM app_run_log
            WHERE created_at >= date_trunc('day', now())
              AND status = 'success'
            GROUP BY app_id
        ) today ON today.app_id = a.id
        WHERE ($1::TEXT IS NULL OR a.name ILIKE '%' || $1 || '%' OR a.description ILIKE '%' || $1 || '%')
          AND ($2::TEXT IS NULL OR a.status = $2)
          AND ($3::TEXT IS NULL OR a.app_type = $3)
        ORDER BY a.created_at DESC, a.id DESC
        "#,
    )
    .bind(search)
    .bind(query.status)
    .bind(query.app_type)
    .fetch_all(&state.db.pool)
    .await?;

    let app_ids: Vec<DbId> = rows
        .iter()
        .map(|row| row.try_get("id"))
        .collect::<Result<_, _>>()?;
    let endpoints = endpoints_by_app(state, &app_ids).await?;
    rows.iter()
        .map(|row| app_from_row(row, endpoints.get(&row.try_get::<DbId, _>("id")?).cloned()))
        .collect()
}

pub(crate) async fn get_app_record(state: &AppState, id: DbId) -> AppResult<AppRecord> {
    let row = sqlx::query(
        r#"
        SELECT a.id, a.name, a.description, a.app_type, a.status, a.model,
               a.system_prompt, a.context_turns, a.max_output_tokens, a.user_key_id,
               uk.name AS user_key_name, uk.project_id, p.name AS project_name,
               a.last_active_at, a.created_at, a.updated_at,
               COALESCE(today.message_count, 0)::BIGINT AS today_message_count,
               COALESCE(today.cost_micro_usd, 0)::BIGINT AS today_cost_micro_usd
        FROM app a
        JOIN user_key uk ON uk.id = a.user_key_id
        JOIN project p ON p.id = uk.project_id
        LEFT JOIN (
            SELECT app_id, COUNT(*)::BIGINT AS message_count,
                   COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS cost_micro_usd
            FROM app_run_log
            WHERE created_at >= date_trunc('day', now())
              AND status = 'success'
            GROUP BY app_id
        ) today ON today.app_id = a.id
        WHERE a.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let endpoints = endpoints_by_app(state, &[id]).await?;
    app_from_row(&row, endpoints.get(&id).cloned())
}

async fn endpoints_by_app(
    state: &AppState,
    app_ids: &[DbId],
) -> AppResult<HashMap<DbId, AppEndpointRecord>> {
    if app_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT id, app_id, endpoint_type, name, enabled, config, secret_ciphertext,
               last_active_at, created_at, updated_at
        FROM app_endpoint
        WHERE app_id = ANY($1)
        ORDER BY created_at ASC
        "#,
    )
    .bind(app_ids)
    .fetch_all(&state.db.pool)
    .await?;
    let mut map = HashMap::new();
    for row in rows {
        let endpoint = endpoint_from_row(state, &row)?;
        map.entry(endpoint.app_id).or_insert(endpoint);
    }
    Ok(map)
}

fn app_from_row(
    row: &sqlx::postgres::PgRow,
    endpoint: Option<AppEndpointRecord>,
) -> AppResult<AppRecord> {
    Ok(AppRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        app_type: row.try_get("app_type")?,
        status: row.try_get("status")?,
        model: row.try_get("model")?,
        system_prompt: row.try_get("system_prompt")?,
        context_turns: row.try_get("context_turns")?,
        max_output_tokens: row.try_get("max_output_tokens")?,
        user_key_id: row.try_get("user_key_id")?,
        user_key_name: row.try_get("user_key_name")?,
        project_id: row.try_get("project_id")?,
        project_name: row.try_get("project_name")?,
        endpoint,
        today_message_count: row.try_get("today_message_count")?,
        today_cost_micro_usd: row.try_get("today_cost_micro_usd")?,
        last_active_at: row.try_get("last_active_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn endpoint_from_row(
    state: &AppState,
    row: &sqlx::postgres::PgRow,
) -> AppResult<AppEndpointRecord> {
    let id: DbId = row.try_get("id")?;
    let endpoint_type: String = row.try_get("endpoint_type")?;
    let secrets: Value = row.try_get("secret_ciphertext")?;
    let secrets_set = secrets
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .unwrap_or_default();
    Ok(AppEndpointRecord {
        id,
        app_id: row.try_get("app_id")?,
        endpoint_type: endpoint_type.clone(),
        name: row.try_get("name")?,
        enabled: row.try_get("enabled")?,
        config: row.try_get("config")?,
        secrets_set,
        callback_url: public_url(state, &format!("/apps/{endpoint_type}/{id}/callback")),
        invoke_url: public_url(state, &format!("/apps/{endpoint_type}/{id}")),
        widget_script_url: if endpoint_type == "widget" {
            public_url(state, &format!("/widget/{id}.js"))
        } else {
            None
        },
        last_active_at: row.try_get("last_active_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) fn run_log_from_row(row: &sqlx::postgres::PgRow) -> AppResult<AppRunLogRecord> {
    Ok(AppRunLogRecord {
        id: row.try_get("id")?,
        app_id: row.try_get("app_id")?,
        endpoint_id: row.try_get("endpoint_id")?,
        conversation_id: row.try_get("conversation_id")?,
        external_user_id: row.try_get("external_user_id")?,
        external_conversation_id: row.try_get("external_conversation_id")?,
        external_message_id: row.try_get("external_message_id")?,
        trace_id: row.try_get("trace_id")?,
        app_type: row.try_get("app_type")?,
        model: row.try_get("model")?,
        status: row.try_get("status")?,
        status_code: row.try_get("status_code")?,
        latency_ms: row.try_get("latency_ms")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        error_summary: row.try_get("error_summary")?,
        created_at: row.try_get("created_at")?,
    })
}

pub(crate) async fn upsert_endpoint_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_id: DbId,
    app_type: &str,
    req: UpsertEndpointRequest,
) -> AppResult<()> {
    let endpoint_type = app_type;
    let existing = sqlx::query(
        "SELECT secret_ciphertext FROM app_endpoint WHERE app_id = $1 AND endpoint_type = $2",
    )
    .bind(app_id)
    .bind(endpoint_type)
    .fetch_optional(&mut **tx)
    .await?;
    let mut secrets = existing
        .as_ref()
        .map(|row| row.try_get::<Value, _>("secret_ciphertext"))
        .transpose()?
        .unwrap_or_else(|| json!({}));
    if let Some(next) = req.secrets {
        let object = secrets
            .as_object_mut()
            .ok_or_else(|| AppError::BadRequest("invalid stored secrets".to_string()))?;
        for (key, value) in next {
            let value = value.trim();
            if !value.is_empty() {
                object.insert(key, Value::String(state.secrets.encrypt(value)?));
            }
        }
    }
    let name = req.name.unwrap_or_else(|| endpoint_type.to_string());
    let enabled = req.enabled.unwrap_or(true);
    let config = req.config.unwrap_or_else(|| json!({}));

    sqlx::query(
        r#"
        INSERT INTO app_endpoint
            (app_id, endpoint_type, name, enabled, config, secret_ciphertext)
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (app_id, endpoint_type)
        DO UPDATE SET name = EXCLUDED.name,
                      enabled = EXCLUDED.enabled,
                      config = EXCLUDED.config,
                      secret_ciphertext = EXCLUDED.secret_ciphertext,
                      updated_at = now()
        "#,
    )
    .bind(app_id)
    .bind(endpoint_type)
    .bind(name)
    .bind(enabled)
    .bind(config)
    .bind(secrets)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn runtime_for_app(state: &AppState, app_id: DbId) -> AppResult<AppRuntime> {
    runtime_query(state, Some(app_id), None, None).await
}

async fn runtime_for_endpoint(
    state: &AppState,
    endpoint_id: DbId,
    endpoint_type: &str,
) -> AppResult<AppRuntime> {
    runtime_query(state, None, Some(endpoint_id), Some(endpoint_type)).await
}

async fn runtime_query(
    state: &AppState,
    app_id: Option<DbId>,
    endpoint_id: Option<DbId>,
    endpoint_type: Option<&str>,
) -> AppResult<AppRuntime> {
    let row = sqlx::query(
        r#"
        SELECT a.id AS app_id, e.id AS endpoint_id, a.app_type, a.name, a.status,
               a.model, a.system_prompt, a.context_turns, a.max_output_tokens, a.user_key_id,
               e.endpoint_type, e.enabled AS endpoint_enabled, e.config AS endpoint_config,
               e.secret_ciphertext AS endpoint_secrets
        FROM app a
        JOIN app_endpoint e ON e.app_id = a.id
        WHERE ($1::BIGINT IS NULL OR a.id = $1)
          AND ($2::BIGINT IS NULL OR e.id = $2)
          AND ($3::TEXT IS NULL OR e.endpoint_type = $3)
        ORDER BY e.created_at ASC
        LIMIT 1
        "#,
    )
    .bind(app_id)
    .bind(endpoint_id)
    .bind(endpoint_type)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(AppRuntime {
        app_id: row.try_get("app_id")?,
        endpoint_id: row.try_get("endpoint_id")?,
        app_type: row.try_get("app_type")?,
        name: row.try_get("name")?,
        status: row.try_get("status")?,
        model: row.try_get("model")?,
        system_prompt: row.try_get("system_prompt")?,
        context_turns: row.try_get("context_turns")?,
        max_output_tokens: row.try_get("max_output_tokens")?,
        user_key_id: row.try_get("user_key_id")?,
        endpoint_type: row.try_get("endpoint_type")?,
        endpoint_enabled: row.try_get("endpoint_enabled")?,
        endpoint_config: row.try_get("endpoint_config")?,
        endpoint_secrets: row.try_get("endpoint_secrets")?,
    })
}

pub(crate) async fn ensure_user_key_exists(state: &AppState, user_key_id: DbId) -> AppResult<()> {
    let exists = sqlx::query("SELECT id FROM user_key WHERE id = $1")
        .bind(user_key_id)
        .fetch_optional(&state.db.pool)
        .await?
        .is_some();
    exists
        .then_some(())
        .ok_or(AppError::BadRequest("invalid API key".to_string()))
}

fn secret_plaintext(state: &AppState, runtime: &AppRuntime, key: &str) -> AppResult<String> {
    let ciphertext = runtime
        .endpoint_secrets
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("");
    if ciphertext.is_empty() {
        return Ok(String::new());
    }
    Ok(state.secrets.plaintext(runtime.endpoint_id, ciphertext)?)
}

pub(crate) fn required_trimmed(value: String, message: &str) -> AppResult<String> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(AppError::BadRequest(message.to_string()));
    }
    Ok(value)
}

pub(crate) fn validate_app_type(value: &str) -> AppResult<()> {
    matches!(
        value,
        "wecom" | "webhook" | "widget" | "feishu" | "dingtalk"
    )
    .then_some(())
    .ok_or_else(|| AppError::BadRequest("invalid app type".to_string()))
}

pub(crate) fn ensure_supported_app_type(value: &str) -> AppResult<()> {
    matches!(value, "wecom" | "webhook" | "widget")
        .then_some(())
        .ok_or_else(|| AppError::BadRequest("this app type is coming soon".to_string()))
}

pub(crate) fn normalize_status(value: &str) -> AppResult<String> {
    matches!(value, "enabled" | "disabled")
        .then(|| value.to_string())
        .ok_or_else(|| AppError::BadRequest("invalid app status".to_string()))
}

fn public_url(state: &AppState, path: &str) -> Option<String> {
    state
        .config
        .public_base_url
        .as_ref()
        .map(|base| format!("{}{}", base.trim_end_matches('/'), path))
}

fn hmac_sha256_hex(key: &[u8], message: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(message);
    hex::encode(mac.finalize().into_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

fn extract_xml_value(bytes: &[u8], tag: &str) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start_tag = format!("<{tag}>");
    let cdata_start_tag = format!("<{tag}><![CDATA[");
    if let Some(start) = text.find(&cdata_start_tag) {
        let value_start = start + cdata_start_tag.len();
        let value_end = text[value_start..].find("]]>")? + value_start;
        return Some(text[value_start..value_end].to_string());
    }
    let start = text.find(&start_tag)? + start_tag.len();
    let end_tag = format!("</{tag}>");
    let end = text[start..].find(&end_tag)? + start;
    Some(text[start..end].to_string())
}
