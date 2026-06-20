use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::id::DbId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMeter {
    Token,
    Image,
}

impl BillingMeter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Image => "image",
        }
    }

    pub fn from_strict_str(value: &str) -> Result<Self, String> {
        match value {
            "token" => Ok(Self::Token),
            "image" => Ok(Self::Image),
            _ => Err(format!("invalid billing meter: {value}")),
        }
    }
}

impl<'de> Deserialize<'de> for BillingMeter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::from_strict_str(&value).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CreditAccountType {
    Project,
    UserKey,
    UserKeyModel,
}

impl CreditAccountType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
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
    pub billing_meter: BillingMeter,
    pub unit_price_usd_micros: Option<i64>,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BillableUsage {
    pub meter: BillingMeter,
    pub token_usage: Option<TokenUsage>,
    pub billable_units: i64,
}

impl BillableUsage {
    pub fn token(usage: TokenUsage) -> Self {
        Self {
            meter: BillingMeter::Token,
            token_usage: Some(usage),
            billable_units: usage.total_tokens().max(0),
        }
    }

    pub fn image(image_count: i64) -> Self {
        Self {
            meter: BillingMeter::Image,
            token_usage: None,
            billable_units: image_count.max(0),
        }
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
    #[serde(default = "default_charge_credit")]
    pub charge_credit: bool,
}

fn default_charge_credit() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingCharge {
    pub transaction_id: Uuid,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub billing_meter: BillingMeter,
    pub billable_units: i64,
    pub cost_micro_usd: i64,
    pub status: String,
    pub parts: Vec<DebitPart>,
    pub returned_parts: Vec<DebitPart>,
}
