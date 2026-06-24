use std::{collections::HashMap, str::FromStr};

use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{AssertSqlSafe, Postgres, Row, Transaction};

use crate::{
    auth::key_prefix,
    billing::{BILLABLE_PROVIDER_PRICE_CONDITION, BILLABLE_PROVIDER_PRICE_CONDITION_PP},
    error::{AppError, AppResult},
    id::DbId,
    input::trimmed_non_empty,
    provider::adapters::{adapter_for_provider, RelayRoute},
    AppState,
};

use super::diagnostics::{recent_probe_samples_by_channel, ChannelProbeSampleRecord};
use super::provider::{
    ensure_custom_provider, ensure_newapi_provider, ensure_sub2api_provider,
    provider_default_endpoint_base_url, provider_default_endpoints, provider_default_models,
    record_provider_models, CUSTOM_PROVIDER_CODE, NEWAPI_PROVIDER_CODE, OPENAI_OAUTH_PROTOCOL,
    SUB2API_PROVIDER_CODE,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeySelectionMode {
    Polling,
    Random,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponsesCapability {
    Unknown,
    Native,
    ChatFallback,
    Disabled,
}

impl ResponsesCapability {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Native => "native",
            Self::ChatFallback => "chat_fallback",
            Self::Disabled => "disabled",
        }
    }
}

impl FromStr for ResponsesCapability {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unknown" => Ok(Self::Unknown),
            "native" => Ok(Self::Native),
            "chat_fallback" => Ok(Self::ChatFallback),
            "disabled" => Ok(Self::Disabled),
            other => Err(AppError::BadRequest(format!(
                "invalid responses_capability: {other}"
            ))),
        }
    }
}

impl KeySelectionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Polling => "polling",
            Self::Random => "random",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelRecord {
    pub id: DbId,
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i32,
    pub weight: i32,
    pub key_selection_mode: String,
    pub use_credentials: bool,
    pub endpoints: Vec<ChannelEndpointRecord>,
    pub models: Vec<ChannelModelRecord>,
    pub probe_samples: Vec<ChannelProbeSampleRecord>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelEndpointRecord {
    pub id: DbId,
    pub channel_id: DbId,
    pub protocol: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub responses_capability: String,
    pub responses_checked_at: Option<DateTime<Utc>>,
    pub responses_probe: Value,
    pub enabled: bool,
    pub healthy: bool,
    pub last_error: Option<String>,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelModelRecord {
    pub id: DbId,
    pub channel_id: DbId,
    pub provider: String,
    pub model: String,
    pub enabled: bool,
    pub status: String,
    pub runtime_status: String,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub missing_since: Option<DateTime<Utc>>,
    pub last_probe_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_status_code: Option<i32>,
    pub success_count: i64,
    pub failure_count: i64,
    pub billing_enabled: bool,
    pub price_configured: bool,
    pub input_price_usd_micros: Option<i64>,
    pub output_price_usd_micros: Option<i64>,
    pub cache_read_price_usd_micros: Option<i64>,
    pub cache_write_price_usd_micros: Option<i64>,
    pub billing_meter: Option<String>,
    pub unit_price_usd_micros: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub provider: String,
    pub name: String,
    #[serde(default)]
    pub endpoints: Vec<ChannelEndpointInput>,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_weight")]
    pub weight: i32,
    #[serde(default = "default_key_selection")]
    pub key_selection_mode: KeySelectionMode,
    #[serde(default)]
    pub use_credentials: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub endpoints: Option<Vec<ChannelEndpointInput>>,
    pub protocol: Option<String>,
    pub base_url: Option<String>,
    pub models: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub priority: Option<i32>,
    pub weight: Option<i32>,
    pub key_selection_mode: Option<KeySelectionMode>,
    pub use_credentials: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChannelEndpointInput {
    pub protocol: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone)]
struct NormalizedEndpoint {
    protocol: String,
    base_url: String,
    models: Vec<String>,
    responses_capability: ResponsesCapability,
    responses_checked_at: Option<DateTime<Utc>>,
    responses_probe: Value,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelKeyRecord {
    pub id: DbId,
    pub channel_id: DbId,
    pub name: String,
    pub key_prefix: String,
    pub enabled: bool,
    pub healthy: bool,
    pub cooldown_until: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelKeyRequest {
    pub name: String,
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelKeyRequest {
    pub name: Option<String>,
    pub secret: Option<String>,
    pub enabled: Option<bool>,
    pub healthy: Option<bool>,
    pub cooldown_until: Option<Option<DateTime<Utc>>>,
    pub last_error: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelModelRequest {
    pub enabled: Option<bool>,
}

fn default_true() -> bool {
    true
}

fn default_weight() -> i32 {
    1
}

fn default_key_selection() -> KeySelectionMode {
    KeySelectionMode::Polling
}

pub async fn create_channel(
    state: &AppState,
    req: CreateChannelRequest,
) -> AppResult<ChannelRecord> {
    validate_weight(req.weight)?;
    let provider_code = req.provider.trim().to_string();
    if provider_code.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    if provider_code == CUSTOM_PROVIDER_CODE {
        ensure_custom_provider(state).await?;
    }
    if provider_code == NEWAPI_PROVIDER_CODE {
        ensure_newapi_provider(state).await?;
    }
    if provider_code == SUB2API_PROVIDER_CODE {
        ensure_sub2api_provider(state).await?;
    }
    ensure_provider_exists(state, &provider_code).await?;
    let endpoints = normalize_create_endpoints(state, &provider_code, &req).await?;
    let endpoint_models = models_from_endpoints(&endpoints);
    record_provider_models(state, &provider_code, &endpoint_models, "channel", true).await?;

    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "INSERT INTO channel
         (provider, name, enabled, priority, weight, key_selection_mode, use_credentials)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, provider, name, enabled, priority, weight,
                   key_selection_mode, use_credentials, created_at, updated_at",
    )
    .bind(&provider_code)
    .bind(req.name)
    .bind(req.enabled)
    .bind(req.priority)
    .bind(req.weight)
    .bind(req.key_selection_mode.as_str())
    .bind(req.use_credentials)
    .fetch_one(&mut *tx)
    .await?;
    let channel_id: DbId = row.try_get("id")?;
    for endpoint in endpoints {
        insert_endpoint(&mut tx, channel_id, endpoint).await?;
    }
    sync_channel_models_for_channel(&mut tx, channel_id, &provider_code).await?;
    tx.commit().await?;

    probe_auto_responses_capabilities(state, channel_id).await?;

    get_channel(state, channel_id).await
}

pub async fn list_channels(state: &AppState) -> AppResult<Vec<ChannelRecord>> {
    let rows = sqlx::query(
        "SELECT id, provider, name, enabled, priority, weight,
                key_selection_mode, use_credentials, created_at, updated_at
         FROM channel ORDER BY priority DESC, created_at DESC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    let channel_ids: Vec<DbId> = rows
        .iter()
        .map(|row| row.try_get("id"))
        .collect::<Result<_, _>>()?;
    let endpoints = endpoints_by_channel(state, &channel_ids).await?;
    let models = models_by_channel(state, &channel_ids).await?;
    let probe_samples = recent_probe_samples_by_channel(state, &channel_ids, 12).await?;

    rows.iter()
        .map(|row| {
            let id: DbId = row.try_get("id")?;
            channel_from_row(
                row,
                endpoints.get(&id).cloned().unwrap_or_default(),
                models.get(&id).cloned().unwrap_or_default(),
                probe_samples.get(&id).cloned().unwrap_or_default(),
            )
        })
        .collect()
}

pub async fn update_channel(
    state: &AppState,
    id: DbId,
    req: UpdateChannelRequest,
) -> AppResult<ChannelRecord> {
    if let Some(weight) = req.weight {
        validate_weight(weight)?;
    }
    let current = sqlx::query(
        "SELECT id, provider, name, enabled, priority, weight,
                key_selection_mode, use_credentials, created_at, updated_at
         FROM channel WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let provider_code: String = current.try_get("provider")?;
    let current_use_credentials: bool = current.try_get("use_credentials")?;
    let next_use_credentials = req.use_credentials.unwrap_or(current_use_credentials);
    let endpoints =
        normalize_update_endpoints(state, &provider_code, next_use_credentials, &req).await?;
    let endpoint_models = endpoints
        .as_ref()
        .map(|endpoints| models_from_endpoints(endpoints));
    if let Some(endpoint_models) = &endpoint_models {
        record_provider_models(state, &provider_code, endpoint_models, "channel", true).await?;
    }
    let mode = req.key_selection_mode.map(|mode| mode.as_str().to_string());

    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "UPDATE channel
         SET name = COALESCE($2, name),
             enabled = COALESCE($3, enabled),
             priority = COALESCE($4, priority),
             weight = COALESCE($5, weight),
             key_selection_mode = COALESCE($6, key_selection_mode),
             use_credentials = COALESCE($7, use_credentials),
             updated_at = now()
         WHERE id = $1
         RETURNING id, provider, name, enabled, priority, weight,
                   key_selection_mode, use_credentials, created_at, updated_at",
    )
    .bind(id)
    .bind(req.name)
    .bind(req.enabled)
    .bind(req.priority)
    .bind(req.weight)
    .bind(mode)
    .bind(req.use_credentials)
    .fetch_one(&mut *tx)
    .await?;

    if let Some(endpoints) = endpoints {
        let protocols: Vec<String> = endpoints
            .iter()
            .map(|endpoint| endpoint.protocol.clone())
            .collect();
        sqlx::query(
            "DELETE FROM channel_endpoint WHERE channel_id = $1 AND NOT (protocol = ANY($2))",
        )
        .bind(id)
        .bind(protocols)
        .execute(&mut *tx)
        .await?;

        for endpoint in endpoints {
            upsert_endpoint(&mut tx, id, endpoint).await?;
        }
        sync_channel_models_for_channel(&mut tx, id, &provider_code).await?;
    }
    tx.commit().await?;

    probe_auto_responses_capabilities(state, id).await?;

    let endpoints = endpoints_by_channel(state, &[id]).await?;
    let models = models_by_channel(state, &[id]).await?;
    let probe_samples = recent_probe_samples_by_channel(state, &[id], 12).await?;
    channel_from_row(
        &row,
        endpoints.get(&id).cloned().unwrap_or_default(),
        models.get(&id).cloned().unwrap_or_default(),
        probe_samples.get(&id).cloned().unwrap_or_default(),
    )
}

pub async fn delete_channel(state: &AppState, id: DbId) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM channel WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn update_channel_model(
    state: &AppState,
    channel_id: DbId,
    model: &str,
    req: UpdateChannelModelRequest,
) -> AppResult<ChannelModelRecord> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }

    if req.enabled == Some(true) {
        ensure_channel_model_has_enabled_price(state, channel_id, model).await?;
    }

    let row = sqlx::query(
        "UPDATE channel_model
         SET enabled = COALESCE($3, enabled),
             status = CASE
                 WHEN COALESCE($3, enabled) = FALSE THEN 'disabled'
                 WHEN status = 'disabled' THEN 'available'
                 ELSE status
             END,
             updated_at = now()
         WHERE channel_id = $1
           AND model = $2
         RETURNING id",
    )
    .bind(channel_id)
    .bind(model)
    .bind(req.enabled)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let model_id: DbId = row.try_get("id")?;
    let models = models_by_channel(state, &[channel_id]).await?;
    models
        .get(&channel_id)
        .and_then(|items| items.iter().find(|item| item.id == model_id).cloned())
        .ok_or(AppError::NotFound)
}

pub async fn create_channel_key(
    state: &AppState,
    channel_id: DbId,
    req: CreateChannelKeyRequest,
) -> AppResult<ChannelKeyRecord> {
    let secret_ciphertext = state.secrets.encrypt(&req.secret)?;
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "INSERT INTO channel_key (channel_id, name, key_prefix, secret_ciphertext, enabled)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, channel_id, name, key_prefix, enabled, healthy, cooldown_until,
                   last_error, last_used_at, created_at, updated_at",
    )
    .bind(channel_id)
    .bind(req.name)
    .bind(key_prefix(&req.secret))
    .bind(secret_ciphertext)
    .bind(req.enabled)
    .fetch_one(&mut *tx)
    .await?;
    reset_channel_endpoint_health(&mut tx, channel_id).await?;
    tx.commit().await?;
    if req.enabled {
        probe_auto_responses_capabilities(state, channel_id).await?;
    }
    channel_key_from_row(&row)
}

pub async fn list_channel_keys(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<Vec<ChannelKeyRecord>> {
    let rows = sqlx::query(
        "SELECT id, channel_id, name, key_prefix, enabled, healthy, cooldown_until,
                last_error, last_used_at, created_at, updated_at
         FROM channel_key WHERE channel_id = $1 ORDER BY created_at ASC",
    )
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(channel_key_from_row).collect()
}

pub async fn list_all_channel_keys(state: &AppState) -> AppResult<Vec<ChannelKeyRecord>> {
    let rows = sqlx::query(
        "SELECT id, channel_id, name, key_prefix, enabled, healthy, cooldown_until,
                last_error, last_used_at, created_at, updated_at
         FROM channel_key ORDER BY channel_id ASC, created_at ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(channel_key_from_row).collect()
}

pub async fn reveal_channel_key_secret(
    state: &AppState,
    channel_id: DbId,
    key_id: DbId,
) -> AppResult<String> {
    let row = sqlx::query(
        "SELECT secret_ciphertext
         FROM channel_key
         WHERE id = $1 AND channel_id = $2",
    )
    .bind(key_id)
    .bind(channel_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    state
        .secrets
        .plaintext(key_id, &secret_ciphertext)
        .map_err(Into::into)
}

pub async fn update_channel_key(
    state: &AppState,
    id: DbId,
    req: UpdateChannelKeyRequest,
) -> AppResult<ChannelKeyRecord> {
    let current = sqlx::query(
        "SELECT id, channel_id, name, key_prefix, enabled, healthy, cooldown_until,
                last_error, last_used_at, created_at, updated_at
         FROM channel_key WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let channel_id: DbId = current.try_get("channel_id")?;
    let current_last_error: Option<String> = current.try_get("last_error")?;
    let current_cooldown_until: Option<DateTime<Utc>> = current.try_get("cooldown_until")?;
    let replacing_secret = req.secret.is_some();
    let last_error = if replacing_secret {
        None
    } else {
        req.last_error.unwrap_or(current_last_error)
    };
    let cooldown_until = if replacing_secret {
        None
    } else {
        req.cooldown_until.unwrap_or(current_cooldown_until)
    };
    let healthy = if replacing_secret {
        Some(true)
    } else {
        req.healthy
    };
    let key_prefix_value = req.secret.as_ref().map(|secret| key_prefix(secret));
    let secret_ciphertext = req
        .secret
        .as_deref()
        .map(|secret| state.secrets.encrypt(secret))
        .transpose()?;

    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "UPDATE channel_key
         SET name = COALESCE($2, name),
             secret_ciphertext = COALESCE($3, secret_ciphertext),
             key_prefix = COALESCE($4, key_prefix),
             enabled = COALESCE($5, enabled),
             healthy = COALESCE($6, healthy),
             cooldown_until = $7,
             last_error = $8,
             updated_at = now()
         WHERE id = $1
         RETURNING id, channel_id, name, key_prefix, enabled, healthy, cooldown_until,
                   last_error, last_used_at, created_at, updated_at",
    )
    .bind(id)
    .bind(req.name)
    .bind(secret_ciphertext)
    .bind(key_prefix_value)
    .bind(req.enabled)
    .bind(healthy)
    .bind(cooldown_until)
    .bind(last_error)
    .fetch_one(&mut *tx)
    .await?;
    if replacing_secret {
        reset_channel_endpoint_health(&mut tx, channel_id).await?;
    }
    tx.commit().await?;
    if replacing_secret {
        state.secrets.forget(id);
        probe_auto_responses_capabilities(state, channel_id).await?;
    }
    channel_key_from_row(&row)
}

pub async fn delete_channel_key(state: &AppState, id: DbId) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM channel_key WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    state.secrets.forget(id);
    Ok(())
}

async fn ensure_provider_exists(state: &AppState, provider_code: &str) -> AppResult<()> {
    provider_default_endpoints(state, provider_code)
        .await?
        .map(|_| ())
        .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider_code}")))?;
    Ok(())
}

async fn probe_auto_responses_capabilities(state: &AppState, channel_id: DbId) -> AppResult<()> {
    let key = first_enabled_channel_key(state, channel_id).await?;
    let rows = sqlx::query(
        "SELECT ce.id, ce.base_url, ce.models, c.provider
         FROM channel_endpoint ce
         JOIN channel c ON c.id = ce.channel_id
         WHERE ce.channel_id = $1
           AND ce.protocol = 'openai'
           AND ce.responses_capability = 'unknown'
           AND ce.enabled = TRUE
         ORDER BY ce.created_at ASC",
    )
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await?;

    let mut updated = false;
    for row in rows {
        let endpoint_id: DbId = row.try_get("id")?;
        let base_url: String = row.try_get("base_url")?;
        let models: Vec<String> = row.try_get("models")?;
        let provider: String = row.try_get("provider")?;
        let adapter = adapter_for_provider(&provider);
        if !adapter.responses_policy().should_probe() {
            let capability = adapter.responses_policy().initial_capability();
            let probe = adapter_responses_probe(adapter.name(), capability);
            update_responses_probe(state, endpoint_id, capability, probe, Some(Utc::now())).await?;
            updated = true;
            continue;
        }

        let (capability, probe, checked_at) = match first_configured_model(&models) {
            None => (
                ResponsesCapability::Unknown,
                skipped_responses_probe(None, "endpoint has no configured model"),
                None,
            ),
            Some(model) => match key.as_ref() {
                None => (
                    ResponsesCapability::Unknown,
                    skipped_responses_probe(Some(model), "channel has no enabled healthy key"),
                    None,
                ),
                Some((_, secret)) => {
                    let probe = probe_responses_capability(
                        state,
                        &provider,
                        &base_url,
                        secret,
                        model,
                        adapter.responses_policy().allow_chat_fallback(),
                    )
                    .await;
                    (mode_from_responses_probe(&probe), probe, Some(Utc::now()))
                }
            },
        };
        update_responses_probe(state, endpoint_id, capability, probe, checked_at).await?;
        updated = true;
    }

    if updated {
        state.selector.invalidate().await;
    }

    Ok(())
}

fn adapter_responses_probe(adapter: &str, capability: ResponsesCapability) -> Value {
    json!({
        "checked_at": Utc::now(),
        "source": "adapter",
        "adapter": adapter,
        "inferred_mode": capability.as_str()
    })
}

fn first_configured_model(models: &[String]) -> Option<&str> {
    models
        .iter()
        .map(String::as_str)
        .find(|model| !model.trim().is_empty())
}

fn skipped_responses_probe(model: Option<&str>, reason: &str) -> Value {
    let mut probe = json!({
        "checked_at": Utc::now(),
        "ok": false,
        "skipped": true,
        "reason": reason
    });
    if let Some(model) = model {
        probe["model"] = Value::String(model.to_string());
    }
    probe
}

fn mode_from_responses_probe(probe: &Value) -> ResponsesCapability {
    if let Some(mode) = probe
        .get("inferred_mode")
        .and_then(Value::as_str)
        .and_then(|value| ResponsesCapability::from_str(value).ok())
    {
        return mode;
    }
    if probe.get("responses").is_some_and(probe_ok) {
        ResponsesCapability::Native
    } else if probe.get("chat_completions").is_some_and(probe_ok) {
        ResponsesCapability::ChatFallback
    } else {
        ResponsesCapability::Unknown
    }
}

fn probe_ok(probe: &Value) -> bool {
    probe.get("ok").and_then(Value::as_bool).unwrap_or(false)
}

async fn first_enabled_channel_key(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<Option<(DbId, String)>> {
    let Some(row) = sqlx::query(
        "SELECT id, secret_ciphertext
         FROM channel_key
         WHERE channel_id = $1
           AND enabled = TRUE
           AND healthy = TRUE
         ORDER BY created_at ASC
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(&state.db.pool)
    .await?
    else {
        return Ok(None);
    };
    let key_id: DbId = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    let secret = state.secrets.plaintext(key_id, &secret_ciphertext)?;
    Ok(Some((key_id, secret)))
}

async fn update_responses_probe(
    state: &AppState,
    endpoint_id: DbId,
    capability: ResponsesCapability,
    probe: Value,
    checked_at: Option<DateTime<Utc>>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE channel_endpoint
         SET responses_capability = $2,
             responses_checked_at = $3,
             responses_probe = $4,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(endpoint_id)
    .bind(capability.as_str())
    .bind(checked_at)
    .bind(probe)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn probe_responses_capability(
    state: &AppState,
    provider: &str,
    base_url: &str,
    secret: &str,
    model: &str,
    allow_chat_fallback: bool,
) -> Value {
    let adapter = adapter_for_provider(provider);
    let responses_body = json!({
        "model": model,
        "input": "Reply with OK only.",
        "max_output_tokens": 8
    });
    let responses_url = adapter.resolve_url(base_url, RelayRoute::OpenAiResponses);
    let responses = probe_openai_url(
        state,
        secret,
        &responses_url,
        responses_probe_headers(provider),
        responses_body,
    )
    .await;
    if probe_ok(&responses) {
        return json!({
            "checked_at": Utc::now(),
            "adapter": adapter.name(),
            "model": model,
            "responses": responses,
            "inferred_mode": ResponsesCapability::Native.as_str()
        });
    }

    let chat_body = json!({
        "model": model,
        "messages": [{ "role": "user", "content": "Reply with OK only." }],
        "max_tokens": 8
    });
    let chat_url = adapter.resolve_url(base_url, RelayRoute::OpenAiChatCompletions);
    let chat = probe_openai_url(state, secret, &chat_url, HeaderMap::new(), chat_body).await;
    let inferred = if probe_ok(&chat) && allow_chat_fallback {
        ResponsesCapability::ChatFallback
    } else {
        ResponsesCapability::Unknown
    };

    json!({
        "checked_at": Utc::now(),
        "adapter": adapter.name(),
        "model": model,
        "responses": responses,
        "chat_completions": chat,
        "inferred_mode": inferred.as_str()
    })
}

fn responses_probe_headers(provider: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    if provider.eq_ignore_ascii_case("qwen") {
        headers.insert("x-dashscope-sse", "enable".parse().expect("valid header"));
    }
    if provider.eq_ignore_ascii_case("jdcloud") {
        headers.insert("accept", "text/event-stream".parse().expect("valid header"));
    }
    headers
}

async fn probe_openai_url(
    state: &AppState,
    secret: &str,
    url: &str,
    headers: HeaderMap,
    body: Value,
) -> Value {
    let mut request = state
        .http
        .post(url)
        .bearer_auth(secret)
        .header("content-type", "application/json")
        .json(&body);
    for (name, value) in &headers {
        request = request.header(name, value.clone());
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let ok = response.status().is_success();
            let body = response
                .text()
                .await
                .unwrap_or_else(|err| format!("failed to read response body: {err}"));
            json!({
                "ok": ok,
                "url": url,
                "status": status,
                "error": if ok { Value::Null } else { Value::String(truncate_probe_error(&body)) }
            })
        }
        Err(err) => json!({
            "ok": false,
            "url": url,
            "status": null,
            "error": truncate_probe_error(&err.to_string())
        }),
    }
}

fn truncate_probe_error(value: &str) -> String {
    const LIMIT: usize = 500;
    let value = value.trim();
    if value.len() <= LIMIT {
        value.to_string()
    } else {
        let end = value
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= LIMIT)
            .last()
            .unwrap_or(0);
        format!("{}...", &value[..end])
    }
}

async fn normalize_create_endpoints(
    state: &AppState,
    provider_code: &str,
    req: &CreateChannelRequest,
) -> AppResult<Vec<NormalizedEndpoint>> {
    if !req.endpoints.is_empty() {
        let oauth_base_url =
            provider_default_endpoint_base_url(state, provider_code, OPENAI_OAUTH_PROTOCOL).await?;
        let inputs = credential_endpoint_inputs(
            provider_code,
            req.use_credentials,
            oauth_base_url.as_deref(),
            req.endpoints.clone(),
        );
        return normalize_endpoint_inputs(provider_code, inputs.iter());
    }

    if req.base_url.is_none()
        && req.protocol.is_none()
        && req.models.is_empty()
        && !(provider_code == "openai" && req.use_credentials)
    {
        if let Some(defaults) = provider_default_endpoints(state, provider_code).await? {
            let default_models = provider_default_models(state, provider_code)
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest(format!("invalid provider: {provider_code}"))
                })?;
            let inputs: Vec<ChannelEndpointInput> = defaults
                .into_iter()
                .filter(|endpoint| !endpoint.base_url.trim().is_empty())
                .map(|endpoint| ChannelEndpointInput {
                    protocol: endpoint.protocol,
                    base_url: Some(endpoint.base_url),
                    models: default_models.clone(),
                    enabled: true,
                })
                .collect();
            if !inputs.is_empty() {
                return normalize_endpoint_inputs(provider_code, inputs.iter());
            }
        }
    }

    let protocol = default_protocol_for_request(
        state,
        provider_code,
        req.use_credentials,
        req.protocol.as_deref(),
    )
    .await?;
    let default_endpoint_base_url =
        provider_default_endpoint_base_url(state, provider_code, &protocol)
            .await?
            .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider_code}")))?;
    let base_url = trimmed_non_empty(req.base_url.as_deref())
        .unwrap_or(default_endpoint_base_url.trim())
        .to_string();

    normalize_endpoint_inputs(
        provider_code,
        [ChannelEndpointInput {
            protocol,
            base_url: Some(base_url),
            models: req.models.clone(),
            enabled: true,
        }]
        .iter(),
    )
}

async fn normalize_update_endpoints(
    state: &AppState,
    provider_code: &str,
    use_credentials: bool,
    req: &UpdateChannelRequest,
) -> AppResult<Option<Vec<NormalizedEndpoint>>> {
    if let Some(endpoints) = &req.endpoints {
        let oauth_base_url =
            provider_default_endpoint_base_url(state, provider_code, OPENAI_OAUTH_PROTOCOL).await?;
        let inputs = credential_endpoint_inputs(
            provider_code,
            use_credentials,
            oauth_base_url.as_deref(),
            endpoints.clone(),
        );
        return Ok(Some(normalize_endpoint_inputs(
            provider_code,
            inputs.iter(),
        )?));
    }

    if req.base_url.is_none() && req.models.is_none() && req.protocol.is_none() {
        return Ok(None);
    }

    let protocol = default_protocol_for_request(
        state,
        provider_code,
        use_credentials,
        req.protocol.as_deref(),
    )
    .await?;
    let default_endpoint_base_url =
        provider_default_endpoint_base_url(state, provider_code, &protocol)
            .await?
            .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider_code}")))?;
    let base_url = trimmed_non_empty(req.base_url.as_deref())
        .unwrap_or(default_endpoint_base_url.trim())
        .to_string();

    Ok(Some(normalize_endpoint_inputs(
        provider_code,
        [ChannelEndpointInput {
            protocol,
            base_url: Some(base_url),
            models: req.models.clone().unwrap_or_default(),
            enabled: true,
        }]
        .iter(),
    )?))
}

async fn default_protocol_for_request(
    state: &AppState,
    provider_code: &str,
    use_credentials: bool,
    requested_protocol: Option<&str>,
) -> AppResult<String> {
    if let Some(protocol) = trimmed_non_empty(requested_protocol)
        .map(validate_protocol)
        .transpose()?
    {
        return Ok(
            rewrite_credential_protocol(provider_code, use_credentials, &protocol).to_string(),
        );
    }

    if provider_code == "openai" && use_credentials {
        return Ok(OPENAI_OAUTH_PROTOCOL.to_string());
    }

    let Some(defaults) = provider_default_endpoints(state, provider_code).await? else {
        return Err(AppError::BadRequest(format!(
            "invalid provider: {provider_code}"
        )));
    };
    defaults
        .into_iter()
        .find(|endpoint| !endpoint.base_url.trim().is_empty())
        .map(|endpoint| endpoint.protocol)
        .ok_or_else(|| AppError::BadRequest("provider has no default endpoint".to_string()))
}

fn normalize_endpoint_inputs<'a>(
    provider_code: &str,
    inputs: impl IntoIterator<Item = &'a ChannelEndpointInput>,
) -> AppResult<Vec<NormalizedEndpoint>> {
    let mut endpoints = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for input in inputs {
        let protocol = validate_protocol(&input.protocol)?;
        if !seen.insert(protocol.clone()) {
            return Err(AppError::BadRequest(format!(
                "duplicate endpoint protocol: {protocol}"
            )));
        }
        let base_url = trimmed_non_empty(input.base_url.as_deref())
            .ok_or_else(|| AppError::BadRequest("base_url is required".to_string()))?
            .to_string();
        let (responses_capability, responses_checked_at, responses_probe) =
            default_responses_capability(provider_code, &protocol);
        endpoints.push(NormalizedEndpoint {
            protocol,
            base_url,
            models: input.models.clone(),
            responses_capability,
            responses_checked_at,
            responses_probe,
            enabled: input.enabled,
        });
    }

    if endpoints.is_empty() {
        return Err(AppError::BadRequest(
            "at least one endpoint is required".to_string(),
        ));
    }
    Ok(endpoints)
}

fn models_from_endpoints(endpoints: &[NormalizedEndpoint]) -> Vec<String> {
    dedupe_models(
        endpoints
            .iter()
            .flat_map(|endpoint| endpoint.models.iter().map(String::as_str)),
    )
}

fn dedupe_models<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut normalized = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for value in values {
        let model = value.trim();
        if !model.is_empty() && seen.insert(model.to_string()) {
            normalized.push(model.to_string());
        }
    }
    normalized
}

fn validate_protocol(protocol: &str) -> AppResult<String> {
    let protocol = protocol.trim();
    match protocol {
        "openai" | "anthropic" | OPENAI_OAUTH_PROTOCOL => Ok(protocol.to_string()),
        other => Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
    }
}

fn default_responses_capability(
    provider_code: &str,
    protocol: &str,
) -> (ResponsesCapability, Option<DateTime<Utc>>, Value) {
    if protocol != "openai" {
        return (
            ResponsesCapability::Unknown,
            None,
            json!({ "source": "not_applicable" }),
        );
    }

    let adapter = adapter_for_provider(provider_code);
    let policy = adapter.responses_policy();
    let capability = policy.initial_capability();
    let source = if policy.should_probe() {
        "probe_pending"
    } else {
        "adapter"
    };
    let checked_at = (!policy.should_probe()).then(Utc::now);
    (
        capability,
        checked_at,
        json!({
            "checked_at": checked_at,
            "source": source,
            "adapter": adapter.name(),
            "inferred_mode": capability.as_str()
        }),
    )
}

fn credential_endpoint_inputs(
    provider_code: &str,
    use_credentials: bool,
    oauth_base_url: Option<&str>,
    inputs: Vec<ChannelEndpointInput>,
) -> Vec<ChannelEndpointInput> {
    if provider_code != "openai" || !use_credentials {
        return inputs;
    }

    let models = dedupe_models(
        inputs
            .iter()
            .flat_map(|input| input.models.iter().map(String::as_str)),
    );

    let base_url = inputs
        .iter()
        .find(|input| input.protocol.trim() == OPENAI_OAUTH_PROTOCOL)
        .and_then(|input| input.base_url.clone())
        .or_else(|| oauth_base_url.map(str::to_string));

    vec![ChannelEndpointInput {
        protocol: OPENAI_OAUTH_PROTOCOL.to_string(),
        base_url,
        models,
        enabled: inputs.iter().any(|input| input.enabled),
    }]
}

fn rewrite_credential_protocol<'a>(
    provider_code: &str,
    use_credentials: bool,
    protocol: &'a str,
) -> &'a str {
    if provider_code == "openai" && use_credentials && protocol == "openai" {
        OPENAI_OAUTH_PROTOCOL
    } else {
        protocol
    }
}

async fn insert_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: DbId,
    endpoint: NormalizedEndpoint,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO channel_endpoint
         (channel_id, protocol, base_url, models, responses_capability, responses_checked_at,
          responses_probe, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(channel_id)
    .bind(endpoint.protocol)
    .bind(endpoint.base_url)
    .bind(endpoint.models)
    .bind(endpoint.responses_capability.as_str())
    .bind(endpoint.responses_checked_at)
    .bind(endpoint.responses_probe)
    .bind(endpoint.enabled)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn upsert_endpoint(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: DbId,
    endpoint: NormalizedEndpoint,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO channel_endpoint
         (channel_id, protocol, base_url, models, responses_capability, responses_checked_at,
          responses_probe, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         ON CONFLICT (channel_id, protocol) DO UPDATE
         SET base_url = EXCLUDED.base_url,
             models = EXCLUDED.models,
             responses_capability = EXCLUDED.responses_capability,
             responses_checked_at = EXCLUDED.responses_checked_at,
             responses_probe = EXCLUDED.responses_probe,
             enabled = EXCLUDED.enabled,
             updated_at = now()",
    )
    .bind(channel_id)
    .bind(endpoint.protocol)
    .bind(endpoint.base_url)
    .bind(endpoint.models)
    .bind(endpoint.responses_capability.as_str())
    .bind(endpoint.responses_checked_at)
    .bind(endpoint.responses_probe)
    .bind(endpoint.enabled)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn sync_channel_models_for_channel(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: DbId,
    provider: &str,
) -> AppResult<()> {
    let rows = sqlx::query("SELECT models FROM channel_endpoint WHERE channel_id = $1")
        .bind(channel_id)
        .fetch_all(&mut **tx)
        .await?;

    let mut active_models = Vec::<String>::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let models: Vec<String> = row.try_get("models")?;
        for model in models {
            let model = model.trim();
            if model.is_empty() || !seen.insert(model.to_string()) {
                continue;
            }
            let price_configured = model_has_enabled_price(tx, provider, model).await?;
            active_models.push(model.to_string());
            sqlx::query(
                "INSERT INTO channel_model
                 (channel_id, provider, model, enabled, status, runtime_status, last_seen_at)
                 VALUES ($1, $2, $3, $4, 'available', 'normal', now())
                 ON CONFLICT (channel_id, model)
                 DO UPDATE SET
                     provider = EXCLUDED.provider,
                     enabled = EXCLUDED.enabled,
                     status = 'available',
                     missing_since = NULL,
                     last_seen_at = COALESCE(channel_model.last_seen_at, now()),
                     updated_at = now()",
            )
            .bind(channel_id)
            .bind(provider)
            .bind(model)
            .bind(price_configured)
            .execute(&mut **tx)
            .await?;
        }
    }

    if !active_models.is_empty() {
        sqlx::query(
            "DELETE FROM channel_model
             WHERE channel_id = $1
               AND NOT (model = ANY($2))",
        )
        .bind(channel_id)
        .bind(&active_models)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query("DELETE FROM channel_model WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&mut **tx)
            .await?;
    }

    Ok(())
}

async fn model_has_enabled_price(
    tx: &mut Transaction<'_, Postgres>,
    provider: &str,
    model: &str,
) -> AppResult<bool> {
    let exists: Option<i32> = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT 1
         FROM provider_price
         WHERE provider = $1
           AND model = $2
           AND enabled = TRUE
           AND {BILLABLE_PROVIDER_PRICE_CONDITION}
         LIMIT 1"
    )))
    .bind(provider)
    .bind(model)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(exists.is_some())
}

async fn reset_channel_endpoint_health(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: DbId,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE channel_endpoint
         SET healthy = TRUE,
             last_error = NULL,
             cooldown_until = NULL,
             updated_at = now()
         WHERE channel_id = $1",
    )
    .bind(channel_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn get_channel(state: &AppState, id: DbId) -> AppResult<ChannelRecord> {
    let row = sqlx::query(
        "SELECT id, provider, name, enabled, priority, weight,
                key_selection_mode, use_credentials, created_at, updated_at
         FROM channel WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let endpoints = endpoints_by_channel(state, &[id]).await?;
    let models = models_by_channel(state, &[id]).await?;
    let probe_samples = recent_probe_samples_by_channel(state, &[id], 12).await?;
    channel_from_row(
        &row,
        endpoints.get(&id).cloned().unwrap_or_default(),
        models.get(&id).cloned().unwrap_or_default(),
        probe_samples.get(&id).cloned().unwrap_or_default(),
    )
}

async fn endpoints_by_channel(
    state: &AppState,
    channel_ids: &[DbId],
) -> AppResult<HashMap<DbId, Vec<ChannelEndpointRecord>>> {
    if channel_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(
        "SELECT id, channel_id, protocol, base_url, models, responses_capability,
                responses_checked_at, responses_probe,
                enabled, healthy,
                last_error, cooldown_until, created_at, updated_at
         FROM channel_endpoint ce
         WHERE ce.channel_id = ANY($1)
         ORDER BY ce.channel_id ASC,
                  CASE ce.protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END,
                  ce.created_at ASC",
    )
    .bind(channel_ids)
    .fetch_all(&state.db.pool)
    .await?;

    let mut endpoints: HashMap<DbId, Vec<ChannelEndpointRecord>> = HashMap::new();
    for row in rows {
        let endpoint = endpoint_from_row(&row)?;
        endpoints
            .entry(endpoint.channel_id)
            .or_default()
            .push(endpoint);
    }
    Ok(endpoints)
}

async fn models_by_channel(
    state: &AppState,
    channel_ids: &[DbId],
) -> AppResult<HashMap<DbId, Vec<ChannelModelRecord>>> {
    if channel_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query(AssertSqlSafe(format!(
        "SELECT cm.id, cm.channel_id, cm.provider, cm.model, cm.enabled,
                cm.status, cm.runtime_status, cm.cooldown_until, cm.last_seen_at,
                cm.missing_since, cm.last_probe_at, cm.last_error, cm.last_status_code,
                cm.success_count, cm.failure_count, cm.created_at, cm.updated_at,
                COALESCE(
                    pp.enabled
                    AND {BILLABLE_PROVIDER_PRICE_CONDITION_PP},
                    FALSE
                ) AS billing_enabled,
                (pp.id IS NOT NULL) AS price_configured,
                pp.input_price_usd_micros, pp.output_price_usd_micros,
                pp.cache_read_price_usd_micros, pp.cache_write_price_usd_micros,
                pp.billing_meter,
                pp.unit_price_usd_micros
         FROM channel_model cm
         LEFT JOIN provider_price pp
           ON pp.provider = cm.provider
          AND pp.model = cm.model
         WHERE cm.channel_id = ANY($1)
         ORDER BY cm.model ASC"
    )))
    .bind(channel_ids)
    .fetch_all(&state.db.pool)
    .await?;

    let mut models: HashMap<DbId, Vec<ChannelModelRecord>> = HashMap::new();
    for row in rows {
        let model = channel_model_from_row(&row)?;
        models.entry(model.channel_id).or_default().push(model);
    }
    Ok(models)
}

async fn ensure_channel_model_has_enabled_price(
    state: &AppState,
    channel_id: DbId,
    model: &str,
) -> AppResult<()> {
    let row = sqlx::query(AssertSqlSafe(format!(
        "SELECT 1
         FROM channel_model cm
         JOIN provider_price pp
          ON pp.provider = cm.provider
         AND pp.model = cm.model
         AND pp.enabled = TRUE
         AND {BILLABLE_PROVIDER_PRICE_CONDITION_PP}
         WHERE cm.channel_id = $1
           AND cm.model = $2
         LIMIT 1"
    )))
    .bind(channel_id)
    .bind(model)
    .fetch_optional(&state.db.pool)
    .await?;
    if row.is_none() {
        return Err(AppError::BadRequest(format!(
            "price is not configured for model {model}"
        )));
    }
    Ok(())
}

fn validate_weight(weight: i32) -> AppResult<()> {
    if weight < 1 {
        return Err(AppError::BadRequest("weight must be >= 1".to_string()));
    }
    Ok(())
}

pub fn channel_from_row(
    row: &sqlx::postgres::PgRow,
    endpoints: Vec<ChannelEndpointRecord>,
    models: Vec<ChannelModelRecord>,
    probe_samples: Vec<ChannelProbeSampleRecord>,
) -> AppResult<ChannelRecord> {
    Ok(ChannelRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        name: row.try_get("name")?,
        enabled: row.try_get("enabled")?,
        priority: row.try_get("priority")?,
        weight: row.try_get("weight")?,
        key_selection_mode: row.try_get("key_selection_mode")?,
        use_credentials: row.try_get("use_credentials")?,
        endpoints,
        models,
        probe_samples,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn endpoint_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ChannelEndpointRecord> {
    Ok(ChannelEndpointRecord {
        id: row.try_get("id")?,
        channel_id: row.try_get("channel_id")?,
        protocol: row.try_get("protocol")?,
        base_url: row.try_get("base_url")?,
        models: row.try_get("models")?,
        responses_capability: row.try_get("responses_capability")?,
        responses_checked_at: row.try_get("responses_checked_at")?,
        responses_probe: row.try_get("responses_probe")?,
        enabled: row.try_get("enabled")?,
        healthy: row.try_get("healthy")?,
        last_error: row.try_get("last_error")?,
        cooldown_until: row.try_get("cooldown_until")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn channel_model_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ChannelModelRecord> {
    Ok(ChannelModelRecord {
        id: row.try_get("id")?,
        channel_id: row.try_get("channel_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        enabled: row.try_get("enabled")?,
        status: row.try_get("status")?,
        runtime_status: row.try_get("runtime_status")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_seen_at: row.try_get("last_seen_at")?,
        missing_since: row.try_get("missing_since")?,
        last_probe_at: row.try_get("last_probe_at")?,
        last_error: row.try_get("last_error")?,
        last_status_code: row.try_get("last_status_code")?,
        success_count: row.try_get("success_count")?,
        failure_count: row.try_get("failure_count")?,
        billing_enabled: row.try_get("billing_enabled")?,
        price_configured: row.try_get("price_configured")?,
        input_price_usd_micros: row.try_get("input_price_usd_micros")?,
        output_price_usd_micros: row.try_get("output_price_usd_micros")?,
        cache_read_price_usd_micros: row.try_get("cache_read_price_usd_micros")?,
        cache_write_price_usd_micros: row.try_get("cache_write_price_usd_micros")?,
        billing_meter: row.try_get("billing_meter")?,
        unit_price_usd_micros: row.try_get("unit_price_usd_micros")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub fn channel_key_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ChannelKeyRecord> {
    Ok(ChannelKeyRecord {
        id: row.try_get("id")?,
        channel_id: row.try_get("channel_id")?,
        name: row.try_get("name")?,
        key_prefix: row.try_get("key_prefix")?,
        enabled: row.try_get("enabled")?,
        healthy: row.try_get("healthy")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_error: row.try_get("last_error")?,
        last_used_at: row.try_get("last_used_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
