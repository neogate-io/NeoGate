use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    auth::UserSessionAuth,
    error::AppResult,
    id::DbId,
    pagination::{created_id_cursor_page, parse_created_id_cursor},
    AppState,
};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/user/usage", get(usage))
}

#[derive(Debug, Deserialize)]
struct ListUsageParams {
    page: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct UsagePage {
    items: Vec<UsageRecord>,
    total: i64,
    page: i64,
    limit: i64,
    next_cursor: Option<String>,
    has_more: bool,
}

#[derive(Debug, Serialize)]
struct UsageRecord {
    id: DbId,
    user_id: Option<DbId>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    provider: String,
    model: Option<String>,
    status_code: Option<i32>,
    streamed: bool,
    latency_ms: i64,
    first_response_ms: Option<i64>,
    output_tokens_per_second: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    total_tokens: Option<i64>,
    cache_in_tokens: Option<i64>,
    cache_create_in_tokens: Option<i64>,
    cache_create_5m_in_tokens: Option<i64>,
    cache_create_1h_in_tokens: Option<i64>,
    reason_out_tokens: Option<i64>,
    audio_in_tokens: Option<i64>,
    audio_out_tokens: Option<i64>,
    cost_micro_usd: Option<i64>,
    billing_status: String,
    error_summary: Option<String>,
    created_at: DateTime<Utc>,
}

async fn usage(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Json<UsagePage>> {
    let page = params.page.unwrap_or(1).max(1);
    let limit = params.limit.unwrap_or(20).clamp(1, 1000);
    let (cursor_created_at, cursor_id) =
        parse_created_id_cursor(params.cursor.as_deref(), "invalid usage cursor")?
            .map(|cursor| (Some(cursor.0), Some(cursor.1)))
            .unwrap_or((None, None));
    let rows = sqlx::query(
        "SELECT id, user_id, user_key_id, channel_id, channel_key_id, credential_id, provider, model,
                status_code, streamed, latency_ms, first_response_ms, output_tokens_per_second,
                input_tokens, output_tokens, total_tokens, cache_in_tokens,
                cache_create_in_tokens, cache_create_5m_in_tokens,
                cache_create_1h_in_tokens, reason_out_tokens, audio_in_tokens,
                audio_out_tokens,
                cost_micro_usd, billing_status, error_summary, created_at
         FROM usage
         WHERE user_id = $1
           AND billing_status IN ('billed', 'undercharged')
           AND cost_micro_usd IS NOT NULL
           AND ($2::timestamptz IS NULL OR created_at >= $2)
           AND ($3::timestamptz IS NULL OR created_at <= $3)
           AND ($4::timestamptz IS NULL OR (created_at, id) < ($4, $5))
         ORDER BY created_at DESC, id DESC
         LIMIT $6",
    )
    .bind(auth.user_id)
    .bind(params.start)
    .bind(params.end)
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit + 1)
    .fetch_all(&state.db.pool)
    .await?;

    let (rows, next_cursor, has_more) = created_id_cursor_page(rows, limit)?;

    Ok(Json(UsagePage {
        total: rows.len() as i64,
        items: rows.iter().map(usage_from_row).collect::<Result<_, _>>()?,
        page,
        limit,
        next_cursor,
        has_more,
    }))
}

fn usage_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageRecord, sqlx::Error> {
    Ok(UsageRecord {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_key_id: row.try_get("user_key_id")?,
        channel_id: row.try_get("channel_id")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        status_code: row.try_get("status_code")?,
        streamed: row.try_get("streamed")?,
        latency_ms: row.try_get("latency_ms")?,
        first_response_ms: row.try_get("first_response_ms")?,
        output_tokens_per_second: row.try_get("output_tokens_per_second")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cache_in_tokens: row.try_get("cache_in_tokens")?,
        cache_create_in_tokens: row.try_get("cache_create_in_tokens")?,
        cache_create_5m_in_tokens: row.try_get("cache_create_5m_in_tokens")?,
        cache_create_1h_in_tokens: row.try_get("cache_create_1h_in_tokens")?,
        reason_out_tokens: row.try_get("reason_out_tokens")?,
        audio_in_tokens: row.try_get("audio_in_tokens")?,
        audio_out_tokens: row.try_get("audio_out_tokens")?,
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        billing_status: row.try_get("billing_status")?,
        error_summary: row.try_get("error_summary")?,
        created_at: row.try_get("created_at")?,
    })
}
