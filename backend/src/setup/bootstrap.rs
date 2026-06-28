use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, Row};
use tokio::sync::{watch, Mutex};
use uuid::Uuid;

use crate::{
    app::build_state,
    config::{
        Config, RuntimeProbe, ServiceMode, DEFAULT_ADMIN_TOKEN_SECRET, DEFAULT_UPSTREAM_SECRET_KEY,
    },
    error::{AppError, AppResult},
    policy::{
        self, CompleteSetupRequest, SetupFetchUpstreamModelsRequest,
        SetupFetchUpstreamModelsResponse, SetupPricingTemplateSyncResponse,
    },
    setup::install::inferred_public_base_url,
    AppState,
};

const SERVICE_POLICY_SETUP_COMPLETED_KEY: &str = "setup_completed";
const SITE_BRAND_SETTING_KEY: &str = "site_brand";

#[derive(Clone)]
pub struct BootstrapState {
    restart_required: Arc<AtomicBool>,
    restart_tx: watch::Sender<bool>,
    runtime_app: Arc<Mutex<Option<BootstrapRuntimeApp>>>,
}

#[derive(Clone)]
struct BootstrapRuntimeApp {
    state: Arc<AppState>,
    runtime_config: PreparedRuntimeConfig,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapBlockedReason {
    ClusterRequiresExternalConfig,
    MissingDatabase,
    MissingRedis,
}

#[derive(Debug, Serialize)]
pub struct SetupRuntimeStatus {
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
    pub bootstrap_blocked_reason: Option<BootstrapBlockedReason>,
    pub restart_required: bool,
    pub site_name: Option<String>,
    pub public_base_url: Option<String>,
    pub service_mode: String,
    pub credit_required: bool,
    pub registration_enabled: bool,
    pub recharge_enabled: bool,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct BootstrapConfigInput {
    pub database_url: Option<String>,
    pub site_name: Option<String>,
    pub public_base_url: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PreparedRuntimeConfig {
    env_file: PathBuf,
    database_url: String,
    site_name: String,
    public_base_url: String,
    admin_token_secret: Option<String>,
    upstream_secret_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TestDatabaseInput {
    pub database_url: String,
}

#[derive(Debug, Serialize)]
pub struct BootstrapConfigResult {
    pub ok: bool,
    pub env_file: String,
    pub restart_required: bool,
}

#[derive(Debug, Serialize)]
pub struct TestDatabaseResult {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct ClusterEnvTemplateResult {
    pub env_text: String,
    pub generated_admin_token_secret: Option<String>,
    pub generated_upstream_secret_key: Option<String>,
    pub required_restart: bool,
}

pub fn router(restart_tx: watch::Sender<bool>) -> Router {
    tracing::warn!("neogate bootstrap mode enabled; runtime configuration is incomplete");
    let state = Arc::new(BootstrapState {
        restart_required: Arc::new(AtomicBool::new(false)),
        restart_tx,
        runtime_app: Arc::new(Mutex::new(None)),
    });

    Router::new()
        .route("/healthz", get(bootstrap_liveness))
        .route("/readyz", get(bootstrap_readiness))
        .route("/api/setup/status", get(setup_status))
        .route(
            "/api/setup/bootstrap",
            axum::routing::post(write_bootstrap_config),
        )
        .route(
            "/api/setup/test-database",
            axum::routing::post(test_database_connection),
        )
        .route(
            "/api/setup/cluster-env-template",
            axum::routing::post(cluster_env_template),
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
        .with_state(state)
}

pub async fn bootstrap_liveness() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

pub async fn bootstrap_readiness() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::OK, Json(serde_json::json!({ "ok": true })))
}

pub async fn setup_status(
    State(state): State<Arc<BootstrapState>>,
    headers: HeaderMap,
) -> AppResult<Json<SetupRuntimeStatus>> {
    if let Some(runtime_app) = state.runtime_app.lock().await.as_ref().cloned() {
        return Ok(Json(
            runtime_status_from_prepared(&runtime_app.runtime_config).await?,
        ));
    }

    Ok(Json(
        runtime_status(
            state.restart_required.load(Ordering::SeqCst),
            inferred_public_base_url(&headers),
        )
        .await?,
    ))
}

async fn write_bootstrap_config(
    State(state): State<Arc<BootstrapState>>,
    Json(req): Json<BootstrapConfigInput>,
) -> AppResult<Json<BootstrapConfigResult>> {
    let runtime_config = prepare_runtime_config(req).await?;
    apply_runtime_env(&runtime_config);
    let config = Config::from_env()?;
    let app_state = build_state(config, state.restart_tx.clone()).await?;
    *state.runtime_app.lock().await = Some(BootstrapRuntimeApp {
        state: app_state,
        runtime_config: runtime_config.clone(),
    });

    Ok(Json(BootstrapConfigResult {
        ok: true,
        env_file: runtime_config.env_file.display().to_string(),
        restart_required: false,
    }))
}

pub async fn save_runtime_config(req: BootstrapConfigInput) -> AppResult<BootstrapConfigResult> {
    let runtime_config = prepare_runtime_config(req).await?;
    save_prepared_runtime_config(&runtime_config, None).await
}

pub async fn prepare_runtime_config(req: BootstrapConfigInput) -> AppResult<PreparedRuntimeConfig> {
    let probe = RuntimeProbe::from_env()?;
    if probe.runtime_mode.is_distributed() {
        return Err(AppError::BadRequest(
            "cluster mode requires external shared configuration".to_string(),
        ));
    }

    let database_url = optional_trimmed(req.database_url).or(probe.database_url.clone());
    let public_base_url = optional_trimmed(req.public_base_url).or(probe.public_base_url.clone());
    let site_name = optional_trimmed(req.site_name).or(probe.site_name.clone());

    let database_url =
        database_url.ok_or_else(|| AppError::BadRequest("DATABASE_URL is required".to_string()))?;
    let public_base_url = public_base_url
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    let site_name = site_name.unwrap_or_else(|| "NeoGate".to_string());
    validate_public_base_url(&public_base_url)?;
    test_database(&database_url).await?;

    let admin_token_secret = if configured_secret(
        probe.admin_token_secret.as_deref(),
        DEFAULT_ADMIN_TOKEN_SECRET,
    ) {
        probe.admin_token_secret.clone()
    } else {
        Some(generate_secret())
    };
    let upstream_secret_key = if configured_secret(
        probe.upstream_secret_key.as_deref(),
        DEFAULT_UPSTREAM_SECRET_KEY,
    ) {
        probe.upstream_secret_key.clone()
    } else {
        Some(generate_secret())
    };

    Ok(PreparedRuntimeConfig {
        env_file: probe.env_file,
        database_url,
        site_name,
        public_base_url,
        admin_token_secret,
        upstream_secret_key,
    })
}

pub async fn save_prepared_runtime_config(
    runtime_config: &PreparedRuntimeConfig,
    service_mode: Option<ServiceMode>,
) -> AppResult<BootstrapConfigResult> {
    if let Some(service_mode) = service_mode {
        validate_service_mode_config(service_mode)?;
    }

    upsert_env_file(
        &runtime_config.env_file,
        &runtime_config.env_values(service_mode),
    )?;
    save_site_brand_to_database(&runtime_config.database_url, &runtime_config.site_name).await?;
    Ok(BootstrapConfigResult {
        ok: true,
        env_file: runtime_config.env_file.display().to_string(),
        restart_required: true,
    })
}

impl PreparedRuntimeConfig {
    fn env_values(&self, service_mode: Option<ServiceMode>) -> Vec<(String, String)> {
        let mut values = vec![
            ("PUBLIC_BASE_URL".to_string(), self.public_base_url.clone()),
            ("DATABASE_URL".to_string(), self.database_url.clone()),
        ];
        if let Some(secret) = &self.admin_token_secret {
            values.push(("ADMIN_TOKEN_SECRET".to_string(), secret.clone()));
        }
        if let Some(secret) = &self.upstream_secret_key {
            values.push(("UPSTREAM_SECRET_KEY".to_string(), secret.clone()));
        }
        if let Some(service_mode) = service_mode {
            values.push((
                "SERVICE_MODE".to_string(),
                service_mode.as_str().to_string(),
            ));
        }
        values
    }
}

pub fn apply_runtime_env(runtime_config: &PreparedRuntimeConfig) {
    for (key, value) in runtime_config.env_values(None) {
        std::env::set_var(key, value);
    }
}

pub fn validate_service_mode_config(service_mode: ServiceMode) -> AppResult<()> {
    let probe = RuntimeProbe::from_env()?;
    if !probe.runtime_mode.is_distributed() {
        return Ok(());
    }
    if !probe.service_mode_configured() {
        return Err(AppError::BadRequest(
            "cluster mode requires SERVICE_MODE in external shared configuration".to_string(),
        ));
    }
    if probe.service_mode != Some(service_mode) {
        return Err(AppError::BadRequest(
            "selected service mode does not match SERVICE_MODE".to_string(),
        ));
    }
    Ok(())
}

pub fn apply_service_mode_env(service_mode: ServiceMode) {
    std::env::set_var("SERVICE_MODE", service_mode.as_str());
}

async fn save_site_brand_to_database(database_url: &str, site_name: &str) -> AppResult<()> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .map_err(database_connection_error)?;
    let value = serde_json::json!({
        "site_name": site_name,
        "logo_url": null,
    });
    sqlx::query(
        r#"
        INSERT INTO setting (key, value)
        VALUES ($1, $2)
        ON CONFLICT (key)
        DO UPDATE SET value = jsonb_set(
            setting.value,
            '{site_name}',
            to_jsonb($3::text),
            true
        ),
        updated_at = now()
        "#,
    )
    .bind(SITE_BRAND_SETTING_KEY)
    .bind(value)
    .bind(site_name)
    .execute(&pool)
    .await?;
    pool.close().await;
    Ok(())
}

pub async fn save_public_base_url_config(public_base_url: String) -> AppResult<()> {
    let probe = RuntimeProbe::from_env()?;
    if probe.runtime_mode.is_distributed() {
        return Err(AppError::BadRequest(
            "cluster mode requires external shared configuration".to_string(),
        ));
    }

    let public_base_url = optional_trimmed(Some(public_base_url))
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    validate_public_base_url(&public_base_url)?;
    std::env::set_var("PUBLIC_BASE_URL", &public_base_url);
    upsert_env_file(
        &probe.env_file,
        &[("PUBLIC_BASE_URL".to_string(), public_base_url)],
    )
}

pub async fn save_service_mode_config(service_mode: ServiceMode) -> AppResult<()> {
    validate_service_mode_config(service_mode)?;
    apply_service_mode_env(service_mode);

    let probe = RuntimeProbe::from_env()?;
    if probe.runtime_mode.is_distributed() {
        return Ok(());
    }
    upsert_env_file(
        &probe.env_file,
        &[(
            "SERVICE_MODE".to_string(),
            service_mode.as_str().to_string(),
        )],
    )
}

fn schedule_bootstrap_restart(restart_tx: watch::Sender<bool>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = restart_tx.send(true);
    });
}

async fn test_database_connection(
    Json(req): Json<TestDatabaseInput>,
) -> AppResult<Json<TestDatabaseResult>> {
    let database_url = req.database_url.trim();
    if database_url.is_empty() {
        return Err(AppError::BadRequest("DATABASE_URL is required".to_string()));
    }
    test_database(database_url).await?;
    Ok(Json(TestDatabaseResult { ok: true }))
}

async fn cluster_env_template() -> AppResult<Json<ClusterEnvTemplateResult>> {
    let probe = RuntimeProbe::from_env()?;
    let generated_admin_token_secret = if configured_secret(
        probe.admin_token_secret.as_deref(),
        DEFAULT_ADMIN_TOKEN_SECRET,
    ) {
        None
    } else {
        Some(generate_secret())
    };
    let generated_upstream_secret_key = if configured_secret(
        probe.upstream_secret_key.as_deref(),
        DEFAULT_UPSTREAM_SECRET_KEY,
    ) {
        None
    } else {
        Some(generate_secret())
    };

    let env_text = format!(
        "RUNTIME_MODE=distributed\nSERVICE_MODE={}\nDATABASE_URL={}\nREDIS_URL={}\nPUBLIC_BASE_URL={}\nADMIN_TOKEN_SECRET={}\nUPSTREAM_SECRET_KEY={}\n",
        probe.service_mode.unwrap_or(ServiceMode::Internal).as_str(),
        probe
            .database_url
            .unwrap_or_else(|| "postgres://user:password@postgres:5432/neogate".to_string()),
        probe
            .redis_url
            .unwrap_or_else(|| "redis://redis:6379/".to_string()),
        probe
            .public_base_url
            .unwrap_or_else(|| "https://neogate.example.com".to_string()),
        generated_admin_token_secret
            .clone()
            .or(probe.admin_token_secret)
            .unwrap_or_else(generate_secret),
        generated_upstream_secret_key
            .clone()
            .or(probe.upstream_secret_key)
            .unwrap_or_else(generate_secret),
    );

    Ok(Json(ClusterEnvTemplateResult {
        env_text,
        generated_admin_token_secret,
        generated_upstream_secret_key,
        required_restart: true,
    }))
}

async fn setup_providers(
    State(state): State<Arc<BootstrapState>>,
) -> AppResult<Json<Vec<policy::ProviderRecord>>> {
    let runtime_app = prepared_runtime_app(&state).await?;
    Ok(Json(
        policy::setup_providers_for_state(&runtime_app.state).await?,
    ))
}

async fn setup_upstream_models(
    State(state): State<Arc<BootstrapState>>,
    Json(req): Json<SetupFetchUpstreamModelsRequest>,
) -> AppResult<Json<SetupFetchUpstreamModelsResponse>> {
    let runtime_app = prepared_runtime_app(&state).await?;
    Ok(Json(
        policy::setup_upstream_models_for_state(&runtime_app.state, req).await?,
    ))
}

async fn setup_sync_pricing_templates(
    State(state): State<Arc<BootstrapState>>,
) -> AppResult<Json<SetupPricingTemplateSyncResponse>> {
    let runtime_app = prepared_runtime_app(&state).await?;
    Ok(Json(
        policy::setup_sync_pricing_templates_for_state(&runtime_app.state).await?,
    ))
}

async fn setup_test_smtp_setting(
    State(state): State<Arc<BootstrapState>>,
    Json(req): Json<crate::admin::setting::UpsertSmtpSettingRequest>,
) -> AppResult<Json<crate::admin::setting::TestSmtpSettingResponse>> {
    let runtime_app = prepared_runtime_app(&state).await?;
    Ok(Json(
        policy::setup_test_smtp_setting_for_state(&runtime_app.state, req).await?,
    ))
}

async fn complete_setup(
    State(state): State<Arc<BootstrapState>>,
    Json(req): Json<CompleteSetupRequest>,
) -> AppResult<Json<policy::ServicePolicyRecord>> {
    let service_mode = req.service_mode;
    let runtime_app = prepared_runtime_app(&state).await?;
    let record = policy::complete_setup_for_state(runtime_app.state.clone(), req).await?;
    let result = save_prepared_runtime_config(&runtime_app.runtime_config, Some(service_mode)).await?;
    state.restart_required.store(true, Ordering::SeqCst);
    schedule_bootstrap_restart(state.restart_tx.clone());
    tracing::info!(
        env_file = %result.env_file,
        "first-run runtime configuration written after setup completion"
    );
    Ok(Json(record))
}

async fn prepared_runtime_app(state: &BootstrapState) -> AppResult<BootstrapRuntimeApp> {
    state
        .runtime_app
        .lock()
        .await
        .as_ref()
        .cloned()
        .ok_or_else(|| {
            AppError::BadRequest("runtime configuration must be saved first".to_string())
        })
}

async fn runtime_status(
    restart_required: bool,
    inferred_public_base_url: Option<String>,
) -> AppResult<SetupRuntimeStatus> {
    let probe = RuntimeProbe::from_env()?;
    let database_connected = match &probe.database_url {
        Some(database_url) => test_database(database_url).await.is_ok(),
        None => false,
    };
    let setup_completed = if database_connected {
        setup_completed(probe.database_url.as_deref().expect("checked")).await
    } else {
        false
    };
    let redis_connected = if probe.runtime_mode.is_distributed() {
        Some(probe.redis_url.is_some())
    } else {
        None
    };
    let bootstrap_required = !probe.full_config_ready();
    let bootstrap_blocked_reason = if probe.runtime_mode.is_distributed() && bootstrap_required {
        Some(BootstrapBlockedReason::ClusterRequiresExternalConfig)
    } else if !probe.database_configured() {
        Some(BootstrapBlockedReason::MissingDatabase)
    } else if probe.runtime_mode.is_distributed() && !probe.redis_configured() {
        Some(BootstrapBlockedReason::MissingRedis)
    } else {
        None
    };

    Ok(SetupRuntimeStatus {
        runtime_mode: probe.runtime_mode.as_str().to_string(),
        env_write_supported: !probe.runtime_mode.is_distributed(),
        database_configured: probe.database_configured(),
        database_connected,
        redis_configured: probe.redis_configured(),
        redis_connected,
        secrets_configured: probe.secrets_configured(),
        site_configured: probe.site_configured(),
        setup_completed,
        bootstrap_required,
        bootstrap_blocked_reason,
        restart_required,
        site_name: probe.site_name.clone(),
        public_base_url: probe.public_base_url.clone().or(inferred_public_base_url),
        service_mode: probe
            .service_mode
            .unwrap_or(ServiceMode::Internal)
            .as_str()
            .to_string(),
        credit_required: false,
        registration_enabled: false,
        recharge_enabled: probe.service_mode == Some(ServiceMode::Paid),
        updated_at: None,
    })
}

async fn runtime_status_from_prepared(
    runtime_config: &PreparedRuntimeConfig,
) -> AppResult<SetupRuntimeStatus> {
    let probe = RuntimeProbe::from_env()?;
    let setup_completed = setup_completed(&runtime_config.database_url).await;
    Ok(SetupRuntimeStatus {
        runtime_mode: probe.runtime_mode.as_str().to_string(),
        env_write_supported: true,
        database_configured: true,
        database_connected: true,
        redis_configured: false,
        redis_connected: None,
        secrets_configured: true,
        site_configured: true,
        setup_completed,
        bootstrap_required: false,
        bootstrap_blocked_reason: None,
        restart_required: false,
        site_name: Some(runtime_config.site_name.clone()),
        public_base_url: Some(runtime_config.public_base_url.clone()),
        service_mode: probe
            .service_mode
            .unwrap_or(ServiceMode::Internal)
            .as_str()
            .to_string(),
        credit_required: false,
        registration_enabled: false,
        recharge_enabled: probe.service_mode == Some(ServiceMode::Paid),
        updated_at: None,
    })
}

pub async fn test_database(database_url: &str) -> AppResult<()> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
        .map_err(database_connection_error)?;
    sqlx::query("SELECT 1")
        .execute(&pool)
        .await
        .map_err(database_connection_error)?;
    pool.close().await;
    Ok(())
}

fn database_connection_error(err: sqlx::Error) -> AppError {
    let kind = database_error_kind(&err);
    tracing::warn!(
        error = %err,
        error_debug = ?err,
        code = kind.code(),
        "database connection test failed"
    );
    AppError::BadRequestWithCode {
        code: kind.code(),
        message: kind.message(),
    }
}

fn database_error_kind(err: &sqlx::Error) -> DatabaseErrorKind {
    match err {
        sqlx::Error::Database(database_error) => {
            let code = database_error.code().map(|code| code.to_string());
            let message = database_error.message().to_ascii_lowercase();

            match code.as_deref() {
                Some("28P01") => DatabaseErrorKind::PasswordInvalid,
                Some("3D000") => DatabaseErrorKind::DatabaseNotFound,
                Some("42501") => DatabaseErrorKind::PermissionDenied,
                Some("57P03") => DatabaseErrorKind::Unavailable,
                Some("28000") if message.contains("role") && message.contains("does not exist") => {
                    DatabaseErrorKind::UserNotFound
                }
                Some("28000") => DatabaseErrorKind::AuthenticationFailed,
                Some("08001") | Some("08003") | Some("08004") | Some("08006") | Some("08007") => {
                    DatabaseErrorKind::ConnectionFailed
                }
                _ if message.contains("role") && message.contains("does not exist") => {
                    DatabaseErrorKind::UserNotFound
                }
                _ if message.contains("database") && message.contains("does not exist") => {
                    DatabaseErrorKind::DatabaseNotFound
                }
                _ => DatabaseErrorKind::ConnectionFailed,
            }
        }
        sqlx::Error::PoolTimedOut => DatabaseErrorKind::ConnectionTimeout,
        sqlx::Error::Configuration(_) => DatabaseErrorKind::UrlInvalid,
        sqlx::Error::Io(_) => DatabaseErrorKind::NetworkError,
        sqlx::Error::Tls(_) => DatabaseErrorKind::TlsError,
        _ => DatabaseErrorKind::ConnectionFailed,
    }
}

#[derive(Clone, Copy, Debug)]
enum DatabaseErrorKind {
    PasswordInvalid,
    DatabaseNotFound,
    PermissionDenied,
    Unavailable,
    UserNotFound,
    AuthenticationFailed,
    ConnectionFailed,
    ConnectionTimeout,
    UrlInvalid,
    NetworkError,
    TlsError,
}

impl DatabaseErrorKind {
    fn code(self) -> &'static str {
        match self {
            Self::PasswordInvalid => "database_password_invalid",
            Self::DatabaseNotFound => "database_not_found",
            Self::PermissionDenied => "database_permission_denied",
            Self::Unavailable => "database_unavailable",
            Self::UserNotFound => "database_user_not_found",
            Self::AuthenticationFailed => "database_authentication_failed",
            Self::ConnectionFailed => "database_connection_failed",
            Self::ConnectionTimeout => "database_connection_timeout",
            Self::UrlInvalid => "database_url_invalid",
            Self::NetworkError => "database_network_error",
            Self::TlsError => "database_tls_error",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::PasswordInvalid => "database password is invalid",
            Self::DatabaseNotFound => "database does not exist",
            Self::PermissionDenied => "database user does not have enough permissions",
            Self::Unavailable => "database is temporarily unavailable",
            Self::UserNotFound => "database user does not exist",
            Self::AuthenticationFailed => "database authentication failed",
            Self::ConnectionFailed => "database connection failed",
            Self::ConnectionTimeout => "database connection timed out",
            Self::UrlInvalid => "database URL is invalid",
            Self::NetworkError => "database network connection failed",
            Self::TlsError => "database TLS handshake failed",
        }
    }
}

async fn setup_completed(database_url: &str) -> bool {
    let Ok(pool) = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
    else {
        return false;
    };
    let result = sqlx::query(
        "SELECT value
         FROM setting
         WHERE key = $1
         LIMIT 1",
    )
    .bind(SERVICE_POLICY_SETUP_COMPLETED_KEY)
    .fetch_optional(&pool)
    .await;
    pool.close().await;
    let Ok(Some(row)) = result else {
        return false;
    };
    let Ok(value) = row.try_get::<serde_json::Value, _>("value") else {
        return false;
    };
    value.as_bool().unwrap_or(false)
}

fn upsert_env_file(path: &Path, values: &[(String, String)]) -> AppResult<()> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let managed_keys = values
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<HashSet<_>>();
    let mut lines = values
        .iter()
        .map(|(key, value)| format!("{key}={}", quote_env_value(value)))
        .collect::<Vec<_>>();
    let mut other_lines = Vec::new();

    for line in existing.lines() {
        let Some((key, _)) = line.split_once('=') else {
            other_lines.push(line.to_string());
            continue;
        };
        let key = key.trim();
        if !managed_keys.contains(key) {
            other_lines.push(line.to_string());
        }
    }

    if !other_lines.is_empty() {
        lines.push(String::new());
        lines.extend(other_lines);
    }

    let mut body = lines.join("\n");
    body.push('\n');
    write_private(path, &body)?;
    Ok(())
}

fn write_private(path: &Path, body: &str) -> AppResult<()> {
    fs::write(path, body)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn quote_env_value(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@'))
    {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn validate_public_base_url(value: &str) -> AppResult<()> {
    let value = value.trim();
    if !(value.starts_with("http://") || value.starts_with("https://")) {
        return Err(AppError::BadRequest(
            "PUBLIC_BASE_URL must start with http:// or https://".to_string(),
        ));
    }
    Ok(())
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
}

fn configured_secret(value: Option<&str>, default: &str) -> bool {
    value
        .map(str::trim)
        .filter(|value| value.len() >= 32 && *value != default)
        .is_some()
}

fn generate_secret() -> String {
    format!(
        "{}{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}
