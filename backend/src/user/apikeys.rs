use std::sync::Arc;

use axum::{
    extract::{Path, State},
    routing::{get, patch},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

use crate::{
    auth::{generate_user_key, key_prefix, UserSessionAuth},
    billing::{account, CreditAccountId, CreditAccountType, DebitPart},
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    id::DbId,
    policy::{service_mode, ServiceMode},
    project, AppState,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/user/apikeys", get(list_apikeys).post(create_apikey))
        .route(
            "/api/user/apikeys/{id}",
            patch(update_apikey).delete(delete_apikey),
        )
}

#[derive(Debug, Serialize)]
struct UserApiKeyRecord {
    id: DbId,
    user_id: DbId,
    name: String,
    key: String,
    key_prefix: String,
    status: String,
    last_active_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    balance_micro_usd: i64,
    reserved_micro_usd: i64,
    available_micro_usd: i64,
    month_cost_micro_usd: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct CreatedUserApiKey {
    record: UserApiKeyRecord,
    key: String,
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyRequest {
    name: String,
}

#[derive(Debug, Deserialize)]
struct UpdateApiKeyRequest {
    status: String,
}

#[derive(Debug, Serialize)]
struct DeleteApiKeyResponse {
    ok: bool,
}

async fn list_apikeys(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
) -> AppResult<Json<Vec<UserApiKeyRecord>>> {
    let rows = sqlx::query(
        "SELECT uk.id, uk.user_id, uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                w.balance_micro_usd, w.reserved_micro_usd,
                COALESCE(month_usage.month_cost_micro_usd, 0)::BIGINT AS month_cost_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         LEFT JOIN (
             SELECT user_key_id,
                    COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS month_cost_micro_usd
             FROM usage_daily
             WHERE user_id = $1
               AND day >= date_trunc('month', now())::date
             GROUP BY user_key_id
         ) month_usage ON month_usage.user_key_id = uk.id
         WHERE uk.user_id = $1
         ORDER BY uk.created_at DESC",
    )
    .bind(auth.user_id)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(Json(
        rows.iter()
            .map(|row| apikey_from_row(&state, row))
            .collect::<Result<_, _>>()?,
    ))
}

async fn create_apikey(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Json(req): Json<CreateApiKeyRequest>,
) -> AppResult<Json<CreatedUserApiKey>> {
    if service_mode(&state).await? != ServiceMode::Paid {
        return Err(AppError::BadRequest(
            "default project is only available in paid service mode".to_string(),
        ));
    }
    let name = normalize_api_key_name(&req.name)?;
    let key = generate_user_key();
    let secret_ciphertext = state.secrets.encrypt(&key)?;
    let mut tx = state.db.pool.begin().await?;
    let project_id = project::default_project_for_user(&mut tx, auth.user_id).await?;
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, project_id, owner_user_id, name, key_prefix, secret_ciphertext, status, expires_at)
        VALUES ($1, $2, $1, $3, $4, $5, 'enabled', NULL)
        RETURNING id
        "#,
    )
    .bind(auth.user_id)
    .bind(project_id)
    .bind(name)
    .bind(key_prefix(&key))
    .bind(secret_ciphertext)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    account::create_credit_account(&mut tx, CreditAccountType::UserKey, user_key_id).await?;
    tx.commit().await?;

    Ok(Json(CreatedUserApiKey {
        record: get_apikey(&state, auth.user_id, user_key_id).await?,
        key,
    }))
}

async fn update_apikey(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateApiKeyRequest>,
) -> AppResult<Json<UserApiKeyRecord>> {
    validate_status(&req.status)?;
    if req.status == "disabled" {
        let credit_accounts = user_apikey_credit_accounts(&state, auth.user_id, id).await?;
        for credit_account in credit_accounts {
            recover_hot_credit_account(&state, credit_account).await?;
        }
    }

    let result = sqlx::query(
        "UPDATE user_key
         SET status = $3, updated_at = now()
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(auth.user_id)
    .bind(&req.status)
    .execute(&state.db.pool)
    .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    state
        .cache_invalidator
        .invalidate(&state, InvalidationEvent::UserKey { id })
        .await;
    Ok(Json(get_apikey(&state, auth.user_id, id).await?))
}

async fn delete_apikey(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<DeleteApiKeyResponse>> {
    let credit_accounts = user_apikey_credit_accounts(&state, auth.user_id, id).await?;
    for credit_account in credit_accounts {
        recover_hot_credit_account(&state, credit_account).await?;
    }

    let result = sqlx::query("DELETE FROM user_key WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }

    state
        .cache_invalidator
        .invalidate(&state, InvalidationEvent::UserKey { id })
        .await;
    Ok(Json(DeleteApiKeyResponse { ok: true }))
}

async fn get_apikey(state: &AppState, user_id: DbId, id: DbId) -> AppResult<UserApiKeyRecord> {
    let row = sqlx::query(
        "SELECT uk.id, uk.user_id, uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                w.balance_micro_usd, w.reserved_micro_usd,
                COALESCE(month_usage.month_cost_micro_usd, 0)::BIGINT AS month_cost_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         LEFT JOIN (
             SELECT user_key_id,
                    COALESCE(SUM(cost_micro_usd), 0)::BIGINT AS month_cost_micro_usd
             FROM usage_daily
             WHERE user_id = $2
               AND day >= date_trunc('month', now())::date
             GROUP BY user_key_id
         ) month_usage ON month_usage.user_key_id = uk.id
         WHERE uk.id = $1 AND uk.user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_one(&state.db.pool)
    .await?;

    apikey_from_row(state, &row)
}

async fn user_apikey_credit_accounts(
    state: &AppState,
    user_id: DbId,
    id: DbId,
) -> AppResult<Vec<CreditAccountId>> {
    let rows = sqlx::query(
        "SELECT w.id
         FROM user_key uk
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.id = $1 AND uk.user_id = $2
         UNION ALL
         SELECT w.id
         FROM user_key uk
         JOIN user_key_model ukm ON ukm.user_key_id = uk.id
         JOIN credit_account w ON w.owner_type = 'user_key_model' AND w.owner_id = ukm.id
         WHERE uk.id = $1 AND uk.user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_all(&state.db.pool)
    .await?;
    if rows.is_empty() {
        return Err(AppError::NotFound);
    }
    rows.iter()
        .map(|row| Ok(CreditAccountId::new(row.try_get("id")?)))
        .collect()
}

async fn recover_hot_credit_account(
    state: &AppState,
    credit_account: CreditAccountId,
) -> AppResult<()> {
    let mut tx = state.db.pool.begin().await?;
    account::lock_for_update(&mut tx, &credit_account).await?;
    let recovered = state
        .billing
        .drain_hot_credit_account(&credit_account)
        .await?;
    recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    tx.commit().await?;
    Ok(())
}

async fn recover_hot_credit_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    parts: &[DebitPart],
) -> AppResult<()> {
    let total = parts.iter().map(|part| part.amount_micro_usd).sum::<i64>();
    if total <= 0 {
        return Ok(());
    }
    let Some(credit_account) = parts.first().map(|part| &part.credit_account) else {
        return Ok(());
    };

    account::decrement_reserved(tx, credit_account, total).await?;

    for part in parts {
        account::mark_allocation_returned(tx, part.allocation_id, part.amount_micro_usd).await?;
    }

    Ok(())
}

fn apikey_from_row(state: &AppState, row: &sqlx::postgres::PgRow) -> AppResult<UserApiKeyRecord> {
    let id = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    Ok(UserApiKeyRecord {
        id,
        user_id: row.try_get("user_id")?,
        name: row.try_get("name")?,
        key: state.secrets.plaintext(id, &secret_ciphertext)?,
        key_prefix: row.try_get("key_prefix")?,
        status: row.try_get("status")?,
        last_active_at: row.try_get("last_active_at")?,
        expires_at: row.try_get("expires_at")?,
        balance_micro_usd: row.try_get("balance_micro_usd")?,
        reserved_micro_usd: row.try_get("reserved_micro_usd")?,
        available_micro_usd: row.try_get::<i64, _>("balance_micro_usd")?
            - row.try_get::<i64, _>("reserved_micro_usd")?,
        month_cost_micro_usd: row.try_get("month_cost_micro_usd")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn normalize_api_key_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("api key name is required".to_string()));
    }
    if name.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "api key name must be 80 characters or fewer".to_string(),
        ));
    }
    Ok(name.to_string())
}

fn validate_status(status: &str) -> AppResult<()> {
    match status {
        "enabled" | "disabled" => Ok(()),
        _ => Err(AppError::BadRequest("invalid api key status".to_string())),
    }
}
