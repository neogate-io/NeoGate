use std::sync::Arc;

use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;
use sqlx::Row;

use crate::{auth::UserSessionAuth, error::AppResult, AppState};

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/api/user/overview", get(overview))
}

#[derive(Debug, Serialize)]
struct UserOverview {
    email: String,
    display_name: String,
    balance_micro_usd: i64,
    reserved_micro_usd: i64,
    available_micro_usd: i64,
    today_cost_micro_usd: i64,
    month_cost_micro_usd: i64,
    request_count: i64,
    daily_costs: Vec<DailyCost>,
}

#[derive(Debug, Serialize)]
struct DailyCost {
    date: String,
    cost_micro_usd: i64,
}

async fn overview(
    State(state): State<Arc<AppState>>,
    auth: UserSessionAuth,
) -> AppResult<Json<UserOverview>> {
    let row = sqlx::query(
        r#"
        SELECT
            u.email::TEXT AS email,
            w.balance_micro_usd,
            w.reserved_micro_usd,
            COALESCE(today.cost_micro_usd, 0)::BIGINT AS today_cost_micro_usd,
            COALESCE(month.cost_micro_usd, 0)::BIGINT AS month_cost_micro_usd,
            COALESCE(total.request_count, 0)::BIGINT AS request_count
        FROM "user" u
        JOIN credit_account w ON w.owner_type = 'user' AND w.owner_id = u.id
        LEFT JOIN LATERAL (
            SELECT SUM(cost_micro_usd) AS cost_micro_usd
            FROM usage_daily
            WHERE user_id = u.id
              AND day = current_date
        ) today ON TRUE
        LEFT JOIN LATERAL (
            SELECT SUM(cost_micro_usd) AS cost_micro_usd
            FROM usage_daily
            WHERE user_id = u.id
              AND day >= date_trunc('month', now())::date
        ) month ON TRUE
        LEFT JOIN LATERAL (
            SELECT SUM(request_count) AS request_count
            FROM usage_daily
            WHERE user_id = u.id
        ) total ON TRUE
        WHERE u.id = $1
        "#,
    )
    .bind(auth.user_id)
    .fetch_one(&state.db.pool)
    .await?;

    let email: String = row.try_get("email")?;
    let balance_micro_usd: i64 = row.try_get("balance_micro_usd")?;
    let reserved_micro_usd: i64 = row.try_get("reserved_micro_usd")?;
    let daily_costs = daily_costs(&state, auth.user_id).await?;

    Ok(Json(UserOverview {
        display_name: display_name_from_email(&email),
        email,
        balance_micro_usd,
        reserved_micro_usd,
        available_micro_usd: balance_micro_usd - reserved_micro_usd,
        today_cost_micro_usd: row.try_get("today_cost_micro_usd")?,
        month_cost_micro_usd: row.try_get("month_cost_micro_usd")?,
        request_count: row.try_get("request_count")?,
        daily_costs,
    }))
}

async fn daily_costs(state: &AppState, user_id: crate::id::DbId) -> AppResult<Vec<DailyCost>> {
    let rows = sqlx::query(
        r#"
        SELECT
            to_char(day, 'YYYY-MM-DD') AS date,
            SUM(cost_micro_usd)::BIGINT AS cost_micro_usd
        FROM usage_daily
        WHERE user_id = $1
          AND day >= (current_date - 29)
          AND cost_micro_usd > 0
        GROUP BY day
        ORDER BY day
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(rows
        .iter()
        .map(|row| {
            Ok(DailyCost {
                date: row.try_get("date")?,
                cost_micro_usd: row.try_get("cost_micro_usd")?,
            })
        })
        .collect::<Result<_, sqlx::Error>>()?)
}

fn display_name_from_email(email: &str) -> String {
    email
        .split('@')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(email)
        .to_string()
}
