use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    apps::{
        ensure_supported_app_type, ensure_user_key_exists, get_app_record, list_app_records,
        normalize_status, required_trimmed, run_log_from_row, runtime_for_app, upsert_endpoint_tx,
        AppRecord, AppRunLogRecord, ListAppRunLogsQuery, ListAppsQuery, UpdateAppRequest,
        UpsertAppRequest, DEFAULT_CONTEXT_TURNS, DEFAULT_MAX_OUTPUT_TOKENS,
    },
    auth::AdminAuth,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/apps", get(list_apps).post(create_app))
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
    ensure_user_key_exists(&state, req.user_key_id).await?;

    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO app
            (name, description, app_type, status, model, system_prompt,
             context_turns, max_output_tokens, user_key_id)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
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
    .bind(req.user_key_id)
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
    let result = sqlx::query("DELETE FROM app WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
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
    let limit = query.limit.unwrap_or(100).clamp(1, 200);
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty());
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
