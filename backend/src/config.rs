use std::{env, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_RELAY_BODY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
pub const RELAY_USAGE_BUFFER_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const CREDENTIAL_UPLOAD_LIMIT_BYTES: usize = 10 * 1024 * 1024;
pub const DEFAULT_ADMIN_TOKEN_SECRET: &str = "change-me-admin-token-secret-in-production";
pub const DEFAULT_UPSTREAM_SECRET_KEY: &str = "change-me-upstream-secret-key-in-production";
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(600);
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
pub const STREAM_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum BillingCurrency {
    Usd,
    Cny,
}

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub public_base_url: Option<String>,
    pub site_name: String,
    pub billing_currency: BillingCurrency,
    pub runtime_mode: RuntimeMode,
    pub process_role: ProcessRole,
    pub admin_token_secret: String,
    pub admin_session_ttl: Duration,
    pub upstream_secret_key: String,
    pub http: HttpClientConfig,
    pub relay: RelayConfig,
    pub cache: CacheConfig,
    pub redis_url: Option<String>,
    pub redis_key_prefix: String,
    pub billing: BillingConfig,
    pub usage_queue: UsageQueueConfig,
    pub health: HealthConfig,
    pub task: TaskConfig,
    pub response_assets: ResponseAssetConfig,
    pub db_pool: DbPoolConfig,
    pub cors_allowed_origins: Vec<String>,
    pub trust_proxy_headers: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    Standalone,
    Distributed,
}

#[derive(Clone, Debug)]
pub struct RuntimeProbe {
    pub runtime_mode: RuntimeMode,
    pub service_mode: Option<ServiceMode>,
    pub bind_addr: SocketAddr,
    pub database_url: Option<String>,
    pub redis_url: Option<String>,
    pub public_base_url: Option<String>,
    pub site_name: Option<String>,
    pub billing_currency: Option<BillingCurrency>,
    pub admin_token_secret: Option<String>,
    pub upstream_secret_key: Option<String>,
    pub env_file: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessRole {
    All,
    Api,
    Worker,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Internal,
    Paid,
}

impl ProcessRole {
    pub fn from_env_value(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "all" => Ok(Self::All),
            "api" => Ok(Self::Api),
            "worker" => Ok(Self::Worker),
            _ => anyhow::bail!("PROCESS_ROLE must be all, api, or worker"),
        }
    }

    pub fn runs_api(self) -> bool {
        matches!(self, Self::All | Self::Api)
    }

    pub fn runs_background(self) -> bool {
        matches!(self, Self::All | Self::Worker)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Api => "api",
            Self::Worker => "worker",
        }
    }
}

impl RuntimeMode {
    pub fn from_env_value(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "standalone" => Ok(Self::Standalone),
            "distributed" => Ok(Self::Distributed),
            _ => anyhow::bail!("RUNTIME_MODE must be standalone or distributed"),
        }
    }

    pub fn is_distributed(self) -> bool {
        matches!(self, Self::Distributed)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Distributed => "distributed",
        }
    }
}

impl ServiceMode {
    pub fn from_env_value(value: &str) -> Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "internal" => Ok(Self::Internal),
            "paid" => Ok(Self::Paid),
            _ => anyhow::bail!("SERVICE_MODE must be internal or paid"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Internal => "internal",
            Self::Paid => "paid",
        }
    }
}

impl BillingCurrency {
    pub fn from_env_value(value: &str) -> Result<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "USD" => Ok(Self::Usd),
            "CNY" => Ok(Self::Cny),
            _ => anyhow::bail!("BILLING_CURRENCY must be USD or CNY"),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Cny => "CNY",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DbPoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct HttpClientConfig {
    pub upstream_connect_timeout: Duration,
    pub pool_max_idle_per_host: usize,
    pub pool_idle_timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct RelayConfig {
    pub key_cooldown: Duration,
    pub quota_exhausted_cooldown: Duration,
    pub max_upstream_failovers: usize,
    pub body_limit_bytes: usize,
    pub usage_buffer_limit_bytes: usize,
    pub credential_upload_limit_bytes: usize,
    pub user_concurrent_request_limit: usize,
    pub global_concurrent_request_limit: usize,
    pub channel_affinity_enabled: bool,
    pub channel_affinity_ttl: Duration,
    pub channel_affinity_max_entries: usize,
    /// 运行时学习到「某 endpoint 的某 model 不支持 /v1/responses」后的屏蔽时长（秒），
    /// 到期后重新尝试原生 responses（上游将来放开后自动恢复）。默认 12 小时。
    pub responses_support_block_seconds: i64,
}

#[derive(Clone, Debug)]
pub struct CacheConfig {
    pub user_auth_ttl: Duration,
    pub user_auth_max_entries: usize,
    pub routing_ttl: Duration,
    pub price_ttl: Duration,
    pub price_max_entries: usize,
    pub secret_max_entries: usize,
}

#[derive(Clone, Debug)]
pub struct BillingConfig {
    pub credit_prefetch_micros: i64,
    pub credit_allocation_recovery_after: Duration,
    pub credit_allocation_recovery_interval: Duration,
    pub default_output_tokens: i64,
}

#[derive(Clone, Debug)]
pub struct UsageQueueConfig {
    pub flush_interval: Duration,
    pub size: usize,
}

#[derive(Clone, Debug)]
pub struct HealthConfig {
    pub billing_outbox_max_pending: i64,
    pub billing_outbox_max_age: Duration,
}

#[derive(Clone, Debug)]
pub struct TaskConfig {
    pub upstream_poll_batch_size: i64,
    pub upstream_retention: Duration,
}

#[derive(Clone, Debug)]
pub struct ResponseAssetConfig {
    pub dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let runtime_mode = RuntimeMode::from_env_value(
            &env::var("RUNTIME_MODE").unwrap_or_else(|_| "standalone".to_string()),
        )?;
        let process_role = ProcessRole::from_env_value(
            &env::var("PROCESS_ROLE").unwrap_or_else(|_| "all".to_string()),
        )?;
        let config = Self {
            database_url: required("DATABASE_URL")?,
            bind_addr: env::var("BIND_ADDR")
                .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
                .parse()
                .context("BIND_ADDR must be host:port")?,
            public_base_url: optional("PUBLIC_BASE_URL")
                .map(normalize_public_base_url)
                .transpose()?,
            site_name: env::var("SITE_NAME")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| "NeoGate".to_string()),
            billing_currency: env::var("BILLING_CURRENCY")
                .ok()
                .as_deref()
                .map(BillingCurrency::from_env_value)
                .transpose()?
                .unwrap_or(BillingCurrency::Cny),
            runtime_mode,
            process_role,
            admin_token_secret: env::var("ADMIN_TOKEN_SECRET")
                .unwrap_or_else(|_| DEFAULT_ADMIN_TOKEN_SECRET.to_string()),
            admin_session_ttl: Duration::from_secs(86_400),
            upstream_secret_key: env::var("UPSTREAM_SECRET_KEY")
                .unwrap_or_else(|_| DEFAULT_UPSTREAM_SECRET_KEY.to_string()),
            http: HttpClientConfig {
                upstream_connect_timeout: Duration::from_secs(10),
                pool_max_idle_per_host: 100,
                pool_idle_timeout: Duration::from_secs(90),
            },
            relay: RelayConfig {
                key_cooldown: Duration::from_secs(60),
                quota_exhausted_cooldown: Duration::from_secs(parse_u64(
                    "QUOTA_COOLDOWN_SECONDS",
                    10 * 60,
                )?),
                max_upstream_failovers: 5,
                body_limit_bytes: parse_usize(
                    "RELAY_BODY_LIMIT_BYTES",
                    DEFAULT_RELAY_BODY_LIMIT_BYTES,
                )?,
                usage_buffer_limit_bytes: RELAY_USAGE_BUFFER_LIMIT_BYTES,
                credential_upload_limit_bytes: CREDENTIAL_UPLOAD_LIMIT_BYTES,
                user_concurrent_request_limit: parse_usize("USER_CONCURRENT_REQUEST_LIMIT", 100)?,
                global_concurrent_request_limit: parse_usize("GLOBAL_CONCURRENT_REQUEST_LIMIT", 0)?,
                channel_affinity_enabled: true,
                channel_affinity_ttl: Duration::from_secs(3600),
                channel_affinity_max_entries: 100_000,
                responses_support_block_seconds: 12 * 3600,
            },
            cache: CacheConfig {
                user_auth_ttl: Duration::from_secs(60),
                user_auth_max_entries: 100_000,
                routing_ttl: Duration::from_secs(30),
                price_ttl: Duration::from_secs(300),
                price_max_entries: 10_000,
                secret_max_entries: 4096,
            },
            redis_url: optional("REDIS_URL"),
            redis_key_prefix: "neogate".to_string(),
            billing: BillingConfig {
                credit_prefetch_micros: 100_000,
                credit_allocation_recovery_after: Duration::from_secs(300),
                credit_allocation_recovery_interval: Duration::from_secs(30),
                default_output_tokens: 16_384,
            },
            usage_queue: UsageQueueConfig {
                flush_interval: Duration::from_millis(parse_u64("USAGE_FLUSH_INTERVAL_MS", 1000)?),
                size: parse_usize("USAGE_QUEUE_SIZE", 8192)?,
            },
            health: HealthConfig {
                billing_outbox_max_pending: 10_000,
                billing_outbox_max_age: Duration::from_secs(300),
            },
            task: TaskConfig {
                upstream_poll_batch_size: 100,
                upstream_retention: Duration::from_secs(2_592_000),
            },
            response_assets: ResponseAssetConfig {
                dir: env::var("NEOGATE_ASSET_DIR")
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .map_or_else(default_response_asset_dir, PathBuf::from),
            },
            db_pool: DbPoolConfig::from_env()?,
            cors_allowed_origins: parse_csv("CORS_ALLOWED_ORIGINS", ""),
            trust_proxy_headers: parse_bool("TRUST_PROXY_HEADERS", true)?,
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.db_pool.min_connections > self.db_pool.max_connections {
            anyhow::bail!("DB_POOL_MAX_CONNECTIONS must be positive");
        }
        if self.runtime_mode.is_distributed() && self.redis_url.is_none() {
            anyhow::bail!("REDIS_URL is required when RUNTIME_MODE=distributed");
        }
        if self.relay.body_limit_bytes == 0 {
            anyhow::bail!("RELAY_BODY_LIMIT_BYTES must be positive");
        }
        if self.relay.user_concurrent_request_limit == 0 {
            anyhow::bail!("USER_CONCURRENT_REQUEST_LIMIT must be positive");
        }
        if self.usage_queue.size > 1_000_000 {
            anyhow::bail!("USAGE_QUEUE_SIZE must be <= 1000000");
        }
        if self.cors_allowed_origins.iter().any(|origin| origin == "*") {
            tracing::warn!(
                "CORS_ALLOWED_ORIGINS=* allows browser requests from any origin; set explicit origins for admin/user deployments"
            );
        }

        Ok(())
    }
}

fn default_response_asset_dir() -> PathBuf {
    env::temp_dir().join("neogate/assets")
}

impl RuntimeProbe {
    pub fn from_env() -> Result<Self> {
        let runtime_mode = RuntimeMode::from_env_value(
            &env::var("RUNTIME_MODE").unwrap_or_else(|_| "standalone".to_string()),
        )?;
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
            .parse()
            .context("BIND_ADDR must be host:port")?;
        let public_base_url = optional("PUBLIC_BASE_URL")
            .map(normalize_public_base_url)
            .transpose()?;
        let env_file = env::var("NEOGATE_ENV_FILE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map_or_else(default_env_file, PathBuf::from);
        let service_mode = service_mode_from_env_file(&env_file)?;

        Ok(Self {
            runtime_mode,
            service_mode,
            bind_addr,
            database_url: optional("DATABASE_URL"),
            redis_url: optional("REDIS_URL"),
            public_base_url,
            site_name: optional("SITE_NAME").map(|value| value.trim().to_string()),
            billing_currency: optional("BILLING_CURRENCY")
                .as_deref()
                .map(BillingCurrency::from_env_value)
                .transpose()?,
            admin_token_secret: optional("ADMIN_TOKEN_SECRET"),
            upstream_secret_key: optional("UPSTREAM_SECRET_KEY"),
            env_file,
        })
    }

    pub fn database_configured(&self) -> bool {
        self.database_url.is_some()
    }

    pub fn redis_configured(&self) -> bool {
        self.redis_url.is_some()
    }

    pub fn site_configured(&self) -> bool {
        self.public_base_url.is_some()
    }

    pub fn secrets_configured(&self) -> bool {
        configured_secret(
            self.admin_token_secret.as_deref(),
            DEFAULT_ADMIN_TOKEN_SECRET,
        ) && configured_secret(
            self.upstream_secret_key.as_deref(),
            DEFAULT_UPSTREAM_SECRET_KEY,
        )
    }

    pub fn full_config_ready(&self) -> bool {
        self.database_configured()
            && self.site_configured()
            && self.secrets_configured()
            && (!self.runtime_mode.is_distributed() || self.redis_configured())
    }

    pub fn service_mode_configured(&self) -> bool {
        self.service_mode.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct PaymentConfig {
    pub enabled_providers: Vec<PaymentProvider>,
    pub zpay: ZpayConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaymentProvider {
    Zpay,
}

#[derive(Clone, Debug)]
pub struct ZpayConfig {
    pub api_url: Option<String>,
    pub merchant_id: Option<String>,
    pub secret_key: Option<String>,
    pub default_pay_type: String,
    pub site_name: String,
}

impl Default for PaymentConfig {
    fn default() -> Self {
        Self {
            enabled_providers: Vec::new(),
            zpay: ZpayConfig {
                api_url: Some("https://zpayz.cn/submit.php".to_string()),
                merchant_id: None,
                secret_key: None,
                default_pay_type: "wxpay".to_string(),
                site_name: "NeoGate".to_string(),
            },
        }
    }
}

impl PaymentConfig {
    pub fn default_for_site(site_name: &str) -> Self {
        let mut config = Self::default();
        config.zpay.site_name = site_name.to_string();
        config
    }

    pub fn provider_enabled(&self, provider: PaymentProvider) -> bool {
        self.enabled_providers.contains(&provider)
    }
}

impl PaymentProvider {
    pub fn from_code(value: &str) -> crate::error::AppResult<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "zpay" => Ok(Self::Zpay),
            other => Err(crate::error::AppError::BadRequest(format!(
                "unsupported payment provider: {other}"
            ))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Zpay => "zpay",
        }
    }
}

impl DbPoolConfig {
    fn from_env() -> Result<Self> {
        Ok(Self {
            min_connections: 1,
            max_connections: parse_u32("DB_POOL_MAX_CONNECTIONS", 10)?,
            acquire_timeout: Duration::from_secs(5),
        })
    }
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} is required"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn optional_from_env_file(name: &str, path: &std::path::Path) -> Option<String> {
    let iter = dotenvy::from_path_iter(path).ok()?;
    iter.flatten()
        .find_map(|(key, value)| (key == name && !value.trim().is_empty()).then_some(value))
}

fn service_mode_from_env_file(path: &std::path::Path) -> Result<Option<ServiceMode>> {
    optional("SERVICE_MODE")
        .or_else(|| optional_from_env_file("SERVICE_MODE", path))
        .map(|value| ServiceMode::from_env_value(&value))
        .transpose()
}

fn parse_u64(name: &str, default: u64) -> Result<u64> {
    parse_env(name, default)
}

fn parse_u32(name: &str, default: u32) -> Result<u32> {
    parse_env(name, default)
}

fn parse_usize(name: &str, default: usize) -> Result<usize> {
    parse_env(name, default)
}

fn parse_bool(name: &str, default: bool) -> Result<bool> {
    let Some(value) = env::var(name)
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
    else {
        return Ok(default);
    };
    match value.as_str() {
        "1" | "true" | "yes" | "y" | "on" => Ok(true),
        "0" | "false" | "no" | "n" | "off" => Ok(false),
        _ => anyhow::bail!("{name} must be a boolean"),
    }
}

fn parse_env<T>(name: &str, default: T) -> Result<T>
where
    T: std::str::FromStr + Copy,
    T::Err: std::fmt::Display,
{
    Ok(parse_env_optional(name)?.unwrap_or(default))
}

fn parse_env_optional<T>(name: &str) -> Result<Option<T>>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let Some(value) = env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    value
        .parse()
        .map(Some)
        .map_err(|err| anyhow::anyhow!("{name} must be a valid number: {err}"))
}

fn parse_csv(name: &str, default: &str) -> Vec<String> {
    env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn normalize_public_base_url(value: String) -> Result<String> {
    let value = value.trim().trim_end_matches('/').to_string();
    if value.is_empty() {
        anyhow::bail!("PUBLIC_BASE_URL must not be empty");
    }
    if !(value.starts_with("https://") || value.starts_with("http://")) {
        anyhow::bail!("PUBLIC_BASE_URL must start with http:// or https://");
    }
    Ok(value)
}

fn configured_secret(value: Option<&str>, default: &str) -> bool {
    value
        .map(str::trim)
        .filter(|value| value.len() >= 32 && *value != default)
        .is_some()
}

fn default_env_file() -> PathBuf {
    PathBuf::from(".env")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_mode_accepts_known_values_case_insensitively() {
        assert_eq!(
            RuntimeMode::from_env_value("standalone").unwrap(),
            RuntimeMode::Standalone
        );
        assert_eq!(
            RuntimeMode::from_env_value("DISTRIBUTED").unwrap(),
            RuntimeMode::Distributed
        );
        assert!(RuntimeMode::from_env_value("redis").is_err());
    }

    #[test]
    fn process_role_accepts_known_values_case_insensitively() {
        assert_eq!(
            ProcessRole::from_env_value("all").unwrap(),
            ProcessRole::All
        );
        assert_eq!(
            ProcessRole::from_env_value("API").unwrap(),
            ProcessRole::Api
        );
        assert_eq!(
            ProcessRole::from_env_value("worker").unwrap(),
            ProcessRole::Worker
        );
        assert!(ProcessRole::from_env_value("scheduler").is_err());
    }

    #[test]
    fn service_mode_accepts_known_values_case_insensitively() {
        assert_eq!(
            ServiceMode::from_env_value("internal").unwrap(),
            ServiceMode::Internal
        );
        assert_eq!(
            ServiceMode::from_env_value("PAID").unwrap(),
            ServiceMode::Paid
        );
        assert!(ServiceMode::from_env_value("billing").is_err());
    }
}
