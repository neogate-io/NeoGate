use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::id::DbId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingMeter {
    Token,
    Image,
    Video,
}

impl BillingMeter {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Image => "image",
            Self::Video => "video",
        }
    }

    pub fn from_strict_str(value: &str) -> Result<Self, String> {
        match value {
            "token" => Ok(Self::Token),
            "image" => Ok(Self::Image),
            "video" => Ok(Self::Video),
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

/// 参考价展示口径。仅用于 `pricing_template.pricing_basis` 列,决定前端如何展示单价。
/// 不参与实际计费(实际计费仍由 `BillingMeter` 决定)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingBasis {
    Token,
    Image,
    Call,
    Per10kToken,
    Hour,
    Second,
    MultiTierVideo,
}

impl PricingBasis {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Image => "image",
            Self::Call => "call",
            Self::Per10kToken => "per_10k_token",
            Self::Hour => "hour",
            Self::Second => "second",
            Self::MultiTierVideo => "multi_tier_video",
        }
    }

    /// 严格解析。未知值返回 Err;调用方若需兼容历史脏值应使用 `from_strict_str(...).unwrap_or(Token)`。
    pub fn from_strict_str(value: &str) -> Result<Self, String> {
        match value {
            "token" => Ok(Self::Token),
            "image" => Ok(Self::Image),
            "call" => Ok(Self::Call),
            "per_10k_token" => Ok(Self::Per10kToken),
            "hour" => Ok(Self::Hour),
            "second" => Ok(Self::Second),
            "multi_tier_video" => Ok(Self::MultiTierVideo),
            _ => Err(format!("invalid pricing basis: {value}")),
        }
    }

    /// 宽松解析:未知值 fallback 到 `Token`,兼容历史数据与未标注口径的模型。
    pub fn from_str_lenient(value: &str) -> Self {
        Self::from_strict_str(value).unwrap_or(Self::Token)
    }
}

impl<'de> Deserialize<'de> for PricingBasis {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(Self::from_str_lenient(&value))
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
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: BillingMeter,
    pub unit_price_micros: Option<i64>,
    pub video_billing_mode: Option<VideoBillingMode>,
    pub video_price_tiers: Vec<VideoPriceTier>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoBillingMode {
    OfficialToken,
    PerSecond,
}

impl VideoBillingMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OfficialToken => "official_token",
            Self::PerSecond => "per_second",
        }
    }

    pub fn from_strict_str(value: &str) -> Result<Self, String> {
        match value {
            "official_token" => Ok(Self::OfficialToken),
            "per_second" => Ok(Self::PerSecond),
            _ => Err(format!("invalid video billing mode: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoPriceTier {
    #[serde(default)]
    pub resolutions: Vec<String>,
    pub input_with_video_micros: Option<i64>,
    pub input_without_video_micros: Option<i64>,
    pub estimated_tokens_per_second: Option<i64>,
    pub input_with_video_unit_micros: Option<i64>,
    pub input_without_video_unit_micros: Option<i64>,
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

    pub fn video_tokens(usage: TokenUsage) -> Self {
        Self {
            meter: BillingMeter::Video,
            token_usage: Some(usage),
            billable_units: usage.total_tokens().max(0),
        }
    }

    pub fn video_seconds(seconds: i64) -> Self {
        Self {
            meter: BillingMeter::Video,
            token_usage: None,
            billable_units: seconds.max(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebitPart {
    pub credit_account: CreditAccountId,
    pub allocation_id: DbId,
    pub amount_micros: i64,
    pub(super) generation: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebitHold {
    pub transaction_id: Uuid,
    pub estimated_micros: i64,
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
    pub cost_micros: i64,
    pub status: String,
    pub parts: Vec<DebitPart>,
    pub returned_parts: Vec<DebitPart>,
}
