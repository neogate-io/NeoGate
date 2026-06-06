use std::sync::Arc;

use axum::{extract::State, routing::get, Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Row, Transaction};

use crate::{
    auth::{AdminAuth, UserSessionAuth},
    error::{AppError, AppResult},
    AppState,
};

pub const SERVICE_POLICY_SETTING_KEY: &str = "service_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceMode {
    Internal,
    Paid,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServicePolicyRecord {
    pub setup_completed: bool,
    pub service_mode: ServiceMode,
    pub credit_required: bool,
    pub recharge_enabled: bool,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CompleteSetupRequest {
    pub service_mode: ServiceMode,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServicePolicyRequest {
    pub credit_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredServicePolicy {
    setup_completed: bool,
    service_mode: ServiceMode,
    credit_required: bool,
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup", axum::routing::post(complete_setup))
        .route("/api/user/service-policy", get(user_service_policy))
        .route(
            "/api/admin/settings/service-policy",
            get(admin_service_policy).post(update_admin_service_policy),
        )
}

pub async fn current_service_policy(state: &AppState) -> AppResult<ServicePolicyRecord> {
    let Some(row) = sqlx::query("SELECT value, updated_at FROM setting WHERE key = $1")
        .bind(SERVICE_POLICY_SETTING_KEY)
        .fetch_optional(&state.db.pool)
        .await?
    else {
        return Ok(record_from_stored(default_stored_policy(), None));
    };

    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    let stored = normalize_stored_policy(serde_json::from_value(value)?);
    Ok(record_from_stored(stored, Some(updated_at)))
}

pub async fn credit_required(state: &AppState) -> AppResult<bool> {
    Ok(current_service_policy(state).await?.credit_required)
}

pub async fn service_mode(state: &AppState) -> AppResult<ServiceMode> {
    Ok(current_service_policy(state).await?.service_mode)
}

async fn setup_status(State(state): State<Arc<AppState>>) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn complete_setup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompleteSetupRequest>,
) -> AppResult<Json<ServicePolicyRecord>> {
    let mut tx = state.db.pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('neogate.service_policy_setup'))")
        .execute(&mut *tx)
        .await?;

    if stored_policy_for_update(&mut tx).await?.setup_completed {
        return Err(AppError::Conflict("setup has already been completed".to_string()));
    }

    let stored = StoredServicePolicy {
        setup_completed: true,
        service_mode: req.service_mode,
        credit_required: req.service_mode == ServiceMode::Paid,
    };
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
    Ok(Json(record))
}

async fn user_service_policy(
    State(state): State<Arc<AppState>>,
    _auth: UserSessionAuth,
) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn admin_service_policy(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
) -> AppResult<Json<ServicePolicyRecord>> {
    Ok(Json(current_service_policy(&state).await?))
}

async fn update_admin_service_policy(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<UpdateServicePolicyRequest>,
) -> AppResult<Json<ServicePolicyRecord>> {
    let mut tx = state.db.pool.begin().await?;
    let mut stored = stored_policy_for_update(&mut tx).await?;
    if !stored.setup_completed {
        return Err(AppError::BadRequest("setup is not completed".to_string()));
    }
    stored.credit_required = match stored.service_mode {
        ServiceMode::Internal => req.credit_required,
        ServiceMode::Paid => true,
    };
    let record = upsert_stored_policy(&mut tx, stored).await?;
    tx.commit().await?;
    Ok(Json(record))
}

async fn stored_policy_for_update(
    tx: &mut Transaction<'_, Postgres>,
) -> AppResult<StoredServicePolicy> {
    let Some(row) = sqlx::query("SELECT value FROM setting WHERE key = $1 FOR UPDATE")
        .bind(SERVICE_POLICY_SETTING_KEY)
        .fetch_optional(&mut **tx)
        .await?
    else {
        return Ok(default_stored_policy());
    };
    let value: serde_json::Value = row.try_get("value")?;
    Ok(normalize_stored_policy(serde_json::from_value(value)?))
}

async fn upsert_stored_policy(
    tx: &mut Transaction<'_, Postgres>,
    stored: StoredServicePolicy,
) -> AppResult<ServicePolicyRecord> {
    let stored = normalize_stored_policy(stored);
    let value = serde_json::to_value(&stored)?;
    let row = sqlx::query(
        "INSERT INTO setting (key, value)
         VALUES ($1, $2)
         ON CONFLICT (key)
         DO UPDATE SET value = EXCLUDED.value, updated_at = now()
         RETURNING value, updated_at",
    )
    .bind(SERVICE_POLICY_SETTING_KEY)
    .bind(value)
    .fetch_one(&mut **tx)
    .await?;
    let value: serde_json::Value = row.try_get("value")?;
    let updated_at: DateTime<Utc> = row.try_get("updated_at")?;
    Ok(record_from_stored(
        normalize_stored_policy(serde_json::from_value(value)?),
        Some(updated_at),
    ))
}

fn default_stored_policy() -> StoredServicePolicy {
    StoredServicePolicy {
        setup_completed: false,
        service_mode: ServiceMode::Internal,
        credit_required: false,
    }
}

fn normalize_stored_policy(mut stored: StoredServicePolicy) -> StoredServicePolicy {
    if stored.service_mode == ServiceMode::Paid {
        stored.credit_required = true;
    }
    stored
}

fn record_from_stored(
    stored: StoredServicePolicy,
    updated_at: Option<DateTime<Utc>>,
) -> ServicePolicyRecord {
    let stored = normalize_stored_policy(stored);
    ServicePolicyRecord {
        setup_completed: stored.setup_completed,
        service_mode: stored.service_mode,
        credit_required: stored.credit_required,
        recharge_enabled: stored.service_mode == ServiceMode::Paid,
        updated_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_is_unfinished_internal_without_credit_requirement() {
        let record = record_from_stored(default_stored_policy(), None);

        assert!(!record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Internal);
        assert!(!record.credit_required);
        assert!(!record.recharge_enabled);
    }

    #[test]
    fn paid_mode_always_requires_credit_and_enables_recharge() {
        let record = record_from_stored(
            StoredServicePolicy {
                setup_completed: true,
                service_mode: ServiceMode::Paid,
                credit_required: false,
            },
            None,
        );

        assert!(record.setup_completed);
        assert_eq!(record.service_mode, ServiceMode::Paid);
        assert!(record.credit_required);
        assert!(record.recharge_enabled);
    }
}
