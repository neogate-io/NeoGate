use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::id::DbId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditAccountType {
    User,
    UserKey,
    UserKeyModel,
}

impl CreditAccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::UserKey => "user_key",
            Self::UserKeyModel => "user_key_model",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CreditAccountId {
    pub id: DbId,
}

impl CreditAccountId {
    pub fn new(id: DbId) -> Self {
        Self { id }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub input_price_usd_micros: i64,
    pub output_price_usd_micros: i64,
    pub cache_read_price_usd_micros: Option<i64>,
    pub cache_write_price_usd_micros: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cached_input_tokens: Option<i64>,
    pub cache_creation_input_tokens: Option<i64>,
    pub cache_creation_input_tokens_5m: Option<i64>,
    pub cache_creation_input_tokens_1h: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub audio_input_tokens: Option<i64>,
    pub audio_output_tokens: Option<i64>,
}

impl TokenUsage {
    pub fn total_tokens(self) -> i64 {
        self.input_tokens + self.output_tokens
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebitPart {
    pub credit_account: CreditAccountId,
    pub allocation_id: DbId,
    pub amount_micro_usd: i64,
    pub(super) generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebitHold {
    pub transaction_id: Uuid,
    pub estimated_micro_usd: i64,
    pub parts: Vec<DebitPart>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingCharge {
    pub transaction_id: Uuid,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cost_micro_usd: i64,
    pub status: String,
    pub parts: Vec<DebitPart>,
    pub returned_parts: Vec<DebitPart>,
}
