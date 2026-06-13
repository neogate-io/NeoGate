use std::sync::Arc;

use crate::{billing::outbox, AppState};
use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::get, Json, Router};
use serde_json::json;

#[derive(Clone)]
pub struct RuntimeHealth {
    redis: Option<redis::Client>,
}

impl RuntimeHealth {
    pub fn new(redis: Option<redis::Client>) -> Self {
        Self { redis }
    }

    pub async fn redis_ready(&self) -> Option<bool> {
        let client = self.redis.as_ref()?;
        Some(redis_ready(client).await)
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
}

async fn liveness() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn readiness(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db.pool)
        .await
        .is_ok();
    let redis_status = state.runtime_health.redis_ready().await;
    let redis_ok = redis_status.unwrap_or(true);
    let write_status = state.billing_outbox.write_status();
    let outbox_status = if db_ok {
        outbox::backlog_status(
            &state.db.pool,
            state.config.health.billing_outbox_max_pending,
        )
        .await
        .ok()
    } else {
        None
    };
    let backlog_ok = outbox_status
        .as_ref()
        .map(|status| {
            status.pending_count <= state.config.health.billing_outbox_max_pending
                && status.oldest_pending_age_seconds
                    <= state.config.health.billing_outbox_max_age.as_secs() as i64
        })
        .unwrap_or(false);
    let billing_outbox_ok = write_status.healthy && backlog_ok;

    if db_ok && redis_ok && billing_outbox_ok {
        (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "runtime_mode": state.config.runtime_mode.as_str(),
                "process_role": state.config.process_role.as_str(),
                "database": true,
                "redis": redis_status,
                "billing_outbox": {
                    "ok": true,
                    "write": write_status,
                    "backlog": outbox_status
                }
            })),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "ok": false,
                "runtime_mode": state.config.runtime_mode.as_str(),
                "process_role": state.config.process_role.as_str(),
                "database": db_ok,
                "redis": redis_status,
                "billing_outbox": {
                    "ok": billing_outbox_ok,
                    "write": write_status,
                    "backlog": outbox_status
                }
            })),
        )
    }
}

async fn redis_ready(client: &redis::Client) -> bool {
    let Ok(mut conn) = client.get_multiplexed_async_connection().await else {
        return false;
    };
    redis::cmd("PING")
        .query_async::<String>(&mut conn)
        .await
        .is_ok()
}
