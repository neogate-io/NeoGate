use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use axum::{extract::State, http::HeaderMap, routing::get, Json, Router};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, Row};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
    config::{RuntimeProbe, DEFAULT_ADMIN_TOKEN_SECRET, DEFAULT_UPSTREAM_SECRET_KEY},
    error::{AppError, AppResult},
    setup::install::inferred_public_base_url,
};

const SERVICE_POLICY_SETTING_KEY: &str = "service_policy";

#[derive(Clone)]
pub struct BootstrapState {
    restart_required: Arc<AtomicBool>,
    restart_tx: watch::Sender<bool>,
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

#[derive(Debug, Deserialize)]
pub struct BootstrapConfigInput {
    pub database_url: Option<String>,
    pub site_name: Option<String>,
    pub public_base_url: Option<String>,
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
    });

    Router::new()
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
        .with_state(state)
}

pub async fn setup_status(
    State(state): State<Arc<BootstrapState>>,
    headers: HeaderMap,
) -> AppResult<Json<SetupRuntimeStatus>> {
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
    let result = save_runtime_config(req).await?;
    state.restart_required.store(true, Ordering::SeqCst);
    schedule_bootstrap_restart(state.restart_tx.clone());
    Ok(Json(result))
}

pub async fn save_runtime_config(req: BootstrapConfigInput) -> AppResult<BootstrapConfigResult> {
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

    let mut values = vec![
        ("SITE_NAME".to_string(), site_name),
        ("PUBLIC_BASE_URL".to_string(), public_base_url),
        ("DATABASE_URL".to_string(), database_url),
    ];
    if let Some(secret) = admin_token_secret {
        values.push(("ADMIN_TOKEN_SECRET".to_string(), secret));
    }
    if let Some(secret) = upstream_secret_key {
        values.push(("UPSTREAM_SECRET_KEY".to_string(), secret));
    }

    upsert_env_file(&probe.env_file, &values)?;
    Ok(BootstrapConfigResult {
        ok: true,
        env_file: probe.env_file.display().to_string(),
        restart_required: true,
    })
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
        "RUNTIME_MODE=distributed\nDATABASE_URL={}\nREDIS_URL={}\nPUBLIC_BASE_URL={}\nSITE_NAME={}\nADMIN_TOKEN_SECRET={}\nUPSTREAM_SECRET_KEY={}\n",
        probe
            .database_url
            .unwrap_or_else(|| "postgres://user:password@postgres:5432/neogate".to_string()),
        probe
            .redis_url
            .unwrap_or_else(|| "redis://redis:6379/".to_string()),
        probe
            .public_base_url
            .unwrap_or_else(|| "https://neogate.example.com".to_string()),
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
        service_mode: "internal".to_string(),
        credit_required: false,
        registration_enabled: false,
        recharge_enabled: false,
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
    let message = friendly_database_error(&err);
    tracing::warn!(
        error = %err,
        error_debug = ?err,
        "database connection test failed"
    );
    AppError::BadRequest(message)
}

fn friendly_database_error(err: &sqlx::Error) -> String {
    let fallback = "数据库连接失败。请检查主机、端口、数据库名称、用户、密码和 SSL 设置。";

    match err {
        sqlx::Error::Database(database_error) => {
            let code = database_error.code().map(|code| code.to_string());
            let raw_message = database_error.message();
            let message = raw_message.to_ascii_lowercase();

            let hint = match code.as_deref() {
                Some("28P01") => "数据库密码不正确。请检查数据库密码。",
                Some("3D000") => "数据库不存在。请检查数据库名称，或先创建该数据库。",
                Some("42501") => "数据库用户权限不足。请为该用户授予连接和建表所需权限。",
                Some("57P03") => "数据库暂时不可用。请稍后重试，或检查数据库是否正在启动。",
                Some("28000") if message.contains("role") && message.contains("does not exist") => {
                    "数据库用户不存在。请检查数据库用户是否已创建。"
                }
                Some("28000") => "数据库用户认证失败。请检查数据库用户和密码。",
                Some("08001") | Some("08003") | Some("08004") | Some("08006") | Some("08007") => {
                    "无法连接到数据库。请检查主机、端口、防火墙和数据库监听地址。"
                }
                _ if message.contains("role") && message.contains("does not exist") => {
                    "数据库用户不存在。请检查数据库用户是否已创建。"
                }
                _ if message.contains("database") && message.contains("does not exist") => {
                    "数据库不存在。请检查数据库名称，或先创建该数据库。"
                }
                _ => fallback,
            };

            format!("{hint}（数据库返回：{raw_message}）")
        }
        sqlx::Error::PoolTimedOut => "数据库连接超时。请检查主机、端口和网络连通性。".to_string(),
        sqlx::Error::Configuration(err) => {
            format!("数据库连接地址格式不正确。请检查填写内容。（{err}）")
        }
        sqlx::Error::Io(err) => {
            format!("无法连接到数据库网络地址。请检查主机、端口和网络连通性。（{err}）")
        }
        sqlx::Error::Tls(err) => {
            format!("数据库 SSL/TLS 握手失败。请检查 SSL 模式和证书配置。（{err}）")
        }
        _ => fallback.to_string(),
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
