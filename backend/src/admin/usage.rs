use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::AdminAuth,
    error::{AppError, AppResult},
    id::DbId,
    input::{bounded_limit, page_number, trimmed_non_empty},
    pagination::{created_id_cursor_page, parse_created_id_cursor},
    project::models::UsageRoutingCandidateSummary,
    AppState,
};

const USAGE_EXPORT_LIMIT: i64 = 100_000;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/usage", get(usage))
        .route("/api/admin/usage/export.csv", get(export_usage_csv))
}

#[derive(Debug, Deserialize)]
struct ListUsageParams {
    page: Option<i64>,
    limit: Option<i64>,
    cursor: Option<String>,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    query: Option<String>,
    model: Option<String>,
    project_id: Option<DbId>,
    user_id: Option<DbId>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    billing_meter: Option<String>,
    status: Option<String>,
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
    user_email: Option<String>,
    user_username: Option<String>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    channel_name: Option<String>,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    relay_trace_id: Option<Uuid>,
    relay_attempt: i32,
    relay_final: bool,
    relay_path: Option<String>,
    relay_path_index: Option<i32>,
    model: Option<String>,
    upstream_model: Option<String>,
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
    video_billing: Option<VideoBillingDetails>,
    routing: Option<UsageRouting>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
struct VideoBillingDetails {
    mode: String,
    resolution: String,
    duration_seconds: i64,
    has_video_input: bool,
    price_micros: i64,
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
    _admin: AdminAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Json<UsagePage>> {
    let page = page_number(params.page);
    let limit = bounded_limit(params.limit, 20, 500);
    let cursor = parse_created_id_cursor(params.cursor.as_deref(), "invalid usage cursor")?;
    let rows = usage_rows(&state, &params, limit + 1, cursor).await?;
    let (rows, next_cursor, has_more) = created_id_cursor_page(rows, limit)?;
    Ok(Json(UsagePage {
        items: rows.iter().map(usage_from_row).collect::<Result<_, _>>()?,
        total: rows.len() as i64,
        page,
        limit,
        next_cursor,
        has_more,
    }))
}

async fn export_usage_csv(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<ListUsageParams>,
) -> AppResult<Response> {
    let rows = usage_rows(&state, &params, USAGE_EXPORT_LIMIT + 1, None).await?;
    if rows.len() > USAGE_EXPORT_LIMIT as usize {
        return Err(AppError::BadRequestWithCode {
            code: "export_limit_exceeded",
            message: "export result exceeds 100000 rows; narrow the filters",
        });
    }

    let records = rows
        .iter()
        .map(usage_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    csv_response(
        &usage_export_filename(params.start, params.end),
        usage_csv_rows(records),
    )
}

async fn usage_rows(
    state: &AppState,
    params: &ListUsageParams,
    limit: i64,
    cursor: Option<(DateTime<Utc>, DbId)>,
) -> AppResult<Vec<sqlx::postgres::PgRow>> {
    let query =
        trimmed_non_empty(params.query.as_deref().or(params.model.as_deref())).map(str::to_string);
    let status = match params.status.as_deref() {
        Some("success") => Some("success"),
        Some("failed") => Some("failed"),
        _ => None,
    };
    let (cursor_created_at, cursor_id) = cursor.map_or((None, None), |(created_at, id)| {
        (Some(created_at), Some(id))
    });
    let query_pattern = query.as_deref().map(|value| format!("%{value}%"));
    let query_pattern = query_pattern.as_deref();

    let rows = sqlx::query(
        r#"SELECT usage_record.id, usage_record.user_id, u.email::text AS user_email,
                u.username AS user_username,
                usage_record.user_key_id, usage_record.channel_id,
                current_channel.name AS channel_name, usage_record.channel_key_id,
                usage_record.credential_id, usage_record.relay_trace_id,
                usage_record.relay_attempt, usage_record.relay_final,
                usage_record.model, usage_record.upstream_model,
                usage_record.status_code, usage_record.streamed, usage_record.latency_ms,
                usage_record.first_response_ms, usage_record.output_tokens_per_second,
                usage_record.input_tokens, usage_record.output_tokens, usage_record.total_tokens,
                usage_record.cache_in_tokens, usage_record.cache_create_in_tokens,
                usage_record.cache_create_5m_in_tokens, usage_record.cache_create_1h_in_tokens,
                usage_record.reason_out_tokens, usage_record.audio_in_tokens,
                usage_record.audio_out_tokens, usage_record.billing_meter,
                usage_record.billable_units, usage_record.cost_micros,
                usage_record.billing_status, usage_record.error_summary, usage_record.created_at,
                video_task.video_billing,
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
                usage_routing.created_at AS routing_created_at,
                rp.relay_path, rp.relay_path_index
         FROM usage AS usage_record
         LEFT JOIN "user" u ON u.id = usage_record.user_id
         LEFT JOIN channel current_channel ON current_channel.id = usage_record.channel_id
         LEFT JOIN usage_routing ON usage_routing.usage_id = usage_record.id
         LEFT JOIN LATERAL (
           SELECT task.upstream_metadata #> '{neogate,video_billing}' AS video_billing
           FROM task_upstream task
           WHERE task.task_type = 'openai_video'
             AND task.upstream_metadata #>> '{neogate,relay_trace_id}'
                 = usage_record.relay_trace_id::text
           ORDER BY task.created_at DESC, task.id DESC
           LIMIT 1
         ) video_task ON usage_record.billing_meter = 'video'
                    AND usage_record.relay_trace_id IS NOT NULL
         LEFT JOIN LATERAL (
           SELECT
             string_agg('#' || sibling.channel_id::text, ' → '
                        ORDER BY sibling.relay_attempt ASC, sibling.id ASC) AS relay_path,
             (
               SELECT count(*)::int
               FROM usage prev
               WHERE prev.relay_trace_id = usage_record.relay_trace_id
                 AND (prev.relay_attempt, prev.id)
                     < (usage_record.relay_attempt, usage_record.id)
             ) AS relay_path_index
           FROM usage sibling
           WHERE sibling.relay_trace_id = usage_record.relay_trace_id
         ) rp ON usage_record.relay_trace_id IS NOT NULL
         WHERE ($1::timestamptz IS NULL OR usage_record.created_at >= $1)
           AND ($2::timestamptz IS NULL OR usage_record.created_at <= $2)
           AND (
             $3::text IS NULL
             OR usage_record.model ILIKE $3
             OR usage_record.upstream_model ILIKE $3
             OR usage_record.relay_trace_id::text ILIKE $3
             OR usage_record.user_id::text ILIKE $3
             OR u.email::text ILIKE $3
             OR current_channel.name ILIKE $3
           )
           AND (
             $4::text IS NULL
             OR ($4 = 'success' AND usage_record.status_code >= 200 AND usage_record.status_code < 400)
             OR ($4 = 'failed' AND (usage_record.status_code >= 400 OR usage_record.error_summary IS NOT NULL))
           )
           AND ($5::BIGINT IS NULL OR usage_record.project_id = $5)
           AND ($6::BIGINT IS NULL OR usage_record.user_id = $6)
           AND ($7::BIGINT IS NULL OR usage_record.user_key_id = $7)
           AND ($8::BIGINT IS NULL OR usage_record.channel_id = $8)
           AND ($9::text IS NULL OR usage_record.billing_meter = $9)
           AND ($10::timestamptz IS NULL OR (usage_record.created_at, usage_record.id) < ($10, $11))
         ORDER BY usage_record.created_at DESC, usage_record.id DESC
         LIMIT $12"#,
    )
    .bind(params.start)
    .bind(params.end)
    .bind(query_pattern)
    .bind(status)
    .bind(params.project_id)
    .bind(params.user_id)
    .bind(params.user_key_id)
    .bind(params.channel_id)
    .bind(params.billing_meter.as_deref())
    .bind(cursor_created_at)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(rows)
}

fn usage_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageRecord, sqlx::Error> {
    Ok(UsageRecord {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        user_email: row.try_get("user_email")?,
        user_username: row.try_get("user_username")?,
        user_key_id: row.try_get("user_key_id")?,
        channel_id: row.try_get("channel_id")?,
        channel_name: row.try_get("channel_name")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        relay_trace_id: row.try_get("relay_trace_id")?,
        relay_attempt: row.try_get("relay_attempt")?,
        relay_final: row.try_get("relay_final")?,
        relay_path: row.try_get("relay_path")?,
        relay_path_index: row.try_get("relay_path_index")?,
        model: row.try_get("model")?,
        upstream_model: row.try_get("upstream_model")?,
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
        video_billing: row
            .try_get::<Option<sqlx::types::Json<VideoBillingDetails>>, _>("video_billing")?
            .map(|value| value.0),
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

fn usage_csv_rows(records: Vec<UsageRecord>) -> Vec<Vec<String>> {
    let mut rows = vec![vec![
        "id".into(),
        "created_at".into(),
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "user_key_id".into(),
        "channel_id".into(),
        "channel_key_id".into(),
        "credential_id".into(),
        "relay_trace_id".into(),
        "relay_attempt".into(),
        "relay_final".into(),
        "relay_path".into(),
        "model".into(),
        "upstream_model".into(),
        "routing_requested_model".into(),
        "routing_selected_model".into(),
        "routing_tier".into(),
        "routing_task_type".into(),
        "routing_reason_code".into(),
        "routing_fallback_reason".into(),
        "status_code".into(),
        "streamed".into(),
        "latency_ms".into(),
        "first_response_ms".into(),
        "output_tokens_per_second".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "cache_in_tokens".into(),
        "cache_create_in_tokens".into(),
        "cache_create_5m_in_tokens".into(),
        "cache_create_1h_in_tokens".into(),
        "reason_out_tokens".into(),
        "audio_in_tokens".into(),
        "audio_out_tokens".into(),
        "billing_meter".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "billing_status".into(),
        "error_summary".into(),
    ]];

    rows.extend(records.into_iter().map(|record| {
        vec![
            record.id.to_string(),
            record.created_at.to_rfc3339(),
            optional_id(record.user_id),
            record.user_email.unwrap_or_default(),
            record.user_username.unwrap_or_default(),
            optional_id(record.user_key_id),
            optional_id(record.channel_id),
            optional_id(record.channel_key_id),
            optional_id(record.credential_id),
            record
                .relay_trace_id
                .map(|value| value.to_string())
                .unwrap_or_default(),
            record.relay_attempt.to_string(),
            record.relay_final.to_string(),
            record.relay_path.unwrap_or_default(),
            record.model.unwrap_or_default(),
            record.upstream_model.unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.requested_model.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.selected_model.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.tier.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.task_type.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .map(|routing| routing.reason_code.clone())
                .unwrap_or_default(),
            record
                .routing
                .as_ref()
                .and_then(|routing| routing.fallback_reason.clone())
                .unwrap_or_default(),
            optional_i32(record.status_code),
            record.streamed.to_string(),
            record.latency_ms.to_string(),
            optional_i64(record.first_response_ms),
            optional_f64(record.output_tokens_per_second),
            optional_i64(record.input_tokens),
            optional_i64(record.output_tokens),
            optional_i64(record.total_tokens),
            optional_i64(record.cache_in_tokens),
            optional_i64(record.cache_create_in_tokens),
            optional_i64(record.cache_create_5m_in_tokens),
            optional_i64(record.cache_create_1h_in_tokens),
            optional_i64(record.reason_out_tokens),
            optional_i64(record.audio_in_tokens),
            optional_i64(record.audio_out_tokens),
            record.billing_meter,
            record.billable_units.to_string(),
            optional_i64(record.cost_micros),
            record.billing_status,
            record.error_summary.unwrap_or_default(),
        ]
    }));

    rows
}

fn usage_export_filename(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> String {
    let start = start.map_or_else(
        || "all".to_string(),
        |value| value.format("%Y%m%d%H%M%S").to_string(),
    );
    let end = end.map_or_else(
        || "all".to_string(),
        |value| value.format("%Y%m%d%H%M%S").to_string(),
    );
    format!("usage-details-{start}-{end}.csv")
}

fn optional_id(value: Option<DbId>) -> String {
    value.map(|id| id.to_string()).unwrap_or_default()
}

fn optional_i32(value: Option<i32>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_i64(value: Option<i64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn optional_f64(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.2}")).unwrap_or_default()
}

fn csv_response(filename: &str, rows: Vec<Vec<String>>) -> AppResult<Response> {
    let mut body = String::from('\u{FEFF}');
    body.push_str(
        &rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|value| escape_csv(value))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );
    body.push('\n');

    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/csv; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .map_err(|_| AppError::BadRequest("invalid export filename".to_string()))?,
    );
    Ok((headers, Body::from(body)).into_response())
}

fn escape_csv(value: &str) -> String {
    if value.contains('"') || value.contains(',') || value.contains('\n') || value.contains('\r') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}
