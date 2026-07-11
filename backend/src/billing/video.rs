use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    BillableUsage, BillingMeter, Price, TokenUsage, VideoBillingMode, VideoPriceTier,
    MICROS_PER_MAJOR_UNIT,
};
use crate::error::{AppError, AppResult};

const DEFAULT_RESOLUTION: &str = "480p";
const DEFAULT_DURATION_SECONDS: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoBillingInput {
    pub resolution: String,
    pub duration_seconds: i64,
    pub has_video_input: bool,
}

impl Default for VideoBillingInput {
    fn default() -> Self {
        Self {
            resolution: DEFAULT_RESOLUTION.to_string(),
            duration_seconds: DEFAULT_DURATION_SECONDS,
            has_video_input: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoBillingMetadata {
    pub mode: VideoBillingMode,
    pub resolution: String,
    pub duration_seconds: i64,
    pub has_video_input: bool,
    pub price_micros: i64,
    pub estimated_tokens_per_second: Option<i64>,
    pub estimated_tokens: Option<i64>,
    pub estimated_micros: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedVideoBilling {
    pub metadata: VideoBillingMetadata,
    pub estimated_micros: i64,
}

pub fn seedance_video_billing_enabled(_provider: &str, price: &Price) -> bool {
    price.billing_meter == BillingMeter::Video && price.video_billing_mode.is_some()
}

pub fn video_billing_input(
    resolution: Option<&str>,
    duration_seconds: Option<i64>,
    has_video_input: bool,
) -> VideoBillingInput {
    VideoBillingInput {
        resolution: normalize_resolution(resolution),
        duration_seconds: duration_seconds
            .filter(|seconds| *seconds > 0)
            .unwrap_or(DEFAULT_DURATION_SECONDS),
        has_video_input,
    }
}

pub fn json_video_billing_input(value: &Value) -> VideoBillingInput {
    video_billing_input(
        string_field(value, "resolution")
            .or_else(|| string_field(value, "size"))
            .as_deref(),
        positive_i64_field(value, "duration").or_else(|| positive_i64_field(value, "seconds")),
        json_has_video_input(value),
    )
}

pub fn prepare_seedance_video_billing(
    provider: &str,
    model: &str,
    price: &Price,
    input: &VideoBillingInput,
) -> AppResult<Option<PreparedVideoBilling>> {
    if !seedance_video_billing_enabled(provider, price) {
        return Ok(None);
    }
    let mode = price.video_billing_mode.ok_or_else(|| {
        AppError::BadRequest(format!(
            "video billing mode is not configured for {provider}/{model}"
        ))
    })?;
    let tier = matching_tier(&price.video_price_tiers, &input.resolution).ok_or_else(|| {
        AppError::BadRequest(format!(
            "video price tier is not configured for {provider}/{model} resolution {}",
            input.resolution
        ))
    })?;

    let metadata = match mode {
        VideoBillingMode::OfficialToken => {
            let token_price_micros = required_positive(
                if input.has_video_input {
                    tier.input_with_video_micros
                } else {
                    tier.input_without_video_micros
                },
                "official token video price",
            )?;
            let estimated_tokens_per_second = required_positive(
                tier.estimated_tokens_per_second,
                "estimated tokens per second",
            )?;
            let estimated_tokens = input
                .duration_seconds
                .saturating_mul(estimated_tokens_per_second);
            VideoBillingMetadata {
                mode,
                resolution: input.resolution.clone(),
                duration_seconds: input.duration_seconds,
                has_video_input: input.has_video_input,
                price_micros: token_price_micros,
                estimated_tokens_per_second: Some(estimated_tokens_per_second),
                estimated_tokens: Some(estimated_tokens),
                estimated_micros: micros_for_tokens(estimated_tokens, token_price_micros),
            }
        }
        VideoBillingMode::PerSecond => {
            let unit_price_micros = required_positive(
                if input.has_video_input {
                    tier.input_with_video_unit_micros
                } else {
                    tier.input_without_video_unit_micros
                },
                "per-second video price",
            )?;
            VideoBillingMetadata {
                mode,
                resolution: input.resolution.clone(),
                duration_seconds: input.duration_seconds,
                has_video_input: input.has_video_input,
                price_micros: unit_price_micros,
                estimated_tokens_per_second: None,
                estimated_tokens: None,
                estimated_micros: input.duration_seconds.saturating_mul(unit_price_micros),
            }
        }
    };

    Ok(Some(PreparedVideoBilling {
        estimated_micros: metadata.estimated_micros,
        metadata,
    }))
}

pub fn attach_video_billing_metadata(value: &mut Value, metadata: &VideoBillingMetadata) {
    if !value.is_object() {
        return;
    }
    if !value.get("neogate").is_some_and(Value::is_object) {
        value["neogate"] = Value::Object(Default::default());
    }
    value["neogate"]["video_billing"] = serde_json::to_value(metadata).unwrap_or(Value::Null);
}

pub fn copy_neogate_metadata(from: &Value, to: &mut Value) {
    let Some(neogate) = from.get("neogate").cloned() else {
        return;
    };
    if to.is_object() {
        to["neogate"] = neogate;
    }
}

pub fn video_billing_metadata(value: &Value) -> Option<VideoBillingMetadata> {
    value
        .get("neogate")
        .and_then(|neogate| neogate.get("video_billing"))
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub fn settlement_usage_and_price(
    metadata: &VideoBillingMetadata,
    upstream_metadata: &Value,
) -> Option<(BillableUsage, Price)> {
    match metadata.mode {
        VideoBillingMode::OfficialToken => {
            let total_tokens = total_tokens_from_metadata(upstream_metadata)?;
            Some((
                BillableUsage::video_tokens(TokenUsage {
                    input_tokens: total_tokens,
                    output_tokens: 0,
                    cached_input_tokens: None,
                    cache_creation_input_tokens: None,
                    cache_creation_input_tokens_5m: None,
                    cache_creation_input_tokens_1h: None,
                    reasoning_output_tokens: None,
                    audio_input_tokens: None,
                    audio_output_tokens: None,
                }),
                video_settlement_price(BillingMeter::Video, metadata.price_micros),
            ))
        }
        VideoBillingMode::PerSecond => Some((
            BillableUsage::video_seconds(metadata.duration_seconds),
            video_settlement_price(BillingMeter::Video, metadata.price_micros),
        )),
    }
}

pub fn total_tokens_from_metadata(value: &Value) -> Option<i64> {
    value
        .get("usage")
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("usage"))
        })
        .and_then(|usage| usage.get("total_tokens"))
        .and_then(value_as_positive_i64)
}

pub fn provider_video_duration_seconds(value: &Value) -> Option<i64> {
    value
        .get("usage")
        .and_then(|usage| {
            usage
                .get("output_video_duration")
                .or_else(|| usage.get("duration"))
        })
        .and_then(value_as_positive_i64)
}

fn video_settlement_price(billing_meter: BillingMeter, price_micros: i64) -> Price {
    Price {
        input_price_micros: price_micros,
        output_price_micros: price_micros,
        cache_read_price_micros: None,
        cache_write_price_micros: None,
        billing_meter,
        unit_price_micros: (billing_meter == BillingMeter::Video).then_some(price_micros),
        video_billing_mode: None,
        video_price_tiers: Vec::new(),
    }
}

fn matching_tier<'a>(tiers: &'a [VideoPriceTier], resolution: &str) -> Option<&'a VideoPriceTier> {
    tiers.iter().find(|tier| {
        tier.resolutions
            .iter()
            .flat_map(|value| value.split(','))
            .any(|candidate| {
                let candidate = candidate.trim();
                candidate == "*" || normalize_resolution(Some(candidate)) == resolution
            })
    })
}

fn required_positive(value: Option<i64>, label: &str) -> AppResult<i64> {
    match value {
        Some(value) if value > 0 => Ok(value),
        _ => Err(AppError::BadRequest(format!("{label} is required"))),
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn positive_i64_field(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(value_as_positive_i64)
}

fn value_as_positive_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<i64>().ok())
        })
        .filter(|value| *value > 0)
}

fn json_has_video_input(value: &Value) -> bool {
    let Some(content) = value.get("content").and_then(Value::as_array) else {
        return false;
    };
    content.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("video_url")
            || item.get("video_url").is_some()
    })
}

fn normalize_resolution(value: Option<&str>) -> String {
    let value = value
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if value.is_empty() {
        return DEFAULT_RESOLUTION.to_string();
    }
    let compact = value.replace([' ', '_'], "");
    if compact.contains("4k") || compact.contains("2160") {
        "4k".to_string()
    } else if compact.contains("1080") {
        "1080p".to_string()
    } else if compact.contains("720") {
        "720p".to_string()
    } else if compact.contains("480") {
        "480p".to_string()
    } else {
        compact
    }
}

fn micros_for_tokens(tokens: i64, price_micros: i64) -> i64 {
    if tokens <= 0 || price_micros <= 0 {
        return 0;
    }
    let product = (tokens as i128).saturating_mul(price_micros as i128);
    let rounded = (product + MICROS_PER_MAJOR_UNIT as i128 - 1) / MICROS_PER_MAJOR_UNIT as i128;
    i64::try_from(rounded).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn price(mode: VideoBillingMode) -> Price {
        Price {
            input_price_micros: 0,
            output_price_micros: 0,
            cache_read_price_micros: None,
            cache_write_price_micros: None,
            billing_meter: BillingMeter::Video,
            unit_price_micros: None,
            video_billing_mode: Some(mode),
            video_price_tiers: vec![
                VideoPriceTier {
                    resolutions: vec!["480p,720p".to_string()],
                    input_with_video_micros: Some(28_000_000),
                    input_without_video_micros: Some(46_000_000),
                    estimated_tokens_per_second: Some(100_000),
                    input_with_video_unit_micros: Some(3_000_000),
                    input_without_video_unit_micros: Some(5_000_000),
                },
                VideoPriceTier {
                    resolutions: vec!["1080p".to_string(), "4k".to_string()],
                    input_with_video_micros: Some(31_000_000),
                    input_without_video_micros: Some(51_000_000),
                    estimated_tokens_per_second: Some(200_000),
                    input_with_video_unit_micros: Some(7_000_000),
                    input_without_video_unit_micros: Some(9_000_000),
                },
            ],
        }
    }

    #[test]
    fn defaults_resolution_and_duration() {
        let input = json_video_billing_input(&json!({"model":"doubao-seedance-2.0"}));
        assert_eq!(input.resolution, "480p");
        assert_eq!(input.duration_seconds, 5);
        assert!(!input.has_video_input);
    }

    #[test]
    fn detects_video_input_and_resolution_tier() {
        let input = json_video_billing_input(&json!({
            "resolution": "1280x720",
            "duration": "8",
            "content": [{"type":"video_url","video_url":{"url":"https://example.test/a.mp4"}}]
        }));
        assert_eq!(input.resolution, "720p");
        assert_eq!(input.duration_seconds, 8);
        assert!(input.has_video_input);
    }

    #[test]
    fn official_token_uses_estimate_for_reservation() {
        let input = video_billing_input(Some("720p"), Some(5), true);
        let prepared = prepare_seedance_video_billing(
            "doubao",
            "doubao-seedance-2.0",
            &price(VideoBillingMode::OfficialToken),
            &input,
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.metadata.price_micros, 28_000_000);
        assert_eq!(prepared.metadata.estimated_tokens, Some(500_000));
        assert_eq!(prepared.estimated_micros, 14_000_000);
    }

    #[test]
    fn video_billing_is_not_limited_to_doubao_provider() {
        let input = video_billing_input(Some("720p"), Some(5), false);
        let prepared = prepare_seedance_video_billing(
            "qwen",
            "happyhorse-1.1-t2v",
            &price(VideoBillingMode::OfficialToken),
            &input,
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.metadata.price_micros, 46_000_000);
        assert_eq!(prepared.estimated_micros, 23_000_000);
    }

    #[test]
    fn per_second_uses_input_kind_unit_price() {
        let input = video_billing_input(Some("4k"), Some(6), false);
        let prepared = prepare_seedance_video_billing(
            "doubao",
            "doubao-seedance-2.0",
            &price(VideoBillingMode::PerSecond),
            &input,
        )
        .unwrap()
        .unwrap();

        assert_eq!(prepared.metadata.price_micros, 9_000_000);
        assert_eq!(prepared.estimated_micros, 54_000_000);
    }

    #[test]
    fn wildcard_resolution_tier_matches_any_resolution() {
        let mut price = price(VideoBillingMode::OfficialToken);
        price.video_price_tiers = vec![VideoPriceTier {
            resolutions: vec!["*".to_string()],
            input_with_video_micros: Some(16_000_000),
            input_without_video_micros: Some(8_000_000),
            estimated_tokens_per_second: Some(100_000),
            input_with_video_unit_micros: None,
            input_without_video_unit_micros: None,
        }];
        let input = video_billing_input(Some("1080p"), Some(5), false);
        let prepared =
            prepare_seedance_video_billing("doubao", "doubao-seedance-1.5-pro", &price, &input)
                .unwrap()
                .unwrap();

        assert_eq!(prepared.metadata.price_micros, 8_000_000);
    }

    #[test]
    fn official_token_settlement_keeps_video_meter() {
        let metadata = VideoBillingMetadata {
            mode: VideoBillingMode::OfficialToken,
            resolution: "720p".to_string(),
            duration_seconds: 5,
            has_video_input: false,
            price_micros: 46_000_000,
            estimated_tokens_per_second: Some(100_000),
            estimated_tokens: Some(500_000),
            estimated_micros: 23_000_000,
        };
        let (usage, price) =
            settlement_usage_and_price(&metadata, &json!({"usage":{"total_tokens":321_000}}))
                .unwrap();

        assert_eq!(usage.meter, BillingMeter::Video);
        assert_eq!(usage.token_usage.unwrap().input_tokens, 321_000);
        assert_eq!(price.billing_meter, BillingMeter::Video);
    }

    #[test]
    fn reads_provider_video_duration_seconds() {
        let value = serde_json::json!({
            "usage": {
                "duration": 5,
                "output_video_duration": 4
            }
        });

        assert_eq!(provider_video_duration_seconds(&value), Some(4));
    }
}
