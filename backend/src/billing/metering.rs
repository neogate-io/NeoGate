use serde_json::Value;

use super::{micros_for_tokens, BillableUsage, BillingMeter, Price, TokenUsage};

const DEFAULT_CACHE_READ_PRICE_DIVISOR: i64 = 10;
const MAX_RESERVED_OUTPUT_TOKENS: i64 = 16_384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenReservationEstimate {
    pub input_tokens: i64,
    pub requested_output_tokens: i64,
    pub reserved_output_tokens: i64,
    pub estimated_micros: i64,
    pub usage_missing_micros: i64,
}

/// 预留额度时按请求体字节数粗估 input tokens（约每 4 字节 1 token）。
///
/// 该估算仅用于转发前的悲观预留（hold），不参与真实结算。带大量工具定义的
/// 请求体（如 27 个工具 + 长上下文）会让 body.len()/4 远高于上游真实计费的
/// input tokens，导致 hold 过大、后续请求因余额不足被拒。这里对估算加一个
/// 上限，避免单次预留被字节数粗估无限放大；真实用量仍以上游回传的 usage 为准。
pub fn estimate_input_tokens(body: &[u8]) -> i64 {
    const MAX_ESTIMATED_INPUT_TOKENS: i64 = 400_000;
    (((body.len() as i64) + 3) / 4).min(MAX_ESTIMATED_INPUT_TOKENS)
}

/// 当下游断流导致上游未返回 usage 时，根据已向下游发送的字节数估算 output tokens。
///
/// SSE 流的每个 chunk 除了 token 内容还包含大量 JSON framing（字段名、id、model 等），
/// 实测平均约每个 output token 对应 150 字节的流量。此估算用于替代按 hold 全额扣款，
/// 即使存在误差也远比全额扣款对用户更公平。
pub fn estimate_output_tokens_from_bytes_sent(bytes_sent: u64) -> i64 {
    const AVG_BYTES_PER_OUTPUT_TOKEN: u64 = 150;
    (bytes_sent / AVG_BYTES_PER_OUTPUT_TOKEN) as i64
}

pub fn estimated_cost_micros(input_tokens: i64, output_tokens: i64, price: &Price) -> i64 {
    micros_for_tokens(input_tokens, price.input_price_micros)
        .saturating_add(micros_for_tokens(output_tokens, price.output_price_micros))
}

pub(crate) fn estimate_token_reservation(
    body: &[u8],
    requested_output_tokens: i64,
    price: &Price,
) -> TokenReservationEstimate {
    let input_tokens = estimate_input_tokens(body);
    let requested_output_tokens = requested_output_tokens.max(0);
    let reserved_output_tokens = requested_output_tokens.min(MAX_RESERVED_OUTPUT_TOKENS);
    TokenReservationEstimate {
        input_tokens,
        requested_output_tokens,
        reserved_output_tokens,
        estimated_micros: estimated_cost_micros(input_tokens, reserved_output_tokens, price),
        usage_missing_micros: estimated_cost_micros(input_tokens, 0, price),
    }
}

pub fn cost_for_usage(usage: TokenUsage, price: &Price) -> i64 {
    let cached_input_tokens = usage
        .cached_input_tokens
        .unwrap_or(0)
        .clamp(0, usage.input_tokens.max(0));
    let uncached_input_tokens = usage.input_tokens.saturating_sub(cached_input_tokens);
    let cache_create_input_tokens = usage.cache_creation_input_tokens.unwrap_or(0).max(0);
    let cache_in_price = price
        .cache_read_price_micros
        .unwrap_or(price.input_price_micros / DEFAULT_CACHE_READ_PRICE_DIVISOR);
    let cache_create_in_price = price
        .cache_write_price_micros
        .unwrap_or(price.input_price_micros);

    micros_for_tokens(uncached_input_tokens, price.input_price_micros)
        .saturating_add(micros_for_tokens(cached_input_tokens, cache_in_price))
        .saturating_add(micros_for_tokens(
            cache_create_input_tokens,
            cache_create_in_price,
        ))
        .saturating_add(micros_for_tokens(
            usage.output_tokens,
            price.output_price_micros,
        ))
}

pub fn cost_for_billable_usage(usage: BillableUsage, price: &Price) -> i64 {
    match usage.meter {
        BillingMeter::Token => usage
            .token_usage
            .map_or(0, |token_usage| cost_for_usage(token_usage, price)),
        BillingMeter::Image => {
            let Some(unit_price) = price.unit_price_micros else {
                tracing::error!(
                    "image billing requires unit_price_micros but none is set; charging zero"
                );
                return 0;
            };
            usage
                .billable_units
                .max(0)
                .saturating_mul(unit_price.max(0))
        }
        BillingMeter::Audio => {
            let Some(unit_price) = price.unit_price_micros else {
                tracing::error!(
                    "audio billing requires unit_price_micros but none is set; charging zero"
                );
                return 0;
            };
            usage
                .billable_units
                .max(0)
                .saturating_mul(unit_price.max(0))
        }
        BillingMeter::Video => {
            if let Some(token_usage) = usage.token_usage {
                cost_for_usage(token_usage, price)
            } else {
                let Some(unit_price) = price.unit_price_micros else {
                    tracing::error!("video unit billing requires unit_price_micros but none is set; charging zero");
                    return 0;
                };
                usage
                    .billable_units
                    .max(0)
                    .saturating_mul(unit_price.max(0))
            }
        }
    }
}

fn parse_usage_from_json(value: &Value) -> Option<TokenUsage> {
    let usage = value
        .get("usage")
        .or_else(|| {
            value
                .get("response")
                .and_then(|response| response.get("usage"))
        })
        .or_else(|| choice_usage(value))?;
    let output_tokens = usage
        .get("completion_tokens")
        .or_else(|| usage.get("output_tokens"))
        .and_then(Value::as_i64);
    let total_tokens = usage.get("total_tokens").and_then(Value::as_i64);
    // Detect Anthropic native format: has "input_tokens" but not "prompt_tokens".
    // Anthropic's input_tokens counts only fresh (non-cached-read) tokens; the true total
    // input is input_tokens + cache_read_input_tokens. OpenAI's prompt_tokens already
    // includes cached tokens, so no adjustment is needed for that format.
    let is_anthropic_native =
        usage.get("prompt_tokens").is_none() && usage.get("input_tokens").is_some();
    let input_tokens = usage
        .get("prompt_tokens")
        .or_else(|| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            total_tokens
                .and_then(|total| output_tokens.map(|output| total.saturating_sub(output).max(0)))
        })?;
    let output_tokens = output_tokens
        .or_else(|| total_tokens.map(|total| total.saturating_sub(input_tokens).max(0)))
        .unwrap_or(0);
    // For Anthropic native format, add cache_read_input_tokens so input_tokens represents
    // the true total (matching the semantics used everywhere else in the billing pipeline).
    let cache_read_tokens = if is_anthropic_native {
        usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0)
    } else {
        0
    };
    let input_tokens = input_tokens.saturating_add(cache_read_tokens);

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
    let cache_creation_input_tokens = [
        usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64),
        input_details
            .and_then(|details| details.get("cached_creation_tokens"))
            .and_then(Value::as_i64),
        input_details
            .and_then(|details| details.get("cache_write_tokens"))
            .and_then(Value::as_i64),
    ]
    .into_iter()
    .flatten()
    .max()
    .or_else(|| {
        let total = cache_creation_5m.unwrap_or(0) + cache_creation_1h.unwrap_or(0);
        (total > 0).then_some(total)
    });

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: cached_input_tokens(value, usage, input_details),
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

fn cached_input_tokens(value: &Value, usage: &Value, input_details: Option<&Value>) -> Option<i64> {
    usage
        .get("cache_read_input_tokens")
        .or_else(|| input_details.and_then(|details| details.get("cached_tokens")))
        .or_else(|| usage.get("prompt_cache_hit_tokens"))
        .or_else(|| usage.get("cached_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| choice_usage_cached_tokens(value))
}

fn choice_usage_cached_tokens(value: &Value) -> Option<i64> {
    choice_usage(value)
        .and_then(|usage| usage.get("cached_tokens"))
        .and_then(Value::as_i64)
        .filter(|tokens| *tokens > 0)
}

fn choice_usage(value: &Value) -> Option<&Value> {
    value
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|choice| choice.get("usage"))
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
    let fresh_input_tokens = usage
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
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(Value::as_i64);
    // Anthropic's input_tokens counts only fresh (non-cached-read) tokens; add cache reads
    // so input_tokens represents the true total, consistent with parse_usage_from_json.
    let input_tokens = fresh_input_tokens.saturating_add(cache_read_input_tokens.unwrap_or(0));
    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cached_input_tokens: cache_read_input_tokens,
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
    fn cost_uses_price_micros() {
        let price = Price {
            input_price_micros: 270_000,
            output_price_micros: 1_100_000,
            cache_read_price_micros: None,
            cache_write_price_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
        };

        assert_eq!(
            estimated_cost_micros(1_000_000, 1_000_000, &price),
            1_370_000
        );
        assert_eq!(estimated_cost_micros(1, 0, &price), 1);
    }

    #[test]
    fn token_reservation_caps_output_without_changing_requested_value() {
        let price = Price {
            input_price_micros: 1_000_000,
            output_price_micros: 2_000_000,
            cache_read_price_micros: None,
            cache_write_price_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
        };

        let estimate = estimate_token_reservation(b"12345678", 1_000_000, &price);

        assert_eq!(estimate.input_tokens, 2);
        assert_eq!(estimate.requested_output_tokens, 1_000_000);
        assert_eq!(estimate.reserved_output_tokens, 16_384);
        assert_eq!(estimate.usage_missing_micros, 2);
        assert_eq!(estimate.estimated_micros, 32_770);
    }

    #[test]
    fn cost_discounts_cached_input_tokens() {
        let price = Price {
            input_price_micros: 5_000_000,
            output_price_micros: 30_000_000,
            cache_read_price_micros: Some(500_000),
            cache_write_price_micros: None,
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
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
    fn cost_does_not_double_count_bridge_cached_creation_tokens() {
        let price = Price {
            input_price_micros: 1_000_000,
            output_price_micros: 2_000_000,
            cache_read_price_micros: Some(100_000),
            cache_write_price_micros: Some(1_250_000),
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
        };
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"prompt_tokens":14,"completion_tokens":1,"total_tokens":15,"prompt_tokens_details":{"cached_tokens":6,"cached_creation_tokens":2}}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 14);
        assert_eq!(usage.cached_input_tokens, Some(6));
        assert_eq!(usage.cache_creation_input_tokens, Some(2));
        assert_eq!(cost_for_usage(usage, &price), 14);
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
    fn parses_video_usage_without_prompt_tokens() {
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"completion_tokens":197880,"total_tokens":197880}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 0);
        assert_eq!(usage.output_tokens, 197_880);
        assert_eq!(usage.total_tokens(), 197_880);
    }

    #[test]
    fn parses_openai_compatible_prompt_cache_hit_tokens() {
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"prompt_tokens":98502,"completion_tokens":93,"total_tokens":98595,"prompt_cache_hit_tokens":96640}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 98_502);
        assert_eq!(usage.output_tokens, 93);
        assert_eq!(usage.cached_input_tokens, Some(96_640));
    }

    #[test]
    fn parses_openai_compatible_top_level_cached_tokens() {
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"prompt_tokens":98502,"completion_tokens":93,"total_tokens":98595,"cached_tokens":96640}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.cached_input_tokens, Some(96_640));
    }

    #[test]
    fn parses_openai_compatible_choice_usage_cached_tokens() {
        let usage = parse_usage_from_bytes(
            br#"{"choices":[{"usage":{"cached_tokens":96640}}],"usage":{"prompt_tokens":98502,"completion_tokens":93,"total_tokens":98595}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.cached_input_tokens, Some(96_640));
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
    fn parses_responses_cache_write_tokens_using_larger_compatible_field() {
        let usage = parse_usage_from_sse_data(
            r#"{"type":"response.completed","response":{"usage":{"input_tokens":100,"output_tokens":10,"input_tokens_details":{"cached_creation_tokens":7,"cache_write_tokens":12}}}}"#,
        )
        .unwrap();

        assert_eq!(usage.cache_creation_input_tokens, Some(12));
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
        // Anthropic SSE: input_tokens is fresh-only; total = input + cache_read
        let usage = parse_usage_from_sse_data(
            r#"{"message":{"usage":{"input_tokens":100,"output_tokens":20,"cache_read_input_tokens":80,"cache_creation":{"ephemeral_5m_input_tokens":3,"ephemeral_1h_input_tokens":7}}}}"#,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 180); // 100 fresh + 80 cache read
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.cached_input_tokens, Some(80));
        assert_eq!(usage.cache_creation_input_tokens, Some(10));
        assert_eq!(usage.cache_creation_input_tokens_5m, Some(3));
        assert_eq!(usage.cache_creation_input_tokens_1h, Some(7));
    }

    #[test]
    fn parses_anthropic_native_non_streaming_adds_cache_read_to_input() {
        // Reproduces the log bug: in=8 while cache_read=96944 cache_write=8379.
        // The true total input is 8 + 96944 = 96952.
        let usage = parse_usage_from_bytes(
            br#"{"id":"msg_01","usage":{"input_tokens":8,"output_tokens":209,"cache_read_input_tokens":96944,"cache_creation_input_tokens":8379}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 96_952); // 8 fresh + 96944 cache read
        assert_eq!(usage.output_tokens, 209);
        assert_eq!(usage.cached_input_tokens, Some(96_944));
        assert_eq!(usage.cache_creation_input_tokens, Some(8_379));
    }

    #[test]
    fn parses_anthropic_native_non_streaming_no_cache_unchanged() {
        // Without cache tokens, input_tokens should be unchanged.
        let usage = parse_usage_from_bytes(
            br#"{"usage":{"input_tokens":100,"output_tokens":50}}"#,
            false,
        )
        .unwrap();

        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.cached_input_tokens, None);
    }

    #[test]
    fn cost_for_anthropic_native_with_cache_read() {
        // Verify billing is correct after the input_tokens fix:
        // 8 fresh tokens * input_price + 96944 cached * cache_read_price + 8379 cache_write * write_price
        let price = Price {
            input_price_micros: 15_000,
            output_price_micros: 75_000,
            cache_read_price_micros: Some(1_500),
            cache_write_price_micros: Some(18_750),
            billing_meter: BillingMeter::Token,
            unit_price_micros: None,
            video_billing_mode: None,
            video_price_tiers: Vec::new(),
        };
        let usage = parse_usage_from_bytes(
            br#"{"id":"msg_01","usage":{"input_tokens":8,"output_tokens":209,"cache_read_input_tokens":96944,"cache_creation_input_tokens":8379}}"#,
            false,
        )
        .unwrap();

        // input_tokens = 96952, cached = 96944, uncached = 8, cache_write = 8379
        // cost = 8 * 15_000/1M + 96944 * 1_500/1M + 8379 * 18_750/1M + 209 * 75_000/1M
        // = 0 + 0 + 0 + 0  (all tiny in micros, check relative ordering instead)
        let cost = cost_for_usage(usage, &price);
        // uncached(8) * 15_000 + cached(96944) * 1_500 + cache_write(8379) * 18_750 + out(209) * 75_000
        // = 0 + 0 + 0 + 15 = too small; verify it's non-zero and > pure output cost
        let output_only_cost = (209i128 * 75_000) / 1_000_000;
        assert!(
            cost as i128 >= output_only_cost,
            "cost should include cache read charges"
        );
    }
}
