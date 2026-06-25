use std::{collections::HashMap, sync::Arc};

use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::IntoResponse,
    routing::get,
    Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    auth::UserSessionAuth,
    billing::{account, MICRO_USD_PER_USD},
    config::{PaymentProvider, ZpayConfig},
    error::{AppError, AppResult},
    id::DbId,
    policy::{self, ServiceMode},
    AppState,
};

pub(crate) mod settings;
mod zpay;

const PAYMENT_CURRENCY: &str = "CNY";
const MICRO_USD_PER_CNY: i64 = MICRO_USD_PER_USD * 5;

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/api/payments/{provider}/notify",
        get(notify_payment_query).post(notify_payment),
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentOrderRecord {
    pub id: Uuid,
    pub user_id: DbId,
    pub provider: String,
    pub provider_order_id: Option<String>,
    pub status: String,
    pub currency: String,
    pub amount_micro_usd: i64,
    pub payable_amount_minor: i64,
    pub checkout_url: Option<String>,
    pub return_url: Option<String>,
    pub paid_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePaymentOrderRequest {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub amount_micro_usd: i64,
    pub pay_type: Option<String>,
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatePaymentOrderResponse {
    pub order: PaymentOrderRecord,
    pub checkout_url: Option<String>,
}

#[derive(Debug, Clone)]
struct GatewayCreateRequest {
    order_id: Uuid,
    payable_amount_minor: i64,
    pay_type: Option<String>,
    subject: String,
    notify_url: String,
    return_url: Option<String>,
}

#[derive(Debug, Clone)]
struct GatewayCreateResponse {
    provider_order_id: Option<String>,
    checkout_url: Option<String>,
}

#[derive(Debug)]
struct GatewayNotification {
    order_id: Uuid,
    provider_order_id: Option<String>,
    payable_amount_minor: Option<i64>,
    status: PaymentStatus,
    payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum PaymentStatus {
    Paid,
    Failed,
    Pending,
}

trait PaymentGateway {
    fn create_checkout(&self, req: GatewayCreateRequest) -> AppResult<GatewayCreateResponse>;
    fn parse_notification(
        &self,
        headers: &HeaderMap,
        body: &Bytes,
    ) -> AppResult<GatewayNotification>;
    fn parse_query_notification(
        &self,
        params: HashMap<String, String>,
    ) -> AppResult<GatewayNotification>;
}

pub async fn create_user_payment_order(
    state: &AppState,
    auth: UserSessionAuth,
    req: CreatePaymentOrderRequest,
) -> AppResult<CreatePaymentOrderResponse> {
    if policy::service_mode(state).await? != ServiceMode::Paid {
        return Err(AppError::BadRequest(
            "recharge is only available in paid service mode".to_string(),
        ));
    }
    let provider = PaymentProvider::from_code(&req.provider)?;
    let payment_config = settings::runtime_payment_config(state).await?;
    if !payment_config.provider_enabled(provider) {
        return Err(AppError::BadRequest(format!(
            "payment provider is not enabled: {}",
            provider.as_str()
        )));
    }
    if req.amount_micro_usd <= 0 {
        return Err(AppError::BadRequest(
            "amount_micro_usd must be positive".to_string(),
        ));
    }

    let order_id = Uuid::new_v4();
    let payable_amount_minor = micro_usd_to_cny_minor_units(req.amount_micro_usd);
    let credit_account = default_project_credit_account(state, auth.user_id).await?;
    let notify_url = notify_url(&payment_config, provider)?;
    let gateway_req = GatewayCreateRequest {
        order_id,
        payable_amount_minor,
        pay_type: req.pay_type.clone(),
        subject: "NeoGate credit recharge".to_string(),
        notify_url,
        return_url: req.return_url.clone(),
    };
    let gateway_res = gateway_for(&payment_config, provider)?.create_checkout(gateway_req)?;

    sqlx::query(
        "INSERT INTO payment
         (id, user_id, credit_account_id, provider, provider_order_id, status, currency,
          amount_micro_usd, payable_amount_minor, checkout_url, return_url)
         VALUES ($1, $2, $3, $4, $5, 'pending', $6, $7, $8, $9, $10)",
    )
    .bind(order_id)
    .bind(auth.user_id)
    .bind(credit_account.id)
    .bind(provider.as_str())
    .bind(gateway_res.provider_order_id.as_deref())
    .bind(PAYMENT_CURRENCY)
    .bind(req.amount_micro_usd)
    .bind(payable_amount_minor)
    .bind(gateway_res.checkout_url.as_deref())
    .bind(req.return_url.as_deref())
    .execute(&state.db.pool)
    .await?;

    let order = get_user_payment_order(state, auth.user_id, order_id).await?;
    Ok(CreatePaymentOrderResponse {
        checkout_url: order.checkout_url.clone(),
        order,
    })
}

async fn default_project_credit_account(
    state: &AppState,
    user_id: DbId,
) -> AppResult<crate::billing::CreditAccountId> {
    let row = sqlx::query(
        r#"
        SELECT w.id
        FROM project p
        JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
        WHERE p.owner_user_id = $1
          AND p.is_default = TRUE
          AND p.status = 'enabled'
        ORDER BY p.id ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("default project is missing".to_string()))?;
    Ok(crate::billing::CreditAccountId::new(row.try_get("id")?))
}

pub async fn list_user_payment_orders(
    state: &AppState,
    auth: UserSessionAuth,
) -> AppResult<Vec<PaymentOrderRecord>> {
    let rows = sqlx::query(
        "SELECT id, user_id, provider, provider_order_id, status, currency,
                amount_micro_usd, payable_amount_minor, checkout_url, return_url,
                paid_at, created_at, updated_at
         FROM payment
         WHERE user_id = $1
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .bind(auth.user_id)
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(payment_order_from_row).collect()
}

async fn notify_payment(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<impl IntoResponse> {
    let provider = PaymentProvider::from_code(&provider)?;
    let payment_config = settings::runtime_payment_config(&state).await?;
    let notification =
        gateway_for(&payment_config, provider)?.parse_notification(&headers, &body)?;
    tracing::info!(
        provider = provider.as_str(),
        order_id = %notification.order_id,
        status = ?notification.status,
        "payment notification received"
    );
    record_payment_event(&state, provider, &notification).await?;
    settle_payment_notification(&state, provider, notification).await?;
    Ok("success")
}

async fn notify_payment_query(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<String>,
    Query(params): Query<HashMap<String, String>>,
) -> AppResult<impl IntoResponse> {
    let provider = PaymentProvider::from_code(&provider)?;
    let payment_config = settings::runtime_payment_config(&state).await?;
    let notification = gateway_for(&payment_config, provider)?.parse_query_notification(params)?;
    tracing::info!(
        provider = provider.as_str(),
        order_id = %notification.order_id,
        status = ?notification.status,
        "payment notification received"
    );
    record_payment_event(&state, provider, &notification).await?;
    settle_payment_notification(&state, provider, notification).await?;
    Ok("success")
}

async fn settle_payment_notification(
    state: &AppState,
    provider: PaymentProvider,
    notification: GatewayNotification,
) -> AppResult<()> {
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "SELECT id, credit_account_id, amount_micro_usd, payable_amount_minor, status
         FROM payment
         WHERE id = $1 AND provider = $2
         FOR UPDATE",
    )
    .bind(notification.order_id)
    .bind(provider.as_str())
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(AppError::NotFound)?;

    let status: String = row.try_get("status")?;
    if status == "paid" {
        tracing::info!(
            provider = provider.as_str(),
            order_id = %notification.order_id,
            "payment notification already settled"
        );
        tx.commit().await?;
        return Ok(());
    }

    let next_status = match notification.status {
        PaymentStatus::Paid => "paid",
        PaymentStatus::Failed => "failed",
        PaymentStatus::Pending => "pending",
    };

    if notification.status == PaymentStatus::Paid {
        let expected_amount: i64 = row.try_get("payable_amount_minor")?;
        if notification.payable_amount_minor != Some(expected_amount) {
            return Err(AppError::BadRequest(
                "payment notification amount does not match order".to_string(),
            ));
        }
    }

    sqlx::query(
        "UPDATE payment
         SET provider_order_id = COALESCE($2, provider_order_id),
             status = $3,
             notify_payload = $4,
             paid_at = CASE WHEN $3 = 'paid' THEN COALESCE(paid_at, now()) ELSE paid_at END,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(notification.order_id)
    .bind(notification.provider_order_id.as_deref())
    .bind(next_status)
    .bind(&notification.payload)
    .execute(&mut *tx)
    .await?;

    if notification.status == PaymentStatus::Paid {
        let credit_account_id: DbId = row.try_get("credit_account_id")?;
        let amount_micro_usd: i64 = row.try_get("amount_micro_usd")?;
        let credit_account = crate::billing::CreditAccountId::new(credit_account_id);
        let balance_after =
            account::adjust_balance(&mut tx, &credit_account, amount_micro_usd).await?;
        sqlx::query(
            "INSERT INTO credit_ledger
             (credit_account_id, amount_micro_usd, balance_after_micro_usd, reason,
              transaction_id, metadata)
             VALUES ($1, $2, $3, 'recharge', $4, $5)",
        )
        .bind(credit_account_id)
        .bind(amount_micro_usd)
        .bind(balance_after)
        .bind(notification.order_id)
        .bind(json!({
            "source": "payment_gateway",
            "provider": provider.as_str(),
            "provider_order_id": notification.provider_order_id,
        }))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

async fn record_payment_event(
    state: &AppState,
    provider: PaymentProvider,
    notification: &GatewayNotification,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO payment_event (payment_id, provider, event_type, payload)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(notification.order_id)
    .bind(provider.as_str())
    .bind(match notification.status {
        PaymentStatus::Paid => "paid",
        PaymentStatus::Failed => "failed",
        PaymentStatus::Pending => "pending",
    })
    .bind(&notification.payload)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn get_user_payment_order(
    state: &AppState,
    user_id: DbId,
    id: Uuid,
) -> AppResult<PaymentOrderRecord> {
    let row = sqlx::query(
        "SELECT id, user_id, provider, provider_order_id, status, currency,
                amount_micro_usd, payable_amount_minor, checkout_url, return_url,
                paid_at, created_at, updated_at
         FROM payment
         WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    payment_order_from_row(&row)
}

fn payment_order_from_row(row: &sqlx::postgres::PgRow) -> AppResult<PaymentOrderRecord> {
    Ok(PaymentOrderRecord {
        id: row.try_get("id")?,
        user_id: row.try_get("user_id")?,
        provider: row.try_get("provider")?,
        provider_order_id: row.try_get("provider_order_id")?,
        status: row.try_get("status")?,
        currency: row.try_get("currency")?,
        amount_micro_usd: row.try_get("amount_micro_usd")?,
        payable_amount_minor: row.try_get("payable_amount_minor")?,
        checkout_url: row.try_get("checkout_url")?,
        return_url: row.try_get("return_url")?,
        paid_at: row.try_get("paid_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn gateway_for(
    payment_config: &crate::config::PaymentConfig,
    provider: PaymentProvider,
) -> AppResult<Box<dyn PaymentGateway>> {
    match provider {
        PaymentProvider::Zpay => Ok(Box::new(zpay::ZpayGateway::new(
            payment_config.zpay.clone(),
        )?)),
    }
}

fn notify_url(
    payment_config: &crate::config::PaymentConfig,
    provider: PaymentProvider,
) -> AppResult<String> {
    let Some(base_url) = payment_config.return_base_url.as_deref() else {
        return Err(AppError::BadRequest(
            "payment return base URL is required to create payment orders".to_string(),
        ));
    };
    Ok(format!(
        "{}/api/payments/{}/notify",
        base_url.trim_end_matches('/'),
        provider.as_str()
    ))
}

fn micro_usd_to_cny_minor_units(amount_micro_usd: i64) -> i64 {
    (amount_micro_usd + (MICRO_USD_PER_CNY / 100) - 1) / (MICRO_USD_PER_CNY / 100)
}

fn default_provider() -> String {
    "zpay".to_string()
}

fn form_or_json_payload(headers: &HeaderMap, body: &Bytes) -> AppResult<HashMap<String, String>> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if content_type.starts_with("application/json") {
        let value: serde_json::Value = serde_json::from_slice(body)?;
        let Some(object) = value.as_object() else {
            return Err(AppError::BadRequest(
                "payment notification JSON must be an object".to_string(),
            ));
        };
        return Ok(object
            .iter()
            .map(|(key, value)| {
                let value = value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .unwrap_or_else(|| value.to_string());
                (key.clone(), value)
            })
            .collect());
    }

    serde_urlencoded::from_bytes(body)
        .map_err(|err| AppError::BadRequest(format!("invalid payment notification body: {err}")))
}

fn payload_json(params: &HashMap<String, String>) -> serde_json::Value {
    let object = params
        .iter()
        .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
        .collect();
    serde_json::Value::Object(object)
}

impl ZpayConfig {
    fn require_ready(&self) -> AppResult<()> {
        if self.api_url.is_none() || self.merchant_id.is_none() || self.secret_key.is_none() {
            return Err(AppError::BadRequest(
                "ZPAY API URL, merchant ID and secret key are required".to_string(),
            ));
        }
        Ok(())
    }
}
