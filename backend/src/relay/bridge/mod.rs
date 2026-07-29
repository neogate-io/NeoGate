mod anthropic_to_chat;
mod anthropic_to_responses;
mod chat_to_anthropic;
mod chat_to_responses;
mod common;
mod reasoning_markup;
mod responses_common;
mod responses_reasoning;
mod responses_to_anthropic;
mod responses_to_chat;
mod stream;

use serde_json::{json, Map, Value};

pub(crate) use anthropic_to_chat::{
    anthropic_cache_creation_tokens, anthropic_usage_input_tokens, anthropic_usage_output_tokens,
};
pub(crate) use anthropic_to_chat::{finish_anthropic_as_openai_chat, messages_to_openai_chat};
pub(crate) use anthropic_to_responses::finish_anthropic_as_openai_response;
pub(crate) use chat_to_anthropic::{finish_chat_as_anthropic, openai_chat_to_anthropic_messages};
pub(crate) use chat_to_responses::finish_openai_chat_as_openai_response;
pub(crate) use responses_reasoning::finish_openai_response_with_reasoning_normalization;
pub(crate) use responses_to_anthropic::openai_response_to_anthropic_messages;
pub(crate) use responses_to_chat::openai_response_to_openai_chat;

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

pub(super) fn openai_reasoning_to_anthropic_thinking(object: &mut Map<String, Value>) {
    let effort = object
        .remove("reasoning_effort")
        .and_then(|value| value.as_str().map(str::to_string));
    let reasoning = object.remove("reasoning");

    if object.get("thinking").is_some() {
        return;
    }

    let mut budget_tokens = reasoning.as_ref().and_then(openai_reasoning_budget_tokens);
    if budget_tokens.is_none() {
        budget_tokens = reasoning
            .as_ref()
            .and_then(openai_reasoning_effort)
            .or(effort.as_deref())
            .and_then(reasoning_budget_from_effort);
    }

    if let Some(budget_tokens) = budget_tokens {
        object.insert(
            "thinking".to_string(),
            json!({ "type": "enabled", "budget_tokens": budget_tokens }),
        );
        ensure_anthropic_max_tokens_for_thinking(object, budget_tokens);
        object.remove("top_p");
        object.insert("temperature".to_string(), json!(1.0));
    }
}

pub(super) fn anthropic_thinking_to_openai_reasoning_effort(object: &mut Map<String, Value>) {
    let thinking = object.remove("thinking");
    let effort = thinking
        .as_ref()
        .and_then(anthropic_thinking_budget_tokens)
        .map(reasoning_effort_from_budget)
        .or_else(|| {
            object
                .get("output_config")
                .and_then(|config| config.get("effort"))
                .and_then(Value::as_str)
                .filter(|effort| reasoning_budget_from_effort(effort).is_some())
                .map(str::to_string)
        })
        .or_else(|| {
            thinking.as_ref().and_then(|thinking| {
                (thinking.get("type").and_then(Value::as_str) == Some("adaptive"))
                    .then_some("high".to_string())
            })
        });

    if let Some(effort) = effort {
        object.insert("reasoning_effort".to_string(), Value::String(effort));
    }
    object.remove("output_config");
}

fn openai_reasoning_budget_tokens(reasoning: &Value) -> Option<i64> {
    if reasoning.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    reasoning
        .get("max_tokens")
        .or_else(|| reasoning.get("budget_tokens"))
        .and_then(Value::as_i64)
        .filter(|budget| *budget > 0)
}

fn openai_reasoning_effort(reasoning: &Value) -> Option<&str> {
    if reasoning.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    reasoning.get("effort").and_then(Value::as_str)
}

fn anthropic_thinking_budget_tokens(thinking: &Value) -> Option<i64> {
    (thinking.get("type").and_then(Value::as_str) == Some("enabled"))
        .then(|| {
            thinking
                .get("budget_tokens")
                .and_then(Value::as_i64)
                .filter(|budget| *budget > 0)
        })
        .flatten()
}

fn reasoning_budget_from_effort(effort: &str) -> Option<i64> {
    match effort {
        "minimal" | "low" => Some(1280),
        "medium" => Some(2048),
        "high" => Some(4096),
        _ => None,
    }
}

fn reasoning_effort_from_budget(budget_tokens: i64) -> String {
    if budget_tokens <= 1280 {
        "low".to_string()
    } else if budget_tokens <= 2048 {
        "medium".to_string()
    } else {
        "high".to_string()
    }
}

fn ensure_anthropic_max_tokens_for_thinking(object: &mut Map<String, Value>, budget_tokens: i64) {
    let minimum = budget_tokens.saturating_add(1);
    let current = object
        .get("max_tokens")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if current < minimum {
        object.insert("max_tokens".to_string(), json!(minimum));
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
