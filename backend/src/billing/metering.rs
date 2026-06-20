use serde_json::Value;

use super::{BillableUsage, BillingMeter, Price, TokenUsage};

const TOKENS_PER_MILLION: i128 = 1_000_000;
const DEFAULT_CACHE_READ_PRICE_DIVISOR: i64 = 10;

pub fn estimate_input_tokens(body: &[u8]) -> i64 {
    ((body.len() as i64) + 3) / 4
}

pub fn estimated_cost_micro_usd(input_tokens: i64, output_tokens: i64, price: &Price) -> i64 {
    micro_usd_for_tokens(input_tokens, price.input_price_usd_micros).saturating_add(
        micro_usd_for_tokens(output_tokens, price.output_price_usd_micros),
    )
}

pub fn cost_for_usage(usage: TokenUsage, price: &Price) -> i64 {
    let cached_input_tokens = usage
        .cached_input_tokens
        .unwrap_or(0)
        .clamp(0, usage.input_tokens.max(0));
    let uncached_input_tokens = usage.input_tokens.saturating_sub(cached_input_tokens);
    let cache_create_input_tokens = usage.cache_creation_input_tokens.unwrap_or(0).max(0);
    let cache_in_price = price
        .cache_read_price_usd_micros
        .unwrap_or(price.input_price_usd_micros / DEFAULT_CACHE_READ_PRICE_DIVISOR);
    let cache_create_in_price = price
        .cache_write_price_usd_micros
        .unwrap_or(price.input_price_usd_micros);

    micro_usd_for_tokens(uncached_input_tokens, price.input_price_usd_micros)
        .saturating_add(micro_usd_for_tokens(cached_input_tokens, cache_in_price))
        .saturating_add(micro_usd_for_tokens(
            cache_create_input_tokens,
            cache_create_in_price,
        ))
        .saturating_add(micro_usd_for_tokens(
            usage.output_tokens,
            price.output_price_usd_micros,
        ))
}

pub fn cost_for_billable_usage(usage: BillableUsage, price: &Price) -> i64 {
    match usage.meter {
        BillingMeter::Token => usage
            .token_usage
            .map(|token_usage| cost_for_usage(token_usage, price))
            .unwrap_or(0),
        BillingMeter::Image => usage.billable_units.max(0).saturating_mul(
            price
                .unit_price_usd_micros
                .expect("image billing requires unit_price_usd_micros")
                .max(0),
        ),
    }
}

fn micro_usd_for_tokens(tokens: i64, price_usd_micros: i64) -> i64 {
    if tokens <= 0 || price_usd_micros <= 0 {
        return 0;
    }

    let product = (tokens as i128).saturating_mul(price_usd_micros as i128);
    let rounded = (product + TOKENS_PER_MILLION - 1) / TOKENS_PER_MILLION;
    i64::try_from(rounded).unwrap_or(i64::MAX)
}

fn parse_usage_from_json(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage").or_else(|| {
        value
            .get("response")
            .and_then(|response| response.get("usage"))
    })?;
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)?;
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .get("total_tokens")
                .and_then(Value::as_i64)
                .map(|total| total.saturating_sub(input_tokens).max(0))
        })
        .unwrap_or(0);

    let input_details = usage
        .get("prompt_tokens_details")
        .or_else(|| usage.get("input_tokens_details"));
    let output_details = usage
        .get("completion_tokens_details")
        .or_else(|| usage.get("output_tokens_details"));
    let cache_creation_5m = usage
        .get("cache_creation")
        .and_then(|details| details.get("ephemeral_5m_input_tokens"))
        .and_then(Value::as_i64);
    let cache_creation_1h = usage
        .get("cache_creation")
        .and_then(|details| details.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_i64);
    let cache_creation_input_tokens = usage
        .get("cache_creation_input_tokens")
        .or_else(|| input_details.and_then(|details| details.get("cached_creation_tokens")))
        .and_then(Value::as_i64)
        .or_else(|| {
            let total = cache_creation_5m.unwrap_or(0) + cache_creation_1h.unwrap_or(0);
            (total > 0).then_some(total)
        });

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: usage
            .get("cache_read_input_tokens")
            .or_else(|| input_details.and_then(|details| details.get("cached_tokens")))
            .and_then(Value::as_i64),
        cache_creation_input_tokens,
        cache_creation_input_tokens_5m: cache_creation_5m,
        cache_creation_input_tokens_1h: cache_creation_1h,
        reasoning_output_tokens: output_details
            .and_then(|details| details.get("reasoning_tokens"))
            .and_then(Value::as_i64),
        audio_input_tokens: input_details
            .and_then(|details| details.get("audio_tokens"))
            .and_then(Value::as_i64),
        audio_output_tokens: output_details
            .and_then(|details| details.get("audio_tokens"))
            .and_then(Value::as_i64),
    })
}

pub fn parse_usage_from_bytes(bytes: &[u8], streamed: bool) -> Option<TokenUsage> {
    if !streamed {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        return parse_usage_from_json(&value);
    }

    let text = std::str::from_utf8(bytes).ok()?;
    let mut latest = None;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        if let Some(usage) = parse_usage_from_sse_data(data) {
            latest = Some(usage);
        }
    }
    latest
}

pub fn parse_usage_from_sse_data(data: &str) -> Option<TokenUsage> {
    let value = serde_json::from_str::<Value>(data).ok()?;
    parse_usage_from_json(&value).or_else(|| parse_anthropic_delta_usage(&value))
}

fn parse_anthropic_delta_usage(value: &Value) -> Option<TokenUsage> {
    let usage = value.get("usage").or_else(|| {
        value
            .get("message")
            .and_then(|message| message.get("usage"))
    })?;
    let input_tokens = usage
        .get("input_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(Value::as_i64)?;
    let cache_creation_5m = usage
        .get("cache_creation")
        .and_then(|details| details.get("ephemeral_5m_input_tokens"))
        .and_then(Value::as_i64);
    let cache_creation_1h = usage
        .get("cache_creation")
        .and_then(|details| details.get("ephemeral_1h_input_tokens"))
        .and_then(Value::as_i64);
    let cache_creation_input_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(Value::as_i64)
        .or_else(|| {
            let total = cache_creation_5m.unwrap_or(0) + cache_creation_1h.unwrap_or(0);
            (total > 0).then_some(total)
        });
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: usage.get("cache_read_input_tokens").and_then(Value::as_i64),
        cache_creation_input_tokens,
        cache_creation_input_tokens_5m: cache_creation_5m,
        cache_creation_input_tokens_1h: cache_creation_1h,
        reasoning_output_tokens: None,
        audio_input_tokens: None,
        audio_output_tokens: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_uses_price_usd_micros() {
        let price = Price {
            input_price_usd_micros: 270_000,
            output_price_usd_micros: 1_100_000,
            cache_read_price_usd_micros: None,
            cache_write_price_usd_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_usd_micros: None,
        };

        assert_eq!(
            estimated_cost_micro_usd(1_000_000, 1_000_000, &price),
            1_370_000
        );
        assert_eq!(estimated_cost_micro_usd(1, 0, &price), 1);
    }

    #[test]
    fn cost_discounts_cached_input_tokens() {
        let price = Price {
            input_price_usd_micros: 5_000_000,
            output_price_usd_micros: 30_000_000,
            cache_read_price_usd_micros: Some(500_000),
            cache_write_price_usd_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_usd_micros: None,
        };
        let usage = TokenUsage {
            input_tokens: 77_931,
            output_tokens: 1_602,
            cached_input_tokens: Some(77_184),
            cache_creation_input_tokens: None,
            cache_creation_input_tokens_5m: None,
            cache_creation_input_tokens_1h: None,
            reasoning_output_tokens: Some(1_034),
            audio_input_tokens: None,
            audio_output_tokens: None,
        };

        assert_eq!(cost_for_usage(usage, &price), 90_387);
    }

    #[test]
    fn parses_openai_usage_details() {
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"prompt_tokens":98502,"completion_tokens":93,"total_tokens":98595,"prompt_tokens_details":{"cached_tokens":96640,"audio_tokens":7},"completion_tokens_details":{"reasoning_tokens":11,"audio_tokens":13}}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 98_502);
        assert_eq!(usage.output_tokens, 93);
        assert_eq!(usage.cached_input_tokens, Some(96_640));
        assert_eq!(usage.reasoning_output_tokens, Some(11));
        assert_eq!(usage.audio_input_tokens, Some(7));
        assert_eq!(usage.audio_output_tokens, Some(13));
    }

    #[test]
    fn parses_openai_responses_completed_stream_usage() {
        let usage = parse_usage_from_sse_data(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":98502,"output_tokens":93,"total_tokens":98595,"input_tokens_details":{"cached_tokens":96640},"output_tokens_details":{"reasoning_tokens":11}}}}"#,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 98_502);
        assert_eq!(usage.output_tokens, 93);
        assert_eq!(usage.cached_input_tokens, Some(96_640));
        assert_eq!(usage.reasoning_output_tokens, Some(11));
    }

    #[test]
    fn parses_openai_embedding_prompt_only_usage() {
        let usage = parse_usage_from_bytes(
            br#"{"object":"list","data":[],"usage":{"prompt_tokens":8,"total_tokens":8}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 8);
        assert_eq!(usage.output_tokens, 0);
    }

    #[test]
    fn parses_openai_image_stream_usage() {
        let usage = parse_usage_from_sse_data(
            r#"{"type":"image_generation.completed","usage":{"input_tokens":120,"output_tokens":42,"total_tokens":162}}"#,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 120);
        assert_eq!(usage.output_tokens, 42);
    }

    #[test]
    fn parses_anthropic_cache_details() {
        let usage = parse_usage_from_sse_data(
            r#"{"message":{"usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":80,"cache_creation":{"ephemeral_5m_input_tokens":3,"ephemeral_1h_input_tokens":7}}}}"#,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cached_input_tokens, Some(80));
        assert_eq!(usage.cache_creation_input_tokens, Some(10));
        assert_eq!(usage.cache_creation_input_tokens_5m, Some(3));
        assert_eq!(usage.cache_creation_input_tokens_1h, Some(7));
    }
}
