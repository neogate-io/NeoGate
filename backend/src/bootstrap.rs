use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{extract::State, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, Row};
use uuid::Uuid;

use crate::{
    config::{RuntimeProbe, DEFAULT_ADMIN_TOKEN_SECRET, DEFAULT_UPSTREAM_SECRET_KEY},
    error::{AppError, AppResult},
};

const SERVICE_POLICY_SETTING_KEY: &str = "service_policy";

#[derive(Clone)]
pub struct BootstrapState {
    setup_token: String,
    restart_required: Arc<AtomicBool>,
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
    pub recharge_enabled: bool,
    pub updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct BootstrapConfigInput {
    pub setup_token: String,
    pub database_url: Option<String>,
    pub site_name: Option<String>,
    pub public_base_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BootstrapConfigResult {
    pub ok: bool,
    pub env_file: String,
    pub restart_required: bool,
}

#[derive(Debug, Serialize)]
pub struct ClusterEnvTemplateResult {
    pub env_text: String,
    pub generated_admin_token_secret: Option<String>,
    pub generated_upstream_secret_key: Option<String>,
    pub required_restart: bool,
}

pub fn router() -> Router {
    let setup_token = generate_secret();
    tracing::warn!(
        "neogate bootstrap mode enabled; use setup token {} to complete first-run configuration",
        setup_token
    );
    let state = Arc::new(BootstrapState {
        setup_token,
        restart_required: Arc::new(AtomicBool::new(false)),
    });

    Router::new()
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup/bootstrap", axum::routing::post(write_bootstrap_config))
        .route(
            "/api/setup/cluster-env-template",
            axum::routing::post(cluster_env_template),
        )
        .with_state(state)
}

pub async fn setup_status(
    State(state): State<Arc<BootstrapState>>,
) -> AppResult<Json<SetupRuntimeStatus>> {
    Ok(Json(runtime_status(state.restart_required.load(Ordering::SeqCst)).await?))
}

async fn write_bootstrap_config(
    State(state): State<Arc<BootstrapState>>,
    Json(req): Json<BootstrapConfigInput>,
) -> AppResult<Json<BootstrapConfigResult>> {
    ensure_setup_token(&state, &req.setup_token)?;
    let probe = RuntimeProbe::from_env()?;
    if probe.runtime_mode.is_distributed() {
        return Err(AppError::BadRequest(
            "cluster mode requires external shared configuration".to_string(),
        ));
    }

    let database_url = optional_trimmed(req.database_url).or(probe.database_url.clone());
    let public_base_url = optional_trimmed(req.public_base_url).or(probe.public_base_url.clone());
    let site_name = optional_trimmed(req.site_name).or(probe.site_name.clone());

    let database_url = database_url
        .ok_or_else(|| AppError::BadRequest("DATABASE_URL is required".to_string()))?;
    let public_base_url = public_base_url
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    let site_name =
        site_name.ok_or_else(|| AppError::BadRequest("SITE_NAME is required".to_string()))?;
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

    let mut values = BTreeMap::new();
    values.insert("DATABASE_URL".to_string(), database_url);
    values.insert("PUBLIC_BASE_URL".to_string(), public_base_url);
    values.insert("SITE_NAME".to_string(), site_name);
    if let Some(secret) = admin_token_secret {
        values.insert("ADMIN_TOKEN_SECRET".to_string(), secret);
    }
    if let Some(secret) = upstream_secret_key {
        values.insert("UPSTREAM_SECRET_KEY".to_string(), secret);
    }

    upsert_env_file(&probe.env_file, &values)?;
    state.restart_required.store(true, Ordering::SeqCst);
    Ok(Json(BootstrapConfigResult {
        ok: true,
        env_file: probe.env_file.display().to_string(),
        restart_required: true,
    }))
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
        "RUNTIME_MODE=distributed\nDATABASE_URL={}\nREDIS_URL={}\nPUBLIC_BASE_URL={}\nSITE_NAME={}\nADMIN_TOKEN_SECRET={}\nUPSTREAM_SECRET_KEY={}\n",
        probe.database_url.unwrap_or_else(|| "postgres://user:password@postgres:5432/neogate".to_string()),
        probe.redis_url.unwrap_or_else(|| "redis://redis:6379/".to_string()),
        probe.public_base_url.unwrap_or_else(|| "https://neogate.example.com".to_string()),
        probe.site_name.unwrap_or_else(|| "NeoGate".to_string()),
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

async fn runtime_status(restart_required: bool) -> AppResult<SetupRuntimeStatus> {
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
        public_base_url: probe.public_base_url.clone(),
        service_mode: "internal".to_string(),
        credit_required: false,
        recharge_enabled: false,
        updated_at: None,
    })
}

async fn test_database(database_url: &str) -> AppResult<()> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;
    sqlx::query("SELECT 1").execute(&pool).await?;
    pool.close().await;
    Ok(())
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
    let result = sqlx::query("SELECT value FROM setting WHERE key = $1")
        .bind(SERVICE_POLICY_SETTING_KEY)
        .fetch_optional(&pool)
        .await;
    pool.close().await;
    let Ok(Some(row)) = result else {
        return false;
    };
    let Ok(value) = row.try_get::<serde_json::Value, _>("value") else {
        return false;
    };
    value
        .get("setup_completed")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

fn ensure_setup_token(state: &BootstrapState, token: &str) -> AppResult<()> {
    if token.trim().is_empty() || token.trim() != state.setup_token {
        return Err(AppError::Forbidden);
    }
    Ok(())
}

fn upsert_env_file(path: &Path, values: &BTreeMap<String, String>) -> AppResult<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let mut seen = HashSet::new();
    let mut lines = Vec::new();

    for line in existing.lines() {
        let Some((key, _)) = line.split_once('=') else {
            lines.push(line.to_string());
            continue;
        };
        let key = key.trim();
        if let Some(value) = values.get(key) {
            lines.push(format!("{key}={}", quote_env_value(value)));
            seen.insert(key.to_string());
        } else {
            lines.push(line.to_string());
        }
    }

    for (key, value) in values {
        if !seen.contains(key) {
            lines.push(format!("{key}={}", quote_env_value(value)));
        }
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

fn validate_public_base_url(value: &str) -> AppResult<()> {
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
