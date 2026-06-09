use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, HeaderMap},
    routing::{get, post},
    Json, Router,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

pub use crate::core::auth::*;
use crate::{
    billing::{account, CreditAccountType},
    email::{smtp_config_error, EmailLocale},
    error::{AppError, AppResult},
    id::DbId,
    policy::{registration_policy, ServiceMode},
    AppState,
};

const PASSWORD_RESET_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const LOGIN_VERIFICATION_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const MIN_USER_PASSWORD_LEN: usize = 8;
const MAX_ADMIN_LOGIN_FAILURES: i32 = 5;
const ADMIN_LOGIN_LOCK_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const LOGIN_CODE_EMAIL_LIMIT: u32 = 5;
const LOGIN_CODE_IP_LIMIT: u32 = 30;
const LOGIN_CODE_ATTEMPT_LIMIT: u32 = 10;
const PASSWORD_RESET_EMAIL_LIMIT: u32 = 3;
const PASSWORD_RESET_IP_LIMIT: u32 = 20;
const PASSWORD_RESET_ATTEMPT_LIMIT: u32 = 10;
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

    async fn check_login_verification_request(
        &self,
        email: &str,
        client_key: &str,
    ) -> AppResult<()> {
        self.check(
            format!("login-code-email:{email}"),
            LOGIN_CODE_EMAIL_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many login verification code requests",
        )
        .await?;
        self.check(
            format!("login-code-ip:{client_key}"),
            LOGIN_CODE_IP_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many login verification code requests",
        )
        .await
    }

    async fn check_login_verification_attempt(&self, email: &str) -> AppResult<()> {
        self.check(
            format!("login-code-attempt:{email}"),
            LOGIN_CODE_ATTEMPT_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many login verification attempts",
        )
        .await
    }

    async fn check_password_reset_request(&self, email: &str, client_key: &str) -> AppResult<()> {
        self.check(
            format!("password-reset-email:{email}"),
            PASSWORD_RESET_EMAIL_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many password reset requests",
        )
        .await?;
        self.check(
            format!("password-reset-ip:{client_key}"),
            PASSWORD_RESET_IP_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many password reset requests",
        )
        .await
    }

    async fn check_password_reset_attempt(&self, token: &str, client_key: &str) -> AppResult<()> {
        self.check(
            format!("password-reset-attempt-token:{}", hash_key(token)),
            PASSWORD_RESET_ATTEMPT_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many password reset attempts",
        )
        .await?;
        self.check(
            format!("password-reset-attempt-ip:{client_key}"),
            PASSWORD_RESET_IP_LIMIT,
            AUTH_RATE_LIMIT_WINDOW,
            "too many password reset attempts",
        )
        .await
    }

    async fn check(
        &self,
        key: String,
        limit: u32,
        window: Duration,
        message: &'static str,
    ) -> AppResult<()> {
        match &self.backend {
            AuthRateLimitBackend::Local(local) => local.check(key, limit, window, message),
            AuthRateLimitBackend::Redis(redis) => redis.check(key, limit, window, message).await,
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
        message: &'static str,
    ) -> AppResult<()> {
        let now = Instant::now();
        let mut buckets = self.buckets.lock().expect("auth rate limiter poisoned");
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
            return Err(AppError::RateLimited(message.to_string()));
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
        message: &'static str,
    ) -> AppResult<()> {
        let mut conn = self.manager.clone();
        let redis_key = format!("{}:auth_rate_limit:{key}", self.key_prefix);
        let ttl_ms = window.as_millis().clamp(1, i64::MAX as u128) as i64;
        let count: i64 = redis::Script::new(
            r#"
            local count = redis.call('INCR', KEYS[1])
            if count == 1 then
                redis.call('PEXPIRE', KEYS[1], ARGV[1])
            end
            return count
            "#,
        )
        .key(redis_key)
        .arg(ttl_ms)
        .invoke_async(&mut conn)
        .await?;

        if count > limit as i64 {
            return Err(AppError::RateLimited(message.to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct UserSessionAuth {
    pub user_id: DbId,
}

#[derive(Debug, Clone, Copy)]
struct UserSessionState {
    user_id: DbId,
    requires_password_change: bool,
}

impl FromRequestParts<Arc<AppState>> for UserSessionAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let session = user_session_from_headers(state, &parts.headers).await?;
        if session.requires_password_change {
            return Err(AppError::PasswordChangeRequired);
        }

        Ok(Self {
            user_id: session.user_id,
        })
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/me", get(me))
        .route(
            "/api/login-verification-codes",
            post(request_login_verification_code),
        )
        .route("/api/password-reset-requests", post(request_password_reset))
        .route("/api/password-reset", post(reset_password))
        .route("/api/user/password", post(update_user_password))
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
    verification_code: Option<String>,
}

#[derive(Debug, Serialize)]
struct LoginResponse {
    token: String,
    role: String,
    requires_password_change: bool,
}

#[derive(Debug, Serialize)]
struct MeResponse {
    role: String,
    requires_password_change: bool,
}

#[derive(Debug, Deserialize)]
struct PasswordResetRequest {
    email: String,
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginVerificationCodeRequest {
    email: String,
    locale: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResetPasswordRequest {
    token: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct UpdateUserPasswordRequest {
    current_password: String,
    new_password: String,
}

#[derive(Debug, Serialize)]
struct OkResponse {
    ok: bool,
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    if let Some(response) = login_admin(&state, &req.username, &req.password).await? {
        return Ok(Json(response));
    }

    let user = login_or_create_user(
        &state,
        &req.username,
        &req.password,
        req.verification_code.as_deref(),
    )
    .await?;
    Ok(Json(LoginResponse {
        token: issue_user_session_token(
            state.config.admin_session_ttl,
            &state.config.admin_token_secret,
            user.user_id,
        ),
        role: "user".to_string(),
        requires_password_change: user.requires_password_change,
    }))
}

async fn me(State(state): State<Arc<AppState>>, headers: HeaderMap) -> AppResult<Json<MeResponse>> {
    let token = bearer(&headers).ok_or(AppError::Unauthorized)?;
    if validate_admin_token(token, &state.config.admin_token_secret) {
        return Ok(Json(MeResponse {
            role: "admin".to_string(),
            requires_password_change: false,
        }));
    }

    let session = user_session_from_token(&state, token).await?;

    Ok(Json(MeResponse {
        role: "user".to_string(),
        requires_password_change: session.requires_password_change,
    }))
}

async fn login_admin(
    state: &AppState,
    username: &str,
    password: &str,
) -> AppResult<Option<LoginResponse>> {
    let row = sqlx::query(
        r#"
        SELECT id, password_hash, status, COALESCE(locked_until > now(), FALSE) AS locked
        FROM admin
        WHERE username = $1
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .bind(username)
    .fetch_optional(&state.db.pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let id: DbId = row.try_get("id")?;
    let status: String = row.try_get("status")?;
    let locked: bool = row.try_get("locked")?;
    if status != "enabled" || locked {
        return Err(AppError::Unauthorized);
    }

    let password_hash: String = row.try_get("password_hash")?;
    if !verify_user_password(password, &state.config.admin_token_secret, &password_hash) {
        record_admin_login_failure(state, id).await?;
        return Err(AppError::Unauthorized);
    }

    record_admin_login_success(state, id).await?;
    Ok(Some(LoginResponse {
        token: issue_admin_token(
            state.config.admin_session_ttl,
            &state.config.admin_token_secret,
        ),
        role: "admin".to_string(),
        requires_password_change: false,
    }))
}

async fn user_session_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> AppResult<UserSessionState> {
    let token = bearer(headers).ok_or(AppError::Unauthorized)?;
    user_session_from_token(state, token).await
}

async fn user_session_from_token(state: &AppState, token: &str) -> AppResult<UserSessionState> {
    let Some(user_id) = validate_user_session_token(token, &state.config.admin_token_secret) else {
        return Err(AppError::Unauthorized);
    };

    let row = sqlx::query(
        r#"
        SELECT status, (password_changed_at IS NULL) AS requires_password_change
        FROM "user"
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    let status: String = row.try_get("status")?;
    if status != "enabled" {
        return Err(AppError::Unauthorized);
    }
    Ok(UserSessionState {
        user_id,
        requires_password_change: row.try_get("requires_password_change")?,
    })
}

async fn record_admin_login_failure(state: &AppState, id: DbId) -> AppResult<()> {
    sqlx::query(
        r#"
        UPDATE admin
        SET failed_login_attempts = failed_login_attempts + 1,
            locked_until = CASE
                WHEN failed_login_attempts + 1 >= $2 THEN now() + $3::interval
                ELSE locked_until
            END,
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(MAX_ADMIN_LOGIN_FAILURES)
    .bind(format!("{} seconds", ADMIN_LOGIN_LOCK_TTL.as_secs()))
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn record_admin_login_success(state: &AppState, id: DbId) -> AppResult<()> {
    sqlx::query(
        r#"
        UPDATE admin
        SET failed_login_attempts = 0,
            locked_until = NULL,
            last_login_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn request_login_verification_code(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<LoginVerificationCodeRequest>,
) -> AppResult<Json<OkResponse>> {
    let email = normalize_email(&req.email)?;
    let client_key = client_rate_key(&headers);
    state
        .auth_rate_limiter
        .check_login_verification_request(&email, &client_key)
        .await?;
    if login_email_needs_verification(&state, &email).await? {
        let (_, registration_enabled) = registration_policy(&state).await?;
        if !registration_enabled {
            return Err(AppError::BadRequest("registration is closed".to_string()));
        }
        let code = generate_login_verification_code();
        let code_hash =
            hash_email_verification_code(&email, &code, &state.config.admin_token_secret);
        sqlx::query(
            r#"
            INSERT INTO user_code (email, code_hash, expires_at)
            VALUES ($1, $2, now() + $3::interval)
            "#,
        )
        .bind(&email)
        .bind(&code_hash)
        .bind(format!("{} seconds", LOGIN_VERIFICATION_TTL.as_secs()))
        .execute(&state.db.pool)
        .await?;

        state
            .email
            .send_login_verification_code(
                &email,
                &code,
                EmailLocale::from_public_locale(req.locale.as_deref()),
            )
            .await
            .map_err(email_error)?;
    }

    Ok(Json(OkResponse { ok: true }))
}

async fn request_password_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PasswordResetRequest>,
) -> AppResult<Json<OkResponse>> {
    let email = normalize_email(&req.email)?;
    let client_key = client_rate_key(&headers);
    state
        .auth_rate_limiter
        .check_password_reset_request(&email, &client_key)
        .await?;
    if user_can_reset_password(&state, &email).await? {
        let token = issue_password_reset_token(
            PASSWORD_RESET_TTL,
            &state.config.admin_token_secret,
            &email,
        );
        let reset_url = reset_url_from_config(state.config.public_base_url.as_deref(), &token)?;
        state
            .email
            .send_password_reset(
                &email,
                &reset_url,
                EmailLocale::from_public_locale(req.locale.as_deref()),
            )
            .await
            .map_err(email_error)?;
    }

    Ok(Json(OkResponse { ok: true }))
}

async fn reset_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ResetPasswordRequest>,
) -> AppResult<Json<OkResponse>> {
    let client_key = client_rate_key(&headers);
    state
        .auth_rate_limiter
        .check_password_reset_attempt(&req.token, &client_key)
        .await?;
    validate_user_password_input(&req.password)?;

    let email = password_reset_email_from_token(&req.token, &state.config.admin_token_secret)
        .ok_or(AppError::Unauthorized)?;
    let password_hash = hash_user_password(&req.password, &state.config.admin_token_secret);
    let result = sqlx::query(
        r#"
        UPDATE "user"
        SET password_hash = $2, password_changed_at = now(), updated_at = now()
        WHERE email = $1 AND status = 'enabled'
        "#,
    )
    .bind(email)
    .bind(password_hash)
    .execute(&state.db.pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::Unauthorized);
    }

    Ok(Json(OkResponse { ok: true }))
}

async fn update_user_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UpdateUserPasswordRequest>,
) -> AppResult<Json<OkResponse>> {
    if req.current_password.is_empty() {
        return Err(AppError::BadRequest(
            "current password is required".to_string(),
        ));
    }
    validate_user_password_input(&req.new_password)?;

    let session = user_session_from_headers(&state, &headers).await?;
    let row = sqlx::query(
        r#"
        SELECT password_hash
        FROM "user"
        WHERE id = $1 AND status = 'enabled'
        "#,
    )
    .bind(session.user_id)
    .fetch_optional(&state.db.pool)
    .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthorized);
    };
    let password_hash: Option<String> = row.try_get("password_hash")?;
    let Some(current_hash) = password_hash else {
        return Err(AppError::Unauthorized);
    };
    if !verify_user_password(
        &req.current_password,
        &state.config.admin_token_secret,
        &current_hash,
    ) {
        return Err(AppError::BadRequest(
            "current password is incorrect".to_string(),
        ));
    }
    if verify_user_password(
        &req.new_password,
        &state.config.admin_token_secret,
        &current_hash,
    ) {
        return Err(AppError::BadRequest(
            "new password cannot be the same as the current password".to_string(),
        ));
    }

    let next_hash = hash_user_password(&req.new_password, &state.config.admin_token_secret);
    sqlx::query(
        r#"
        UPDATE "user"
        SET password_hash = $2,
            password_changed_at = now(),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(session.user_id)
    .bind(next_hash)
    .execute(&state.db.pool)
    .await?;

    Ok(Json(OkResponse { ok: true }))
}

#[derive(Debug, Clone, Copy)]
struct UserLogin {
    user_id: DbId,
    requires_password_change: bool,
}

async fn login_or_create_user(
    state: &AppState,
    email: &str,
    password: &str,
    verification_code: Option<&str>,
) -> AppResult<UserLogin> {
    let email = normalize_email(email)?;
    validate_user_password_input(password)?;

    let password_hash = hash_user_password(password, &state.config.admin_token_secret);
    let mut tx = state.db.pool.begin().await?;
    lock_user_login(&mut tx, &email).await?;

    let existing = sqlx::query(
        r#"
        SELECT id, status, password_hash,
               (password_changed_at IS NULL) AS requires_password_change
        FROM "user"
        WHERE email = $1
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let user = if let Some(row) = existing {
        let user_id: DbId = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let stored_hash: Option<String> = row.try_get("password_hash")?;
        let mut requires_password_change: bool = row.try_get("requires_password_change")?;
        if status != "enabled" {
            if status == "pending" {
                return Err(AppError::BadRequest("account pending approval".to_string()));
            }
            return Err(AppError::Unauthorized);
        }

        if let Some(stored_hash) = stored_hash {
            if !verify_user_password(password, &state.config.admin_token_secret, &stored_hash) {
                return Err(AppError::Unauthorized);
            }
        } else {
            sqlx::query(
                r#"
                UPDATE "user"
                SET password_hash = $2,
                    password_changed_at = now(),
                    updated_at = now()
                WHERE id = $1
                "#,
            )
            .bind(user_id)
            .bind(&password_hash)
            .execute(&mut *tx)
            .await?;
            requires_password_change = false;
        }
        UserLogin {
            user_id,
            requires_password_change,
        }
    } else {
        let (service_mode, registration_enabled) = registration_policy(state).await?;
        if !registration_enabled {
            return Err(AppError::BadRequest("registration is closed".to_string()));
        }

        let Some(verification_code) = verification_code
            .map(str::trim)
            .filter(|code| !code.is_empty())
        else {
            return Err(AppError::BadRequest(
                "verification code required".to_string(),
            ));
        };
        state
            .auth_rate_limiter
            .check_login_verification_attempt(&email)
            .await?;
        consume_login_verification_code(&mut tx, &email, verification_code, state).await?;

        let row = sqlx::query(
            r#"
            INSERT INTO "user" (email, status, user_group_id, password_hash, password_changed_at)
            VALUES ($1, $2, (SELECT id FROM user_group WHERE is_default = TRUE), $3, now())
            RETURNING id
            "#,
        )
        .bind(&email)
        .bind(match service_mode {
            ServiceMode::Internal => "pending",
            ServiceMode::Paid => "enabled",
        })
        .bind(&password_hash)
        .fetch_one(&mut *tx)
        .await?;
        let user_id: DbId = row.try_get("id")?;
        account::create_credit_account(&mut tx, CreditAccountType::User, user_id).await?;
        if service_mode == ServiceMode::Internal {
            tx.commit().await?;
            return Err(AppError::BadRequest("account pending approval".to_string()));
        }
        UserLogin {
            user_id,
            requires_password_change: false,
        }
    };

    tx.commit().await?;
    Ok(user)
}

async fn login_email_needs_verification(state: &AppState, email: &str) -> AppResult<bool> {
    let row = sqlx::query(r#"SELECT id FROM "user" WHERE email = $1 LIMIT 1"#)
        .bind(email)
        .fetch_optional(&state.db.pool)
        .await?;

    Ok(row.is_none())
}

async fn consume_login_verification_code(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    code: &str,
    state: &AppState,
) -> AppResult<()> {
    if code.len() != 6 || !code.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AppError::BadRequest(
            "invalid verification code".to_string(),
        ));
    }

    let code_hash = hash_email_verification_code(email, code, &state.config.admin_token_secret);
    let row = sqlx::query(
        r#"
        SELECT id
        FROM user_code
        WHERE email = $1
            AND code_hash = $2
            AND consumed_at IS NULL
            AND expires_at > now()
        ORDER BY created_at DESC
        LIMIT 1
        FOR UPDATE
        "#,
    )
    .bind(email)
    .bind(&code_hash)
    .fetch_optional(&mut **tx)
    .await?;

    let Some(row) = row else {
        return Err(AppError::BadRequest(
            "invalid verification code".to_string(),
        ));
    };
    let code_id: DbId = row.try_get("id")?;

    sqlx::query(
        r#"
        UPDATE user_code
        SET consumed_at = now()
        WHERE id = $1
        "#,
    )
    .bind(code_id)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

async fn user_can_reset_password(state: &AppState, email: &str) -> AppResult<bool> {
    let row = sqlx::query(
        r#"SELECT status FROM "user" WHERE email = $1 ORDER BY created_at ASC LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(&state.db.pool)
    .await?;

    Ok(row
        .map(|row| row.try_get::<String, _>("status"))
        .transpose()?
        .as_deref()
        == Some("enabled"))
}

fn normalize_email(email: &str) -> AppResult<String> {
    let email = email.trim().to_ascii_lowercase();
    let has_single_at = email.matches('@').count() == 1;
    if email.is_empty()
        || email.len() > 254
        || !has_single_at
        || email.starts_with('@')
        || email.ends_with('@')
    {
        return Err(AppError::BadRequest("invalid email".to_string()));
    }
    Ok(email)
}

async fn lock_user_login(tx: &mut Transaction<'_, Postgres>, email: &str) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(email)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn generate_login_verification_code() -> String {
    format!("{:06}", rand::rng().random_range(0..1_000_000))
}

pub(crate) fn validate_user_password_input(password: &str) -> AppResult<()> {
    if password.is_empty() {
        return Err(AppError::BadRequest("password is required".to_string()));
    }
    if password.chars().count() < MIN_USER_PASSWORD_LEN {
        return Err(AppError::BadRequest(
            "password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

fn client_rate_key(headers: &HeaderMap) -> String {
    forwarded_client_ip(headers)
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn email_error(err: anyhow::Error) -> AppError {
    smtp_config_error(&err)
        .map(|(code, message)| AppError::BadRequestWithCode { code, message })
        .unwrap_or_else(|| AppError::Anyhow(err))
}

fn forwarded_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse().ok())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(|value| value.parse().ok())
        })
}

fn reset_url_from_config(public_base_url: Option<&str>, token: &str) -> AppResult<String> {
    let base = public_base_url.ok_or_else(|| {
        AppError::BadRequest("PUBLIC_BASE_URL is required for password reset".to_string())
    })?;
    Ok(format!("{base}/reset-password?token={token}"))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderMap;

    use super::*;

    #[test]
    fn reset_url_uses_configured_public_base_url_only() {
        let url = reset_url_from_config(Some("https://app.example.com"), "reset-token").unwrap();

        assert_eq!(
            url,
            "https://app.example.com/reset-password?token=reset-token"
        );
        assert!(reset_url_from_config(None, "reset-token").is_err());
    }

    #[test]
    fn client_rate_key_prefers_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.0.2.10".parse().unwrap());
        headers.insert(
            "x-forwarded-for",
            "198.51.100.20, 203.0.113.30".parse().unwrap(),
        );

        assert_eq!(client_rate_key(&headers), "198.51.100.20");
    }

    #[test]
    fn client_rate_key_uses_real_ip_without_forwarded_for() {
        let mut headers = HeaderMap::new();
        headers.insert("x-real-ip", "192.0.2.10".parse().unwrap());

        assert_eq!(client_rate_key(&headers), "192.0.2.10");
    }

    #[test]
    fn client_rate_key_uses_unknown_without_proxy_headers() {
        assert_eq!(client_rate_key(&HeaderMap::new()), "unknown");
    }

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
        assert!(matches!(err, AppError::RateLimited(_)));
    }
}
