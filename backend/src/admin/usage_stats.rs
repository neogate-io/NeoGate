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
        .route("/api/admin/usage/statistics/users", get(users))
        .route("/api/admin/usage/statistics/user-models", get(user_models))
        .route("/api/admin/usage/statistics/options", get(options))
        .route("/api/admin/usage/statistics/export.csv", get(export_csv))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UsageStatsParams {
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    user_id: Option<DbId>,
    user_query: Option<String>,
    provider: Option<String>,
    model: Option<String>,
    billing_meter: Option<String>,
    page: Option<i64>,
    limit: Option<i64>,
    sort: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Clone)]
struct UsageStatsFilter {
    start: NaiveDate,
    end: NaiveDate,
    user_id: Option<DbId>,
    user_query_pattern: Option<String>,
    provider: Option<String>,
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
    providers: Vec<ProviderUsageStats>,
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
    cost_micro_usd: i64,
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
    cost_micro_usd: i64,
    avg_latency_ms: Option<f64>,
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
    cost_micro_usd: i64,
    avg_latency_ms: Option<f64>,
    model_count: i64,
}

#[derive(Debug, Serialize)]
struct ModelUsageStats {
    provider: String,
    model: String,
    billing_meter: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micro_usd: i64,
    avg_latency_ms: Option<f64>,
    user_count: i64,
}

#[derive(Debug, Serialize)]
struct UserModelUsageStats {
    user_id: Option<DbId>,
    user_email: Option<String>,
    user_username: Option<String>,
    user_display_name: String,
    provider: String,
    model: String,
    billing_meter: String,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micro_usd: i64,
    avg_latency_ms: Option<f64>,
}

#[derive(Debug, Serialize)]
struct ProviderUsageStats {
    provider: String,
    request_count: i64,
    total_tokens: i64,
    billable_units: i64,
    cost_micro_usd: i64,
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
    providers: Vec<String>,
    models: Vec<ModelOption>,
    users: Vec<UserOption>,
}

#[derive(Debug, Serialize)]
struct ModelOption {
    provider: String,
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
    let totals = aggregate_totals(&state.db.pool, &filter).await?;
    let daily = daily_stats(&state.db.pool, &filter).await?;
    let top_users = user_stats(&state.db.pool, &filter, 1, 10, SortMode::CostDesc).await?;
    let top_models = model_stats(&state.db.pool, &filter, 10, SortMode::CostDesc).await?;
    let providers = provider_stats(&state.db.pool, &filter).await?;

    Ok(Json(UsageStatsSummary {
        start: filter.start.to_string(),
        end: filter.end.to_string(),
        totals,
        daily,
        top_users: top_users.items,
        top_models,
        providers,
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

async fn options(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Query(params): Query<UsageStatsParams>,
) -> AppResult<Json<UsageStatsOptions>> {
    let filter = UsageStatsFilter::from_params(&params)?;
    let providers = option_providers(&state.db.pool, &filter).await?;
    let models = option_models(&state.db.pool, &filter).await?;
    let users = option_users(&state.db.pool, &filter).await?;
    Ok(Json(UsageStatsOptions {
        providers,
        models,
        users,
    }))
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
        ExportScope::Users => export_users(&state.db.pool, &filter, sort).await?,
        ExportScope::UserModels => export_user_models(&state.db.pool, &filter, sort).await?,
        ExportScope::Daily => export_daily(&state.db.pool, &filter).await?,
        ExportScope::Models => export_models(&state.db.pool, &filter, sort).await?,
    };
    Ok(csv_response(&filename, rows)?)
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
            user_query_pattern: trimmed_non_empty(params.user_query.as_deref())
                .map(|value| format!("%{value}%")),
            provider: trimmed_non_empty(params.provider.as_deref()).map(str::to_string),
            model: trimmed_non_empty(params.model.as_deref()).map(str::to_string),
            billing_meter,
        })
    }
}

#[derive(Clone, Copy)]
enum SortMode {
    CostDesc,
    TokensDesc,
    RequestsDesc,
}

impl SortMode {
    fn from_param(value: Option<&str>) -> Self {
        match value {
            Some("tokens_desc") => Self::TokensDesc,
            Some("requests_desc") => Self::RequestsDesc,
            _ => Self::CostDesc,
        }
    }

    fn model_order_by(self) -> &'static str {
        match self {
            Self::CostDesc => "cost_micro_usd DESC, request_count DESC, provider ASC, model ASC",
            Self::TokensDesc => "total_tokens DESC, cost_micro_usd DESC, provider ASC, model ASC",
            Self::RequestsDesc => {
                "request_count DESC, cost_micro_usd DESC, provider ASC, model ASC"
            }
        }
    }

    fn user_order_by(self) -> &'static str {
        match self {
            Self::CostDesc => "cost_micro_usd DESC, request_count DESC, user_id ASC NULLS LAST",
            Self::TokensDesc => "total_tokens DESC, cost_micro_usd DESC, user_id ASC NULLS LAST",
            Self::RequestsDesc => "request_count DESC, cost_micro_usd DESC, user_id ASC NULLS LAST",
        }
    }
}

enum ExportScope {
    Users,
    UserModels,
    Daily,
    Models,
}

impl ExportScope {
    fn from_param(value: Option<&str>) -> AppResult<Self> {
        match value.unwrap_or("users") {
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
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          SUM(ud.first_response_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.first_response_count), 0)::DOUBLE PRECISION AS avg_first_response_ms
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_one(pool)
    .await?;

    Ok(aggregate_from_row(&row)?)
}

async fn daily_stats(pool: &PgPool, filter: &UsageStatsFilter) -> AppResult<Vec<DailyUsageStats>> {
    let rows = sqlx::query(
        r#"
        SELECT
          to_char(ud.day, 'YYYY-MM-DD') AS date,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.day
        ORDER BY ud.day ASC
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
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
                cost_micro_usd: row.try_get("cost_micro_usd")?,
                avg_latency_ms: row.try_get("avg_latency_ms")?,
            })
        })
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
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR ud.provider = $5)
            AND ($6::TEXT IS NULL OR ud.model = $6)
            AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
          GROUP BY ud.user_id
        ) grouped
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
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
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          COUNT(DISTINCT NULLIF(ud.provider || '/' || ud.model, '/'))::BIGINT AS model_count
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.user_id, u.email, u.username
        ORDER BY {}
        OFFSET $8
        LIMIT $9
        "#,
        sort.user_order_by()
    );
    let offset = (page - 1) * limit;
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.provider.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
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
          SELECT ud.user_id, ud.provider, ud.model, ud.billing_meter
          FROM usage_daily ud
          LEFT JOIN "user" u ON u.id = ud.user_id
          WHERE ud.day >= $1
            AND ud.day <= $2
            AND ($3::BIGINT IS NULL OR ud.user_id = $3)
            AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
            AND ($5::TEXT IS NULL OR ud.provider = $5)
            AND ($6::TEXT IS NULL OR ud.model = $6)
            AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
          GROUP BY ud.user_id, ud.provider, ud.model, ud.billing_meter
        ) grouped
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
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
          ud.provider,
          ud.model,
          ud.billing_meter,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.user_id, u.email, u.username, ud.provider, ud.model, ud.billing_meter
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
        .bind(filter.provider.as_deref())
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
    let sql = format!(
        r#"
        SELECT
          ud.provider,
          ud.model,
          ud.billing_meter,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.success_count), 0)::BIGINT AS success_count,
          COALESCE(SUM(ud.error_count), 0)::BIGINT AS error_count,
          COALESCE(SUM(ud.input_tokens), 0)::BIGINT AS input_tokens,
          COALESCE(SUM(ud.output_tokens), 0)::BIGINT AS output_tokens,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd,
          SUM(ud.latency_ms_total)::DOUBLE PRECISION / NULLIF(SUM(ud.request_count), 0)::DOUBLE PRECISION AS avg_latency_ms,
          COUNT(DISTINCT ud.user_id)::BIGINT AS user_count
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.provider, ud.model, ud.billing_meter
        ORDER BY {}
        LIMIT $8
        "#,
        sort.model_order_by()
    );
    let rows = sqlx::query(AssertSqlSafe(sql))
        .bind(filter.start)
        .bind(filter.end)
        .bind(filter.user_id)
        .bind(filter.user_query_pattern.as_deref())
        .bind(filter.provider.as_deref())
        .bind(filter.model.as_deref())
        .bind(filter.billing_meter.as_deref())
        .bind(limit)
        .fetch_all(pool)
        .await?;

    rows.iter()
        .map(model_stats_from_row)
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn provider_stats(
    pool: &PgPool,
    filter: &UsageStatsFilter,
) -> AppResult<Vec<ProviderUsageStats>> {
    let rows = sqlx::query(
        r#"
        SELECT
          ud.provider,
          COALESCE(SUM(ud.request_count), 0)::BIGINT AS request_count,
          COALESCE(SUM(ud.total_tokens), 0)::BIGINT AS total_tokens,
          COALESCE(SUM(ud.billable_units), 0)::BIGINT AS billable_units,
          COALESCE(SUM(ud.cost_micro_usd), 0)::BIGINT AS cost_micro_usd
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.provider
        ORDER BY cost_micro_usd DESC, request_count DESC, provider ASC
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
    .bind(filter.model.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(ProviderUsageStats {
                provider: row.try_get("provider")?,
                request_count: row.try_get("request_count")?,
                total_tokens: row.try_get("total_tokens")?,
                billable_units: row.try_get("billable_units")?,
                cost_micro_usd: row.try_get("cost_micro_usd")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn option_providers(pool: &PgPool, filter: &UsageStatsFilter) -> AppResult<Vec<String>> {
    let rows = sqlx::query(
        r#"
        SELECT ud.provider
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.billing_meter = $5)
        GROUP BY ud.provider
        ORDER BY ud.provider ASC
        LIMIT 100
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| row.try_get("provider"))
        .collect::<Result<_, sqlx::Error>>()
        .map_err(AppError::from)
}

async fn option_models(pool: &PgPool, filter: &UsageStatsFilter) -> AppResult<Vec<ModelOption>> {
    let rows = sqlx::query(
        r#"
        SELECT ud.provider, ud.model
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ud.model <> ''
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.billing_meter = $6)
        GROUP BY ud.provider, ud.model
        ORDER BY ud.provider ASC, ud.model ASC
        LIMIT 500
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
    .bind(filter.billing_meter.as_deref())
    .fetch_all(pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(ModelOption {
                provider: row.try_get("provider")?,
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
          SUM(ud.cost_micro_usd) AS cost_micro_usd
        FROM usage_daily ud
        LEFT JOIN "user" u ON u.id = ud.user_id
        WHERE ud.day >= $1
          AND ud.day <= $2
          AND ud.user_id IS NOT NULL
          AND ($3::BIGINT IS NULL OR ud.user_id = $3)
          AND ($4::TEXT IS NULL OR u.email::TEXT ILIKE $4 OR u.username ILIKE $4 OR ud.user_id::TEXT ILIKE $4)
          AND ($5::TEXT IS NULL OR ud.provider = $5)
          AND ($6::TEXT IS NULL OR ud.model = $6)
          AND ($7::TEXT IS NULL OR ud.billing_meter = $7)
        GROUP BY ud.user_id, u.email, u.username
        ORDER BY last_day DESC, cost_micro_usd DESC, ud.user_id ASC
        LIMIT 50
        "#,
    )
    .bind(filter.start)
    .bind(filter.end)
    .bind(filter.user_id)
    .bind(filter.user_query_pattern.as_deref())
    .bind(filter.provider.as_deref())
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
        "cost_micro_usd".into(),
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
            item.cost_micro_usd.to_string(),
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
        "provider".into(),
        "model".into(),
        "billing_meter".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micro_usd".into(),
        "avg_latency_ms".into(),
    ]];
    rows.extend(page.items.into_iter().map(|item| {
        vec![
            optional_id(item.user_id),
            item.user_email.unwrap_or_default(),
            item.user_username.unwrap_or_default(),
            item.provider,
            item.model,
            item.billing_meter,
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micro_usd.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("user-models", filter), rows))
}

async fn export_daily(
    pool: &PgPool,
    filter: &UsageStatsFilter,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let items = daily_stats(pool, filter).await?;
    ensure_export_limit(items.len())?;
    let mut rows = vec![vec![
        "date".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micro_usd".into(),
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
            item.cost_micro_usd.to_string(),
            optional_f64(item.avg_latency_ms),
        ]
    }));
    Ok((export_filename("daily", filter), rows))
}

async fn export_models(
    pool: &PgPool,
    filter: &UsageStatsFilter,
    sort: SortMode,
) -> AppResult<(String, Vec<Vec<String>>)> {
    let items = model_stats(pool, filter, EXPORT_LIMIT + 1, sort).await?;
    ensure_export_limit(items.len())?;
    let mut rows = vec![vec![
        "provider".into(),
        "model".into(),
        "billing_meter".into(),
        "request_count".into(),
        "success_count".into(),
        "error_count".into(),
        "input_tokens".into(),
        "output_tokens".into(),
        "total_tokens".into(),
        "billable_units".into(),
        "cost_micro_usd".into(),
        "avg_latency_ms".into(),
        "user_count".into(),
    ]];
    rows.extend(items.into_iter().map(|item| {
        vec![
            item.provider,
            item.model,
            item.billing_meter,
            item.request_count.to_string(),
            item.success_count.to_string(),
            item.error_count.to_string(),
            item.input_tokens.to_string(),
            item.output_tokens.to_string(),
            item.total_tokens.to_string(),
            item.billable_units.to_string(),
            item.cost_micro_usd.to_string(),
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
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        avg_first_response_ms: row.try_get("avg_first_response_ms")?,
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
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        model_count: row.try_get("model_count")?,
    })
}

fn model_stats_from_row(row: &sqlx::postgres::PgRow) -> Result<ModelUsageStats, sqlx::Error> {
    Ok(ModelUsageStats {
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        billing_meter: row.try_get("billing_meter")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
        user_count: row.try_get("user_count")?,
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
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        billing_meter: row.try_get("billing_meter")?,
        request_count: row.try_get("request_count")?,
        success_count: row.try_get("success_count")?,
        error_count: row.try_get("error_count")?,
        input_tokens: row.try_get("input_tokens")?,
        output_tokens: row.try_get("output_tokens")?,
        total_tokens: row.try_get("total_tokens")?,
        billable_units: row.try_get("billable_units")?,
        cost_micro_usd: row.try_get("cost_micro_usd")?,
        avg_latency_ms: row.try_get("avg_latency_ms")?,
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
    user_id
        .map(|id| format!("Deleted user #{id}"))
        .unwrap_or_else(|| "Unknown user".to_string())
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
            user_query: None,
            provider: None,
            model: None,
            billing_meter: None,
            page: None,
            limit: None,
            sort: None,
            scope: None,
        };
        let filter = UsageStatsFilter::from_params(&params).unwrap();
        assert_eq!(filter.start, NaiveDate::from_ymd_opt(2026, 5, 28).unwrap());
        assert_eq!(filter.end, end);
    }
}
