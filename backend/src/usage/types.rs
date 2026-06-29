use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    billing::{BillingCharge, BillingMeter, TokenUsage},
    id::DbId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageInsert {
    pub user_id: DbId,
    pub project_id: DbId,
    pub user_key_id: DbId,
    pub channel_id: DbId,
    pub channel_key_id: Option<DbId>,
    pub credential_id: Option<DbId>,
    pub relay_trace_id: Option<Uuid>,
    pub relay_attempt: i32,
    pub relay_final: bool,
    pub provider: String,
    pub model: Option<String>,
    pub upstream_model: Option<String>,
    pub routing_phase: String,
    pub status_code: Option<i32>,
    pub streamed: bool,
    pub latency_ms: i64,
    pub first_response_ms: Option<i64>,
    pub output_tokens_per_second: Option<f64>,
    pub error_summary: Option<String>,
    pub token_usage: Option<TokenUsage>,
    pub billing_meter: BillingMeter,
    pub billable_units: i64,
    pub billing: Option<BillingCharge>,
}

pub struct KeyFailure {
    pub channel_key_id: DbId,
    pub cooldown_until: DateTime<Utc>,
    pub error: String,
}
