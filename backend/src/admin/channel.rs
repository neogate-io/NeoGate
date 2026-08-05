use std::{
    collections::{HashMap, HashSet, VecDeque},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{AssertSqlSafe, Postgres, Row, Transaction};

use crate::{
    auth::key_prefix,
    billing::{BILLABLE_PRICE_CONDITION, BILLABLE_PRICE_CONDITION_CP},
    error::{AppError, AppResult},
    id::DbId,
    input::trimmed_non_empty,
    AppState,
};

use super::diagnostics::{recent_probe_samples_by_channel, ChannelProbeSampleRecord};
use super::provider::{
    provider_default_endpoint_base_url, provider_default_endpoints, record_provider_models,
    OPENAI_OAUTH_PROTOCOL,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeySelectionMode {
    Polling,
    Random,
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
    pub base_model: Option<String>,
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
    pub input_price_micros: Option<i64>,
    pub output_price_micros: Option<i64>,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: Option<String>,
    pub unit_price_micros: Option<i64>,
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
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelKeyRecord {
    pub id: DbId,
    pub channel_id: DbId,
    pub name: String,
    pub masked_key: String,
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
    #[serde(default, deserialize_with = "deserialize_nullable_base_model")]
    pub base_model: Option<Option<String>>,
}

fn deserialize_nullable_base_model<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
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

    // 后台探测 adapter hint，不阻塞响应
    {
        let state_bg = state.clone();
        tokio::spawn(async move {
            detect_and_update_channel_endpoint_hints(&state_bg, channel_id).await;
        });
    }

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

    // 后台探测 adapter hint（仅对 hint 为 NULL 的 endpoint），不阻塞响应
    {
        let state_bg = state.clone();
        tokio::spawn(async move {
            detect_and_update_channel_endpoint_hints(&state_bg, id).await;
        });
    }

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

    let update_base_model = req.base_model.is_some();
    let base_model = req
        .base_model
        .flatten()
        .and_then(|value| trimmed_non_empty(Some(&value)).map(str::to_string));

    let row = sqlx::query(
        "UPDATE channel_model
         SET enabled = COALESCE($3, enabled),
             base_model = CASE WHEN $4 THEN $5 ELSE base_model END,
             status = CASE
                 WHEN COALESCE($3, enabled) = FALSE THEN 'disabled'
                 WHEN status = 'disabled' THEN 'available'
                 ELSE status
             END,
             runtime_status = CASE
                 WHEN $3 = TRUE THEN 'normal'
                 ELSE runtime_status
             END,
             cooldown_until = CASE
                 WHEN $3 = TRUE THEN NULL
                 ELSE cooldown_until
             END,
             last_error = CASE
                 WHEN $3 = TRUE THEN NULL
                 ELSE last_error
             END,
             last_status_code = CASE
                 WHEN $3 = TRUE THEN NULL
                 ELSE last_status_code
             END,
             updated_at = now()
         WHERE channel_id = $1
           AND model = $2
         RETURNING id",
    )
    .bind(channel_id)
    .bind(model)
    .bind(req.enabled)
    .bind(update_base_model)
    .bind(base_model)
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
         RETURNING id, channel_id, name, enabled, healthy, cooldown_until,
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
    channel_key_from_row(&row, &req.secret)
}

pub async fn list_channel_keys(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<Vec<ChannelKeyRecord>> {
    let rows = sqlx::query(
        "SELECT id, channel_id, name, secret_ciphertext, enabled, healthy, cooldown_until,
                last_error, last_used_at, created_at, updated_at
         FROM channel_key WHERE channel_id = $1 ORDER BY created_at ASC",
    )
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter()
        .map(|row| channel_key_from_secret_row(state, row))
        .collect()
}

pub async fn list_all_channel_keys(state: &AppState) -> AppResult<Vec<ChannelKeyRecord>> {
    let rows = sqlx::query(
        "SELECT id, channel_id, name, secret_ciphertext, enabled, healthy, cooldown_until,
                last_error, last_used_at, created_at, updated_at
         FROM channel_key ORDER BY channel_id ASC, created_at ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter()
        .map(|row| channel_key_from_secret_row(state, row))
        .collect()
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
        "SELECT id, channel_id, name, enabled, healthy, cooldown_until,
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
         RETURNING id, channel_id, name, enabled, healthy, cooldown_until,
                   last_error, last_used_at, created_at, updated_at, secret_ciphertext",
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
    }
    channel_key_from_secret_row(state, &row)
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
            let inputs: Vec<ChannelEndpointInput> = defaults
                .into_iter()
                .filter(|endpoint| !endpoint.base_url.trim().is_empty())
                .map(|endpoint| ChannelEndpointInput {
                    protocol: endpoint.protocol,
                    base_url: Some(endpoint.base_url),
                    models: Vec::new(),
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
    _provider_code: &str,
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
        endpoints.push(NormalizedEndpoint {
            protocol,
            base_url,
            models: input.models.clone(),
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
         (channel_id, protocol, base_url, models, enabled)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(channel_id)
    .bind(endpoint.protocol)
    .bind(endpoint.base_url)
    .bind(endpoint.models)
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
         (channel_id, protocol, base_url, models, enabled)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (channel_id, protocol) DO UPDATE
         SET base_url = EXCLUDED.base_url,
             models = EXCLUDED.models,
             enabled = EXCLUDED.enabled,
             updated_at = now()",
    )
    .bind(channel_id)
    .bind(endpoint.protocol)
    .bind(endpoint.base_url)
    .bind(endpoint.models)
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

    let templates = base_model_templates(tx).await?;
    let mut active_models = Vec::<String>::new();
    let mut seen = HashSet::new();
    for row in rows {
        let models: Vec<String> = row.try_get("models")?;
        for model in models {
            let model = model.trim();
            if model.is_empty() || !seen.insert(model.to_string()) {
                continue;
            }
            let price_configured = model_has_enabled_price(tx, channel_id, model).await?;
            let base_model = find_base_model(&templates, provider, model);
            active_models.push(model.to_string());
            sqlx::query(
                "INSERT INTO channel_model
                 (channel_id, model, base_model, enabled, status, runtime_status, last_seen_at)
                 VALUES ($1, $2, $3, $4, 'available', 'normal', now())
                 ON CONFLICT (channel_id, model)
                 DO UPDATE SET
                     base_model = EXCLUDED.base_model,
                     enabled = EXCLUDED.enabled,
                     status = 'available',
                     missing_since = NULL,
                     last_seen_at = COALESCE(channel_model.last_seen_at, now()),
                     updated_at = now()",
            )
            .bind(channel_id)
            .bind(model)
            .bind(base_model)
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

#[derive(Debug)]
struct BaseModelTemplate {
    provider: String,
    model: String,
}

async fn base_model_templates(
    tx: &mut Transaction<'_, Postgres>,
) -> AppResult<Vec<BaseModelTemplate>> {
    let rows = sqlx::query(
        "SELECT provider, model
         FROM pricing_template
         WHERE enabled = TRUE
           AND source <> 'confirmed_price'
         ORDER BY provider ASC, model ASC",
    )
    .fetch_all(&mut **tx)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(BaseModelTemplate {
                provider: row.try_get("provider")?,
                model: row.try_get("model")?,
            })
        })
        .collect()
}

fn find_base_model(templates: &[BaseModelTemplate], provider: &str, model: &str) -> Option<String> {
    let provider = provider.trim();
    let model = model.trim().to_lowercase();
    let aliases = pricing_reference_model_aliases(&model);
    let matching: Vec<&BaseModelTemplate> = templates
        .iter()
        .filter(|template| {
            pricing_reference_model_aliases(&template.model)
                .iter()
                .any(|alias| aliases.contains(alias))
        })
        .collect();

    matching
        .iter()
        .copied()
        .find(|template| {
            template.provider.trim() == provider
                && template.model.trim().eq_ignore_ascii_case(&model)
        })
        .or_else(|| {
            matching
                .iter()
                .copied()
                .find(|template| template.provider.trim() == provider)
        })
        .or_else(|| {
            matching.iter().copied().find(|template| {
                template.provider.trim() != provider
                    && template.model.trim().eq_ignore_ascii_case(&model)
            })
        })
        .or_else(|| {
            matching
                .iter()
                .copied()
                .find(|template| template.provider.trim() != provider)
        })
        .map(|template| template.model.clone())
}

fn pricing_reference_model_aliases(model: &str) -> HashSet<String> {
    let mut aliases = HashSet::new();
    let mut queue = VecDeque::from([model.trim().to_lowercase()]);

    while let Some(alias) = queue.pop_front() {
        if alias.is_empty() || !aliases.insert(alias.clone()) {
            continue;
        }

        let without_display_prefix = strip_model_display_prefixes(&alias);
        if without_display_prefix != alias {
            queue.push_back(without_display_prefix);
        }

        let dotted_version = dot_version_alias(&alias);
        if dotted_version != alias {
            queue.push_back(dotted_version);
        }

        if let Some(without_date) = strip_six_digit_suffix(&alias) {
            queue.push_back(without_date.clone());
            let dotted_without_date = dot_version_alias(&without_date);
            if dotted_without_date != alias {
                queue.push_back(dotted_without_date);
            }
        }

        if let Some(without_resolution) = strip_resolution_suffix(&alias) {
            queue.push_back(without_resolution);
        }

        if let Some((prefix, remainder)) = alias.split_once(':') {
            if !prefix.is_empty() && prefix.chars().all(|value| value.is_ascii_digit()) {
                queue.push_back(remainder.to_string());
            }
        }

        if let Some(seedance) = alias.strip_prefix("dreamina-seedance-") {
            queue.push_back(format!("seedance-{seedance}"));
        }

        if let Some(seedance) = alias.strip_prefix("seedance-") {
            queue.push_back(format!("doubao-seedance-{seedance}"));
        }

        let seedance = alias.strip_prefix("doubao-").unwrap_or(&alias);
        if matches!(seedance, "seedance-2.0-fast" | "seedance-2.0-mini") {
            queue.push_back(format!("{alias}-1080p"));
        }
    }

    aliases
}

fn strip_model_display_prefixes(model: &str) -> String {
    let mut value = model;
    loop {
        let closing = if value.starts_with('【') {
            '】'
        } else if value.starts_with('[') {
            ']'
        } else {
            break;
        };
        let Some(index) = value.find(closing) else {
            break;
        };
        value = value[index + closing.len_utf8()..].trim_start();
    }
    value.to_string()
}

fn strip_six_digit_suffix(model: &str) -> Option<String> {
    let (base, suffix) = model.rsplit_once('-')?;
    (suffix.len() == 6 && suffix.chars().all(|value| value.is_ascii_digit()))
        .then(|| base.to_string())
}

fn strip_resolution_suffix(model: &str) -> Option<String> {
    ["-480p", "-720p", "-1080p", "-4k"]
        .iter()
        .find_map(|suffix| model.strip_suffix(suffix).map(str::to_string))
}

fn dot_version_alias(model: &str) -> String {
    let chars: Vec<char> = model.chars().collect();
    let mut result = String::with_capacity(model.len());
    let mut index = 0;

    while index < chars.len() {
        if chars[index] != '-' {
            result.push(chars[index]);
            index += 1;
            continue;
        }

        let major_start = index + 1;
        let mut major_end = major_start;
        while major_end < chars.len() && chars[major_end].is_ascii_digit() {
            major_end += 1;
        }
        if major_end == major_start || major_end >= chars.len() || chars[major_end] != '-' {
            result.push(chars[index]);
            index += 1;
            continue;
        }

        let minor_start = major_end + 1;
        let mut minor_end = minor_start;
        while minor_end < chars.len() && chars[minor_end].is_ascii_digit() {
            minor_end += 1;
        }
        if minor_end == minor_start || (minor_end < chars.len() && chars[minor_end] != '-') {
            result.push(chars[index]);
            index += 1;
            continue;
        }

        result.push('-');
        result.extend(&chars[major_start..major_end]);
        result.push('.');
        result.extend(&chars[minor_start..minor_end]);
        index = minor_end;
    }

    result
}

async fn model_has_enabled_price(
    tx: &mut Transaction<'_, Postgres>,
    channel_id: DbId,
    model: &str,
) -> AppResult<bool> {
    let exists: Option<i32> = sqlx::query_scalar(AssertSqlSafe(format!(
        "SELECT 1
         FROM channel_price
         WHERE channel_id = $1
           AND model = $2
           AND enabled = TRUE
           AND {BILLABLE_PRICE_CONDITION}
         LIMIT 1"
    )))
    .bind(channel_id)
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
        "SELECT id, channel_id, protocol, base_url, models,
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
        "SELECT cm.id, cm.channel_id, c.provider, cm.model, cm.base_model, cm.enabled,
                cm.status, cm.runtime_status, cm.cooldown_until, cm.last_seen_at,
                cm.missing_since, cm.last_probe_at, cm.last_error, cm.last_status_code,
                cm.success_count, cm.failure_count, cm.created_at, cm.updated_at,
                COALESCE(
                    cp.enabled
                    AND {BILLABLE_PRICE_CONDITION_CP},
                    FALSE
                ) AS billing_enabled,
                (cp.id IS NOT NULL) AS price_configured,
                cp.input_price_micros, cp.output_price_micros,
                cp.cache_read_price_micros, cp.cache_write_price_micros,
                cp.billing_meter,
                cp.unit_price_micros
         FROM channel_model cm
         JOIN channel c ON c.id = cm.channel_id
         LEFT JOIN channel_price cp
           ON cp.channel_id = cm.channel_id
          AND cp.model = cm.model
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
         JOIN channel_price cp
          ON cp.channel_id = cm.channel_id
         AND cp.model = cm.model
         AND cp.enabled = TRUE
         AND {BILLABLE_PRICE_CONDITION_CP}
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
        base_model: row.try_get("base_model")?,
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
        input_price_micros: row.try_get("input_price_micros")?,
        output_price_micros: row.try_get("output_price_micros")?,
        cache_read_price_micros: row.try_get("cache_read_price_micros")?,
        cache_write_price_micros: row.try_get("cache_write_price_micros")?,
        billing_meter: row.try_get("billing_meter")?,
        unit_price_micros: row.try_get("unit_price_micros")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn channel_key_from_secret_row(
    state: &AppState,
    row: &sqlx::postgres::PgRow,
) -> AppResult<ChannelKeyRecord> {
    let id = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    let secret = state.secrets.plaintext(id, &secret_ciphertext)?;
    channel_key_from_row(row, &secret)
}

fn channel_key_from_row(row: &sqlx::postgres::PgRow, secret: &str) -> AppResult<ChannelKeyRecord> {
    Ok(ChannelKeyRecord {
        id: row.try_get("id")?,
        channel_id: row.try_get("channel_id")?,
        name: row.try_get("name")?,
        masked_key: mask_channel_key(secret),
        enabled: row.try_get("enabled")?,
        healthy: row.try_get("healthy")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_error: row.try_get("last_error")?,
        last_used_at: row.try_get("last_used_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub(crate) fn mask_channel_key(secret: &str) -> String {
    const HEAD_LEN: usize = 8;
    const TAIL_LEN: usize = 6;
    const MASK_THRESHOLD: usize = 18;

    let length = secret.chars().count();
    if length <= MASK_THRESHOLD {
        return secret.to_string();
    }

    let head: String = secret.chars().take(HEAD_LEN).collect();
    let tail: String = secret
        .chars()
        .skip(length.saturating_sub(TAIL_LEN))
        .collect();
    format!("{head}********{tail}")
}

/// 剥离 base_url 末尾的 API 版本路径（/v1、/v2 等）以还原根 URL，
/// 用于拼接厂商自有的非 /v1 路径（如 new-api 的 /api/status）。
fn strip_api_version_suffix(base_url: &str) -> &str {
    let trimmed = base_url.trim_end_matches('/');
    if let Some(idx) = trimmed.rfind('/') {
        let last = &trimmed[idx + 1..];
        if matches!(
            last,
            "v1" | "v2" | "v3" | "v4" | "v1beta" | "v1beta1" | "openai"
        ) {
            return &trimmed[..idx];
        }
    }
    trimmed
}

/// 向 `{base_url}/api/status` 发送一次轻量探测，识别上游服务类型。
/// 目前仅识别 new-api（响应 JSON 同时包含 `version` 和 `start_time` 字段）。
/// 超时或响应不匹配时静默返回 None，不影响调用方流程。
async fn probe_adapter_hint(
    http: &reqwest::Client,
    base_url: &str,
    secret: Option<&str>,
) -> Option<&'static str> {
    let root = strip_api_version_suffix(base_url);
    let url = format!("{root}/api/status");
    let mut req = http.get(&url).timeout(Duration::from_secs(3));
    if let Some(key) = secret {
        req = req.bearer_auth(key);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    // new-api 特有指纹：同时返回 version 和 start_time
    if body.get("version").is_some() && body.get("start_time").is_some() {
        return Some("newapi");
    }
    None
}

/// 后台任务：对 channel 下 adapter_hint 为 NULL 的 openai 端点运行指纹探测，
/// 若识别成功则写入 DB 并使路由缓存失效。
/// 仅在 hint 为 NULL 时运行，不覆盖已有的手动配置。
async fn detect_and_update_channel_endpoint_hints(state: &AppState, channel_id: DbId) {
    let rows = sqlx::query(
        "SELECT ce.id, ce.base_url, ck.id AS key_id, ck.secret_ciphertext
         FROM channel_endpoint ce
         LEFT JOIN LATERAL (
             SELECT id, secret_ciphertext
             FROM channel_key
             WHERE channel_id = ce.channel_id
               AND enabled = TRUE
               AND healthy = TRUE
             ORDER BY created_at ASC
             LIMIT 1
         ) ck ON TRUE
         WHERE ce.channel_id = $1
           AND ce.protocol = 'openai'
           AND ce.adapter_hint IS NULL",
    )
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await;

    let rows = match rows {
        Ok(r) => r,
        Err(err) => {
            tracing::debug!(channel_id, %err, "adapter hint detection: failed to load endpoints");
            return;
        }
    };

    let mut any_updated = false;
    for row in &rows {
        let Ok(endpoint_id): Result<DbId, _> = row.try_get("id") else {
            continue;
        };
        let Ok(base_url): Result<String, _> = row.try_get("base_url") else {
            continue;
        };
        let key_id: Option<DbId> = row.try_get("key_id").ok().flatten();
        let ciphertext: Option<String> = row.try_get("secret_ciphertext").ok().flatten();

        let secret = if let (Some(kid), Some(ct)) = (key_id, ciphertext.as_deref()) {
            state.secrets.plaintext(kid, ct).ok()
        } else {
            None
        };

        let Some(hint) = probe_adapter_hint(&state.http, &base_url, secret.as_deref()).await else {
            continue;
        };

        match sqlx::query(
            "UPDATE channel_endpoint
             SET adapter_hint = $2, updated_at = now()
             WHERE id = $1 AND adapter_hint IS NULL",
        )
        .bind(endpoint_id)
        .bind(hint)
        .execute(&state.db.pool)
        .await
        {
            Ok(_) => {
                tracing::info!(
                    channel_id,
                    endpoint_id,
                    %base_url,
                    hint,
                    "adapter hint detected and saved"
                );
                any_updated = true;
            }
            Err(err) => {
                tracing::debug!(endpoint_id, %err, "failed to save adapter hint");
            }
        }
    }

    if any_updated {
        state
            .cache_invalidator
            .invalidate(state, crate::cache::InvalidationEvent::Routing)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        find_base_model, pricing_reference_model_aliases, BaseModelTemplate,
        UpdateChannelModelRequest,
    };

    fn template(provider: &str, model: &str) -> BaseModelTemplate {
        BaseModelTemplate {
            provider: provider.to_string(),
            model: model.to_string(),
        }
    }

    #[test]
    fn base_model_aliases_normalize_upstream_model_variants() {
        let aliases = pricing_reference_model_aliases("【按秒计费】dreamina-seedance-2-0-260128");

        assert!(aliases.contains("doubao-seedance-2.0"));
    }

    #[test]
    fn base_model_match_prefers_same_provider_reference() {
        let templates = vec![
            template("other", "doubao-seedance-1.0-pro-fast"),
            template("doubao", "doubao-seedance-1.0-pro-fast"),
        ];

        assert_eq!(
            find_base_model(&templates, "doubao", "doubao-seedance-1-0-pro-fast-251015").as_deref(),
            Some("doubao-seedance-1.0-pro-fast")
        );
    }

    #[test]
    fn base_model_match_can_use_cross_provider_reference() {
        let templates = vec![template("doubao", "doubao-seedance-2.0")];

        assert_eq!(
            find_base_model(
                &templates,
                "openai",
                "[per second] dreamina-seedance-2-0-260128"
            )
            .as_deref(),
            Some("doubao-seedance-2.0")
        );
    }

    #[test]
    fn base_model_match_returns_none_for_unknown_model() {
        let templates = vec![template("openai", "gpt-5")];

        assert_eq!(find_base_model(&templates, "openai", "unknown-model"), None);
    }

    #[test]
    fn channel_model_update_distinguishes_missing_null_and_named_base_model() {
        let missing: UpdateChannelModelRequest = serde_json::from_value(serde_json::json!({}))
            .expect("missing base_model should deserialize");
        let cleared: UpdateChannelModelRequest =
            serde_json::from_value(serde_json::json!({ "base_model": null }))
                .expect("null base_model should deserialize");
        let named: UpdateChannelModelRequest =
            serde_json::from_value(serde_json::json!({ "base_model": "gpt-5" }))
                .expect("named base_model should deserialize");

        assert_eq!(missing.base_model, None);
        assert_eq!(cleared.base_model, Some(None));
        assert_eq!(named.base_model, Some(Some("gpt-5".to_string())));
    }
}
