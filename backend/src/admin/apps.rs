use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{AssertSqlSafe, Row};

use crate::{
    apps::{
        ensure_supported_app_type, ensure_user_key_exists, get_app_record, list_app_records,
        normalize_status, required_trimmed, run_log_from_row, runtime_for_app, upsert_endpoint_tx,
        AppRecord, AppRunLogRecord, ListAppRunLogsQuery, ListAppsQuery, UpdateAppRequest,
        UpsertAppRequest, DEFAULT_CONTEXT_TURNS, DEFAULT_MAX_OUTPUT_TOKENS,
    },
    auth::{generate_user_key, key_prefix, AdminAuth},
    billing::{account, CreditAccountType, BILLABLE_PROVIDER_PRICE_CONDITION_PP},
    cache::InvalidationEvent,
    error::{AppError, AppResult},
    id::DbId,
    input::{bounded_limit, trimmed_non_empty},
    project, AppState,
};

const SYSTEM_APPS_USER_EMAIL: &str = "system-apps@neogate.local";
const SYSTEM_APPS_USER_NAME: &str = "系统应用";
const SYSTEM_APPS_PROJECT_NAME: &str = "系统应用";

#[derive(Debug, Serialize)]
struct AppModelOption {
    model: String,
    providers: Vec<String>,
    channel_count: i64,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/apps", get(list_apps).post(create_app))
        .route("/api/admin/app-model-options", get(app_model_options))
        .route(
            "/api/admin/apps/{id}",
            get(get_app_handler).patch(update_app).delete(delete_app),
        )
        .route("/api/admin/apps/{id}/test", post(test_app))
        .route("/api/admin/app-run-logs", get(app_run_logs))
}

async fn list_apps(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(query): Query<ListAppsQuery>,
) -> AppResult<Json<Vec<AppRecord>>> {
    Ok(Json(list_app_records(&state, query).await?))
}

async fn get_app_handler(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<AppRecord>> {
    Ok(Json(get_app_record(&state, id).await?))
}

async fn app_model_options(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<Vec<AppModelOption>>> {
    let rows = sqlx::query(AssertSqlSafe(format!(
        r#"
        SELECT cm.model,
               array_agg(DISTINCT c.provider ORDER BY c.provider) AS providers,
               COUNT(DISTINCT c.id)::BIGINT AS channel_count
        FROM channel_model cm
        JOIN channel c ON c.id = cm.channel_id
        JOIN provider p ON p.code = c.provider
        JOIN channel_endpoint ce ON ce.channel_id = c.id
        JOIN provider_price pp
         ON pp.provider = cm.provider
         AND pp.model = cm.model
         AND pp.enabled = TRUE
         AND {BILLABLE_PROVIDER_PRICE_CONDITION_PP}
        WHERE p.enabled = TRUE
          AND c.enabled = TRUE
          AND ce.enabled = TRUE
          AND ce.healthy = TRUE
          AND cm.enabled = TRUE
          AND cm.status = 'available'
          AND (
              cm.runtime_status = 'normal'
              OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
          )
          AND EXISTS (
              SELECT 1
              FROM unnest(ce.models) AS endpoint_model(model)
              WHERE btrim(endpoint_model.model) = cm.model
          )
          AND (
              (
                  c.use_credentials = FALSE
                  AND EXISTS (
                      SELECT 1
                      FROM channel_key ck
                      WHERE ck.channel_id = c.id
                        AND ck.enabled = TRUE
                        AND ck.healthy = TRUE
                  )
              )
              OR (
                  c.use_credentials = TRUE
                  AND EXISTS (
                      SELECT 1
                      FROM credential cr
                      WHERE cr.provider = c.provider
                        AND cr.enabled = TRUE
                  )
              )
          )
        GROUP BY cm.model
        ORDER BY COUNT(DISTINCT c.id) DESC, cm.model ASC
        "#
    )))
    .fetch_all(&state.db.pool)
    .await?;

    let options = rows
        .iter()
        .map(|row| {
            Ok(AppModelOption {
                model: row.try_get("model")?,
                providers: row.try_get("providers")?,
                channel_count: row.try_get("channel_count")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(Json(options))
}

async fn create_app(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpsertAppRequest>,
) -> AppResult<Json<AppRecord>> {
    crate::apps::validate_app_type(&req.app_type)?;
    ensure_supported_app_type(&req.app_type)?;
    let name = required_trimmed(req.name, "app name is required")?;
    let model = required_trimmed(req.model, "model is required")?;
    let status = normalize_status(req.status.as_deref().unwrap_or("enabled"))?;
    let description = req.description.unwrap_or_default().trim().to_string();
    let system_prompt = req.system_prompt.unwrap_or_default();
    let context_turns = req
        .context_turns
        .unwrap_or(DEFAULT_CONTEXT_TURNS)
        .clamp(0, 50);
    let max_output_tokens = req
        .max_output_tokens
        .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
        .clamp(1, 128000);

    let mut tx = state.db.pool.begin().await?;
    let user_key_id = create_system_app_user_key_tx(&state, &mut tx, &req.app_type, &name).await?;
    let metadata = json!({
        "auto_user_key_id": user_key_id,
        "auto_user_key_owner": "system_apps"
    });
    let row = sqlx::query(
        r#"
        INSERT INTO app
            (name, description, app_type, status, model, system_prompt,
             context_turns, max_output_tokens, user_key_id, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        RETURNING id
        "#,
    )
    .bind(name)
    .bind(description)
    .bind(&req.app_type)
    .bind(status)
    .bind(model)
    .bind(system_prompt)
    .bind(context_turns)
    .bind(max_output_tokens)
    .bind(user_key_id)
    .bind(metadata)
    .fetch_one(&mut *tx)
    .await?;
    let app_id: DbId = row.try_get("id")?;
    upsert_endpoint_tx(&state, &mut tx, app_id, &req.app_type, req.endpoint).await?;
    tx.commit().await?;
    Ok(Json(get_app_record(&state, app_id).await?))
}

async fn update_app(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
    Json(req): Json<UpdateAppRequest>,
) -> AppResult<Json<AppRecord>> {
    let current = runtime_for_app(&state, id).await?;
    let name = req
        .name
        .map(|value| required_trimmed(value, "app name is required"))
        .transpose()?;
    let model = req
        .model
        .map(|value| required_trimmed(value, "model is required"))
        .transpose()?;
    let status = req.status.as_deref().map(normalize_status).transpose()?;
    if let Some(user_key_id) = req.user_key_id {
        ensure_user_key_exists(&state, user_key_id).await?;
    }

    let mut tx = state.db.pool.begin().await?;
    sqlx::query(
        r#"
        UPDATE app
        SET name = COALESCE($2, name),
            description = COALESCE($3, description),
            status = COALESCE($4, status),
            model = COALESCE($5, model),
            system_prompt = COALESCE($6, system_prompt),
            context_turns = COALESCE($7, context_turns),
            max_output_tokens = COALESCE($8, max_output_tokens),
            user_key_id = COALESCE($9, user_key_id),
            updated_at = now()
        WHERE id = $1
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(req.description.map(|value| value.trim().to_string()))
    .bind(status)
    .bind(model)
    .bind(req.system_prompt)
    .bind(req.context_turns.map(|value| value.clamp(0, 50)))
    .bind(req.max_output_tokens.map(|value| value.clamp(1, 128000)))
    .bind(req.user_key_id)
    .execute(&mut *tx)
    .await?;

    if let Some(endpoint) = req.endpoint {
        upsert_endpoint_tx(&state, &mut tx, id, &current.endpoint_type, endpoint).await?;
    }

    tx.commit().await?;
    Ok(Json(get_app_record(&state, id).await?))
}

async fn delete_app(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query("SELECT user_key_id, metadata FROM app WHERE id = $1 FOR UPDATE")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(AppError::NotFound)?;
    let user_key_id: DbId = row.try_get("user_key_id")?;
    let metadata: Value = row.try_get("metadata")?;
    sqlx::query("DELETE FROM app WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let mut disabled_user_key_id = None;
    if auto_user_key_id(&metadata) == Some(user_key_id) {
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::BIGINT FROM app WHERE user_key_id = $1")
                .bind(user_key_id)
                .fetch_one(&mut *tx)
                .await?;
        if remaining == 0 {
            sqlx::query(
                "UPDATE user_key SET status = 'disabled', updated_at = now() WHERE id = $1",
            )
            .bind(user_key_id)
            .execute(&mut *tx)
            .await?;
            disabled_user_key_id = Some(user_key_id);
        }
    }
    tx.commit().await?;
    if let Some(user_key_id) = disabled_user_key_id {
        state
            .cache_invalidator
            .invalidate(&state, InvalidationEvent::UserKey { id: user_key_id })
            .await;
    }
    Ok(Json(json!({ "ok": true })))
}

async fn test_app(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Path(id): Path<DbId>,
) -> AppResult<Json<Value>> {
    let runtime = runtime_for_app(&state, id).await?;
    if runtime.status != "enabled" || !runtime.endpoint_enabled {
        return Err(AppError::BadRequest(
            "app or endpoint is disabled".to_string(),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "app_id": runtime.app_id,
        "endpoint_id": runtime.endpoint_id,
        "endpoint_type": runtime.endpoint_type,
    })))
}

async fn app_run_logs(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(query): Query<ListAppRunLogsQuery>,
) -> AppResult<Json<Vec<AppRunLogRecord>>> {
    let limit = bounded_limit(query.limit, 100, 200);
    let search = trimmed_non_empty(query.search.as_deref());
    let rows = sqlx::query(
        r#"
        SELECT id, app_id, endpoint_id, conversation_id, external_user_id,
               external_conversation_id, external_message_id, trace_id, app_type, model,
               status, status_code, latency_ms, input_tokens, output_tokens, total_tokens,
               cost_micro_usd, error_summary, created_at
        FROM app_run_log
        WHERE ($1::BIGINT IS NULL OR app_id = $1)
          AND ($2::BIGINT IS NULL OR endpoint_id = $2)
          AND ($3::TEXT IS NULL OR status = $3)
          AND (
            $4::TEXT IS NULL
            OR external_user_id ILIKE '%' || $4 || '%'
            OR external_conversation_id ILIKE '%' || $4 || '%'
            OR trace_id ILIKE '%' || $4 || '%'
            OR error_summary ILIKE '%' || $4 || '%'
          )
        ORDER BY created_at DESC, id DESC
        LIMIT $5
        "#,
    )
    .bind(query.app_id)
    .bind(query.endpoint_id)
    .bind(query.status)
    .bind(search)
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(Json(
        rows.iter()
            .map(run_log_from_row)
            .collect::<AppResult<Vec<_>>>()?,
    ))
}

async fn create_system_app_user_key_tx(
    state: &AppState,
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    app_type: &str,
    app_name: &str,
) -> AppResult<DbId> {
    let user_id = ensure_system_apps_user_tx(tx).await?;
    let project_id = ensure_system_apps_project_tx(tx, user_id).await?;
    let key = generate_user_key();
    let secret_ciphertext = state.secrets.encrypt(&key)?;
    let key_name = app_user_key_name(app_type, app_name);
    let row = sqlx::query(
        r#"
        INSERT INTO user_key
            (user_id, project_id, owner_user_id, name, key_prefix, secret_ciphertext,
             status, expires_at, model_limits)
        VALUES ($1, $2, $1, $3, $4, $5, 'enabled', NULL, NULL)
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(project_id)
    .bind(key_name)
    .bind(key_prefix(&key))
    .bind(secret_ciphertext)
    .fetch_one(&mut **tx)
    .await?;
    let user_key_id: DbId = row.try_get("id")?;
    account::create_credit_account(tx, CreditAccountType::UserKey, user_key_id).await?;
    Ok(user_key_id)
}

async fn ensure_system_apps_user_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> AppResult<DbId> {
    let user_group_id: DbId = sqlx::query_scalar(
        "SELECT id FROM user_group WHERE is_default = TRUE AND enabled = TRUE ORDER BY id ASC LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::BadRequest("default user group is missing".to_string()))?;
    let row = sqlx::query(
        r#"
        INSERT INTO "user" (email, username, status, password_hash, user_group_id)
        VALUES ($1, $2, 'enabled', NULL, $3)
        ON CONFLICT (email)
        DO UPDATE SET status = 'enabled',
                      username = COALESCE("user".username, EXCLUDED.username),
                      updated_at = now()
        RETURNING id
        "#,
    )
    .bind(SYSTEM_APPS_USER_EMAIL)
    .bind(SYSTEM_APPS_USER_NAME)
    .bind(user_group_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row.try_get("id")?)
}

async fn ensure_system_apps_project_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: DbId,
) -> AppResult<DbId> {
    let row = sqlx::query(
        r#"
        SELECT id
        FROM project
        WHERE owner_user_id = $1
          AND is_default = TRUE
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;

    let project_id = if let Some(row) = row {
        let project_id: DbId = row.try_get("id")?;
        sqlx::query(
            "UPDATE project SET name = $2, status = 'enabled', updated_at = now() WHERE id = $1",
        )
        .bind(project_id)
        .bind(SYSTEM_APPS_PROJECT_NAME)
        .execute(&mut **tx)
        .await?;
        project_id
    } else {
        project::ensure_default_project_for_user(tx, user_id).await?
    };

    sqlx::query(
        "UPDATE project SET name = $2, status = 'enabled', updated_at = now() WHERE id = $1",
    )
    .bind(project_id)
    .bind(SYSTEM_APPS_PROJECT_NAME)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        r#"
        INSERT INTO project_member (project_id, user_id, role)
        VALUES ($1, $2, 'owner')
        ON CONFLICT (project_id, user_id)
        DO UPDATE SET role = 'owner', updated_at = now()
        "#,
    )
    .bind(project_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    account::get_or_create_credit_account_for_update(tx, CreditAccountType::Project, project_id)
        .await?;
    Ok(project_id)
}

fn app_user_key_name(app_type: &str, app_name: &str) -> String {
    let label = match app_type {
        "wecom" => "企业微信应用",
        "webhook" => "Webhook 应用",
        "widget" => "网页组件应用",
        "feishu" => "飞书应用",
        "dingtalk" => "钉钉应用",
        _ => "应用",
    };
    let name = format!("{label} - {}", app_name.trim());
    name.chars().take(80).collect()
}

fn auto_user_key_id(metadata: &Value) -> Option<DbId> {
    metadata
        .get("auto_user_key_owner")
        .and_then(Value::as_str)
        .filter(|owner| *owner == "system_apps")?;
    metadata.get("auto_user_key_id")?.as_i64()
}
