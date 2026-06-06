use std::{env, net::SocketAddr, time::Duration};

use anyhow::{Context, Result};

pub const DEFAULT_RELAY_BODY_LIMIT_BYTES: usize = 16 * 1024 * 1024;
pub const DEFAULT_CREDENTIAL_UPLOAD_LIMIT_BYTES: usize = 10 * 1024 * 1024;
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub public_base_url: Option<String>,
    pub production: bool,
    pub runtime_mode: RuntimeMode,
    pub process_role: ProcessRole,
    pub admin_token_secret: String,
    pub admin_session_ttl: Duration,
    pub upstream_secret_key: String,
    pub anthropic_version: String,
    pub key_cooldown: Duration,
    pub request_timeout: Duration,
    pub upstream_connect_timeout: Duration,
    pub upstream_response_timeout: Duration,
    pub relay_body_limit_bytes: usize,
    pub credential_upload_limit_bytes: usize,
    pub http_pool_max_idle_per_host: usize,
    pub http_pool_idle_timeout: Duration,
    pub user_auth_cache_ttl: Duration,
    pub user_auth_cache_max_entries: usize,
    pub routing_cache_ttl: Duration,
    pub price_cache_ttl: Duration,
    pub price_cache_max_entries: usize,
    pub secret_cache_max_entries: usize,
    pub redis_url: Option<String>,
    pub redis_key_prefix: String,
    pub credit_prefetch_micro_usd: i64,
    pub credit_allocation_recovery_after: Duration,
    pub credit_allocation_recovery_interval: Duration,
    pub default_output_tokens: i64,
    pub usage_flush_interval: Duration,
    pub usage_queue_size: usize,
    pub billing_outbox_max_pending: i64,
    pub billing_outbox_max_age: Duration,
    pub task_upstream_poll_interval: Duration,
    pub task_upstream_poll_batch_size: i64,
    pub task_upstream_retention: Duration,
    pub task_upstream_stale_hold_release: Duration,
    pub payment: PaymentConfig,
    pub db_pool: DbPoolConfig,
    pub cors_allowed_origins: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeMode {
    Standalone,
    Distributed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessRole {
    All,
    Api,
    Worker,
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

#[derive(Clone, Debug)]
pub struct DbPoolConfig {
    pub min_connections: u32,
    pub max_connections: u32,
    pub acquire_timeout: Duration,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let production = env::var("APP_ENV")
            .ok()
            .map(|value| value.eq_ignore_ascii_case("production"))
            .unwrap_or(false);
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
            production,
            runtime_mode,
            process_role,
            admin_token_secret: env::var("ADMIN_TOKEN_SECRET")
                .unwrap_or_else(|_| "change-me-admin-token-secret-in-production".to_string()),
            admin_session_ttl: Duration::from_secs(parse_u64("ADMIN_SESSION_TTL_SECONDS", 86400)),
            upstream_secret_key: env::var("UPSTREAM_SECRET_KEY")
                .unwrap_or_else(|_| "change-me-upstream-secret-key-in-production".to_string()),
            anthropic_version: DEFAULT_ANTHROPIC_VERSION.to_string(),
            key_cooldown: Duration::from_secs(parse_u64("KEY_COOLDOWN_SECONDS", 60)),
            request_timeout: Duration::from_secs(parse_u64("REQUEST_TIMEOUT_SECONDS", 120)),
            upstream_connect_timeout: Duration::from_secs(parse_u64(
                "UPSTREAM_CONNECT_TIMEOUT_SECONDS",
                10,
            )),
            upstream_response_timeout: Duration::from_secs(parse_u64(
                "UPSTREAM_RESPONSE_TIMEOUT_SECONDS",
                30,
            )),
            relay_body_limit_bytes: parse_usize(
                "RELAY_BODY_LIMIT_BYTES",
                DEFAULT_RELAY_BODY_LIMIT_BYTES,
            ),
            credential_upload_limit_bytes: parse_usize(
                "CREDENTIAL_UPLOAD_LIMIT_BYTES",
                DEFAULT_CREDENTIAL_UPLOAD_LIMIT_BYTES,
            ),
            http_pool_max_idle_per_host: parse_usize("HTTP_POOL_MAX_IDLE_PER_HOST", 100),
            http_pool_idle_timeout: Duration::from_secs(parse_u64(
                "HTTP_POOL_IDLE_TIMEOUT_SECONDS",
                90,
            )),
            user_auth_cache_ttl: Duration::from_secs(parse_u64("USER_AUTH_CACHE_TTL_SECONDS", 60)),
            user_auth_cache_max_entries: parse_usize("USER_AUTH_CACHE_MAX_ENTRIES", 100_000),
            routing_cache_ttl: Duration::from_secs(parse_u64("ROUTING_CACHE_TTL_SECONDS", 30)),
            price_cache_ttl: Duration::from_secs(parse_u64("PRICE_CACHE_TTL_SECONDS", 300)),
            price_cache_max_entries: parse_usize("PRICE_CACHE_MAX_ENTRIES", 10_000),
            secret_cache_max_entries: parse_usize("SECRET_CACHE_MAX_ENTRIES", 4096),
            redis_url: optional("REDIS_URL"),
            redis_key_prefix: env::var("REDIS_KEY_PREFIX")
                .unwrap_or_else(|_| "neogate".to_string()),
            credit_prefetch_micro_usd: parse_i64("CREDIT_PREFETCH_MICRO_USD", 100_000),
            credit_allocation_recovery_after: Duration::from_secs(parse_u64(
                "CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS",
                900,
            )),
            credit_allocation_recovery_interval: Duration::from_secs(parse_u64(
                "CREDIT_ALLOCATION_RECOVERY_INTERVAL_SECONDS",
                60,
            )),
            default_output_tokens: parse_i64("DEFAULT_OUTPUT_TOKENS", 2048),
            usage_flush_interval: Duration::from_millis(parse_u64("USAGE_FLUSH_INTERVAL_MS", 1000)),
            usage_queue_size: parse_usize("USAGE_QUEUE_SIZE", 8192),
            billing_outbox_max_pending: parse_i64("BILLING_OUTBOX_MAX_PENDING", 10_000),
            billing_outbox_max_age: Duration::from_secs(parse_u64(
                "BILLING_OUTBOX_MAX_AGE_SECONDS",
                300,
            )),
            task_upstream_poll_interval: Duration::from_secs(parse_u64(
                "TASK_UPSTREAM_POLL_INTERVAL_SECONDS",
                30,
            )),
            task_upstream_poll_batch_size: parse_i64("TASK_UPSTREAM_POLL_BATCH_SIZE", 100),
            task_upstream_retention: Duration::from_secs(parse_u64(
                "TASK_UPSTREAM_RETENTION_SECONDS",
                2_592_000,
            )),
            task_upstream_stale_hold_release: Duration::from_secs(parse_u64(
                "TASK_UPSTREAM_STALE_HOLD_RELEASE_SECONDS",
                parse_u64("CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS", 900),
            )),
            payment: PaymentConfig::default(),
            db_pool: DbPoolConfig::from_env()?,
            cors_allowed_origins: parse_csv("CORS_ALLOWED_ORIGINS", "*"),
        };
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.db_pool.min_connections > self.db_pool.max_connections {
            anyhow::bail!("DB_POOL_MIN_CONNECTIONS must be <= DB_POOL_MAX_CONNECTIONS");
        }
        if self.runtime_mode.is_distributed() && self.redis_url.is_none() {
            anyhow::bail!("REDIS_URL is required when RUNTIME_MODE=distributed");
        }
        if self.credit_prefetch_micro_usd <= 0 {
            anyhow::bail!("CREDIT_PREFETCH_MICRO_USD must be positive");
        }
        if self.credit_allocation_recovery_after.is_zero() {
            anyhow::bail!("CREDIT_ALLOCATION_RECOVERY_AFTER_SECONDS must be positive");
        }
        if self.credit_allocation_recovery_interval.is_zero() {
            anyhow::bail!("CREDIT_ALLOCATION_RECOVERY_INTERVAL_SECONDS must be positive");
        }
        if self.default_output_tokens <= 0 {
            anyhow::bail!("DEFAULT_OUTPUT_TOKENS must be positive");
        }
        if self.request_timeout.is_zero() {
            anyhow::bail!("REQUEST_TIMEOUT_SECONDS must be positive");
        }
        if self.upstream_connect_timeout.is_zero() {
            anyhow::bail!("UPSTREAM_CONNECT_TIMEOUT_SECONDS must be positive");
        }
        if self.upstream_response_timeout.is_zero() {
            anyhow::bail!("UPSTREAM_RESPONSE_TIMEOUT_SECONDS must be positive");
        }
        if self.relay_body_limit_bytes == 0 {
            anyhow::bail!("RELAY_BODY_LIMIT_BYTES must be positive");
        }
        if self.credential_upload_limit_bytes == 0 {
            anyhow::bail!("CREDENTIAL_UPLOAD_LIMIT_BYTES must be positive");
        }
        if self.billing_outbox_max_pending < 0 {
            anyhow::bail!("BILLING_OUTBOX_MAX_PENDING must be non-negative");
        }
        if self.task_upstream_poll_interval.is_zero() {
            anyhow::bail!("TASK_UPSTREAM_POLL_INTERVAL_SECONDS must be positive");
        }
        if self.task_upstream_poll_batch_size <= 0 {
            anyhow::bail!("TASK_UPSTREAM_POLL_BATCH_SIZE must be positive");
        }
        if self.task_upstream_retention.is_zero() {
            anyhow::bail!("TASK_UPSTREAM_RETENTION_SECONDS must be positive");
        }
        if self.task_upstream_stale_hold_release.is_zero() {
            anyhow::bail!("TASK_UPSTREAM_STALE_HOLD_RELEASE_SECONDS must be positive");
        }
        if self.user_auth_cache_max_entries == 0 {
            anyhow::bail!("USER_AUTH_CACHE_MAX_ENTRIES must be positive");
        }
        if self.price_cache_max_entries == 0 {
            anyhow::bail!("PRICE_CACHE_MAX_ENTRIES must be positive");
        }
        if self.secret_cache_max_entries == 0 {
            anyhow::bail!("SECRET_CACHE_MAX_ENTRIES must be positive");
        }
        if self.usage_queue_size > 1_000_000 {
            anyhow::bail!("USAGE_QUEUE_SIZE must be <= 1000000");
        }
        if self.production && self.public_base_url.is_none() {
            anyhow::bail!("PUBLIC_BASE_URL is required in production");
        }

        if self.production {
            reject_default(
                "ADMIN_TOKEN_SECRET",
                &self.admin_token_secret,
                "change-me-admin-token-secret-in-production",
            )?;
            require_secret_len("ADMIN_TOKEN_SECRET", &self.admin_token_secret, 32)?;
            reject_default(
                "UPSTREAM_SECRET_KEY",
                &self.upstream_secret_key,
                "change-me-upstream-secret-key-in-production",
            )?;
            require_secret_len("UPSTREAM_SECRET_KEY", &self.upstream_secret_key, 32)?;
        }

        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PaymentConfig {
    pub enabled_providers: Vec<PaymentProvider>,
    pub return_base_url: Option<String>,
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
            return_base_url: None,
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
            min_connections: parse_u32("DB_POOL_MIN_CONNECTIONS", 1),
            max_connections: parse_u32("DB_POOL_MAX_CONNECTIONS", 10),
            acquire_timeout: Duration::from_secs(parse_u64("DB_POOL_ACQUIRE_TIMEOUT_SECONDS", 5)),
        })
    }
}

fn required(name: &str) -> Result<String> {
    env::var(name).with_context(|| format!("{name} is required"))
}

fn optional(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn parse_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_u32(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_i64(name: &str, default: i64) -> i64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn parse_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
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

fn reject_default(name: &str, value: &str, default: &str) -> Result<()> {
    if value == default {
        anyhow::bail!("{name} must be changed in production");
    }
    Ok(())
}

fn require_secret_len(name: &str, value: &str, min_len: usize) -> Result<()> {
    if value.len() < min_len {
        anyhow::bail!("{name} must be at least {min_len} characters in production");
    }
    Ok(())
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
}
