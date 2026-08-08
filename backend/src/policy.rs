use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{extract::State, routing::get, Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

use crate::{
    admin::{
        channel::{
            create_channel, create_channel_key, ChannelEndpointInput, CreateChannelKeyRequest,
            CreateChannelRequest, KeySelectionMode,
        },
        fetch_upstream_models,
        price::{
            list_pricing_templates, sync_pricing_templates, upsert_channel_price,
            PricingTemplateRecord, PricingTemplateSyncResult, SyncPricingTemplatesRequest,
            UpsertChannelPriceRequest,
        },
        provider::{list_providers, provider_default_endpoints, record_provider_models},
        setting::{
            test_smtp_setting, upsert_smtp_setting_in_tx,
            TestSmtpSettingResponse, UpsertSmtpSettingRequest,
        },
    },
    auth::{AdminAuth, UserSessionAuth},
    billing::BillingMeter,
    cache::InvalidationEvent,
    config::RuntimeProbe,
    error::{AppError, AppResult},
    payment::settings::{
        upsert_payment_setting_in_tx, UpsertPaymentSettingRequest,
    },
    setup::bootstrap::{
        apply_service_mode_env, save_runtime_config, save_service_mode_config, test_database,
        validate_service_mode_config, BootstrapConfigInput, BootstrapConfigResult,
        TestDatabaseInput, TestDatabaseResult,
    },
    AppState,
};

pub use crate::admin::provider::ProviderRecord;

const SERVICE_POLICY_SETUP_COMPLETED_KEY: &str = "setup_completed";
const SERVICE_POLICY_CREDIT_REQUIRED_KEY: &str = "credit_required";
const SERVICE_POLICY_REGISTRATION_ENABLED_KEY: &str = "registration_enabled";
const SERVICE_POLICY_UPDATED_KEYS: &[&str] = &[
    SERVICE_POLICY_SETUP_COMPLETED_KEY,
    SERVICE_POLICY_CREDIT_REQUIRED_KEY,
    SERVICE_POLICY_REGISTRATION_ENABLED_KEY,
];
const SERVICE_POLICY_CACHE_TTL: Duration = Duration::from_secs(10);
pub use crate::config::ServiceMode;

#[derive(Debug, Clone, Serialize)]
pub struct ServicePolicyRecord {
    pub runtime_mode: String,
    pub env_write_supported: bool,
    pub database_configured: bool,
    pub database_connected: bool,
    pub redis_configured: bool,
    pub redis_connected: Option<bool>,
    pub secrets_configured: bool,
    pub site_configured: bool,
    pub setup_completed: bool,
    pub bootstrap_required: bool,
    pub bootstrap_blocked_reason: Option<String>,
    pub restart_required: bool,
    pub site_name: Option<String>,
    pub public_base_url: Option<String>,
    pub service_mode: ServiceMode,
    pub credit_required: bool,
    pub registration_enabled: bool,
    pub recharge_enabled: bool,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Default)]
pub struct ServicePolicyCache {
    inner: Arc<Mutex<Option<CachedServicePolicy>>>,
}

struct CachedServicePolicy {
    record: ServicePolicyRecord,
    expires_at: Instant,
}

impl ServicePolicyCache {
    pub fn get(&self) -> Option<ServicePolicyRecord> {
        let now = Instant::now();
        let mut cached = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = cached.as_ref()?;
        if entry.expires_at > now {
            return Some(entry.record.clone());
        }
        *cached = None;
        None
    }

    pub fn store(&self, record: ServicePolicyRecord) {
        let mut cached = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *cached = Some(CachedServicePolicy {
            record,
            expires_at: Instant::now() + SERVICE_POLICY_CACHE_TTL,
        });
    }

    #[cfg(test)]
    pub fn invalidate(&self) {
        let mut cached = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        *cached = None;
    }
}

#[derive(Debug, Deserialize)]
pub struct CompleteSetupRequest {
    pub service_mode: ServiceMode,
    pub admin_username: Option<String>,
    pub admin_password: Option<String>,
    pub credit_required: Option<bool>,
    pub registration_enabled: Option<bool>,
    pub channel: Option<SetupChannelRequest>,
    #[serde(default)]
    pub prices: Vec<SetupChannelPriceRequest>,
    pub smtp: Option<UpsertSmtpSettingRequest>,
    pub payment: Option<UpsertPaymentSettingRequest>,
}

#[derive(Debug, Deserialize)]
pub struct SetupChannelRequest {
    pub provider: String,
    pub name: String,
    pub endpoints: Vec<ChannelEndpointInput>,
    pub secret: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupChannelPriceRequest {
    #[serde(rename = "provider")]
    pub _provider: String,
    pub model: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct SetupFetchUpstreamModelsRequest {
    provider: String,
    protocol: String,
    base_url: String,
    secret: String,
}

#[derive(Debug, Serialize)]
pub struct SetupFetchUpstreamModelsResponse {
    models: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SetupPricingTemplateSyncResponse {
    result: PricingTemplateSyncResult,
    templates: Vec<PricingTemplateRecord>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServicePolicyRequest {
    pub credit_required: Option<bool>,
    pub registration_enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredServicePolicy {
    setup_completed: bool,
    service_mode: ServiceMode,
    credit_required: bool,
    #[serde(default)]
    registration_enabled: Option<bool>,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/setup/status", get(setup_status))
        .route(
            "/api/setup/bootstrap",
            axum::routing::post(update_runtime_config),
        )
        .route(
            "/api/setup/test-database",
            axum::routing::post(test_database_connection),
        )
        .route("/api/setup/complete", axum::routing::post(complete_setup))
        .route("/api/setup/providers", get(setup_providers))
        .route(
            "/api/setup/upstream-models",
            axum::routing::post(setup_upstream_models),
        )
        .route(
            "/api/setup/pricing-templates/sync",
            axum::routing::post(setup_sync_pricing_templates),
        )
        .route(
            "/api/setup/smtp/test",
            axum::routing::post(setup_test_smtp_setting),
        )
        .route("/api/user/service-policy", get(user_service_policy))
        .route(
            "/api/admin/settings/service-policy",
            get(admin_service_policy).post(update_admin_service_policy),
        )
}

pub async fn current_service_policy(state: &AppState) -> AppResult<ServicePolicyRecord> {
    if let Some(record) = state.service_policy_cache.get() {
        return Ok(record);
    }

    let (stored, updated_at) = load_stored_policy(&state.db.pool).await?;
    let record = record_from_stored(stored, updated_at);
    state.service_policy_cache.store(record.clone());
    Ok(record)
}

pub async fn credit_required(state: &AppState) -> AppResult<bool> {
    Ok(current_service_policy(state).await?.credit_required)
}

pub async fn service_mode(state: &AppState) -> AppResult<ServiceMode> {
    Ok(current_service_policy(state).await?.service_mode)
}

pub async fn registration_policy(state: &AppState) -> AppResult<(ServiceMode, bool)> {
    let policy = current_service_policy(state).await?;
    Ok((policy.service_mode, policy.registration_enabled))
}

async fn setup_status(State(state): State<Arc<AppState>>) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn update_runtime_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<BootstrapConfigInput>,
) -> AppResult<Json<BootstrapConfigResult>> {
    if current_service_policy(&state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    let result = save_runtime_config(req).await?;
    schedule_runtime_restart(state.runtime_restart_tx.clone());
    Ok(Json(result))
}

async fn test_database_connection(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TestDatabaseInput>,
) -> AppResult<Json<TestDatabaseResult>> {
    if current_service_policy(&state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    let database_url = req.database_url.trim();
    if database_url.is_empty() {
        return Err(AppError::BadRequest("DATABASE_URL is required".to_string()));
    }
    test_database(database_url).await?;
    Ok(Json(TestDatabaseResult { ok: true }))
}

async fn complete_setup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompleteSetupRequest>,
) -> AppResult<Json<ServicePolicyRecord>> {
    let service_mode = req.service_mode;
    let record = complete_setup_for_state(state.clone(), req).await?;
    save_service_mode_config(service_mode).await?;
    schedule_runtime_restart(state.runtime_restart_tx.clone());
    Ok(Json(record))
}

pub async fn complete_setup_for_state(
    state: Arc<AppState>,
    req: CompleteSetupRequest,
) -> AppResult<ServicePolicyRecord> {
    let mut tx = state.db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('neogate.service_policy_setup'))")
        .execute(&mut *tx)
        .await?;

    if stored_policy_for_update(&mut tx).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }

    let admin_password = req
        .admin_password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest("admin password is required".to_string()))?;
    let admin_username = req
        .admin_username
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("admin");
    crate::auth::validate_user_password_input(admin_password)?;
    if req.channel.is_none() && !req.prices.is_empty() {
        return Err(AppError::BadRequest(
            "upstream channel is required when model prices are provided".to_string(),
        ));
    }
    if req.channel.is_some() && req.prices.is_empty() {
        return Err(AppError::BadRequest(
            "at least one model price is required".to_string(),
        ));
    }
    configure_initial_admin_credentials(
        &mut tx,
        &state.config.admin_token_secret,
        admin_username,
        admin_password,
    )
    .await?;
    let channel = if let Some(channel_req) = req.channel {
        Some(create_setup_channel(&state, channel_req).await?)
    } else {
        None
    };
    for price in req.prices {
        let channel_id = channel
            .as_ref()
            .map(|channel| channel.id)
            .ok_or_else(|| AppError::BadRequest("upstream channel is required".to_string()))?;
        upsert_channel_price(
            &state,
            UpsertChannelPriceRequest {
                channel_id,
                model: price.model,
                input_price_micros: price.input_price_micros,
                output_price_micros: price.output_price_micros,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: None,
                video_billing_mode: None,
                video_price_tiers: Vec::new(),
                enabled: price.enabled,
            },
        )
        .await?;
    }
    if let Some(smtp) = req.smtp {
        // 在事务内写入，与 service_policy 同一原子提交。
        // 修复前：写入在 tx 外执行，若后续 commit 失败则 setup_completed=false 但 SMTP 已持久化，导致半初始化状态。
        upsert_smtp_setting_in_tx(&mut tx, &state, smtp).await?;
    }
    if let Some(payment) = req.payment {
        if req.service_mode != ServiceMode::Paid && payment.payment_enabled {
            return Err(AppError::BadRequest(
                "payment can only be enabled in paid service mode".to_string(),
            ));
        }
        upsert_payment_setting_in_tx(&mut tx, &state, payment).await?;
    }
    validate_service_mode_config(req.service_mode)?;
    apply_service_mode_env(req.service_mode);

    let stored = StoredServicePolicy {
        setup_completed: true,
        service_mode: req.service_mode,
        credit_required: req.service_mode == ServiceMode::Paid
            || req.credit_required.unwrap_or(false),
        registration_enabled: Some(
            req.registration_enabled
                .unwrap_or(req.service_mode == ServiceMode::Paid),
        ),
    };
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
    state.service_policy_cache.store(record.clone());
    state.billing.invalidate_all_prices();
    state
        .cache_invalidator
        .invalidate(&state, InvalidationEvent::Routing)
        .await;
    if let Some(channel) = channel {
        tracing::info!(
            "first-run setup completed with initial channel {}",
            channel.id
        );
    } else {
        tracing::info!("first-run setup completed without an initial upstream channel");
    }
    Ok(record)
}

async fn setup_upstream_models(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupFetchUpstreamModelsRequest>,
) -> AppResult<Json<SetupFetchUpstreamModelsResponse>> {
    Ok(Json(setup_upstream_models_for_state(&state, req).await?))
}

pub async fn setup_upstream_models_for_state(
    state: &AppState,
    req: SetupFetchUpstreamModelsRequest,
) -> AppResult<SetupFetchUpstreamModelsResponse> {
    if current_service_policy(state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    let provider = req.provider.trim();
    let protocol = req.protocol.trim();
    let base_url = req.base_url.trim();
    let secret = req.secret.trim();
    if provider.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    if protocol != "openai" && protocol != "anthropic" {
        return Err(AppError::BadRequest(format!(
            "invalid protocol: {protocol}"
        )));
    }
    if base_url.is_empty() {
        return Err(AppError::BadRequest("base_url is required".to_string()));
    }
    if secret.is_empty() {
        return Err(AppError::BadRequest(
            "upstream api key is required".to_string(),
        ));
    }
    provider_default_endpoints(state, provider)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider}")))?;

    let models = fetch_upstream_models(state, protocol, base_url, secret).await?;
    record_provider_models(state, provider, &models, "upstream", false).await?;
    Ok(SetupFetchUpstreamModelsResponse { models })
}

async fn setup_providers(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<Vec<ProviderRecord>>> {
    Ok(Json(setup_providers_for_state(&state).await?))
}

pub async fn setup_providers_for_state(state: &AppState) -> AppResult<Vec<ProviderRecord>> {
    if current_service_policy(state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    list_providers(state).await
}

async fn setup_sync_pricing_templates(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<SetupPricingTemplateSyncResponse>> {
    Ok(Json(setup_sync_pricing_templates_for_state(&state).await?))
}

pub async fn setup_sync_pricing_templates_for_state(
    state: &AppState,
) -> AppResult<SetupPricingTemplateSyncResponse> {
    if current_service_policy(state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    let result = sync_pricing_templates(
        state,
        SyncPricingTemplatesRequest {
            source: String::new(),
        },
    )
    .await?;
    let templates = list_pricing_templates(state).await?;
    Ok(SetupPricingTemplateSyncResponse { result, templates })
}

async fn setup_test_smtp_setting(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpsertSmtpSettingRequest>,
) -> AppResult<Json<TestSmtpSettingResponse>> {
    Ok(Json(setup_test_smtp_setting_for_state(&state, req).await?))
}

pub async fn setup_test_smtp_setting_for_state(
    state: &AppState,
    req: UpsertSmtpSettingRequest,
) -> AppResult<TestSmtpSettingResponse> {
    if current_service_policy(state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }

    test_smtp_setting(state, req).await
}

async fn user_service_policy(
    State(state): State<Arc<AppState>>,
    _auth: UserSessionAuth,
) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn admin_service_policy(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn update_admin_service_policy(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpdateServicePolicyRequest>,
) -> AppResult<Json<ServicePolicyRecord>> {
    let mut tx = state.db.pool.begin().await?;
    let mut stored = stored_policy_for_update(&mut tx).await?;
    if !stored.setup_completed {
        return Err(AppError::BadRequest("setup is not completed".to_string()));
    }
    if let Some(credit_required) = req.credit_required {
        stored.credit_required = match stored.service_mode {
            ServiceMode::Internal => credit_required,
            ServiceMode::Paid => true,
        };
    }
    if let Some(registration_enabled) = req.registration_enabled {
        stored.registration_enabled = Some(registration_enabled);
    }
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
    state.service_policy_cache.store(record.clone());
    Ok(Json(record))
}

async fn stored_policy_for_update(
    tx: &mut Transaction<'_, Postgres>,
) -> AppResult<StoredServicePolicy> {
    let rows = sqlx::query(
        "SELECT key, value
         FROM setting
         WHERE key = ANY($1)
         FOR UPDATE",
    )
    .bind(SERVICE_POLICY_UPDATED_KEYS)
    .fetch_all(&mut **tx)
    .await?;
    if !rows.is_empty() {
        return stored_policy_from_setting_rows(&rows);
    }

    Ok(default_stored_policy())
}

async fn upsert_stored_policy(
    tx: &mut Transaction<'_, Postgres>,
    stored: StoredServicePolicy,
) -> AppResult<ServicePolicyRecord> {
    let stored = normalize_stored_policy(stored, None);
    upsert_policy_setting(
        tx,
        SERVICE_POLICY_SETUP_COMPLETED_KEY,
        stored.setup_completed,
    )
    .await?;
    upsert_policy_setting(
        tx,
        SERVICE_POLICY_CREDIT_REQUIRED_KEY,
        stored.credit_required,
    )
    .await?;
    upsert_policy_setting(
        tx,
        SERVICE_POLICY_REGISTRATION_ENABLED_KEY,
        stored.registration_enabled.unwrap_or(false),
    )
    .await?;
    let row = sqlx::query(
        "SELECT MAX(updated_at) AS updated_at
         FROM setting
         WHERE key = ANY($1)",
    )
    .bind(SERVICE_POLICY_UPDATED_KEYS)
    .fetch_one(&mut **tx)
    .await?;
    let updated_at: Option<DateTime<Utc>> = row.try_get("updated_at")?;
    Ok(record_from_stored(stored, updated_at))
}

async fn upsert_policy_setting(
    tx: &mut Transaction<'_, Postgres>,
    key: &str,
    value: bool,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO setting (key, value)
         VALUES ($1, $2)
         ON CONFLICT (key)
         DO UPDATE SET value = EXCLUDED.value, updated_at = now()",
    )
    .bind(key)
    .bind(serde_json::Value::Bool(value))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn load_stored_policy(
    pool: &sqlx::PgPool,
) -> AppResult<(StoredServicePolicy, Option<DateTime<Utc>>)> {
    let rows = sqlx::query(
        "SELECT key, value, updated_at
         FROM setting
         WHERE key = ANY($1)",
    )
    .bind(SERVICE_POLICY_UPDATED_KEYS)
    .fetch_all(pool)
    .await?;
    if !rows.is_empty() {
        let updated_at = latest_policy_updated_at(&rows)?;
        return Ok((stored_policy_from_setting_rows(&rows)?, updated_at));
    }

    Ok((default_stored_policy(), None))
}

fn stored_policy_from_setting_rows(
    rows: &[sqlx::postgres::PgRow],
) -> AppResult<StoredServicePolicy> {
    let mut stored = default_stored_policy();
    for row in rows {
        let key: String = row.try_get("key")?;
        let value: serde_json::Value = row.try_get("value")?;
        match key.as_str() {
            SERVICE_POLICY_SETUP_COMPLETED_KEY => stored.setup_completed = json_bool(&value),
            SERVICE_POLICY_CREDIT_REQUIRED_KEY => stored.credit_required = json_bool(&value),
            SERVICE_POLICY_REGISTRATION_ENABLED_KEY => {
                stored.registration_enabled = Some(json_bool(&value))
            }
            _ => {}
        }
    }
    Ok(normalize_stored_policy(stored, None))
}

fn latest_policy_updated_at(rows: &[sqlx::postgres::PgRow]) -> AppResult<Option<DateTime<Utc>>> {
    rows.iter()
        .map(|row| row.try_get("updated_at"))
        .collect::<Result<Vec<DateTime<Utc>>, _>>()
        .map(|values| values.into_iter().max())
        .map_err(Into::into)
}

fn json_bool(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(value) => *value,
        serde_json::Value::String(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "y" | "on"
        ),
        serde_json::Value::Number(value) => value.as_i64().is_some_and(|value| value != 0),
        _ => false,
    }
}

async fn configure_initial_admin_credentials(
    tx: &mut Transaction<'_, Postgres>,
    admin_token_secret: &str,
    username: &str,
    password: &str,
) -> AppResult<()> {
    let password_hash = crate::auth::hash_user_password(password, admin_token_secret);
    let updated = sqlx::query(
        r#"
        UPDATE admin
        SET username = $1,
            password_hash = $2,
            failed_login_attempts = 0,
            locked_until = NULL,
            password_changed_at = now(),
            updated_at = now()
        WHERE id = (
            SELECT id
            FROM admin
            WHERE status = 'enabled'
            ORDER BY id ASC
            LIMIT 1
        )
        RETURNING id
        "#,
    )
    .bind(username)
    .bind(&password_hash)
    .fetch_optional(&mut **tx)
    .await?;

    if updated.is_none() {
        sqlx::query(
            r#"
            INSERT INTO admin (
                username, password_hash, status, role,
                failed_login_attempts, locked_until, password_changed_at
            )
            VALUES ($1, $2, 'enabled', 'owner', 0, NULL, now())
            ON CONFLICT (username) DO UPDATE
            SET password_hash = EXCLUDED.password_hash,
                status = 'enabled',
                role = 'owner',
                failed_login_attempts = 0,
                locked_until = NULL,
                password_changed_at = now(),
                updated_at = now()
            "#,
        )
        .bind(username)
        .bind(password_hash)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn create_setup_channel(
    state: &AppState,
    req: SetupChannelRequest,
) -> AppResult<crate::admin::channel::ChannelRecord> {
    let provider = req.provider.trim().to_string();
    let name = req.name.trim().to_string();
    let secret = req.secret.trim().to_string();
    let endpoints: Vec<ChannelEndpointInput> = req
        .endpoints
        .into_iter()
        .map(|endpoint| {
            let protocol = endpoint.protocol.trim().to_string();
            let base_url = endpoint
                .base_url
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            let models = endpoint
                .models
                .into_iter()
                .map(|model| model.trim().to_string())
                .filter(|model| !model.is_empty())
                .collect();
            ChannelEndpointInput {
                protocol,
                base_url,
                models,
                enabled: endpoint.enabled,
            }
        })
        .collect();
    if provider.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    if name.is_empty() {
        return Err(AppError::BadRequest("channel name is required".to_string()));
    }
    if secret.is_empty() {
        return Err(AppError::BadRequest(
            "upstream api key is required".to_string(),
        ));
    }
    if endpoints.is_empty() {
        return Err(AppError::BadRequest(
            "at least one endpoint is required".to_string(),
        ));
    }
    if let Some(endpoint) = endpoints
        .iter()
        .find(|endpoint| endpoint.protocol != "openai" && endpoint.protocol != "anthropic")
    {
        return Err(AppError::BadRequest(format!(
            "invalid protocol: {}",
            endpoint.protocol
        )));
    }
    if endpoints
        .iter()
        .flat_map(|endpoint| endpoint.models.iter())
        .next()
        .is_none()
    {
        return Err(AppError::BadRequest(
            "at least one upstream model is required".to_string(),
        ));
    }

    let channel = create_channel(
        state,
        CreateChannelRequest {
            provider,
            name: name.clone(),
            endpoints,
            protocol: None,
            base_url: None,
            models: Vec::new(),
            enabled: true,
            priority: 0,
            weight: 1,
            key_selection_mode: KeySelectionMode::Polling,
            use_credentials: false,
        },
    )
    .await?;
    create_channel_key(
        state,
        channel.id,
        CreateChannelKeyRequest {
            name,
            secret,
            enabled: true,
        },
    )
    .await?;
    Ok(channel)
}

fn default_stored_policy() -> StoredServicePolicy {
    StoredServicePolicy {
        setup_completed: false,
        service_mode: ServiceMode::Internal,
        credit_required: false,
        registration_enabled: Some(false),
    }
}

/// 根据运行时环境变量覆盖部分策略字段（service_mode/credit_required/registration_enabled）。
/// 接收已解析的 `probe` 而非自行调用 `from_env()`，供 `record_from_stored` 统一传入，
/// 避免原来 normalize + record_from_stored 两处各调一次 `from_env()` 的冗余。
fn normalize_stored_policy(
    mut stored: StoredServicePolicy,
    probe: Option<&RuntimeProbe>,
) -> StoredServicePolicy {
    let service_mode = probe
        .and_then(|probe| probe.service_mode)
        .unwrap_or(ServiceMode::Internal);
    stored.service_mode = service_mode;
    if service_mode == ServiceMode::Paid {
        stored.credit_required = true;
    }
    if stored.registration_enabled.is_none() {
        stored.registration_enabled = Some(service_mode == ServiceMode::Paid);
    }
    stored
}

fn record_from_stored(
    stored: StoredServicePolicy,
    updated_at: Option<DateTime<Utc>>,
) -> ServicePolicyRecord {
    // 调用一次 from_env() 后将 probe 传给 normalize_stored_policy，
    // 消除之前 normalize 内部再次调用 from_env() 的重复。
    let probe = RuntimeProbe::from_env().ok();
    let stored = normalize_stored_policy(stored, probe.as_ref());
    let runtime_mode = probe.as_ref().map_or_else(
        || "standalone".to_string(),
        |probe| probe.runtime_mode.as_str().to_string(),
    );
    let redis_configured = probe.as_ref().is_some_and(|probe| probe.redis_configured());
    let is_distributed = runtime_mode == "distributed";
    ServicePolicyRecord {
        runtime_mode,
        env_write_supported: !is_distributed,
        database_configured: true,
        database_connected: true,
        redis_configured,
        redis_connected: is_distributed.then_some(redis_configured),
        secrets_configured: true,
        site_configured: true,
        setup_completed: stored.setup_completed,
        bootstrap_required: false,
        bootstrap_blocked_reason: None,
        restart_required: false,
        site_name: probe
            .as_ref()
            .and_then(|probe| probe.site_name.clone())
            .or_else(|| Some("NeoGate".to_string())),
        public_base_url: probe
            .as_ref()
            .and_then(|probe| probe.public_base_url.clone()),
        service_mode: stored.service_mode,
        credit_required: stored.credit_required,
        registration_enabled: stored.registration_enabled.unwrap_or(false),
        recharge_enabled: stored.service_mode == ServiceMode::Paid,
        updated_at,
    }
}

fn schedule_runtime_restart(restart_tx: tokio::sync::watch::Sender<bool>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = restart_tx.send(true);
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, MutexGuard};

    use super::*;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        service_mode: Option<String>,
        neogate_env_file: Option<String>,
        _guard: MutexGuard<'static, ()>,
    }

    impl EnvRestore {
        fn capture() -> Self {
            let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Self {
                service_mode: std::env::var("SERVICE_MODE").ok(),
                neogate_env_file: std::env::var("NEOGATE_ENV_FILE").ok(),
                _guard: guard,
            }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            restore_env_var("SERVICE_MODE", self.service_mode.as_deref());
            restore_env_var("NEOGATE_ENV_FILE", self.neogate_env_file.as_deref());
        }
    }

    fn restore_env_var(key: &str, value: Option<&str>) {
        if let Some(value) = value {
            std::env::set_var(key, value);
        } else {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn default_policy_is_unfinished_internal_without_credit_requirement() {
        let _env = EnvRestore::capture();
        std::env::remove_var("SERVICE_MODE");
        std::env::remove_var("NEOGATE_ENV_FILE");
        let record = record_from_stored(default_stored_policy(), None);

        assert!(!record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Internal);
        assert!(!record.credit_required);
        assert!(!record.recharge_enabled);
    }

    #[test]
    fn paid_mode_always_requires_credit_and_enables_recharge() {
        let _env = EnvRestore::capture();
        std::env::set_var("SERVICE_MODE", "paid");
        let record = record_from_stored(
            StoredServicePolicy {
                setup_completed: true,
                service_mode: ServiceMode::Internal,
                credit_required: false,
                registration_enabled: None,
            },
            None,
        );

        assert!(record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Paid);
        assert!(record.credit_required);
        assert!(record.registration_enabled);
        assert!(record.recharge_enabled);
    }

    #[test]
    fn stored_service_mode_does_not_drive_runtime_policy() {
        let _env = EnvRestore::capture();
        std::env::remove_var("SERVICE_MODE");
        std::env::remove_var("NEOGATE_ENV_FILE");
        let record = record_from_stored(
            StoredServicePolicy {
                setup_completed: true,
                service_mode: ServiceMode::Paid,
                credit_required: false,
                registration_enabled: Some(true),
            },
            None,
        );

        assert_eq!(record.service_mode, ServiceMode::Internal);
        assert!(!record.credit_required);
        assert!(!record.recharge_enabled);
    }

    #[test]
    fn json_bool_accepts_scalar_setting_values() {
        assert!(json_bool(&serde_json::Value::Bool(true)));
        assert!(json_bool(&serde_json::Value::String("true".to_string())));
        assert!(json_bool(&serde_json::Value::Number(1.into())));
        assert!(!json_bool(&serde_json::Value::Bool(false)));
        assert!(!json_bool(&serde_json::Value::String("false".to_string())));
        assert!(!json_bool(&serde_json::Value::Number(0.into())));
    }
}
