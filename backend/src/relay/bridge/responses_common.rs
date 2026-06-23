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
    let cache_read = usage
        .and_then(|usage| usage.get("cache_read_input_tokens"))
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
