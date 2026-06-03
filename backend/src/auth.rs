use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, HeaderMap},
    routing::post,
    Json, Router,
};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

pub use crate::core::auth::*;
use crate::{
    billing::{wallet, WalletType},
    email::EmailLocale,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

const PASSWORD_RESET_TTL: std::time::Duration = std::time::Duration::from_secs(30 * 60);
const LOGIN_VERIFICATION_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const MIN_USER_PASSWORD_LEN: usize = 8;

#[derive(Debug, Clone)]
pub struct UserSessionAuth {
    pub user_id: DbId,
}

impl FromRequestParts<Arc<AppState>> for UserSessionAuth {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let Some(user_id) = bearer(&parts.headers)
            .and_then(|token| validate_user_session_token(token, &state.config.admin_token_secret))
        else {
            return Err(AppError::Unauthorized);
        };

        let row = sqlx::query(r#"SELECT status FROM "user" WHERE id = $1"#)
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

        Ok(Self { user_id })
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/login", post(login))
        .route(
            "/api/login-verification-codes",
            post(request_login_verification_code),
        )
        .route("/api/password-reset-requests", post(request_password_reset))
        .route("/api/password-reset", post(reset_password))
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

#[derive(Debug, Serialize)]
struct OkResponse {
    ok: bool,
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> AppResult<Json<LoginResponse>> {
    if req
        .username
        .eq_ignore_ascii_case(&state.config.admin_username)
    {
        if req.password != state.config.admin_password {
            return Err(AppError::Unauthorized);
        }

        return Ok(Json(LoginResponse {
            token: issue_admin_token(
                state.config.admin_session_ttl,
                &state.config.admin_token_secret,
            ),
            role: "admin".to_string(),
        }));
    }

    let user_id = login_or_create_user(
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
            user_id,
        ),
        role: "user".to_string(),
    }))
}

async fn request_login_verification_code(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginVerificationCodeRequest>,
) -> AppResult<Json<OkResponse>> {
    let email = normalize_email(&req.email)?;
    if login_email_needs_verification(&state, &email).await? {
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
            .await?;
    }

    Ok(Json(OkResponse { ok: true }))
}

async fn request_password_reset(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PasswordResetRequest>,
) -> AppResult<Json<OkResponse>> {
    let email = normalize_email(&req.email)?;
    if user_can_reset_password(&state, &email).await? {
        let token = issue_password_reset_token(
            PASSWORD_RESET_TTL,
            &state.config.admin_token_secret,
            &email,
        );
        let reset_url = reset_url_from_headers(&headers, &token)?;
        state
            .email
            .send_password_reset(
                &email,
                &reset_url,
                EmailLocale::from_public_locale(req.locale.as_deref()),
            )
            .await?;
    }

    Ok(Json(OkResponse { ok: true }))
}

async fn reset_password(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ResetPasswordRequest>,
) -> AppResult<Json<OkResponse>> {
    validate_user_password_input(&req.password)?;

    let email = password_reset_email_from_token(&req.token, &state.config.admin_token_secret)
        .ok_or(AppError::Unauthorized)?;
    let password_hash = hash_user_password(&req.password, &state.config.admin_token_secret);
    let result = sqlx::query(
        r#"
        UPDATE "user"
        SET password_hash = $2, updated_at = now()
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

async fn login_or_create_user(
    state: &AppState,
    email: &str,
    password: &str,
    verification_code: Option<&str>,
) -> AppResult<DbId> {
    let email = normalize_email(email)?;
    validate_user_password_input(password)?;

    let password_hash = hash_user_password(password, &state.config.admin_token_secret);
    let mut tx = state.db.pool.begin().await?;
    lock_user_login(&mut tx, &email).await?;

    let existing = sqlx::query(
        r#"
        SELECT id, status, password_hash
        FROM "user"
        WHERE email = $1
        ORDER BY created_at ASC
        LIMIT 1
        "#,
    )
    .bind(&email)
    .fetch_optional(&mut *tx)
    .await?;

    let user_id = if let Some(row) = existing {
        let user_id: DbId = row.try_get("id")?;
        let status: String = row.try_get("status")?;
        let stored_hash: Option<String> = row.try_get("password_hash")?;
        if status != "enabled" {
            return Err(AppError::Unauthorized);
        }

        if let Some(stored_hash) = stored_hash {
            if !verify_user_password(password, &state.config.admin_token_secret, &stored_hash) {
                return Err(AppError::Unauthorized);
            }
        } else {
            sqlx::query(
                r#"UPDATE "user" SET password_hash = $2, updated_at = now() WHERE id = $1"#,
            )
            .bind(user_id)
            .bind(&password_hash)
            .execute(&mut *tx)
            .await?;
        }
        user_id
    } else {
        let Some(verification_code) = verification_code
            .map(str::trim)
            .filter(|code| !code.is_empty())
        else {
            return Err(AppError::BadRequest(
                "verification code required".to_string(),
            ));
        };
        consume_login_verification_code(&mut tx, &email, verification_code, state).await?;

        let row = sqlx::query(
            r#"
            INSERT INTO "user" (email, status, user_group_id, password_hash)
            VALUES ($1, 'enabled', (SELECT id FROM user_group WHERE is_default = TRUE), $2)
            RETURNING id
            "#,
        )
        .bind(&email)
        .bind(&password_hash)
        .fetch_one(&mut *tx)
        .await?;
        let user_id: DbId = row.try_get("id")?;
        wallet::create_owner_wallet(&mut tx, WalletType::User, user_id).await?;
        user_id
    };

    tx.commit().await?;
    Ok(user_id)
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

fn validate_user_password_input(password: &str) -> AppResult<()> {
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

fn reset_url_from_headers(headers: &HeaderMap, token: &str) -> AppResult<String> {
    let origin = headers
        .get("origin")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"));
    let base = if let Some(origin) = origin {
        origin.trim_end_matches('/').to_string()
    } else {
        let host = headers
            .get("host")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::BadRequest("missing host header".to_string()))?;
        format!("http://{host}")
    };

    Ok(format!("{base}/reset-password?token={token}"))
}
