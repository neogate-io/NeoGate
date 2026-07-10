use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::UserSessionAuth,
    error::AppResult,
    id::DbId,
    input::{bounded_limit, page_number},
    pagination::{created_id_cursor_page, parse_created_id_cursor},
    project::models::UsageRoutingCandidateSummary,
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
    relay_trace_id: Option<Uuid>,
    relay_attempt: i32,
    relay_final: bool,
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
    billing_meter: String,
    billable_units: i64,
    cost_micros: Option<i64>,
    billing_status: String,
    error_summary: Option<String>,
    routing: Option<UsageRouting>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct UsageRouting {
    id: DbId,
    project_id: DbId,
    project_model_id: Option<DbId>,
    requested_model: String,
    selected_model: String,
    selected_channel_id: Option<DbId>,
    decision_source: String,
    tier: String,
    task_type: String,
    confidence: f64,
    reason_code: String,
    matched_rule_ids: Vec<String>,
    candidate_summary: Vec<UsageRoutingCandidateSummary>,
    fallback_reason: Option<String>,
    classifier_model: Option<String>,
    latency_ms: i64,
    created_at: DateTime<Utc>,
}

async fn usage(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Json<UsagePage>> {
    let page = page_number(params.page);
    let limit = bounded_limit(params.limit, 20, 1000);
    let (cursor_created_at, cursor_id) =
        parse_created_id_cursor(params.cursor.as_deref(), "invalid usage cursor")?
            .map_or((None, None), |cursor| (Some(cursor.0), Some(cursor.1)));
    let rows = sqlx::query(
        r#"SELECT usage_record.id, usage_record.user_id, usage_record.user_key_id,
                usage_record.channel_id, usage_record.channel_key_id, usage_record.credential_id,
                usage_record.relay_trace_id, usage_record.relay_attempt, usage_record.relay_final,
                usage_record.model,
                usage_record.status_code, usage_record.streamed, usage_record.latency_ms,
                usage_record.first_response_ms, usage_record.output_tokens_per_second,
                usage_record.input_tokens, usage_record.output_tokens, usage_record.total_tokens,
                usage_record.cache_in_tokens, usage_record.cache_create_in_tokens,
                usage_record.cache_create_5m_in_tokens,
                usage_record.cache_create_1h_in_tokens, usage_record.reason_out_tokens,
                usage_record.audio_in_tokens, usage_record.audio_out_tokens,
                usage_record.billing_meter, usage_record.billable_units,
                usage_record.cost_micros, usage_record.billing_status,
                usage_record.error_summary, usage_record.created_at,
                usage_routing.id AS routing_id,
                usage_routing.project_id AS routing_project_id,
                usage_routing.project_model_id AS routing_project_model_id,
                usage_routing.requested_model AS routing_requested_model,
                usage_routing.selected_model AS routing_selected_model,
                usage_routing.selected_channel_id AS routing_selected_channel_id,
                usage_routing.decision_source AS routing_decision_source,
                usage_routing.tier AS routing_tier,
                usage_routing.task_type AS routing_task_type,
                usage_routing.confidence AS routing_confidence,
                usage_routing.reason_code AS routing_reason_code,
                usage_routing.matched_rule_ids AS routing_matched_rule_ids,
                usage_routing.candidate_summary AS routing_candidate_summary,
                usage_routing.fallback_reason AS routing_fallback_reason,
                usage_routing.classifier_model AS routing_classifier_model,
                usage_routing.latency_ms AS routing_latency_ms,
                usage_routing.created_at AS routing_created_at
         FROM usage AS usage_record
         LEFT JOIN usage_routing ON usage_routing.usage_id = usage_record.id
         WHERE usage_record.user_id = $1
           AND usage_record.billing_status IN ('billed', 'undercharged')
           AND usage_record.cost_micros IS NOT NULL
           AND ($2::timestamptz IS NULL OR usage_record.created_at >= $2)
           AND ($3::timestamptz IS NULL OR usage_record.created_at <= $3)
           AND ($4::timestamptz IS NULL OR (usage_record.created_at, usage_record.id) < ($4, $5))
         ORDER BY usage_record.created_at DESC, usage_record.id DESC
         LIMIT $6"#,
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
        relay_trace_id: row.try_get("relay_trace_id")?,
        relay_attempt: row.try_get("relay_attempt")?,
        relay_final: row.try_get("relay_final")?,
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
        billing_meter: row.try_get("billing_meter")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        billing_status: row.try_get("billing_status")?,
        error_summary: row.try_get("error_summary")?,
        routing: usage_routing_from_row(row)?,
        created_at: row.try_get("created_at")?,
    })
}

fn usage_routing_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<Option<UsageRouting>, sqlx::Error> {
    let Some(id) = row.try_get::<Option<DbId>, _>("routing_id")? else {
        return Ok(None);
    };
    Ok(Some(UsageRouting {
        id,
        project_id: row.try_get("routing_project_id")?,
        project_model_id: row.try_get("routing_project_model_id")?,
        requested_model: row.try_get("routing_requested_model")?,
        selected_model: row.try_get("routing_selected_model")?,
        selected_channel_id: row.try_get("routing_selected_channel_id")?,
        decision_source: row.try_get("routing_decision_source")?,
        tier: row.try_get("routing_tier")?,
        task_type: row.try_get("routing_task_type")?,
        confidence: row.try_get("routing_confidence")?,
        reason_code: row.try_get("routing_reason_code")?,
        matched_rule_ids: row
            .try_get::<sqlx::types::Json<Vec<String>>, _>("routing_matched_rule_ids")?
            .0,
        candidate_summary: row
            .try_get::<sqlx::types::Json<Vec<UsageRoutingCandidateSummary>>, _>(
                "routing_candidate_summary",
            )?
            .0,
        fallback_reason: row.try_get("routing_fallback_reason")?,
        classifier_model: row.try_get("routing_classifier_model")?,
        latency_ms: row.try_get("routing_latency_ms")?,
        created_at: row.try_get("routing_created_at")?,
    }))
}
