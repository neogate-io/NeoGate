use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    auth::{
        generate_user_key, generate_user_key_from_parts_and_seed, hash_user_password,
        is_generated_user_key, issue_user_key_draft_token, key_prefix,
        user_key_draft_parts_from_token, validate_user_password_input, UserAuth,
    },
    billing::{account, CreditAccountId, CreditAccountType, DebitPart, MICRO_USD_PER_USD},
    email::{smtp_config_error, EmailLocale},
    error::{AppError, AppResult},
    id::DbId,
    input::{bounded_limit, trimmed_non_empty},
    pagination::{created_id_cursor_page, parse_created_id_cursor},
    policy::{registration_policy, service_mode, ServiceMode},
    project, AppState,
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
    pub username: Option<String>,
    pub status: String,
    pub default_project_id: Option<DbId>,
    pub default_project_name: Option<String>,
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
    pub user_count: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub username: Option<String>,
    pub password: String,
    #[serde(default = "default_enabled_status")]
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub username: Option<Option<String>>,
    pub password: Option<String>,
    pub status: Option<String>,
    pub user_group_id: Option<DbId>,
}

#[derive(Debug, Serialize)]
pub struct UserKeyRecord {
    pub id: DbId,
    pub user_id: DbId,
    pub project_id: DbId,
    pub project_name: String,
    pub owner_user_id: Option<DbId>,
    pub name: String,
    pub key: String,
    pub key_prefix: String,
    pub status: String,
    pub last_active_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub balance_micro_usd: i64,
    pub reserved_micro_usd: i64,
    pub available_micro_usd: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct UserKeyModelCreditRecord {
    pub user_key_model_id: DbId,
    pub credit_account_id: DbId,
    pub balance_micro_usd: i64,
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
}

#[derive(Debug, Serialize)]
pub struct PublicUserKeyDraftResponse {
    pub draft_id: String,
    pub masked_api_key: String,
}

#[derive(Debug, Serialize)]
pub struct UserPage {
    pub items: Vec<UserRecord>,
    pub limit: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize)]
pub struct UserKeyPage {
    pub items: Vec<UserKeyRecord>,
    pub limit: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
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
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserKeyRequest {
    pub status: Option<String>,
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub search: Option<String>,
    pub email: Option<String>,
    pub api_key: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListUserKeysQuery {
    pub user_id: Option<DbId>,
    pub project_id: Option<DbId>,
    pub default_project_only: Option<bool>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
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
    let _ = auth;
    Json(UserKeyVerifyResponse { ok: true })
}

pub async fn create_user(state: &AppState, req: CreateUserRequest) -> AppResult<UserRecord> {
    validate_user_status(&req.status)?;
    validate_user_password_input(&req.password)?;
    let email = normalize_email(&req.email)?;
    let password_hash = hash_user_password(&req.password, &state.config.admin_token_secret);
    let service_mode = service_mode(state).await?;
    let username = create_user_username(req.username.as_deref(), &email)?;
    let mut tx = state.db.pool.begin().await?;
    if find_user_by_email(&mut tx, &email).await?.is_some() {
        return Err(user_email_exists_error());
    }
    let row = sqlx::query(
        r#"
        INSERT INTO "user" (email, username, status, user_group_id, password_hash)
        VALUES ($1, $2, $3, (SELECT id FROM user_group WHERE is_default = TRUE), $4)
        RETURNING id
        "#,
    )
    .bind(email)
    .bind(username)
    .bind(req.status)
    .bind(password_hash)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_user_write_error)?;
    let user_id: DbId = row.try_get("id")?;
    if service_mode == ServiceMode::Paid {
        project::ensure_default_project_for_user(&mut tx, user_id).await?;
    }
    tx.commit().await?;
    get_user(state, user_id).await
}

pub async fn list_users(state: &AppState, query: ListUsersQuery) -> AppResult<UserPage> {
    let limit = bounded_limit(query.limit, 50, 200);
    let cursor = parse_created_id_cursor(query.cursor.as_deref(), "invalid cursor")?;
    let user_search = trimmed_non_empty(query.search.as_deref()).map(str::to_ascii_lowercase);
    let email = trimmed_non_empty(query.email.as_deref()).map(str::to_ascii_lowercase);
    let api_key = trimmed_non_empty(query.api_key.as_deref()).map(ToOwned::to_owned);

    let matching_user_ids = match api_key.as_deref() {
        Some(api_key) => user_ids_for_api_key(state, api_key).await?,
        None => Vec::new(),
    };
    if api_key.is_some() && matching_user_ids.is_empty() {
        return Ok(UserPage {
            items: Vec::new(),
            limit,
            next_cursor: None,
            has_more: false,
        });
    }

    let mut query_builder = sqlx::QueryBuilder::new(
        r#"WITH page_users AS (
               SELECT u.id, u.email, u.username, u.status, u.user_group_id,
                      u.last_active_at, u.created_at, u.updated_at
               FROM "user" u"#,
    );

    let mut has_where = false;
    if let Some(search) = user_search {
        let pattern = format!("%{search}%");
        push_where_clause(&mut query_builder, &mut has_where)
            .push("(u.email::TEXT ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR u.username ILIKE ")
            .push_bind(pattern)
            .push(")");
    }

    if let Some(email) = email {
        push_where_clause(&mut query_builder, &mut has_where)
            .push("u.email::TEXT ILIKE ")
            .push_bind(format!("%{email}%"));
    }

    if !matching_user_ids.is_empty() {
        push_where_clause(&mut query_builder, &mut has_where)
            .push("u.id = ANY(")
            .push_bind(matching_user_ids)
            .push(")");
    }

    if let Some((created_at, id)) = cursor {
        push_where_clause(&mut query_builder, &mut has_where)
            .push("(u.created_at, u.id) < (")
            .push_bind(created_at)
            .push(", ")
            .push_bind(id)
            .push(")");
    }

    query_builder
        .push(" ORDER BY u.created_at DESC, u.id DESC LIMIT ")
        .push_bind(limit + 1)
        .push(
            r#"
           )
           SELECT u.id, u.email, u.username, u.status,
                  pw.default_project_id, pw.default_project_name,
                  ug.id AS user_group_id, ug.code AS user_group_code, ug.name AS user_group_name,
                  COALESCE(ukw.user_key_count, 0) AS user_key_count,
                  COALESCE(pw.balance_micro_usd, 0) AS balance_micro_usd,
                  COALESCE(pw.reserved_micro_usd, 0) AS reserved_micro_usd,
                  u.last_active_at, u.created_at, u.updated_at
           FROM page_users u
           JOIN user_group ug ON ug.id = u.user_group_id
           LEFT JOIN LATERAL (
               SELECT p.id AS default_project_id,
                      p.name AS default_project_name,
                      COALESCE(sum(w.balance_micro_usd), 0)::BIGINT AS balance_micro_usd,
                      COALESCE(sum(w.reserved_micro_usd), 0)::BIGINT AS reserved_micro_usd
               FROM project p
               LEFT JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
               WHERE p.owner_user_id = u.id AND p.is_default = TRUE
               GROUP BY p.id, p.name
           ) pw ON TRUE
           LEFT JOIN LATERAL (
               SELECT count(uk.id) AS user_key_count
               FROM user_key uk
               JOIN project p ON p.id = uk.project_id
               WHERE uk.user_id = u.id
                 AND p.owner_user_id = u.id
                 AND p.is_default = TRUE
           ) ukw ON TRUE
           ORDER BY u.created_at DESC, u.id DESC"#,
        );

    let rows = query_builder.build().fetch_all(&state.db.pool).await?;
    let (rows, next_cursor, has_more) = created_id_cursor_page(rows, limit)?;
    Ok(UserPage {
        items: rows.iter().map(user_from_row).collect::<Result<_, _>>()?,
        limit,
        next_cursor,
        has_more,
    })
}

pub async fn update_user(
    state: &AppState,
    id: DbId,
    req: UpdateUserRequest,
) -> AppResult<UserRecord> {
    if let Some(status) = &req.status {
        validate_user_status(status)?;
    }
    if let Some(user_group_id) = req.user_group_id {
        ensure_user_group_exists(state, user_group_id).await?;
    }
    let email = req.email.as_deref().map(normalize_email).transpose()?;
    let username_provided = req.username.is_some();
    let username = req
        .username
        .as_ref()
        .map(|value| value.as_deref().map(normalize_username).transpose())
        .transpose()?
        .flatten();
    let password = req.password.as_deref().filter(|value| !value.is_empty());
    if let Some(password) = password {
        validate_user_password_input(password)?;
    }
    let password_hash =
        password.map(|value| hash_user_password(value, &state.config.admin_token_secret));
    let disabling = matches!(req.status.as_deref(), Some("disabled"));
    let row = sqlx::query(
        r#"
        UPDATE "user"
        SET email = COALESCE($2, email),
            status = COALESCE($3, status),
            user_group_id = COALESCE($4, user_group_id),
            username = CASE WHEN $5 THEN $6 ELSE username END,
            password_hash = COALESCE($7, password_hash),
            password_changed_at = CASE WHEN $7 IS NULL THEN password_changed_at ELSE now() END,
            updated_at = now()
        WHERE id = $1
        RETURNING id
        "#,
    )
    .bind(id)
    .bind(email)
    .bind(req.status)
    .bind(req.user_group_id)
    .bind(username_provided)
    .bind(username)
    .bind(password_hash)
    .fetch_optional(&state.db.pool)
    .await
    .map_err(map_user_write_error)?
    .ok_or(AppError::NotFound)?;
    let user_id: DbId = row.try_get("id")?;
    if disabling {
        recover_user_hot_credit_accounts(state, id).await?;
    }
    get_user(state, user_id).await
}

async fn ensure_user_group_exists(state: &AppState, id: DbId) -> AppResult<()> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_group WHERE id = $1)")
        .bind(id)
        .fetch_one(&state.db.pool)
        .await?;
    if !exists {
        return Err(AppError::BadRequest("invalid user group".to_string()));
    }
    Ok(())
}

pub async fn list_user_groups(state: &AppState) -> AppResult<Vec<UserGroupRecord>> {
    let rows = sqlx::query(
        r#"SELECT ug.id, ug.code, ug.name, ug.is_default, ug.enabled,
                  COUNT(u.id)::bigint AS user_count,
                  ug.created_at, ug.updated_at
           FROM user_group ug
           LEFT JOIN "user" u ON u.user_group_id = ug.id
           GROUP BY ug.id, ug.code, ug.name, ug.is_default, ug.enabled, ug.created_at, ug.updated_at
           ORDER BY ug.is_default DESC, ug.created_at ASC"#,
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(user_group_from_row).collect()
}

pub async fn delete_user(state: &AppState, id: DbId) -> AppResult<()> {
    recover_user_hot_credit_accounts(state, id).await?;
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
        r#"SELECT u.id, u.email, u.username, u.status,
                  pw.default_project_id, pw.default_project_name,
                  ug.id AS user_group_id, ug.code AS user_group_code, ug.name AS user_group_name,
                  COALESCE(ukw.user_key_count, 0) AS user_key_count,
                  COALESCE(pw.balance_micro_usd, 0) AS balance_micro_usd,
                  COALESCE(pw.reserved_micro_usd, 0) AS reserved_micro_usd,
                  u.last_active_at, u.created_at, u.updated_at
           FROM "user" u
           JOIN user_group ug ON ug.id = u.user_group_id
           LEFT JOIN LATERAL (
               SELECT p.id AS default_project_id,
                      p.name AS default_project_name,
                      COALESCE(sum(w.balance_micro_usd), 0)::BIGINT AS balance_micro_usd,
                      COALESCE(sum(w.reserved_micro_usd), 0)::BIGINT AS reserved_micro_usd
               FROM project p
               LEFT JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
               WHERE p.owner_user_id = u.id AND p.is_default = TRUE
               GROUP BY p.id, p.name
           ) pw ON TRUE
           LEFT JOIN (
               SELECT uk.user_id, count(uk.id) AS user_key_count
               FROM user_key uk
               JOIN project p ON p.id = uk.project_id
                             AND p.owner_user_id = uk.user_id
                             AND p.is_default = TRUE
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
    validate_user_key_status(&req.status)?;
    if service_mode(state).await? != ServiceMode::Paid {
        return Err(AppError::BadRequest(
            "default project is only available in paid service mode".to_string(),
        ));
    }
    let name = normalize_user_key_name(&req.name)?;
    let key = generate_user_key();
    let secret_ciphertext = state.secrets.encrypt(&key)?;
    let mut tx = state.db.pool.begin().await?;
    let project_id = project::default_project_for_user(&mut tx, req.user_id).await?;
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, project_id, owner_user_id, name, key_prefix, secret_ciphertext, status, expires_at)
        VALUES ($1, $2, $1, $3, $4, $5, $6, $7)
        RETURNING id
        "#,
    )
    .bind(req.user_id)
    .bind(project_id)
    .bind(name)
    .bind(key_prefix(&key))
    .bind(secret_ciphertext)
    .bind(req.status)
    .bind(req.expires_at)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    account::create_credit_account(&mut tx, CreditAccountType::UserKey, user_key_id).await?;
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
    let (service_mode, registration_enabled) = registration_policy(state).await?;
    if service_mode != ServiceMode::Paid {
        return Err(AppError::BadRequest(
            "public api key claim is only available in paid service mode".to_string(),
        ));
    }
    if !registration_enabled {
        return Err(AppError::BadRequest("registration is closed".to_string()));
    }

    let api_key = consume_public_user_key_draft(state, &req.draft_id).await?;
    if !is_generated_user_key(&api_key) {
        return Err(AppError::BadRequest("invalid api key".to_string()));
    }

    let mut tx = state.db.pool.begin().await?;
    lock_public_user_key_claim(&mut tx, &email).await?;

    if user_key_exists(&mut tx, state, &api_key).await? {
        return Err(AppError::BadRequest("api key already exists".to_string()));
    }

    let existing_user = find_user_by_email(&mut tx, &email).await?;
    if matches!(
        existing_user.as_ref().map(|record| record.status.as_str()),
        Some("pending")
    ) {
        return Err(AppError::BadRequest("account pending approval".to_string()));
    }
    if matches!(
        existing_user.as_ref().map(|record| record.status.as_str()),
        Some("disabled")
    ) {
        return Err(AppError::Forbidden);
    }

    let (user_id, created_user) = if let Some(record) = existing_user {
        (record.id, false)
    } else {
        (
            create_user_by_email_with_status(&mut tx, &email, "enabled").await?,
            true,
        )
    };
    if created_user {
        let project_id = project::ensure_default_project_for_user(&mut tx, user_id).await?;
        let credit_account = account::owner_credit_account_for_update(
            &mut tx,
            CreditAccountType::Project,
            project_id,
        )
        .await?;
        adjust_credit_in_tx(
            &mut tx,
            credit_account,
            MICRO_USD_PER_USD,
            "gift",
            serde_json::json!({ "source": "public_user_key_claim" }),
        )
        .await?;
    }
    let secret_ciphertext = state.secrets.encrypt(&api_key)?;
    let project_id = project::default_project_for_user(&mut tx, user_id).await?;
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, project_id, owner_user_id, key_prefix, secret_ciphertext, status, expires_at)
        VALUES ($1, $2, $1, $3, $4, 'enabled', NULL)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(key_prefix(&api_key))
    .bind(secret_ciphertext)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    account::create_credit_account(&mut tx, CreditAccountType::UserKey, user_key_id).await?;
    tx.commit().await?;

    state
        .email
        .send_api_key(
            &email,
            &api_key,
            EmailLocale::from_public_locale(req.locale.as_deref()),
        )
        .await
        .map_err(email_error)?;

    Ok(())
}

pub async fn create_public_user_key_draft(
    state: &AppState,
) -> AppResult<PublicUserKeyDraftResponse> {
    let (service_mode, registration_enabled) = registration_policy(state).await?;
    if service_mode != ServiceMode::Paid {
        return Err(AppError::BadRequest(
            "public api key claim is only available in paid service mode".to_string(),
        ));
    }
    if !registration_enabled {
        return Err(AppError::BadRequest("registration is closed".to_string()));
    }

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

pub async fn list_user_keys(state: &AppState, query: ListUserKeysQuery) -> AppResult<UserKeyPage> {
    let limit = bounded_limit(query.limit, 100, 500);
    let cursor = parse_created_id_cursor(query.cursor.as_deref(), "invalid cursor")?;
    let mut query_builder = sqlx::QueryBuilder::new(
        "SELECT uk.id, uk.user_id, uk.project_id, uk.owner_user_id,
                p.name AS project_name,
                uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                w.balance_micro_usd, w.reserved_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN project p ON p.id = uk.project_id
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id",
    );

    let mut has_where = false;
    if let Some(user_id) = query.user_id {
        push_where_clause(&mut query_builder, &mut has_where).push("uk.user_id = ");
        query_builder.push_bind(user_id);
    }

    if let Some(project_id) = query.project_id {
        push_where_clause(&mut query_builder, &mut has_where).push("uk.project_id = ");
        query_builder.push_bind(project_id);
    }

    if query.default_project_only.unwrap_or(false) {
        push_where_clause(&mut query_builder, &mut has_where)
            .push("p.owner_user_id = uk.user_id AND p.is_default = TRUE");
    }

    if let Some((created_at, id)) = cursor {
        push_where_clause(&mut query_builder, &mut has_where)
            .push("(uk.created_at, uk.id) < (")
            .push_bind(created_at)
            .push(", ")
            .push_bind(id)
            .push(")");
    }

    query_builder
        .push(" ORDER BY uk.created_at DESC, uk.id DESC LIMIT ")
        .push_bind(limit + 1);

    let rows = query_builder.build().fetch_all(&state.db.pool).await?;
    let (rows, next_cursor, has_more) = created_id_cursor_page(rows, limit)?;
    Ok(UserKeyPage {
        items: rows
            .iter()
            .map(|row| user_key_from_row(state, row))
            .collect::<Result<_, _>>()?,
        limit,
        next_cursor,
        has_more,
    })
}

fn email_error(err: anyhow::Error) -> AppError {
    smtp_config_error(&err).map_or_else(
        || AppError::Anyhow(err),
        |(code, message)| AppError::BadRequestWithCode { code, message },
    )
}

fn push_where_clause<'a>(
    query_builder: &'a mut sqlx::QueryBuilder<Postgres>,
    has_where: &mut bool,
) -> &'a mut sqlx::QueryBuilder<Postgres> {
    query_builder.push(if *has_where { " AND " } else { " WHERE " });
    *has_where = true;
    query_builder
}

pub async fn update_user_key(
    state: &AppState,
    id: DbId,
    req: UpdateUserKeyRequest,
) -> AppResult<UserKeyRecord> {
    if let Some(status) = &req.status {
        validate_user_key_status(status)?;
    }
    let disabling = matches!(req.status.as_deref(), Some("disabled"));
    let current = sqlx::query(
        "SELECT id, expires_at
         FROM user_key WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let current_expires_at: Option<DateTime<Utc>> = current.try_get("expires_at")?;

    let expires_at = req.expires_at.unwrap_or(current_expires_at);

    let row = sqlx::query(
        "UPDATE user_key
         SET status = COALESCE($2, status),
             expires_at = $3,
             updated_at = now()
         WHERE id = $1
         RETURNING id",
    )
    .bind(id)
    .bind(req.status.as_deref())
    .bind(expires_at)
    .fetch_one(&state.db.pool)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    if disabling {
        recover_user_key_hot_credit_accounts(state, id).await?;
    }
    get_user_key(state, user_key_id).await
}

pub async fn delete_user_key(state: &AppState, id: DbId) -> AppResult<()> {
    recover_user_key_hot_credit_accounts(state, id).await?;
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
        "SELECT uk.id, uk.user_id, uk.project_id, uk.owner_user_id,
                p.name AS project_name,
                uk.name, uk.key_prefix, uk.secret_ciphertext,
                uk.status, uk.last_active_at, uk.expires_at,
                w.balance_micro_usd, w.reserved_micro_usd,
                uk.created_at, uk.updated_at
         FROM user_key uk
         JOIN project p ON p.id = uk.project_id
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    user_key_from_row(state, &row)
}

pub fn validate_user_status(status: &str) -> AppResult<()> {
    match status {
        "enabled" | "disabled" | "pending" => Ok(()),
        other => Err(AppError::BadRequest(format!("invalid status: {other}"))),
    }
}

pub fn validate_user_key_status(status: &str) -> AppResult<()> {
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
        return Err(AppError::BadRequestWithCode {
            code: "invalid_email",
            message: "invalid email",
        });
    }
    Ok(email)
}

fn normalize_username(username: &str) -> AppResult<String> {
    let username = username.trim();
    if username.is_empty() {
        return Err(AppError::BadRequest("username is required".to_string()));
    }
    if username.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "username must be at most 80 characters".to_string(),
        ));
    }
    Ok(username.to_string())
}

fn normalize_optional_username(username: Option<&str>) -> AppResult<Option<String>> {
    trimmed_non_empty(username)
        .map(normalize_username)
        .transpose()
}

fn create_user_username(username: Option<&str>, email: &str) -> AppResult<Option<String>> {
    let username = normalize_optional_username(username)?;
    if username.is_some() {
        return Ok(username);
    }

    let email_prefix = email.split('@').next().unwrap_or(email);
    let derived = email_prefix.chars().take(80).collect::<String>();
    Ok(Some(normalize_username(&derived)?))
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

fn normalize_model_name(model: &str) -> AppResult<String> {
    let model = model.trim();
    if model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    if model.chars().count() > 255 {
        return Err(AppError::BadRequest(
            "model must be 255 characters or fewer".to_string(),
        ));
    }
    Ok(model.to_string())
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

struct ExistingUser {
    id: DbId,
    status: String,
}

async fn find_user_by_email(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> AppResult<Option<ExistingUser>> {
    let existing = sqlx::query(
        r#"SELECT id, status FROM "user" WHERE email = $1 ORDER BY created_at ASC LIMIT 1"#,
    )
    .bind(email)
    .fetch_optional(&mut **tx)
    .await?;

    if let Some(row) = existing {
        return Ok(Some(ExistingUser {
            id: row.try_get("id")?,
            status: row.try_get("status")?,
        }));
    }

    Ok(None)
}

async fn create_user_by_email_with_status(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
    status: &str,
) -> AppResult<DbId> {
    let row = sqlx::query(
        r#"
        INSERT INTO "user" (email, status, user_group_id)
        VALUES ($1, $2, (SELECT id FROM user_group WHERE is_default = TRUE))
        RETURNING id
        "#,
    )
    .bind(email)
    .bind(status)
    .fetch_one(&mut **tx)
    .await
    .map_err(map_user_write_error)?;
    Ok(row.try_get("id")?)
}

fn map_user_write_error(err: sqlx::Error) -> AppError {
    if has_database_constraint(&err, "user_email_unique") {
        return user_email_exists_error();
    }
    AppError::Sqlx(err)
}

fn has_database_constraint(err: &sqlx::Error, constraint: &str) -> bool {
    err.as_database_error()
        .and_then(|db_error| db_error.constraint())
        == Some(constraint)
}

fn user_email_exists_error() -> AppError {
    AppError::ConflictWithCode {
        code: "user_email_exists",
        message: "user email already exists",
    }
}

pub async fn adjust_credit(
    state: &AppState,
    credit_account_type: CreditAccountType,
    owner_id: DbId,
    amount_micro_usd: i64,
    reason: &str,
) -> AppResult<i64> {
    let mut tx = state.db.pool.begin().await?;
    let credit_account =
        account::owner_credit_account_for_update(&mut tx, credit_account_type, owner_id).await?;
    if amount_micro_usd < 0 {
        let recovered = state
            .billing
            .drain_hot_credit_account(&credit_account)
            .await?;
        recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    }
    let balance_after = adjust_credit_in_tx(
        &mut tx,
        credit_account,
        amount_micro_usd,
        reason,
        serde_json::json!({ "source": "admin" }),
    )
    .await?;
    tx.commit().await?;
    Ok(balance_after)
}

pub async fn adjust_default_project_credit(
    state: &AppState,
    user_id: DbId,
    amount_micro_usd: i64,
    reason: &str,
) -> AppResult<i64> {
    let mut tx = state.db.pool.begin().await?;
    let project_id = project::default_project_for_user(&mut tx, user_id).await?;
    let credit_account =
        account::owner_credit_account_for_update(&mut tx, CreditAccountType::Project, project_id)
            .await?;
    if amount_micro_usd < 0 {
        let recovered = state
            .billing
            .drain_hot_credit_account(&credit_account)
            .await?;
        recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    }
    let balance_after = adjust_credit_in_tx(
        &mut tx,
        credit_account,
        amount_micro_usd,
        reason,
        serde_json::json!({ "source": "admin", "user_id": user_id }),
    )
    .await?;
    tx.commit().await?;
    Ok(balance_after)
}

pub async fn adjust_user_key_model_credit(
    state: &AppState,
    user_key_id: DbId,
    model: String,
    amount_micro_usd: i64,
    reason: &str,
) -> AppResult<UserKeyModelCreditRecord> {
    let model = normalize_model_name(&model)?;
    let mut tx = state.db.pool.begin().await?;
    let user_key_exists = sqlx::query("SELECT id FROM user_key WHERE id = $1 FOR KEY SHARE")
        .bind(user_key_id)
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
    if !user_key_exists {
        return Err(AppError::NotFound);
    }

    let row = sqlx::query(
        "INSERT INTO user_key_model (user_key_id, model, enabled)
         VALUES ($1, $2, TRUE)
         ON CONFLICT (user_key_id, model)
         DO UPDATE SET enabled = TRUE, updated_at = now()
         RETURNING id",
    )
    .bind(user_key_id)
    .bind(&model)
    .fetch_one(&mut *tx)
    .await?;
    let user_key_model_id: DbId = row.try_get("id")?;
    let credit_account = account::get_or_create_credit_account_for_update(
        &mut tx,
        CreditAccountType::UserKeyModel,
        user_key_model_id,
    )
    .await?;

    if amount_micro_usd < 0 {
        let recovered = state
            .billing
            .drain_hot_credit_account(&credit_account)
            .await?;
        recover_hot_credit_in_tx(&mut tx, &recovered).await?;
    }
    let balance_after = adjust_credit_in_tx(
        &mut tx,
        credit_account.clone(),
        amount_micro_usd,
        reason,
        serde_json::json!({
            "source": "admin",
            "user_key_id": user_key_id,
            "model": model.clone(),
        }),
    )
    .await?;
    tx.commit().await?;

    state
        .cache_invalidator
        .invalidate(
            state,
            crate::cache::InvalidationEvent::UserKey { id: user_key_id },
        )
        .await;

    Ok(UserKeyModelCreditRecord {
        user_key_model_id,
        credit_account_id: credit_account.id,
        balance_micro_usd: balance_after,
    })
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

async fn recover_user_hot_credit_accounts(state: &AppState, user_id: DbId) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT w.id
         FROM project p
         JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
         WHERE p.owner_user_id = $1
         UNION ALL
         SELECT w.id
         FROM user_key uk
         JOIN credit_account w ON w.owner_type = 'user_key' AND w.owner_id = uk.id
         WHERE uk.user_id = $1
         UNION ALL
         SELECT w.id
         FROM user_key uk
         JOIN user_key_model ukm ON ukm.user_key_id = uk.id
         JOIN credit_account w ON w.owner_type = 'user_key_model' AND w.owner_id = ukm.id
         WHERE uk.user_id = $1",
    )
    .bind(user_id)
    .fetch_all(&state.db.pool)
    .await?;
    recover_hot_credit_account_rows(state, rows).await
}

async fn recover_user_key_hot_credit_accounts(
    state: &AppState,
    user_key_id: DbId,
) -> AppResult<()> {
    let rows = sqlx::query(
        "SELECT w.id
         FROM credit_account w
         WHERE w.owner_type = 'user_key' AND w.owner_id = $1
         UNION ALL
         SELECT w.id
         FROM user_key_model ukm
         JOIN credit_account w ON w.owner_type = 'user_key_model' AND w.owner_id = ukm.id
         WHERE ukm.user_key_id = $1",
    )
    .bind(user_key_id)
    .fetch_all(&state.db.pool)
    .await?;
    if rows.is_empty() {
        return Err(AppError::NotFound);
    }
    recover_hot_credit_account_rows(state, rows).await
}

async fn recover_hot_credit_account_rows(
    state: &AppState,
    rows: Vec<sqlx::postgres::PgRow>,
) -> AppResult<()> {
    for row in rows {
        recover_hot_credit_account(state, CreditAccountId::new(row.try_get("id")?)).await?;
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
    let Some(credit_account) = parts.first().map(|part| &part.credit_account) else {
        return Ok(());
    };

    account::decrement_reserved(tx, credit_account, total).await?;

    for part in parts {
        account::mark_allocation_returned(tx, part.allocation_id, part.amount_micro_usd).await?;
    }

    Ok(())
}

async fn adjust_credit_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    credit_account: CreditAccountId,
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

    let balance_after = account::adjust_balance(tx, &credit_account, amount_micro_usd).await?;

    sqlx::query(
        "INSERT INTO credit_ledger
         (credit_account_id, amount_micro_usd, balance_after_micro_usd, reason,
          transaction_id, metadata)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(credit_account.id)
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
        username: row.try_get("username")?,
        status: row.try_get("status")?,
        default_project_id: row.try_get("default_project_id")?,
        default_project_name: row.try_get("default_project_name")?,
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
        user_count: row.try_get("user_count")?,
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
        project_id: row.try_get("project_id")?,
        project_name: row.try_get("project_name")?,
        owner_user_id: row.try_get("owner_user_id")?,
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

    #[test]
    fn create_user_username_derives_from_email_when_missing() {
        assert_eq!(
            create_user_username(None, "kevin@example.com").unwrap(),
            Some("kevin".to_string())
        );
        assert_eq!(
            create_user_username(Some("   "), "team.member@example.com").unwrap(),
            Some("team.member".to_string())
        );
    }

    #[test]
    fn create_user_username_keeps_explicit_username() {
        assert_eq!(
            create_user_username(Some("  Alice  "), "alice@example.com").unwrap(),
            Some("Alice".to_string())
        );
    }
}
