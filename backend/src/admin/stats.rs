use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use chrono::{Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, PgPool, Row};

use crate::{
    auth::AdminAuth,
    error::{AppError, AppResult},
    id::DbId,
    input::{bounded_limit, trimmed_non_empty},
    AppState,
};

const DEFAULT_RANGE_DAYS: i64 = 30;
const MAX_RANGE_DAYS: i64 = 366;
const EXPORT_LIMIT: i64 = 100_000;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/admin/usage/statistics/summary", get(summary))
        .route("/api/admin/usage/statistics/timeseries", get(timeseries))
        .route("/api/admin/usage/statistics/users", get(users))
        .route("/api/admin/usage/statistics/user-models", get(user_models))
        .route("/api/admin/usage/statistics/models", get(models))
        .route("/api/admin/usage/statistics/projects", get(projects))
        .route("/api/admin/usage/statistics/keys", get(keys))
        .route("/api/admin/usage/statistics/options", get(options))
        .route("/api/admin/usage/statistics/export.csv", get(export_csv))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageStatsParams {
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    user_id: Option<DbId>,
    project_id: Option<DbId>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    project_query: Option<String>,
    user_query: Option<String>,
    model: Option<String>,
    billing_meter: Option<String>,
    page: Option<i64>,
    limit: Option<i64>,
    series_limit: Option<i64>,
    sort: Option<String>,
    granularity: Option<String>,
    scope: Option<String>,
    group_by: Option<String>,
}

#[derive(Debug, Clone)]
struct UsageStatsFilter {
    start: NaiveDate,
    end: NaiveDate,
    user_id: Option<DbId>,
    project_id: Option<DbId>,
    user_key_id: Option<DbId>,
    channel_id: Option<DbId>,
    project_query_pattern: Option<String>,
    user_query_pattern: Option<String>,
    model: Option<String>,
    billing_meter: Option<String>,
}

#[derive(Debug, Serialize)]
struct UsageStatsSummary {
    start: String,
    end: String,
    totals: UsageStatsAggregate,
    daily: Vec<DailyUsageStats>,
    top_users: Vec<UserUsageStats>,
    top_models: Vec<ModelUsageStats>,
}

#[derive(Debug, Serialize)]
struct UsageStatsTimeSeries {
    start: String,
    end: String,
    granularity: String,
    points: Vec<UsageTimeSeriesPoint>,
    model_points: Vec<ModelUsageTimeSeriesPoint>,
}

#[derive(Debug, Serialize)]
struct UsageStatsAggregate {
    request_count: i64,
    success_count: i64,
    error_count: i64,
    streamed_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cache_in_tokens: i64,
    cache_write_tokens: i64,
    reason_out_tokens: i64,
    audio_in_tokens: i64,
    audio_out_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
    avg_first_response_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct DailyUsageStats {
    date: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UsageTimeSeriesPoint {
    bucket: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
    avg_first_response_ms: Option<f64>,
    avg_output_tokens_per_second: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UserUsageStats {
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
    model_count: i64,
}

#[derive(Debug, Serialize)]
struct ModelUsageStats {
    channel_id: Option<DbId>,
    channel_name: String,
    model: String,
    billing_meter: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
    user_count: i64,
}

#[derive(Debug, Serialize)]
struct ModelUsageTimeSeriesPoint {
    bucket: String,
    channel_name: String,
    model: String,
    billing_meter: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
    avg_first_response_ms: Option<f64>,
    avg_output_tokens_per_second: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UserModelUsageStats {
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
    channel_name: String,
    model: String,
    billing_meter: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UsageCostBreakdown {
    chat_cost_micros: i64,
    image_cost_micros: i64,
    coding_cost_micros: i64,
    other_cost_micros: i64,
}

#[derive(Debug, Serialize)]
struct ProjectUsageStats {
    project_id: Option<DbId>,
    project_name: String,
    owner_user_id: Option<DbId>,
    member_count: i64,
    key_count: i64,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    cost_breakdown: UsageCostBreakdown,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ProjectMemberUsageStats {
    project_id: Option<DbId>,
    project_name: String,
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
    key_count: i64,
    model_count: i64,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    cost_breakdown: UsageCostBreakdown,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct KeyUsageStats {
    user_key_id: Option<DbId>,
    user_key_name: String,
    key_prefix: Option<String>,
    project_id: Option<DbId>,
    project_name: String,
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
    model_count: i64,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micros: i64,
    cost_breakdown: UsageCostBreakdown,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct UsageStatsPage<T> {
    items: Vec<T>,
    total: i64,
    page: i64,
    limit: i64,
}

#[derive(Debug, Serialize)]
struct UsageStatsOptions {
    models: Vec<ModelOption>,
    users: Vec<UserOption>,
}

#[derive(Debug, Serialize)]
struct ModelOption {
    channel_name: String,
    model: String,
}

#[derive(Debug, Serialize)]
struct UserOption {
    user_id: DbId,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
}

async fn summary(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsSummary>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let granularity = SummaryGranularity::from_param(params.granularity.as_deref())?;
    let totals = aggregate_totals(&state.db.pool, &filter).await?;
    let daily = daily_stats(&state.db.pool, &filter, granularity).await?;
    let top_users = user_stats(&state.db.pool, &filter, 1, 10, SortMode::Cost).await?;
    let top_models = model_stats(&state.db.pool, &filter, 10, SortMode::Cost).await?;

    Ok(Json(UsageStatsSummary {
        start: filter.start.to_string(),
        end: filter.end.to_string(),
        totals,
        daily,
        top_users: top_users.items,
        top_models,
    }))
}

async fn timeseries(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsTimeSeries>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let granularity = TimeGranularity::from_params(&filter, params.granularity.as_deref())?;
    let series_limit = bounded_limit(params.series_limit, 8, 20);
    let points = usage_timeseries(&state.db.pool, &filter, granularity).await?;
    let model_points =
        model_usage_timeseries(&state.db.pool, &filter, granularity, series_limit).await?;

    Ok(Json(UsageStatsTimeSeries {
        start: filter.start.to_string(),
        end: filter.end.to_string(),
        granularity: granularity.as_str().to_string(),
        points,
        model_points,
    }))
}

async fn users(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsPage<UserUsageStats>>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 100);
    let sort = SortMode::from_param(params.sort.as_deref());
    Ok(Json(
        user_stats(&state.db.pool, &filter, page, limit, sort).await?,
    ))
}

async fn user_models(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsPage<UserModelUsageStats>>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 100);
    let sort = SortMode::from_param(params.sort.as_deref());
    Ok(Json(
        user_model_stats(&state.db.pool, &filter, page, limit, sort).await?,
    ))
}

async fn models(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsPage<ModelUsageStats>>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 100);
    let sort = SortMode::from_param(params.sort.as_deref());
    Ok(Json(
        model_stats_page(&state.db.pool, &filter, page, limit, sort).await?,
    ))
}

async fn projects(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Response> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 100);
    let sort = SortMode::from_param(params.sort.as_deref());
    match ProjectGroupBy::from_param(params.group_by.as_deref())? {
        ProjectGroupBy::Project => {
            let page = project_stats(&state.db.pool, &filter, page, limit, sort).await?;
            Ok(Json(page).into_response())
        }
        ProjectGroupBy::User => {
            let page = project_member_stats(&state.db.pool, &filter, page, limit, sort).await?;
            Ok(Json(page).into_response())
        }
    }
}

async fn keys(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsPage<KeyUsageStats>>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let page = params.page.unwrap_or(1).max(1);
    let limit = bounded_limit(params.limit, 20, 100);
    let sort = SortMode::from_param(params.sort.as_deref());
    Ok(Json(
        key_stats(&state.db.pool, &filter, page, limit, sort).await?,
    ))
}

async fn options(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsOptions>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let models = option_models(&state.db.pool, &filter).await?;
    let users = option_users(&state.db.pool, &filter).await?;
    Ok(Json(UsageStatsOptions { models, users }))
}

async fn export_csv(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Response> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let sort = SortMode::from_param(params.sort.as_deref());
    let scope = ExportScope::from_param(params.scope.as_deref())?;
    let (filename, rows) = match scope {
        ExportScope::Projects => export_projects(&state.db.pool, &filter, sort).await?,
        ExportScope::ProjectMembers => {
            export_project_members(&state.db.pool, &filter, sort).await?
        }
        ExportScope::Keys => export_keys(&state.db.pool, &filter, sort).await?,
        ExportScope::Users => export_users(&state.db.pool, &filter, sort).await?,
        ExportScope::UserModels => export_user_models(&state.db.pool, &filter, sort).await?,
        ExportScope::Daily => {
            let granularity = SummaryGranularity::from_param(params.granularity.as_deref())?;
            export_daily(&state.db.pool, &filter, granularity).await?
        }
        ExportScope::Models => export_models(&state.db.pool, &filter, sort).await?,
    };
    csv_response(&filename, rows)
}

impl UsageStatsFilter {
    fn from_params(params: &UsageStatsParams) -> AppResult<Self> {
        let today = Utc::now().date_naive();
        let end = params.end.unwrap_or(today);
        let start = params
            .start
            .unwrap_or_else(|| end - Duration::days(DEFAULT_RANGE_DAYS - 1));
        if start > end {
            return Err(AppError::BadRequest(
                "start date must be before end date".to_string(),
            ));
        }
        if end.signed_duration_since(start).num_days() >= MAX_RANGE_DAYS {
            return Err(AppError::BadRequest(format!(
                "date range must be at most {MAX_RANGE_DAYS} days"
            )));
        }

        let billing_meter = trimmed_non_empty(params.billing_meter.as_deref()).map(str::to_string);
        if let Some(value) = billing_meter.as_deref() {
            if value != "token" && value != "image" {
                return Err(AppError::BadRequest("invalid billing_meter".to_string()));
            }
        }

        Ok(Self {
            start,
            end,
            user_id: params.user_id,
            project_id: params.project_id,
            user_key_id: params.user_key_id,
            channel_id: params.channel_id,
            project_query_pattern: trimmed_non_empty(params.project_query.as_deref())
                .map(|value| format!("%{value}%")),
            user_query_pattern: trimmed_non_empty(params.user_query.as_deref())
                .map(|value| format!("%{value}%")),
            model: trimmed_non_empty(params.model.as_deref()).map(str::to_string),
            billing_meter,
        })
    }
}

#[derive(Clone, Copy)]
enum SortMode {
    Cost,
    Tokens,
    Requests,
}

#[derive(Clone, Copy)]
enum TimeGranularity {
    Hour,
    Day,
    Month,
}

#[derive(Clone, Copy)]
enum SummaryGranularity {
    Day,
    Month,
}

enum ProjectGroupBy {
    Project,
    User,
}

impl ProjectGroupBy {
    fn from_param(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("project") {
            "project" => Ok(Self::Project),
            "user" => Ok(Self::User),
            _ => Err(AppError::BadRequest("invalid group_by".to_string())),
        }
    }
}

impl TimeGranularity {
    fn from_params(filter: &UsageStatsFilter, value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("auto") {
            "hour" => Ok(Self::Hour),
            "day" => Ok(Self::Day),
            "month" => Ok(Self::Month),
            "auto" => {
                if filter.end.signed_duration_since(filter.start).num_days() <= 14 {
                    Ok(Self::Hour)
                } else {
                    Ok(Self::Day)
                }
            }
            _ => Err(AppError::BadRequest("invalid granularity".to_string())),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Hour => "hour",
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    fn bucket_format(self) -> &'static str {
        match self {
            Self::Hour => "YYYY-MM-DD\"T\"HH24:00:00\"Z\"",
            Self::Day => "YYYY-MM-DD",
            Self::Month => "YYYY-MM",
        }
    }
}

impl SummaryGranularity {
    fn from_param(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("day") {
            "auto" | "hour" | "day" => Ok(Self::Day),
            "month" => Ok(Self::Month),
            _ => Err(AppError::BadRequest("invalid granularity".to_string())),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Month => "month",
        }
    }

    fn bucket_format(self) -> &'static str {
        match self {
            Self::Day => "YYYY-MM-DD",
            Self::Month => "YYYY-MM",
        }
    }
}

impl SortMode {
    fn from_param(value: Option<&str>) -> Self {
        match value {
            Some("tokens_desc") => Self::Tokens,
            Some("requests_desc") => Self::Requests,
            _ => Self::Cost,
        }
    }

    fn model_order_by(self) -> &'static str {
        match self {
            Self::Cost => "cost_micros DESC, request_count DESC, channel_name ASC, model ASC",
            Self::Tokens => "total_tokens DESC, cost_micros DESC, channel_name ASC, model ASC",
            Self::Requests => "request_count DESC, cost_micros DESC, channel_name ASC, model ASC",
        }
    }

    fn user_order_by(self) -> &'static str {
        match self {
            Self::Cost => "cost_micros DESC, request_count DESC, user_id ASC NULLS LAST",
            Self::Tokens => "total_tokens DESC, cost_micros DESC, user_id ASC NULLS LAST",
            Self::Requests => "request_count DESC, cost_micros DESC, user_id ASC NULLS LAST",
        }
    }

    fn project_order_by(self) -> &'static str {
        match self {
            Self::Cost => "cost_micros DESC, request_count DESC, project_id ASC NULLS LAST",
            Self::Tokens => "total_tokens DESC, cost_micros DESC, project_id ASC NULLS LAST",
            Self::Requests => "request_count DESC, cost_micros DESC, project_id ASC NULLS LAST",
        }
    }

    fn key_order_by(self) -> &'static str {
        match self {
            Self::Cost => "cost_micros DESC, request_count DESC, user_key_id ASC NULLS LAST",
            Self::Tokens => "total_tokens DESC, cost_micros DESC, user_key_id ASC NULLS LAST",
            Self::Requests => "request_count DESC, cost_micros DESC, user_key_id ASC NULLS LAST",
        }
    }
}

enum ExportScope {
    Projects,
    ProjectMembers,
    Keys,
    Users,
    UserModels,
    Daily,
    Models,
}

impl ExportScope {
    fn from_param(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("users") {
            "projects" => Ok(Self::Projects),
            "project_members" => Ok(Self::ProjectMembers),
            "keys" => Ok(Self::Keys),
            "users" => Ok(Self::Users),
            "user_models" => Ok(Self::UserModels),
            "daily" => Ok(Self::Daily),
            "models" => Ok(Self::Models),
            _ => Err(AppError::BadRequest("invalid export scope".to_string())),
        }
    }
}

async fn aggregate_totals(
    pool: &PgPool,
    filter: &UsageStatsFilter,
) -> AppResult<UsageStatsAggregate> {
    let row = sqlx::query(
        r#"
        SELECT
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.streamed_count), 0)::BIGINT AS streamed_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.cache_in_tokens), 0)::BIGINT AS cache_in_tokens,
          COALESCE(SUM(ud.cache_create_in_tokens + ud.cache_create_5m_in_tokens + ud.cache_create_1h_in_tokens), 0)::BIGINT AS cache_write_tokens,
          COALESCE(SUM(ud.reason_out_tokens), 0)::BIGINT AS reason_out_tokens,
          COALESCE(SUM(ud.audio_in_tokens), 0)::BIGINT AS audio_in_tokens,
          COALESCE(SUM(ud.audio_out_tokens), 0)::BIGINT AS audio_out_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          SUM(ud.first_response_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.first_response_count), 0)::DOUBLE PRECISION AS avg_first_response_ms
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_one(pool)
    .await?;

    Ok(aggregate_from_row(&row)?)
}

async fn daily_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    granularity: SummaryGranularity,
) -> AppResult<Vec<DailyUsageStats>> {
    let sql = format!(
        r#"
        SELECT
          to_char(date_trunc('{bucket}', ud.day::TIMESTAMP), '{format}') AS date,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY date_trunc('{bucket}', ud.day::TIMESTAMP)
        ORDER BY date_trunc('{bucket}', ud.day::TIMESTAMP) ASC
        "#,
        bucket = granularity.as_str(),
        format = granularity.bucket_format()
    );

    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .fetch_all(pool)
        .await?;

    rows.iter()
        .map(|row| {
            Ok(DailyUsageStats {
                date: row.try_get("date")?,
                request_count: row.try_get("request_count")?,
                success_count: row.try_get("success_count")?,
                error_count: row.try_get("error_count")?,
                input_tokens: row.try_get("input_tokens")?,
                output_tokens: row.try_get("output_tokens")?,
                total_tokens: row.try_get("total_tokens")?,
                billable_units: row.try_get("billable_units")?,
                cost_micros: row.try_get("cost_micros")?,
                avg_latency_ms: row.try_get("avg_latency_ms")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn usage_timeseries(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    granularity: TimeGranularity,
) -> AppResult<Vec<UsageTimeSeriesPoint>> {
    let sql = format!(
        r#"
        SELECT
          to_char(date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC'), '{format}') AS bucket,
          COUNT(*)::BIGINT AS request_count,
          COALESCE(SUM(CASE WHEN CASE WHEN usage_record.status_code IS NULL THEN usage_record.error_summary IS NULL ELSE usage_record.status_code >= 200 AND usage_record.status_code < 400 END THEN 1 ELSE 0 END), 0)::BIGINT AS success_count,
          COALESCE(SUM(CASE WHEN CASE WHEN usage_record.status_code IS NULL THEN usage_record.error_summary IS NULL ELSE usage_record.status_code >= 200 AND usage_record.status_code < 400 END THEN 0 ELSE 1 END), 0)::BIGINT AS error_count,
          COALESCE(SUM(usage_record.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(usage_record.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(usage_record.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(usage_record.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(usage_record.cost_micros), 0)::BIGINT AS cost_micros,
          AVG(usage_record.latency_ms)::DOUBLE PRECISION AS avg_latency_ms,
          AVG(usage_record.first_response_ms)::DOUBLE PRECISION AS avg_first_response_ms,
          AVG(usage_record.output_tokens_per_second)::DOUBLE PRECISION AS avg_output_tokens_per_second
        FROM usage AS usage_record
        LEFT JOIN project p ON p.id = usage_record.project_id
        LEFT JOIN "user" u ON u.id = usage_record.user_id
        WHERE usage_record.created_at >= $1::DATE
          AND usage_record.created_at < ($2::DATE + INTERVAL '1 day')
          AND ($3::BIGINT IS NULL OR usage_record.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR usage_record.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR usage_record.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR COALESCE(usage_record.model, '') = $6)
          AND ($7::TEXT IS NULL OR usage_record.billing_meter = $7)
        GROUP BY date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC')
        ORDER BY date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC') ASC
        "#,
        bucket = granularity.as_str(),
        format = granularity.bucket_format()
    );

    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .fetch_all(pool)
        .await?;

    rows.iter()
        .map(usage_timeseries_point_from_row)
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn model_usage_timeseries(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    granularity: TimeGranularity,
    series_limit: i64,
) -> AppResult<Vec<ModelUsageTimeSeriesPoint>> {
    let sql = format!(
        r#"
        WITH top_series AS (
          SELECT
            COALESCE(usage_record.channel_id, -1) AS channel_id_key,
            COALESCE(c.name, '') AS channel_name,
            COALESCE(usage_record.model, '') AS model,
            usage_record.billing_meter,
            COALESCE(SUM(usage_record.cost_micros), 0)::BIGINT AS cost_micros,
            COUNT(*)::BIGINT AS request_count
          FROM usage AS usage_record
          LEFT JOIN channel c ON c.id = usage_record.channel_id
          LEFT JOIN project p ON p.id = usage_record.project_id
          LEFT JOIN "user" u ON u.id = usage_record.user_id
          WHERE usage_record.created_at >= $1::DATE
            AND usage_record.created_at < ($2::DATE + INTERVAL '1 day')
            AND ($3::BIGINT IS NULL OR usage_record.user_id = $3)
            AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR usage_record.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR usage_record.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR COALESCE(usage_record.model, '') = $6)
            AND ($7::TEXT IS NULL OR usage_record.billing_meter = $7)
          GROUP BY COALESCE(usage_record.channel_id, -1), COALESCE(c.name, ''), COALESCE(usage_record.model, ''), usage_record.billing_meter
          ORDER BY cost_micros DESC, request_count DESC, channel_name ASC, model ASC
          LIMIT $8
        )
        SELECT
          to_char(date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC'), '{format}') AS bucket,
          COALESCE(c.name, '') AS channel_name,
          COALESCE(usage_record.model, '') AS model,
          usage_record.billing_meter,
          COUNT(*)::BIGINT AS request_count,
          COALESCE(SUM(CASE WHEN CASE WHEN usage_record.status_code IS NULL THEN usage_record.error_summary IS NULL ELSE usage_record.status_code >= 200 AND usage_record.status_code < 400 END THEN 1 ELSE 0 END), 0)::BIGINT AS success_count,
          COALESCE(SUM(CASE WHEN CASE WHEN usage_record.status_code IS NULL THEN usage_record.error_summary IS NULL ELSE usage_record.status_code >= 200 AND usage_record.status_code < 400 END THEN 0 ELSE 1 END), 0)::BIGINT AS error_count,
          COALESCE(SUM(usage_record.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(usage_record.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(usage_record.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(usage_record.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(usage_record.cost_micros), 0)::BIGINT AS cost_micros,
          AVG(usage_record.latency_ms)::DOUBLE PRECISION AS avg_latency_ms,
          AVG(usage_record.first_response_ms)::DOUBLE PRECISION AS avg_first_response_ms,
          AVG(usage_record.output_tokens_per_second)::DOUBLE PRECISION AS avg_output_tokens_per_second
        FROM usage AS usage_record
        LEFT JOIN channel c ON c.id = usage_record.channel_id
        LEFT JOIN project p ON p.id = usage_record.project_id
        JOIN top_series
          ON top_series.channel_id_key = COALESCE(usage_record.channel_id, -1)
         AND top_series.model = COALESCE(usage_record.model, '')
         AND top_series.billing_meter = usage_record.billing_meter
        LEFT JOIN "user" u ON u.id = usage_record.user_id
        WHERE usage_record.created_at >= $1::DATE
          AND usage_record.created_at < ($2::DATE + INTERVAL '1 day')
          AND ($3::BIGINT IS NULL OR usage_record.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR usage_record.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR usage_record.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR COALESCE(usage_record.model, '') = $6)
          AND ($7::TEXT IS NULL OR usage_record.billing_meter = $7)
        GROUP BY date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC'),
                 COALESCE(usage_record.channel_id, -1),
                 COALESCE(c.name, ''),
                 COALESCE(usage_record.model, ''),
                 usage_record.billing_meter
        ORDER BY date_trunc('{bucket}', usage_record.created_at AT TIME ZONE 'UTC') ASC,
                 channel_name ASC,
                 model ASC
        "#,
        bucket = granularity.as_str(),
        format = granularity.bucket_format()
    );

    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(series_limit)
        .fetch_all(pool)
        .await?;

    rows.iter()
        .map(model_usage_timeseries_point_from_row)
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn user_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<UserUsageStats>> {
    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM (
          SELECT ud.user_id
          FROM usage_daily ud
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR u.email::TEXT ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR ud.model = $6)
            AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
            AND ($8::BIGINT IS NULL OR ud.project_id = $8)
            AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
            AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
          GROUP BY ud.user_id
        ) grouped
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .bind(filter.project_id)
    .bind(filter.user_key_id)
    .bind(filter.channel_id)
    .fetch_one(pool)
    .await?;

    let sql = format!(
        r#"
        SELECT
          ud.user_id,
          u.email::TEXT AS user_email,
          u.username AS user_username,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          COUNT(DISTINCT NULLIF(COALESCE(c.name, '') || '/' || ud.model, '/'))::BIGINT AS model_count
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR u.email::TEXT ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
          AND ($8::BIGINT IS NULL OR ud.project_id = $8)
          AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
          AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
        GROUP BY ud.user_id, u.email, u.username
        ORDER BY {}
        OFFSET $11
        LIMIT $12
        "#,
        sort.user_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(filter.project_id)
        .bind(filter.user_key_id)
        .bind(filter.channel_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(user_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn user_model_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<UserModelUsageStats>> {
    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM (
          SELECT ud.user_id, COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
          FROM usage_daily ud
          LEFT JOIN channel c ON c.id = ud.channel_id
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR ud.model = $6)
            AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
          GROUP BY ud.user_id, COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
        ) grouped
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_one(pool)
    .await?;

    let sql = format!(
        r#"
        SELECT
          ud.user_id,
          u.email::TEXT AS user_email,
          u.username AS user_username,
          COALESCE(c.name, '') AS channel_name,
          ud.model,
          ud.billing_meter,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.user_id, u.email, u.username, COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
        ORDER BY {}
        OFFSET $8
        LIMIT $9
        "#,
        sort.model_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(user_model_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn model_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    limit: i64,
    sort: SortMode,
) -> AppResult<Vec<ModelUsageStats>> {
    Ok(model_stats_page(pool, filter, 1, limit, sort).await?.items)
}

async fn model_stats_page(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<ModelUsageStats>> {
    let total = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*)::BIGINT
        FROM (
          SELECT COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
          FROM usage_daily ud
          LEFT JOIN channel c ON c.id = ud.channel_id
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR ud.model = $6)
            AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
            AND ($8::BIGINT IS NULL OR ud.project_id = $8)
            AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
            AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
          GROUP BY COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
        ) grouped
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .bind(filter.project_id)
    .bind(filter.user_key_id)
    .bind(filter.channel_id)
    .fetch_one(pool)
    .await?;

    let sql = format!(
        r#"
        SELECT
          ud.channel_id,
          COALESCE(c.name, '') AS channel_name,
          ud.model,
          ud.billing_meter,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          COUNT(DISTINCT ud.user_id)::BIGINT AS user_count
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
          AND ($8::BIGINT IS NULL OR ud.project_id = $8)
          AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
          AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
        GROUP BY ud.channel_id, COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model, ud.billing_meter
        ORDER BY {}
        OFFSET $11
        LIMIT $12
        "#,
        sort.model_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(filter.project_id)
        .bind(filter.user_key_id)
        .bind(filter.channel_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(model_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn project_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<ProjectUsageStats>> {
    let sql = format!(
        r#"
        WITH grouped AS (
          SELECT
            ud.project_id,
            COALESCE(p.name, CASE
              WHEN ud.project_id IS NULL THEN 'Unknown project'
              ELSE 'Deleted project #' || ud.project_id::TEXT
            END) AS project_name,
            p.owner_user_id,
            COUNT(DISTINCT ud.user_id)::BIGINT AS member_count,
            COUNT(DISTINCT ud.user_key_id)::BIGINT AS key_count,
            COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
            COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
            COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
            COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
            COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
            COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
            COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
            COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'token' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS chat_cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'image' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS image_cost_micros,
            0::BIGINT AS coding_cost_micros,
            0::BIGINT AS other_cost_micros,
            SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
          FROM usage_daily ud
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::BIGINT IS NULL OR ud.project_id = $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR u.username ILIKE $6 OR u.email::TEXT ILIKE $6 OR ud.user_id::TEXT ILIKE $6)
            AND ($7::TEXT IS NULL OR ud.model = $7)
            AND ($8::TEXT IS NULL OR ud.billing_meter = $8)
            AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
            AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
          GROUP BY ud.project_id, p.name, p.owner_user_id
        )
        SELECT grouped.*, COUNT(*) OVER()::BIGINT AS total
        FROM grouped
        ORDER BY {}
        OFFSET $11
        LIMIT $12
        "#,
        sort.project_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.project_id)
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(filter.user_key_id)
        .bind(filter.channel_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    let total = rows
        .first()
        .map(|row| row.try_get("total"))
        .transpose()?
        .unwrap_or(0);

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(project_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn project_member_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<ProjectMemberUsageStats>> {
    let sql = format!(
        r#"
        WITH grouped AS (
          SELECT
            ud.project_id,
            COALESCE(p.name, CASE
              WHEN ud.project_id IS NULL THEN 'Unknown project'
              ELSE 'Deleted project #' || ud.project_id::TEXT
            END) AS project_name,
            ud.user_id,
            u.email::TEXT AS user_email,
            u.username AS user_username,
            COUNT(DISTINCT ud.user_key_id)::BIGINT AS key_count,
            COUNT(DISTINCT NULLIF(COALESCE(c.name, '') || '/' || ud.model, '/'))::BIGINT AS model_count,
            COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
            COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
            COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
            COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
            COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
            COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
            COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
            COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'token' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS chat_cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'image' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS image_cost_micros,
            0::BIGINT AS coding_cost_micros,
            0::BIGINT AS other_cost_micros,
            SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
          FROM usage_daily ud
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN channel c ON c.id = ud.channel_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::BIGINT IS NULL OR ud.project_id = $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR u.username ILIKE $6 OR u.email::TEXT ILIKE $6 OR ud.user_id::TEXT ILIKE $6)
            AND ($7::TEXT IS NULL OR ud.model = $7)
            AND ($8::TEXT IS NULL OR ud.billing_meter = $8)
            AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
            AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
          GROUP BY ud.project_id, p.name, ud.user_id, u.email, u.username
        )
        SELECT grouped.*, COUNT(*) OVER()::BIGINT AS total
        FROM grouped
        ORDER BY {}
        OFFSET $11
        LIMIT $12
        "#,
        sort.user_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.project_id)
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(filter.user_key_id)
        .bind(filter.channel_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    let total = rows
        .first()
        .map(|row| row.try_get("total"))
        .transpose()?
        .unwrap_or(0);

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(project_member_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn key_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    page: i64,
    limit: i64,
    sort: SortMode,
) -> AppResult<UsageStatsPage<KeyUsageStats>> {
    let sql = format!(
        r#"
        WITH grouped AS (
          SELECT
            ud.user_key_id,
            COALESCE(uk.name, CASE
              WHEN ud.user_key_id IS NULL THEN 'Unknown key'
              ELSE 'Deleted key #' || ud.user_key_id::TEXT
            END) AS user_key_name,
            uk.key_prefix,
            ud.project_id,
            COALESCE(p.name, CASE
              WHEN ud.project_id IS NULL THEN 'Unknown project'
              ELSE 'Deleted project #' || ud.project_id::TEXT
            END) AS project_name,
            ud.user_id,
            u.email::TEXT AS user_email,
            u.username AS user_username,
            COUNT(DISTINCT NULLIF(COALESCE(c.name, '') || '/' || ud.model, '/'))::BIGINT AS model_count,
            COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
            COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
            COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
            COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
            COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
            COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
            COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
            COALESCE(SUM(ud.cost_micros), 0)::BIGINT AS cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'token' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS chat_cost_micros,
            COALESCE(SUM(CASE WHEN ud.billing_meter = 'image' THEN ud.cost_micros ELSE 0 END), 0)::BIGINT AS image_cost_micros,
            0::BIGINT AS coding_cost_micros,
            0::BIGINT AS other_cost_micros,
            SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
          FROM usage_daily ud
          LEFT JOIN user_key uk ON uk.id = ud.user_key_id
          LEFT JOIN project p ON p.id = ud.project_id
          LEFT JOIN channel c ON c.id = ud.channel_id
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::BIGINT IS NULL OR ud.project_id = $4)
            AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
            AND ($6::TEXT IS NULL OR u.username ILIKE $6 OR u.email::TEXT ILIKE $6 OR ud.user_id::TEXT ILIKE $6)
            AND ($7::TEXT IS NULL OR ud.model = $7)
            AND ($8::TEXT IS NULL OR ud.billing_meter = $8)
            AND ($9::BIGINT IS NULL OR ud.user_key_id = $9)
            AND ($10::BIGINT IS NULL OR ud.channel_id = $10)
          GROUP BY ud.user_key_id, uk.name, uk.key_prefix, ud.project_id, p.name, ud.user_id, u.email, u.username
        )
        SELECT grouped.*, COUNT(*) OVER()::BIGINT AS total
        FROM grouped
        ORDER BY {}
        OFFSET $11
        LIMIT $12
        "#,
        sort.key_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.project_id)
        .bind(filter.project_query_pattern.as_deref())
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(filter.user_key_id)
        .bind(filter.channel_id)
        .bind(offset)
        .bind(limit)
        .fetch_all(pool)
        .await?;
    let total = rows
        .first()
        .map(|row| row.try_get("total"))
        .transpose()?
        .unwrap_or(0);

    Ok(UsageStatsPage {
        items: rows
            .iter()
            .map(key_stats_from_row)
            .collect::<Result<_, sqlx::Error>>()?,
        total,
        page,
        limit,
    })
}

async fn option_models(pool: &PgPool, filter: &UsageStatsFilter) -> AppResult<Vec<ModelOption>> {
    let rows = sqlx::query(
        r#"
        SELECT COALESCE(c.name, '') AS channel_name, ud.model
        FROM usage_daily ud
        LEFT JOIN channel c ON c.id = ud.channel_id
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ud.model <> ''
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.billing_meter = $6)
        GROUP BY COALESCE(ud.channel_id, -1), COALESCE(c.name, ''), ud.model
        ORDER BY channel_name ASC, ud.model ASC
        LIMIT 500
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(ModelOption {
                channel_name: row.try_get("channel_name")?,
                model: row.try_get("model")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn option_users(pool: &PgPool, filter: &UsageStatsFilter) -> AppResult<Vec<UserOption>> {
    let rows = sqlx::query(
        r#"
        SELECT
          ud.user_id,
          u.email::TEXT AS user_email,
          u.username AS user_username,
          MAX(ud.day) AS last_day,
          SUM(ud.cost_micros) AS cost_micros
        FROM usage_daily ud
        LEFT JOIN project p ON p.id = ud.project_id
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ud.user_id IS NOT NULL
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR p.name ILIKE $5 OR ud.project_id::TEXT ILIKE $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.user_id, u.email, u.username
        ORDER BY last_day DESC, cost_micros DESC, ud.user_id ASC
        LIMIT 50
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.project_query_pattern.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            let user_id: DbId = row.try_get("user_id")?;
            let user_email = row.try_get("user_email")?;
            let user_username = row.try_get("user_username")?;
            Ok(UserOption {
                user_id,
                user_display_name: user_display_name(Some(user_id), &user_email, &user_username),
                user_email,
                user_username,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn export_projects(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let page = project_stats(pool, filter, 1, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(page.items.len())?;
    let mut rows = vec![vec![
        "project_id".into(),
        "project_name".into(),
        "owner_user_id".into(),
        "member_count".into(),
        "key_count".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "chat_cost_micros".into(),
        "image_cost_micros".into(),
        "coding_cost_micros".into(),
        "other_cost_micros".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.project_id),
            item.project_name,
            optional_id(item.owner_user_id),
            item.member_count.to_string(),
            item.key_count.to_string(),
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            item.cost_breakdown.chat_cost_micros.to_string(),
            item.cost_breakdown.image_cost_micros.to_string(),
            item.cost_breakdown.coding_cost_micros.to_string(),
            item.cost_breakdown.other_cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("projects", filter), rows))
}

async fn export_project_members(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let page = project_member_stats(pool, filter, 1, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(page.items.len())?;
    let mut rows = vec![vec![
        "project_id".into(),
        "project_name".into(),
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "key_count".into(),
        "model_count".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "chat_cost_micros".into(),
        "image_cost_micros".into(),
        "coding_cost_micros".into(),
        "other_cost_micros".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.project_id),
            item.project_name,
            optional_id(item.user_id),
            item.user_email.unwrap_or_default(),
            item.user_username.unwrap_or_default(),
            item.key_count.to_string(),
            item.model_count.to_string(),
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            item.cost_breakdown.chat_cost_micros.to_string(),
            item.cost_breakdown.image_cost_micros.to_string(),
            item.cost_breakdown.coding_cost_micros.to_string(),
            item.cost_breakdown.other_cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("project-members", filter), rows))
}

async fn export_keys(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let page = key_stats(pool, filter, 1, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(page.items.len())?;
    let mut rows = vec![vec![
        "user_key_id".into(),
        "user_key_name".into(),
        "key_prefix".into(),
        "project_id".into(),
        "project_name".into(),
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "model_count".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "chat_cost_micros".into(),
        "image_cost_micros".into(),
        "coding_cost_micros".into(),
        "other_cost_micros".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.user_key_id),
            item.user_key_name,
            item.key_prefix.unwrap_or_default(),
            optional_id(item.project_id),
            item.project_name,
            optional_id(item.user_id),
            item.user_email.unwrap_or_default(),
            item.user_username.unwrap_or_default(),
            item.model_count.to_string(),
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            item.cost_breakdown.chat_cost_micros.to_string(),
            item.cost_breakdown.image_cost_micros.to_string(),
            item.cost_breakdown.coding_cost_micros.to_string(),
            item.cost_breakdown.other_cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("keys", filter), rows))
}

async fn export_users(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let page = user_stats(pool, filter, 1, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(page.items.len())?;
    let mut rows = vec![vec![
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "avg_latency_ms".into(),
        "model_count".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.user_id),
            item.user_email.unwrap_or_default(),
            item.user_username.unwrap_or_default(),
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
            item.model_count.to_string(),
        ]
    }));
    Ok((export_filename("users", filter), rows))
}

async fn export_user_models(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let page = user_model_stats(pool, filter, 1, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(page.items.len())?;
    let mut rows = vec![vec![
        "user_id".into(),
        "user_email".into(),
        "user_username".into(),
        "channel_name".into(),
        "model".into(),
        "billing_meter".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.user_id),
            item.user_email.unwrap_or_default(),
            item.user_username.unwrap_or_default(),
            item.channel_name,
            item.model,
            item.billing_meter,
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("user-models", filter), rows))
}

async fn export_daily(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    granularity: SummaryGranularity,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let items = daily_stats(pool, filter, granularity).await?;
    ensure_export_limit(items.len())?;
    let mut rows = vec![vec![
        "period".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(items.into_iter().map(|item| {
        vec![
            item.date,
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("trend", filter), rows))
}

async fn export_models(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let items = model_stats(pool, filter, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(items.len())?;
    let mut rows = vec![vec![
        "channel_id".into(),
        "channel_name".into(),
        "model".into(),
        "billing_meter".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micros".into(),
        "avg_latency_ms".into(),
        "user_count".into(),
    ]];
    rows.extend(items.into_iter().map(|item| {
        vec![
            optional_id(item.channel_id),
            item.channel_name,
            item.model,
            item.billing_meter,
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micros.to_string(),
            optional_f64(item.avg_latency_ms),
            item.user_count.to_string(),
        ]
    }));
    Ok((export_filename("models", filter), rows))
}

fn aggregate_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageStatsAggregate, sqlx::Error> {
    Ok(UsageStatsAggregate {
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        streamed_count: row.try_get("streamed_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        cache_in_tokens: row.try_get("cache_in_tokens")?,
        cache_write_tokens: row.try_get("cache_write_tokens")?,
        reason_out_tokens: row.try_get("reason_out_tokens")?,
        audio_in_tokens: row.try_get("audio_in_tokens")?,
        audio_out_tokens: row.try_get("audio_out_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        avg_first_response_ms: row.try_get("avg_first_response_ms")?,
    })
}

fn usage_timeseries_point_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<UsageTimeSeriesPoint, sqlx::Error> {
    Ok(UsageTimeSeriesPoint {
        bucket: row.try_get("bucket")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        avg_first_response_ms: row.try_get("avg_first_response_ms")?,
        avg_output_tokens_per_second: row.try_get("avg_output_tokens_per_second")?,
    })
}

fn user_stats_from_row(row: &sqlx::postgres::PgRow) -> Result<UserUsageStats, sqlx::Error> {
    let user_id = row.try_get("user_id")?;
    let user_email = row.try_get("user_email")?;
    let user_username = row.try_get("user_username")?;
    Ok(UserUsageStats {
        user_id,
        user_display_name: user_display_name(user_id, &user_email, &user_username),
        user_email,
        user_username,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        model_count: row.try_get("model_count")?,
    })
}

fn model_stats_from_row(row: &sqlx::postgres::PgRow) -> Result<ModelUsageStats, sqlx::Error> {
    Ok(ModelUsageStats {
        channel_id: row.try_get("channel_id")?,
        channel_name: row.try_get("channel_name")?,
        model: row.try_get("model")?,
        billing_meter: row.try_get("billing_meter")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        user_count: row.try_get("user_count")?,
    })
}

fn model_usage_timeseries_point_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ModelUsageTimeSeriesPoint, sqlx::Error> {
    Ok(ModelUsageTimeSeriesPoint {
        bucket: row.try_get("bucket")?,
        channel_name: row.try_get("channel_name")?,
        model: row.try_get("model")?,
        billing_meter: row.try_get("billing_meter")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        avg_first_response_ms: row.try_get("avg_first_response_ms")?,
        avg_output_tokens_per_second: row.try_get("avg_output_tokens_per_second")?,
    })
}

fn user_model_stats_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<UserModelUsageStats, sqlx::Error> {
    let user_id = row.try_get("user_id")?;
    let user_email = row.try_get("user_email")?;
    let user_username = row.try_get("user_username")?;
    Ok(UserModelUsageStats {
        user_id,
        user_display_name: user_display_name(user_id, &user_email, &user_username),
        user_email,
        user_username,
        channel_name: row.try_get("channel_name")?,
        model: row.try_get("model")?,
        billing_meter: row.try_get("billing_meter")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
    })
}

fn project_stats_from_row(row: &sqlx::postgres::PgRow) -> Result<ProjectUsageStats, sqlx::Error> {
    Ok(ProjectUsageStats {
        project_id: row.try_get("project_id")?,
        project_name: row.try_get("project_name")?,
        owner_user_id: row.try_get("owner_user_id")?,
        member_count: row.try_get("member_count")?,
        key_count: row.try_get("key_count")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        cost_breakdown: cost_breakdown_from_row(row)?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
    })
}

fn project_member_stats_from_row(
    row: &sqlx::postgres::PgRow,
) -> Result<ProjectMemberUsageStats, sqlx::Error> {
    let user_id = row.try_get("user_id")?;
    let user_email = row.try_get("user_email")?;
    let user_username = row.try_get("user_username")?;
    Ok(ProjectMemberUsageStats {
        project_id: row.try_get("project_id")?,
        project_name: row.try_get("project_name")?,
        user_id,
        user_display_name: user_display_name(user_id, &user_email, &user_username),
        user_email,
        user_username,
        key_count: row.try_get("key_count")?,
        model_count: row.try_get("model_count")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        cost_breakdown: cost_breakdown_from_row(row)?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
    })
}

fn key_stats_from_row(row: &sqlx::postgres::PgRow) -> Result<KeyUsageStats, sqlx::Error> {
    let user_id = row.try_get("user_id")?;
    let user_email = row.try_get("user_email")?;
    let user_username = row.try_get("user_username")?;
    Ok(KeyUsageStats {
        user_key_id: row.try_get("user_key_id")?,
        user_key_name: row.try_get("user_key_name")?,
        key_prefix: row.try_get("key_prefix")?,
        project_id: row.try_get("project_id")?,
        project_name: row.try_get("project_name")?,
        user_id,
        user_display_name: user_display_name(user_id, &user_email, &user_username),
        user_email,
        user_username,
        model_count: row.try_get("model_count")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micros: row.try_get("cost_micros")?,
        cost_breakdown: cost_breakdown_from_row(row)?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
    })
}

fn cost_breakdown_from_row(row: &sqlx::postgres::PgRow) -> Result<UsageCostBreakdown, sqlx::Error> {
    Ok(UsageCostBreakdown {
        chat_cost_micros: row.try_get("chat_cost_micros")?,
        image_cost_micros: row.try_get("image_cost_micros")?,
        coding_cost_micros: row.try_get("coding_cost_micros")?,
        other_cost_micros: row.try_get("other_cost_micros")?,
    })
}

fn user_display_name(
    user_id: Option<DbId>,
    email: &Option<String>,
    username: &Option<String>,
) -> String {
    if let Some(username) = username.as_deref().filter(|value| !value.is_empty()) {
        return username.to_string();
    }
    if let Some(email) = email.as_deref().filter(|value| !value.is_empty()) {
        return email.to_string();
    }
    user_id.map_or_else(
        || "Unknown user".to_string(),
        |id| format!("Deleted user #{id}"),
    )
}

fn ensure_export_limit(len: usize) -> AppResult<()> {
    if len > EXPORT_LIMIT as usize {
        return Err(AppError::BadRequestWithCode {
            code: "export_limit_exceeded",
            message: "export result exceeds 100000 rows; narrow the filters",
        });
    }
    Ok(())
}

fn export_filename(scope: &str, filter: &UsageStatsFilter) -> String {
    format!(
        "usage-statistics-{scope}-{}-{}.csv",
        filter.start, filter.end
    )
}

fn optional_id(value: Option<DbId>) -> String {
    value.map(|id| id.to_string()).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_values_are_escaped() {
        assert_eq!(escape_csv("plain"), "plain");
        assert_eq!(escape_csv("a,b"), "\"a,b\"");
        assert_eq!(escape_csv("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn default_range_is_inclusive_thirty_days() {
        let end = NaiveDate::from_ymd_opt(2026, 6, 26).unwrap();
        let params = UsageStatsParams {
            start: None,
            end: Some(end),
            user_id: None,
            project_id: None,
            user_key_id: None,
            channel_id: None,
            project_query: None,
            user_query: None,
            model: None,
            billing_meter: None,
            page: None,
            limit: None,
            series_limit: None,
            sort: None,
            granularity: None,
            scope: None,
            group_by: None,
        };
        let filter = UsageStatsFilter::from_params(&params).unwrap();
        assert_eq!(filter.start, NaiveDate::from_ymd_opt(2026, 5, 28).unwrap());
        assert_eq!(filter.end, end);
    }

    #[test]
    fn automatic_timeseries_granularity_prefers_hour_for_short_ranges() {
        let filter = UsageStatsFilter {
            start: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 14).unwrap(),
            user_id: None,
            project_id: None,
            user_key_id: None,
            channel_id: None,
            project_query_pattern: None,
            user_query_pattern: None,
            model: None,
            billing_meter: None,
        };
        assert!(matches!(
            TimeGranularity::from_params(&filter, Some("auto")).unwrap(),
            TimeGranularity::Hour
        ));
    }

    #[test]
    fn automatic_timeseries_granularity_prefers_day_for_long_ranges() {
        let filter = UsageStatsFilter {
            start: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            user_id: None,
            project_id: None,
            user_key_id: None,
            channel_id: None,
            project_query_pattern: None,
            user_query_pattern: None,
            model: None,
            billing_meter: None,
        };
        assert!(matches!(
            TimeGranularity::from_params(&filter, Some("auto")).unwrap(),
            TimeGranularity::Day
        ));
    }

    #[test]
    fn explicit_month_granularity_is_supported() {
        let filter = UsageStatsFilter {
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            user_id: None,
            project_id: None,
            user_key_id: None,
            channel_id: None,
            project_query_pattern: None,
            user_query_pattern: None,
            model: None,
            billing_meter: None,
        };
        assert!(matches!(
            TimeGranularity::from_params(&filter, Some("month")).unwrap(),
            TimeGranularity::Month
        ));
        assert!(matches!(
            SummaryGranularity::from_param(Some("month")).unwrap(),
            SummaryGranularity::Month
        ));
    }
}
