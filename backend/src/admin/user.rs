use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{
        generate_user_key, generate_user_key_from_parts_and_seed, is_generated_user_key,
        issue_user_key_draft_token, key_prefix, user_key_draft_parts_from_token, UserAuth,
    },
    billing::{wallet, DebitPart, WalletId, WalletType, MICRO_USD_PER_USD},
    email::EmailLocale,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};

#[derive(Debug, Serialize)]
pub struct UserRecord {
    pub id: DbId,
    pub email: String,
    pub status: String,
    pub user_group_id: DbId,
    pub user_group_code: String,
    pub user_group_name: String,
    pub user_key_count: i64,
    pub balance_micro_usd: i64,
    pub reserved_micro_usd: i64,
    pub available_micro_usd: i64,
    pub last_active_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserGroupRecord {
    pub id: DbId,
    pub code: String,
    pub name: String,
    pub is_default: bool,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    #[serde(default = "default_enabled_status")]
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UserKeyRecord {
    pub id: DbId,
    pub user_id: DbId,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub status: String,
    pub last_active_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub model_limits: Option<Vec<String>>,
    pub balance_micro_usd: i64,
    pub reserved_micro_usd: i64,
    pub available_micro_usd: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct CreatedUserKey {
    pub record: UserKeyRecord,
    pub key: String,
}

#[derive(Debug, Serialize)]
pub struct PublicUserKeyResponse {
    pub ok: bool,
}

#[derive(Debug, Serialize)]
pub struct UserKeyVerifyResponse {
    pub ok: bool,
    pub model_limits: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct PublicUserKeyDraftResponse {
    pub draft_id: String,
    pub masked_api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct ClaimPublicUserKeyRequest {
    pub email: String,
    pub draft_id: String,
    pub locale: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserKeyRequest {
    pub user_id: DbId,
    #[serde(default = "default_user_key_name")]
    pub name: String,
    #[serde(default = "default_enabled_status")]
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub model_limits: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserKeyRequest {
    pub status: Option<String>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
    pub model_limits: Option<Option<Vec<String>>>,
}

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub email: Option<String>,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListUserKeysQuery {
    pub user_id: Option<DbId>,
}

pub fn default_enabled_status() -> String {
    "enabled".to_string()
}

pub fn default_user_key_name() -> String {
    "API Key".to_string()
}

pub fn public_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/user-key-drafts", post(create_user_key_draft_handler))
        .route("/api/user-keys", post(claim_public_user_key_handler))
        .route("/api/user-key/verify", get(verify_user_key_handler))
}

async fn create_user_key_draft_handler(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<PublicUserKeyDraftResponse>> {
    Ok(Json(create_public_user_key_draft(&state).await?))
}

async fn claim_public_user_key_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ClaimPublicUserKeyRequest>,
) -> AppResult<Json<PublicUserKeyResponse>> {
    claim_public_user_key(&state, req).await?;
    Ok(Json(PublicUserKeyResponse { ok: true }))
}

async fn verify_user_key_handler(auth: UserAuth) -> Json<UserKeyVerifyResponse> {
    Json(UserKeyVerifyResponse {
        ok: true,
        model_limits: auth.model_limits,
    })
}

pub async fn create_user(state: &AppState, req: CreateUserRequest) -> AppResult<UserRecord> {
    validate_status(&req.status)?;
    let email = normalize_email(&req.email)?;
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO "user" (email, status, user_group_id)
        VALUES ($1, $2, (SELECT id FROM user_group WHERE is_default = TRUE))
        RETURNING id
        "#,
    )
    .bind(email)
    .bind(req.status)
    .fetch_one(&mut *tx)
    .await?;
    let user_id: DbId = row.try_get("id")?;
    wallet::create_owner_wallet(&mut tx, WalletType::User, user_id).await?;
    tx.commit().await?;
    get_user(state, user_id).await
}

pub async fn list_users(state: &AppState, query: ListUsersQuery) -> AppResult<Vec<UserRecord>> {
    let email = query
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase());
    let api_key = query
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let matching_user_ids = match api_key.as_deref() {
        Some(api_key) => user_ids_for_api_key(state, api_key).await?,
        None => Vec::new(),
    };
    if api_key.is_some() && matching_user_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut query_builder = sqlx::QueryBuilder::new(
        r#"SELECT u.id, u.email, u.status,
                  ug.id AS user_group_id, ug.code AS user_group_code, ug.name AS user_group_name,
                  COALESCE(ukw.user_key_count, 0) AS user_key_count,
                  w.balance_micro_usd + COALESCE(ukw.balance_micro_usd, 0) AS balance_micro_usd,
                  w.reserved_micro_usd + COALESCE(ukw.reserved_micro_usd, 0) AS reserved_micro_usd,
                  u.last_active_at, u.created_at, u.updated_at
           FROM "user" u
           JOIN user_group ug ON ug.id = u.user_group_id
           JOIN wallet w ON w.owner_type = 'user' AND w.owner_id = u.id
           LEFT JOIN (
               SELECT uk.user_id,
                      count(uk.id) AS user_key_count,
                      COALESCE(sum(kw.balance_micro_usd), 0)::BIGINT AS balance_micro_usd,
                      COALESCE(sum(kw.reserved_micro_usd), 0)::BIGINT AS reserved_micro_usd
               FROM user_key uk
               LEFT JOIN wallet kw ON kw.owner_type = 'user_key' AND kw.owner_id = uk.id
               GROUP BY uk.user_id
           ) ukw ON ukw.user_id = u.id"#,
    );

    let mut has_where = false;
    if let Some(email) = email {
        query_builder
            .push(" WHERE u.email LIKE ")
            .push_bind(format!("%{email}%"));
        has_where = true;
    }

    if !matching_user_ids.is_empty() {
        query_builder
            .push(if has_where { " AND " } else { " WHERE " })
            .push("u.id = ANY(")
            .push_bind(matching_user_ids)
            .push(")");
    }

    query_builder.push(" ORDER BY u.created_at DESC");

    let rows = query_builder.build().fetch_all(&state.db.pool).await?;
    rows.iter().map(user_from_row).collect()
}

pub async fn update_user(
    state: &AppState,
    id: DbId,
    req: UpdateUserRequest,
) -> AppResult<UserRecord> {
    if let Some(status) = &req.status {
        validate_status(status)?;
    }
    let email = req.email.as_deref().map(normalize_email).transpose()?;
    let disabling = matches!(req.status.as_deref(), Some("disabled"));
    let row = sqlx::query(
        r#"
        UPDATE "user"
        SET email = COALESCE($2, email),
            status = COALESCE($3, status),
            updated_at = now()
        WHERE id = $1
        RETURNING id
        "#,
    )
    .bind(id)
    .bind(email)
    .bind(req.status)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let user_id: DbId = row.try_get("id")?;
    if disabling {
        recover_user_hot_wallets(state, id).await?;
    }
    get_user(state, user_id).await
}

pub async fn list_user_groups(state: &AppState) -> AppResult<Vec<UserGroupRecord>> {
    let rows = sqlx::query(
        "SELECT id, code, name, is_default, enabled, created_at, updated_at
         FROM user_group
         ORDER BY is_default DESC, created_at ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(user_group_from_row).collect()
}

pub async fn delete_user(state: &AppState, id: DbId) -> AppResult<()> {
    recover_user_hot_wallets(state, id).await?;
    let result = sqlx::query(r#"DELETE FROM "user" WHERE id = $1"#)
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn get_user(state: &AppState, id: DbId) -> AppResult<UserRecord> {
    let row = sqlx::query(
        r#"SELECT u.id, u.email, u.status,
                  ug.id AS user_group_id, ug.code AS user_group_code, ug.name AS user_group_name,
                  COALESCE(ukw.user_key_count, 0) AS user_key_count,
                  w.balance_micro_usd + COALESCE(ukw.balance_micro_usd, 0) AS balance_micro_usd,
                  w.reserved_micro_usd + COALESCE(ukw.reserved_micro_usd, 0) AS reserved_micro_usd,
                  u.last_active_at, u.created_at, u.updated_at
           FROM "user" u
           JOIN user_group ug ON ug.id = u.user_group_id
           JOIN wallet w ON w.owner_type = 'user' AND w.owner_id = u.id
           LEFT JOIN (
               SELECT uk.user_id,
                      count(uk.id) AS user_key_count,
                      COALESCE(sum(kw.balance_micro_usd), 0)::BIGINT AS balance_micro_usd,
                      COALESCE(sum(kw.reserved_micro_usd), 0)::BIGINT AS reserved_micro_usd
               FROM user_key uk
               LEFT JOIN wallet kw ON kw.owner_type = 'user_key' AND kw.owner_id = uk.id
               GROUP BY uk.user_id
           ) ukw ON ukw.user_id = u.id
           WHERE u.id = $1"#,
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    user_from_row(&row)
}

async fn user_ids_for_api_key(state: &AppState, api_key: &str) -> AppResult<Vec<DbId>> {
    if !is_generated_user_key(api_key) {
        return Ok(Vec::new());
    }

    let rows = sqlx::query(
        "SELECT DISTINCT user_id, id, secret_ciphertext
         FROM user_key
         WHERE key_prefix = $1",
    )
    .bind(key_prefix(api_key))
    .fetch_all(&state.db.pool)
    .await?;

    let mut user_ids = Vec::new();
    for row in rows {
        let id: DbId = row.try_get("id")?;
        let user_id: DbId = row.try_get("user_id")?;
        let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
        if state.secrets.plaintext(id, &secret_ciphertext)? == api_key {
            user_ids.push(user_id);
        }
    }

    user_ids.sort_unstable();
    user_ids.dedup();
    Ok(user_ids)
}

pub async fn create_user_key(
    state: &AppState,
    req: CreateUserKeyRequest,
) -> AppResult<CreatedUserKey> {
    validate_status(&req.status)?;
    let name = normalize_user_key_name(&req.name)?;
    let key = generate_user_key();
    let secret_ciphertext = state.secrets.encrypt(&key)?;
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, name, key_prefix, secret_ciphertext, status, expires_at, model_limits)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
    )
    .bind(req.user_id)
    .bind(name)
    .bind(key_prefix(&key))
    .bind(secret_ciphertext)
    .bind(req.status)
    .bind(req.expires_at)
    .bind(req.model_limits)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    wallet::create_owner_wallet(&mut tx, WalletType::UserKey, user_key_id).await?;
    tx.commit().await?;

    Ok(CreatedUserKey {
        record: get_user_key(state, user_key_id).await?,
        key,
    })
}

pub async fn claim_public_user_key(
    state: &AppState,
    req: ClaimPublicUserKeyRequest,
) -> AppResult<()> {
    let email = normalize_email(&req.email)?;
    let api_key = consume_public_user_key_draft(state, &req.draft_id).await?;
    if !is_generated_user_key(&api_key) {
        return Err(AppError::BadRequest("invalid api key".to_string()));
    }

    let mut tx = state.db.pool.begin().await?;
    lock_public_user_key_claim(&mut tx, &email).await?;

    if user_key_exists(&mut tx, state, &api_key).await? {
        return Err(AppError::BadRequest("api key already exists".to_string()));
    }

    let (user_id, created_user) = find_or_create_user_by_email(&mut tx, &email).await?;
    if created_user {
        let wallet = wallet::owner_wallet_for_update(&mut tx, WalletType::User, user_id).await?;
        adjust_credit_in_tx(
            &mut tx,
            wallet,
            MICRO_USD_PER_USD,
            "gift",
            serde_json::json!({ "source": "public_user_key_claim" }),
        )
        .await?;
    }
    let secret_ciphertext = state.secrets.encrypt(&api_key)?;
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, key_prefix, secret_ciphertext, status, expires_at, model_limits)
        VALUES ($1, $2, $3, 'enabled', NULL, NULL)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(key_prefix(&api_key))
    .bind(secret_ciphertext)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    wallet::create_owner_wallet(&mut tx, WalletType::UserKey, user_key_id).await?;
    tx.commit().await?;

    state
        .email
        .send_api_key(
            &email,
            &api_key,
            EmailLocale::from_public_locale(req.locale.as_deref()),
        )
        .await?;

    Ok(())
}

pub async fn create_public_user_key_draft(
    state: &AppState,
) -> AppResult<PublicUserKeyDraftResponse> {
    let api_key = generate_user_key();
    let masked_api_key = mask_api_key(&api_key);
    let draft_id = issue_user_key_draft_token(
        std::time::Duration::from_secs(15 * 60),
        &state.config.admin_token_secret,
        &api_key[..MASK_HEAD_LEN],
        &api_key[api_key.len() - MASK_TAIL_LEN..],
    );

    Ok(PublicUserKeyDraftResponse {
        draft_id,
        masked_api_key,
    })
}

async fn consume_public_user_key_draft(state: &AppState, draft_id: &str) -> AppResult<String> {
    let Some((head, tail, signature)) =
        user_key_draft_parts_from_token(draft_id, &state.config.admin_token_secret)
    else {
        return Err(AppError::BadRequest(
            "invalid or expired api key draft".to_string(),
        ));
    };
    generate_user_key_from_parts_and_seed(&head, &tail, &signature)
        .ok_or_else(|| AppError::BadRequest("invalid api key draft".to_string()))
}

pub async fn list_user_keys(
    state: &AppState,
    query: ListUserKeysQuery,
) -> AppResult<Vec<UserKeyRecord>> {
    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT uk.id, uk.user_id, uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                uk.model_limits, w.balance_micro_usd, w.reserved_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN wallet w ON w.owner_type = 'user_key' AND w.owner_id = uk.id",
    );

    if let Some(user_id) = query.user_id {
        query_builder.push(" WHERE uk.user_id = ");
        query_builder.push_bind(user_id);
    }

    query_builder.push(" ORDER BY uk.created_at DESC");

    let rows = query_builder.build().fetch_all(&state.db.pool).await?;
    rows.iter()
        .map(|row| user_key_from_row(state, row))
        .collect()
}

pub async fn update_user_key(
    state: &AppState,
    id: DbId,
    req: UpdateUserKeyRequest,
) -> AppResult<UserKeyRecord> {
    if let Some(status) = &req.status {
        validate_status(status)?;
    }
    let disabling = matches!(req.status.as_deref(), Some("disabled"));
    let current = sqlx::query(
        "SELECT id, expires_at, model_limits
         FROM user_key WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let current_expires_at: Option<DateTime<Utc>> = current.try_get("expires_at")?;
    let current_model_limits: Option<Vec<String>> = current.try_get("model_limits")?;

    let expires_at = req.expires_at.unwrap_or(current_expires_at);
    let model_limits = req.model_limits.unwrap_or(current_model_limits);

    let row = sqlx::query(
        "UPDATE user_key
         SET status = COALESCE($2, status),
             expires_at = $3,
             model_limits = $4,
             updated_at = now()
         WHERE id = $1
         RETURNING id",
    )
    .bind(id)
    .bind(req.status.as_deref())
    .bind(expires_at)
    .bind(model_limits)
    .fetch_one(&state.db.pool)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    if disabling {
        let wallet = wallet::owner_wallet(&state.db.pool, WalletType::UserKey, id).await?;
        recover_hot_wallet(state, wallet).await?;
    }
    get_user_key(state, user_key_id).await
}

pub async fn delete_user_key(state: &AppState, id: DbId) -> AppResult<()> {
    let wallet = wallet::owner_wallet(&state.db.pool, WalletType::UserKey, id).await?;
    recover_hot_wallet(state, wallet).await?;
    let result = sqlx::query("DELETE FROM user_key WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn get_user_key(state: &AppState, id: DbId) -> AppResult<UserKeyRecord> {
    let row = sqlx::query(
        "SELECT uk.id, uk.user_id, uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                uk.model_limits, w.balance_micro_usd, w.reserved_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN wallet w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    user_key_from_row(state, &row)
}

pub fn validate_status(status: &str) -> AppResult<()> {
    match status {
        "enabled" | "disabled" => Ok(()),
        other => Err(AppError::BadRequest(format!("invalid status: {other}"))),
    }
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

fn normalize_user_key_name(name: &str) -> AppResult<String> {
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

fn mask_api_key(api_key: &str) -> String {
    if api_key.len() <= MASK_HEAD_LEN + MASK_TAIL_LEN {
        return api_key.to_string();
    }
    format!(
        "{}********{}",
        &api_key[..MASK_HEAD_LEN],
        &api_key[api_key.len() - MASK_TAIL_LEN..]
    )
}

const MASK_HEAD_LEN: usize = 18;
const MASK_TAIL_LEN: usize = 10;

async fn lock_public_user_key_claim(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> AppResult<()> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
        .bind(email)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn user_key_exists(
    tx: &mut Transaction<'_, Postgres>,
    state: &AppState,
    api_key: &str,
) -> AppResult<bool> {
    let rows = sqlx::query(
        "SELECT id, secret_ciphertext
         FROM user_key
         WHERE key_prefix = $1",
    )
    .bind(key_prefix(api_key))
    .fetch_all(&mut **tx)
    .await?;

    for row in rows {
        let id: DbId = row.try_get("id")?;
        let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
        if state.secrets.plaintext(id, &secret_ciphertext)? == api_key {
            return Ok(true);
        }
    }

    Ok(false)
}

async fn find_or_create_user_by_email(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> AppResult<(DbId, bool)> {
    let existing =
        sqlx::query(r#"SELECT id FROM "user" WHERE email = $1 ORDER BY created_at ASC LIMIT 1"#)
            .bind(email)
            .fetch_optional(&mut **tx)
            .await?;

    if let Some(row) = existing {
        return Ok((row.try_get("id")?, false));
    }

    let row = sqlx::query(
        r#"
        INSERT INTO "user" (email, status, user_group_id)
        VALUES ($1, 'enabled', (SELECT id FROM user_group WHERE is_default = TRUE))
        RETURNING id
        "#,
    )
    .bind(email)
    .fetch_one(&mut **tx)
    .await?;
    let user_id: DbId = row.try_get("id")?;
    wallet::create_owner_wallet(tx, WalletType::User, user_id).await?;
    Ok((user_id, true))
}

pub async fn adjust_credit(
    state: &AppState,
    wallet_type: WalletType,
    owner_id: DbId,
    amount_micro_usd: i64,
    reason: &str,
) -> AppResult<i64> {
    let mut tx = state.db.pool.begin().await?;
    let wallet = wallet::owner_wallet_for_update(&mut tx, wallet_type, owner_id).await?;
    if amount_micro_usd < 0 {
        let recovered = state.billing.drain_hot_wallet(&wallet).await?;
        recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    }
    let balance_after = adjust_credit_in_tx(
        &mut tx,
        wallet,
        amount_micro_usd,
        reason,
        serde_json::json!({ "source": "admin" }),
    )
    .await?;
    tx.commit().await?;
    Ok(balance_after)
}

async fn recover_hot_wallet(state: &AppState, wallet: WalletId) -> AppResult<()> {
    let mut tx = state.db.pool.begin().await?;
    wallet::lock_for_update(&mut tx, &wallet).await?;
    let recovered = state.billing.drain_hot_wallet(&wallet).await?;
    recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    tx.commit().await?;
    Ok(())
}

async fn recover_user_hot_wallets(state: &AppState, user_id: DbId) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT w.id
         FROM wallet w
         WHERE w.owner_type = 'user' AND w.owner_id = $1
         UNION ALL
         SELECT w.id
         FROM user_key uk
         JOIN wallet w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db.pool)
    .await?;
    for row in rows {
        recover_hot_wallet(state, WalletId::new(row.try_get("id")?)).await?;
    }
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
    let Some(wallet) = parts.first().map(|part| &part.wallet) else {
        return Ok(());
    };

    wallet::decrement_reserved(tx, wallet, total).await?;

    for part in parts {
        wallet::mark_allocation_returned(tx, part.allocation_id, part.amount_micro_usd).await?;
    }

    Ok(())
}

async fn adjust_credit_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    wallet: WalletId,
    amount_micro_usd: i64,
    reason: &str,
    metadata: serde_json::Value,
) -> AppResult<i64> {
    if amount_micro_usd == 0 {
        return Err(AppError::BadRequest(
            "amount_micro_usd cannot be zero".to_string(),
        ));
    }
    if !matches!(reason, "recharge" | "gift" | "adjustment") {
        return Err(AppError::BadRequest(format!(
            "invalid credit reason: {reason}"
        )));
    }

    let balance_after = wallet::adjust_balance(tx, &wallet, amount_micro_usd).await?;

    sqlx::query(
        "INSERT INTO credit_ledger
         (wallet_id, amount_micro_usd, balance_after_micro_usd, reason,
          transaction_id, metadata)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(wallet.id)
    .bind(amount_micro_usd)
    .bind(balance_after)
    .bind(reason)
    .bind(Uuid::new_v4())
    .bind(metadata)
    .execute(&mut **tx)
    .await?;

    Ok(balance_after)
}

pub fn user_from_row(row: &sqlx::postgres::PgRow) -> AppResult<UserRecord> {
    Ok(UserRecord {
        id: row.try_get("id")?,
        email: row.try_get("email")?,
        status: row.try_get("status")?,
        user_group_id: row.try_get("user_group_id")?,
        user_group_code: row.try_get("user_group_code")?,
        user_group_name: row.try_get("user_group_name")?,
        user_key_count: row.try_get("user_key_count")?,
        balance_micro_usd: row.try_get("balance_micro_usd")?,
        reserved_micro_usd: row.try_get("reserved_micro_usd")?,
        available_micro_usd: row.try_get::<i64, _>("balance_micro_usd")?
            - row.try_get::<i64, _>("reserved_micro_usd")?,
        last_active_at: row.try_get("last_active_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub fn user_group_from_row(row: &sqlx::postgres::PgRow) -> AppResult<UserGroupRecord> {
    Ok(UserGroupRecord {
        id: row.try_get("id")?,
        code: row.try_get("code")?,
        name: row.try_get("name")?,
        is_default: row.try_get("is_default")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

pub fn user_key_from_row(
    state: &AppState,
    row: &sqlx::postgres::PgRow,
) -> AppResult<UserKeyRecord> {
    let id = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    Ok(UserKeyRecord {
        id,
        user_id: row.try_get("user_id")?,
        name: row.try_get("name")?,
        key: state.secrets.plaintext(id, &secret_ciphertext)?,
        key_prefix: row.try_get("key_prefix")?,
        status: row.try_get("status")?,
        last_active_at: row.try_get("last_active_at")?,
        expires_at: row.try_get("expires_at")?,
        model_limits: row.try_get("model_limits")?,
        balance_micro_usd: row.try_get("balance_micro_usd")?,
        reserved_micro_usd: row.try_get("reserved_micro_usd")?,
        available_micro_usd: row.try_get::<i64, _>("balance_micro_usd")?
            - row.try_get::<i64, _>("reserved_micro_usd")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn backend_masks_public_api_key_preview() {
        assert_eq!(
            mask_api_key("sk-1234567890abcdef1234567890abcdef1234567890abcdef"),
            "sk-1234567890abcde********7890abcdef"
        );
    }

    #[test]
    fn public_key_draft_response_returns_masked_preview() {
        let draft = PublicUserKeyDraftResponse {
            draft_id: Uuid::new_v4().to_string(),
            masked_api_key: mask_api_key(&generate_user_key()),
        };
        assert!(draft.masked_api_key.starts_with("sk-"));
        assert!(draft.masked_api_key.contains("********"));
        assert!(!is_generated_user_key(&draft.masked_api_key));
    }

    #[test]
    fn public_user_key_response_does_not_include_api_key() {
        let value = serde_json::to_value(PublicUserKeyResponse { ok: true }).unwrap();

        assert_eq!(value["ok"], true);
        assert!(value.get("api_key").is_none());
        assert!(value.get("key").is_none());
    }
}
