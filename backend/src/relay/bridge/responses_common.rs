use serde_json::{json, Value};

use super::anthropic_cache_creation_tokens;

pub(super) fn openai_response_reasoning_item(id: String, text: String, status: &str) -> Value {
    json!({
        "id": id,
        "type": "reasoning",
        "status": status,
        "summary": [{
            "type": "summary_text",
            "text": text,
        }],
    })
}

pub(super) fn openai_response_message_item(id: String, text: String, status: &str) -> Value {
    json!({
        "id": id,
        "type": "message",
        "status": status,
        "role": "assistant",
        "content": [{
            "type": "output_text",
            "text": text,
            "annotations": [],
        }],
    })
}

pub(super) fn openai_response_usage(
    usage: Option<&Value>,
    input_tokens: i64,
    output_tokens: i64,
) -> Value {
    // 兼容两条来源路径：Anthropic 原生用 cache_read_input_tokens；OpenAI chat
    // 用 prompt_tokens_details.cached_tokens / prompt_cache_hit_tokens / cached_tokens。
    // 只读前者会让 chat→responses 的缓存 token 归零，缓存输入被按全价重复计费。
    let cache_read = usage
        .and_then(|usage| {
            usage
                .get("cache_read_input_tokens")
                .or_else(|| {
                    usage
                        .get("prompt_tokens_details")
                        .and_then(|details| details.get("cached_tokens"))
                })
                .or_else(|| {
                    usage
                        .get("input_tokens_details")
                        .and_then(|details| details.get("cached_tokens"))
                })
                .or_else(|| usage.get("prompt_cache_hit_tokens"))
                .or_else(|| usage.get("cached_tokens"))
        })
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let cache_create = anthropic_cache_creation_tokens(usage);
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens.saturating_add(output_tokens),
        "input_tokens_details": {
            "cached_tokens": cache_read,
            "cached_creation_tokens": cache_create,
        },
        "output_tokens_details": { "reasoning_tokens": 0 },
    })
}

/// 以 SSE 格式（`event: <event>\ndata: <json>\n\n`）写入一条事件。
/// 多个桥接转换器共用此逻辑，避免各自重复实现。
pub(super) fn push_sse_event(out: &mut Vec<u8>, event: &str, data: &Value) {
    out.extend_from_slice(b"event: ");
    out.extend_from_slice(event.as_bytes());
    out.extend_from_slice(b"\ndata: ");
    serde_json::to_writer(&mut *out, data).expect("serializing JSON value to Vec cannot fail");
    out.extend_from_slice(b"\n\n");
}

/// 从 OpenAI 兼容响应的 choices[].usage.cached_tokens 提取首个正值缓存 token 数。
pub(super) fn choice_usage_cached_tokens(value: &Value) -> Option<i64> {
    value
        .get("choices")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|choice| {
            choice
                .get("usage")
                .and_then(|usage| usage.get("cached_tokens"))
                .and_then(Value::as_i64)
        })
        .find(|tokens| *tokens > 0)
}

pub(super) fn drain_sse_lines(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut lines = Vec::new();
    while let Some(index) = buffer.iter().position(|byte| *byte == b'\n') {
        let mut line = buffer.drain(..=index).collect::<Vec<_>>();
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        lines.push(line);
    }
    lines
}

pub(super) struct StreamingToolCall {
    pub(super) output_index: i64,
    pub(super) item_id: String,
    pub(super) call_id: String,
    pub(super) name: String,
    pub(super) arguments: String,
}
