use std::sync::Arc;

use axum::{
    extract::{FromRequestParts, State},
    http::{request::Parts, HeaderMap},
    Json,
};
use serde::Serialize;
use sqlx::Row;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

const MAX_ADMIN_LOGIN_FAILURES: i32 = 5;
const ADMIN_LOGIN_LOCK_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct UserSessionAuth {
    pub user_id: DbId,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct UserSessionState {
    pub(super) user_id: DbId,
    requires_password_change: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct LoginResponse {
    pub token: String,
    pub role: String,
    pub requires_password_change: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct MeResponse {
    pub role: String,
    pub requires_password_change: bool,
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

pub(super) async fn me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Json<MeResponse>> {
    let token = super::bearer(&headers).ok_or(AppError::Unauthorized)?;
    if super::validate_admin_token(token, &state.config.admin_token_secret) {
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

pub(super) async fn login_admin(
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
    if !super::verify_user_password(password, &state.config.admin_token_secret, &password_hash) {
        record_admin_login_failure(state, id).await?;
        return Err(AppError::Unauthorized);
    }

    record_admin_login_success(state, id).await?;
    Ok(Some(LoginResponse {
        token: super::issue_admin_token(
            state.config.admin_session_ttl,
            &state.config.admin_token_secret,
        ),
        role: "admin".to_string(),
        requires_password_change: false,
    }))
}

pub(super) async fn user_session_from_headers(
    state: &AppState,
    headers: &HeaderMap,
) -> AppResult<UserSessionState> {
    let token = super::bearer(headers).ok_or(AppError::Unauthorized)?;
    user_session_from_token(state, token).await
}

async fn user_session_from_token(state: &AppState, token: &str) -> AppResult<UserSessionState> {
    let Some(user_id) = super::validate_user_session_token(token, &state.config.admin_token_secret)
    else {
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
