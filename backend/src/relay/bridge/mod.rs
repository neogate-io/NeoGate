mod chat;
mod responses;
mod stream;

use serde_json::Value;

pub(crate) use chat::{
    finish_anthropic_as_openai_chat, finish_chat_as_anthropic, messages_to_openai_chat,
    openai_chat_to_anthropic_messages,
};
pub(crate) use responses::{
    finish_anthropic_as_openai_response, openai_response_to_anthropic_messages,
};

pub(super) fn finish_reason_to_anthropic(reason: &str) -> &'static str {
    match reason {
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        _ => "end_turn",
    }
}

pub(super) fn anthropic_stop_reason_to_openai(reason: &str) -> &'static str {
    match reason {
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        _ => "stop",
    }
}

pub(super) fn estimate_tokens(text: &str) -> i64 {
    ((text.len() as i64) + 3) / 4
}

pub(super) fn content_value_to_text(
    value: &Value,
    text_types: &[&str],
    allow_object: bool,
) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| content_item_text(item, text_types))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) if allow_object => content_item_text(value, text_types)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    }
}

fn content_item_text<'a>(item: &'a Value, text_types: &[&str]) -> Option<&'a str> {
    match item {
        Value::String(text) => Some(text.as_str()),
        Value::Object(object)
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| text_types.contains(&kind)) =>
        {
            object.get("text").and_then(Value::as_str)
        }
        _ => None,
    }
}
