use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

use crate::{
    auth::key_prefix,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

use super::diagnostics::{recent_probe_samples_by_channel, ChannelProbeSampleRecord};
use super::provider::{
    ensure_custom_provider, ensure_newapi_provider, provider_default_endpoint_base_url,
    provider_default_endpoints, provider_default_models, record_provider_models,
    CUSTOM_PROVIDER_CODE, NEWAPI_PROVIDER_CODE, OPENAI_OAUTH_PROTOCOL,
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
    ensure_provider_exists(state, &provider_code).await?;
    let endpoints = normalize_create_endpoints(state, &provider_code, &req).await?;
    let endpoint_models = models_from_endpoints(&endpoints);

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
    tx.commit().await?;
    record_provider_models(state, &provider_code, &endpoint_models, "channel", true).await?;

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
    let probe_samples = recent_probe_samples_by_channel(state, &channel_ids, 12).await?;

    rows.iter()
        .map(|row| {
            let id: DbId = row.try_get("id")?;
            channel_from_row(
                row,
                endpoints.get(&id).cloned().unwrap_or_default(),
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
    }
    tx.commit().await?;
    if let Some(endpoint_models) = endpoint_models {
        record_provider_models(state, &provider_code, &endpoint_models, "channel", true).await?;
    }

    let endpoints = endpoints_by_channel(state, &[id]).await?;
    let probe_samples = recent_probe_samples_by_channel(state, &[id], 12).await?;
    channel_from_row(
        &row,
        endpoints.get(&id).cloned().unwrap_or_default(),
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

pub async fn create_channel_key(
    state: &AppState,
    channel_id: DbId,
    req: CreateChannelKeyRequest,
) -> AppResult<ChannelKeyRecord> {
    let secret_ciphertext = state.secrets.encrypt(&req.secret)?;
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
    .fetch_one(&state.db.pool)
    .await?;
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
    state.secrets.plaintext(key_id, &secret_ciphertext).map_err(Into::into)
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
    let current_last_error: Option<String> = current.try_get("last_error")?;
    let current_cooldown_until: Option<DateTime<Utc>> = current.try_get("cooldown_until")?;
    let last_error = req.last_error.unwrap_or(current_last_error);
    let cooldown_until = req.cooldown_until.unwrap_or(current_cooldown_until);
    let key_prefix_value = req.secret.as_ref().map(|secret| key_prefix(secret));
    let secret_ciphertext = req
        .secret
        .as_deref()
        .map(|secret| state.secrets.encrypt(secret))
        .transpose()?;

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
    .bind(req.healthy)
    .bind(cooldown_until)
    .bind(last_error)
    .fetch_one(&state.db.pool)
    .await?;
    if req.secret.is_some() {
        state.secrets.forget(id);
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
        return normalize_endpoint_inputs(inputs.iter());
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
                return normalize_endpoint_inputs(inputs.iter());
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
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_endpoint_base_url.trim())
        .to_string();

    normalize_endpoint_inputs(
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
        return Ok(Some(normalize_endpoint_inputs(inputs.iter())?));
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
    let base_url = req
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_endpoint_base_url.trim())
        .to_string();

    Ok(Some(normalize_endpoint_inputs(
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
    if let Some(protocol) = requested_protocol
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
        let base_url = input
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
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
    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for endpoint in endpoints {
        for model in &endpoint.models {
            let model = model.trim();
            if !model.is_empty() && seen.insert(model.to_string()) {
                models.push(model.to_string());
            }
        }
    }
    models
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

    let mut models = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for input in &inputs {
        for model in &input.models {
            let model = model.trim();
            if !model.is_empty() && seen.insert(model.to_string()) {
                models.push(model.to_string());
            }
        }
    }

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
    let probe_samples = recent_probe_samples_by_channel(state, &[id], 12).await?;
    channel_from_row(
        &row,
        endpoints.get(&id).cloned().unwrap_or_default(),
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
        "SELECT id, channel_id, protocol, base_url, models, enabled, healthy,
                last_error, cooldown_until, created_at, updated_at
         FROM channel_endpoint
         WHERE channel_id = ANY($1)
         ORDER BY channel_id ASC,
                  CASE protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END,
                  created_at ASC",
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

fn validate_weight(weight: i32) -> AppResult<()> {
    if weight < 1 {
        return Err(AppError::BadRequest("weight must be >= 1".to_string()));
    }
    Ok(())
}

pub fn channel_from_row(
    row: &sqlx::postgres::PgRow,
    endpoints: Vec<ChannelEndpointRecord>,
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
