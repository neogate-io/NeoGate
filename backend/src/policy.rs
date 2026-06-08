use std::sync::Arc;
use std::time::Duration;

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
            list_pricing_templates, sync_pricing_templates, upsert_provider_price,
            PricingTemplateRecord, PricingTemplateSyncResult, SyncPricingTemplatesRequest,
            UpsertProviderPriceRequest,
        },
        provider::{
            ensure_custom_provider, list_providers, provider_default_endpoints,
            record_provider_models, ProviderRecord,
        },
        setting::{
            test_smtp_setting, upsert_smtp_setting, TestSmtpSettingResponse,
            UpsertSmtpSettingRequest,
        },
    },
    auth::{AdminAuth, UserSessionAuth},
    bootstrap::{
        save_runtime_config, test_database, BootstrapConfigInput, BootstrapConfigResult,
        TestDatabaseInput, TestDatabaseResult,
    },
    cache::InvalidationEvent,
    config::RuntimeProbe,
    error::{AppError, AppResult},
    payment::settings::{upsert_payment_setting, UpsertPaymentSettingRequest},
    AppState,
};

pub const SERVICE_POLICY_SETTING_KEY: &str = "service_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Internal,
    Paid,
}

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
    pub recharge_enabled: bool,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteSetupRequest {
    pub service_mode: ServiceMode,
    pub admin_username: Option<String>,
    pub admin_password: Option<String>,
    pub credit_required: Option<bool>,
    pub channel: Option<SetupChannelRequest>,
    #[serde(default)]
    pub prices: Vec<SetupProviderPriceRequest>,
    pub smtp: Option<UpsertSmtpSettingRequest>,
    pub payment: Option<UpsertPaymentSettingRequest>,
}

#[derive(Debug, Deserialize)]
pub struct SetupChannelRequest {
    pub provider: String,
    pub name: String,
    pub protocol: String,
    pub base_url: String,
    #[serde(default)]
    pub models: Vec<String>,
    pub secret: String,
}

#[derive(Debug, Deserialize)]
pub struct SetupProviderPriceRequest {
    pub provider: String,
    pub model: String,
    pub input_price_usd_micros: i64,
    pub output_price_usd_micros: i64,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
struct SetupFetchUpstreamModelsRequest {
    provider: String,
    protocol: String,
    base_url: String,
    secret: String,
}

#[derive(Debug, Serialize)]
struct SetupFetchUpstreamModelsResponse {
    models: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SetupPricingTemplateSyncResponse {
    result: PricingTemplateSyncResult,
    templates: Vec<PricingTemplateRecord>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServicePolicyRequest {
    pub credit_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredServicePolicy {
    setup_completed: bool,
    service_mode: ServiceMode,
    credit_required: bool,
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
    let Some(row) = sqlx::query("SELECT value, updated_at FROM setting WHERE key = $1")
        .bind(SERVICE_POLICY_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(record_from_stored(default_stored_policy(), None));
    };

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let stored = normalize_stored_policy(serde_json::from_value(value)?);
    Ok(record_from_stored(stored, Some(updated_at)))
}

pub async fn credit_required(state: &AppState) -> AppResult<bool> {
    Ok(current_service_policy(state).await?.credit_required)
}

pub async fn service_mode(state: &AppState) -> AppResult<ServiceMode> {
    Ok(current_service_policy(state).await?.service_mode)
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
        upsert_provider_price(
            &state,
            UpsertProviderPriceRequest {
                provider: price.provider,
                model: price.model,
                input_price_usd_micros: price.input_price_usd_micros,
                output_price_usd_micros: price.output_price_usd_micros,
                cache_read_price_usd_micros: None,
                cache_write_price_usd_micros: None,
                enabled: price.enabled,
            },
        )
        .await?;
    }
    if let Some(smtp) = req.smtp {
        upsert_smtp_setting(&state, smtp).await?;
    }
    if let Some(payment) = req.payment {
        if req.service_mode != ServiceMode::Paid && payment.payment_enabled {
            return Err(AppError::BadRequest(
                "payment can only be enabled in paid service mode".to_string(),
            ));
        }
        upsert_payment_setting(&state, payment).await?;
    }

    let stored = StoredServicePolicy {
        setup_completed: true,
        service_mode: req.service_mode,
        credit_required: req.service_mode == ServiceMode::Paid
            || req.credit_required.unwrap_or(false),
    };
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
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
    Ok(Json(record))
}

async fn setup_upstream_models(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupFetchUpstreamModelsRequest>,
) -> AppResult<Json<SetupFetchUpstreamModelsResponse>> {
    if current_service_policy(&state).await?.setup_completed {
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
    if provider == "custom" {
        ensure_custom_provider(&state).await?;
    }
    provider_default_endpoints(&state, provider)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider}")))?;

    let models = fetch_upstream_models(&state, protocol, base_url, secret).await?;
    record_provider_models(&state, provider, &models, "upstream", false).await?;
    Ok(Json(SetupFetchUpstreamModelsResponse { models }))
}

async fn setup_providers(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<Vec<ProviderRecord>>> {
    if current_service_policy(&state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    Ok(Json(list_providers(&state).await?))
}

async fn setup_sync_pricing_templates(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<SetupPricingTemplateSyncResponse>> {
    if current_service_policy(&state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }
    let result = sync_pricing_templates(
        &state,
        SyncPricingTemplatesRequest {
            source: "models_dev".to_string(),
        },
    )
    .await?;
    let templates = list_pricing_templates(&state).await?;
    Ok(Json(SetupPricingTemplateSyncResponse { result, templates }))
}

async fn setup_test_smtp_setting(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpsertSmtpSettingRequest>,
) -> AppResult<Json<TestSmtpSettingResponse>> {
    if current_service_policy(&state).await?.setup_completed {
        return Err(AppError::Conflict(
            "setup has already been completed".to_string(),
        ));
    }

    Ok(Json(test_smtp_setting(&state, req).await?))
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
    stored.credit_required = match stored.service_mode {
        ServiceMode::Internal => req.credit_required,
        ServiceMode::Paid => true,
    };
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
    Ok(Json(record))
}

async fn stored_policy_for_update(
    tx: &mut Transaction<'_, Postgres>,
) -> AppResult<StoredServicePolicy> {
    let Some(row) = sqlx::query("SELECT value FROM setting WHERE key = $1 FOR UPDATE")
        .bind(SERVICE_POLICY_SETTING_KEY)
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(default_stored_policy());
    };
    let value: serde_json::Value = row.try_get("value")?;
    Ok(normalize_stored_policy(serde_json::from_value(value)?))
}

async fn upsert_stored_policy(
    tx: &mut Transaction<'_, Postgres>,
    stored: StoredServicePolicy,
) -> AppResult<ServicePolicyRecord> {
    let stored = normalize_stored_policy(stored);
    let value = serde_json::to_value(&stored)?;
    let row = sqlx::query(
        "INSERT INTO setting (key, value)
         VALUES ($1, $2)
         ON CONFLICT (key)
         DO UPDATE SET value = EXCLUDED.value, updated_at = now()
         RETURNING value, updated_at",
    )
    .bind(SERVICE_POLICY_SETTING_KEY)
    .bind(value)
    .fetch_one(&mut **tx)
    .await?;
    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    Ok(record_from_stored(
        normalize_stored_policy(serde_json::from_value(value)?),
        Some(updated_at),
    ))
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
    let protocol = req.protocol.trim().to_string();
    let base_url = req.base_url.trim().to_string();
    let secret = req.secret.trim().to_string();
    let models: Vec<String> = req
        .models
        .into_iter()
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty())
        .collect();
    if provider.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    if name.is_empty() {
        return Err(AppError::BadRequest("channel name is required".to_string()));
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
    if models.is_empty() {
        return Err(AppError::BadRequest(
            "at least one upstream model is required".to_string(),
        ));
    }

    let channel = create_channel(
        state,
        CreateChannelRequest {
            provider,
            name: name.clone(),
            endpoints: vec![ChannelEndpointInput {
                protocol,
                base_url: Some(base_url),
                models,
                enabled: true,
            }],
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
    }
}

fn normalize_stored_policy(mut stored: StoredServicePolicy) -> StoredServicePolicy {
    if stored.service_mode == ServiceMode::Paid {
        stored.credit_required = true;
    }
    stored
}

fn record_from_stored(
    stored: StoredServicePolicy,
    updated_at: Option<DateTime<Utc>>,
) -> ServicePolicyRecord {
    let stored = normalize_stored_policy(stored);
    let probe = RuntimeProbe::from_env().ok();
    let runtime_mode = probe
        .as_ref()
        .map(|probe| probe.runtime_mode.as_str().to_string())
        .unwrap_or_else(|| "standalone".to_string());
    let redis_configured = probe
        .as_ref()
        .map(|probe| probe.redis_configured())
        .unwrap_or(false);
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
    use super::*;

    #[test]
    fn default_policy_is_unfinished_internal_without_credit_requirement() {
        let record = record_from_stored(default_stored_policy(), None);

        assert!(!record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Internal);
        assert!(!record.credit_required);
        assert!(!record.recharge_enabled);
    }

    #[test]
    fn paid_mode_always_requires_credit_and_enables_recharge() {
        let record = record_from_stored(
            StoredServicePolicy {
                setup_completed: true,
                service_mode: ServiceMode::Paid,
                credit_required: false,
            },
            None,
        );

        assert!(record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Paid);
        assert!(record.credit_required);
        assert!(record.recharge_enabled);
    }
}
