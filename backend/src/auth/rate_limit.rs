use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{
    core::tokens::hash_key,
    error::{AppError, AppResult},
};

const AUTH_RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60 * 60);
const AUTH_RATE_LIMIT_MAX_ENTRIES: usize = 20_000;

#[derive(Clone)]
pub struct AuthRateLimiter {
    backend: AuthRateLimitBackend,
}

#[derive(Clone)]
enum AuthRateLimitBackend {
    Local(LocalAuthRateLimiter),
    Redis(RedisAuthRateLimiter),
}

#[derive(Clone, Default)]
struct LocalAuthRateLimiter {
    buckets: Arc<Mutex<HashMap<String, RateLimitBucket>>>,
}

#[derive(Clone)]
struct RedisAuthRateLimiter {
    manager: redis::aio::ConnectionManager,
    key_prefix: String,
}

#[derive(Clone)]
struct RateLimitBucket {
    count: u32,
    reset_at: Instant,
}

impl AuthRateLimiter {
    pub fn local() -> Self {
        Self {
            backend: AuthRateLimitBackend::Local(LocalAuthRateLimiter::default()),
        }
    }

    pub async fn redis(redis_url: &str, key_prefix: String) -> AppResult<Self> {
        let client = redis::Client::open(redis_url)?;
        let manager = client.get_connection_manager().await?;
        Ok(Self {
            backend: AuthRateLimitBackend::Redis(RedisAuthRateLimiter {
                manager,
                key_prefix,
            }),
        })
    }

    pub(crate) async fn check_login_verification_request(
        &self,
        email: &str,
        client_key: &str,
    ) -> AppResult<()> {
        self.check(
            format!("login-code-email:{email}"),
            AUTH_RATE_LIMIT_WINDOW,
            "login_verification_rate_limited",
            "too many login verification code requests",
            LOGIN_CODE_EMAIL_LIMIT,
        )
        .await?;
        self.check(
            format!("login-code-ip:{client_key}"),
            AUTH_RATE_LIMIT_WINDOW,
            "login_verification_rate_limited",
            "too many login verification code requests",
            LOGIN_CODE_IP_LIMIT,
        )
        .await
    }

    pub(crate) async fn check_login_verification_attempt(&self, email: &str) -> AppResult<()> {
        self.check(
            format!("login-code-attempt:{email}"),
            AUTH_RATE_LIMIT_WINDOW,
            "login_verification_rate_limited",
            "too many login verification attempts",
            LOGIN_CODE_ATTEMPT_LIMIT,
        )
        .await
    }

    pub(crate) async fn check_password_reset_request(
        &self,
        email: &str,
        client_key: &str,
    ) -> AppResult<()> {
        self.check(
            format!("password-reset-email:{email}"),
            AUTH_RATE_LIMIT_WINDOW,
            "password_reset_rate_limited",
            "too many password reset requests",
            PASSWORD_RESET_EMAIL_LIMIT,
        )
        .await?;
        self.check(
            format!("password-reset-ip:{client_key}"),
            AUTH_RATE_LIMIT_WINDOW,
            "password_reset_rate_limited",
            "too many password reset requests",
            PASSWORD_RESET_IP_LIMIT,
        )
        .await
    }

    pub(crate) async fn check_password_reset_attempt(
        &self,
        token: &str,
        client_key: &str,
    ) -> AppResult<()> {
        self.check(
            format!("password-reset-attempt-token:{}", hash_key(token)),
            AUTH_RATE_LIMIT_WINDOW,
            "password_reset_rate_limited",
            "too many password reset attempts",
            PASSWORD_RESET_ATTEMPT_LIMIT,
        )
        .await?;
        self.check(
            format!("password-reset-attempt-ip:{client_key}"),
            AUTH_RATE_LIMIT_WINDOW,
            "password_reset_rate_limited",
            "too many password reset attempts",
            PASSWORD_RESET_IP_LIMIT,
        )
        .await
    }

    async fn check(
        &self,
        key: String,
        window: Duration,
        code: &'static str,
        message: &'static str,
        limit: u32,
    ) -> AppResult<()> {
        match &self.backend {
            AuthRateLimitBackend::Local(local) => local.check(key, limit, window, code, message),
            AuthRateLimitBackend::Redis(redis) => {
                redis.check(key, limit, window, code, message).await
            }
        }
    }
}

impl Default for AuthRateLimiter {
    fn default() -> Self {
        Self::local()
    }
}

impl LocalAuthRateLimiter {
    fn check(
        &self,
        key: String,
        limit: u32,
        window: Duration,
        code: &'static str,
        message: &'static str,
    ) -> AppResult<()> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if buckets.len() > AUTH_RATE_LIMIT_MAX_ENTRIES {
            buckets.retain(|_, bucket| bucket.reset_at > now);
        }
        let bucket = buckets.entry(key).or_insert_with(|| RateLimitBucket {
            count: 0,
            reset_at: now + window,
        });
        if bucket.reset_at <= now {
            bucket.count = 0;
            bucket.reset_at = now + window;
        }
        if bucket.count >= limit {
            return Err(AppError::RateLimitedWithCode { code, message });
        }
        bucket.count += 1;
        Ok(())
    }
}

impl RedisAuthRateLimiter {
    async fn check(
        &self,
        key: String,
        limit: u32,
        window: Duration,
        code: &'static str,
        message: &'static str,
    ) -> AppResult<()> {
        let mut conn = self.manager.clone();
        let redis_key = format!("{}:auth_rate_limit:{key}", self.key_prefix);
        let ttl_ms = window.as_millis().clamp(1, i64::MAX as u128) as i64;
        let count: i64 = redis::Script::new(
            r#"
            local count = redis.call('INCR', KEYS[1])
            -- 无条件刷新 TTL：防止旧 key 惰性未清理时 INCR 返回 > 1 而 TTL 未被重设，
            -- 导致该 key 永久积累计数。固定窗口语义不变（每次请求重置到 window 后过期）。
            redis.call('PEXPIRE', KEYS[1], ARGV[1])
            return count
            "#,
        )
        .key(redis_key)
        .arg(ttl_ms)
        .invoke_async(&mut conn)
        .await?;

        if count > limit as i64 {
            return Err(AppError::RateLimitedWithCode { code, message });
        }
        Ok(())
    }
}

const LOGIN_CODE_EMAIL_LIMIT: u32 = 5;
const LOGIN_CODE_IP_LIMIT: u32 = 30;
const LOGIN_CODE_ATTEMPT_LIMIT: u32 = 10;
const PASSWORD_RESET_EMAIL_LIMIT: u32 = 3;
const PASSWORD_RESET_IP_LIMIT: u32 = 20;
const PASSWORD_RESET_ATTEMPT_LIMIT: u32 = 10;

#[cfg(test)]
mod tests {
    use crate::error::AppError;

    use super::*;

    #[tokio::test]
    async fn auth_rate_limiter_blocks_repeated_requests() {
        let limiter = AuthRateLimiter::default();
        let client_key = "192.0.2.10";

        for _ in 0..LOGIN_CODE_EMAIL_LIMIT {
            limiter
                .check_login_verification_request("user@example.com", client_key)
                .await
                .unwrap();
        }

        let err = limiter
            .check_login_verification_request("user@example.com", client_key)
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::RateLimitedWithCode {
                code: "login_verification_rate_limited",
                ..
            }
        ));
    }
}
