use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use chrono::Utc;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::{
    error::{AppError, AppResult},
    relay::RelayContext,
};

use super::{
    anthropic_stop_reason_to_openai, anthropic_thinking_to_openai_reasoning_effort,
    common::rename_field,
    content_value_to_text, estimate_tokens,
    stream::{finish_bridge_json, finish_bridge_stream, BridgeSseConverter},
};

const OPENAI_CHAT_REQUEST_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "max_tokens",
    "messages",
    "model",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "prompt_cache_key",
    "prompt_cache_options",
    "reasoning",
    "reasoning_effort",
    "response_format",
    "seed",
    "stop",
    "stream",
    "stream_options",
    "temperature",
    "tool_choice",
    "tools",
    "top_logprobs",
    "top_p",
    "user",
    "verbosity",
];

pub(crate) fn messages_to_openai_chat(body: Bytes) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;
    let mut messages = Vec::new();
    let mut tool_names_by_id = HashMap::new();
    if let Some(system) = object.remove("system") {
        messages.push(json!({
            "role": "system",
            "content": anthropic_content_to_openai_content(&system),
        }));
    }
    let anthropic_messages = object
        .remove("messages")
        .ok_or_else(|| AppError::BadRequest("messages is required".to_string()))?;
    for message in anthropic_messages
        .as_array()
        .ok_or_else(|| AppError::BadRequest("messages must be an array".to_string()))?
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("message role is required".to_string()))?;
        let content = message
            .get("content")
            .ok_or_else(|| AppError::BadRequest("message content is required".to_string()))?;
        append_anthropic_message_as_openai_chat(
            &mut messages,
            &mut tool_names_by_id,
            role,
            content,
        );
    }

    let explicit_prompt_cache_key = object
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let prompt_cache_key = explicit_prompt_cache_key.or_else(|| {
        derive_anthropic_cache_key(
            object
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            &messages,
            object.get("tools"),
            object
                .get("metadata")
                .and_then(|metadata| metadata.get("user_id"))
                .and_then(Value::as_str),
        )
    });
    if let Some(prompt_cache_key) = prompt_cache_key {
        object.insert(
            "prompt_cache_key".to_string(),
            Value::String(prompt_cache_key),
        );
    }
    let target_model = object
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let uses_openai_gpt_model = is_openai_gpt_model(target_model);
    let uses_openai_explicit_cache = supports_openai_explicit_prompt_cache(target_model);
    if uses_openai_explicit_cache {
        if replace_anthropic_cache_controls(&mut messages) > 0 {
            object.insert(
                "prompt_cache_options".to_string(),
                json!({ "mode": "explicit" }),
            );
        }
    } else if uses_openai_gpt_model {
        remove_cache_control_from_content(&mut messages);
    }
    object.insert("messages".to_string(), Value::Array(messages));
    rename_field(object, "stop_sequences", "stop");
    let has_tools = if let Some(tools) = object
        .remove("tools")
        .and_then(anthropic_tools_to_openai)
        .map(|mut tools| {
            if uses_openai_gpt_model {
                remove_anthropic_cache_control_from_tools(&mut tools);
            }
            tools
        }) {
        object.insert("tools".to_string(), tools);
        true
    } else {
        false
    };
    let tool_choice = object.remove("tool_choice");
    if has_tools {
        if let Some(tool_choice) =
            tool_choice.and_then(|value| anthropic_tool_choice_to_openai(&value))
        {
            object.insert("tool_choice".to_string(), tool_choice);
        }
    }
    anthropic_thinking_to_openai_reasoning_effort(object);
    retain_openai_chat_request_fields(object);
    if stream {
        object.insert(
            "stream_options".to_string(),
            json!({ "include_usage": true }),
        );
    }
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn retain_openai_chat_request_fields(object: &mut Map<String, Value>) {
    object.retain(|key, _| OPENAI_CHAT_REQUEST_FIELDS.contains(&key.as_str()));
}

fn derive_anthropic_cache_key(
    model: &str,
    messages: &[Value],
    tools: Option<&Value>,
    session_id: Option<&str>,
) -> Option<String> {
    let has_message_marker = messages.iter().any(|message| {
        message
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|content| {
                content
                    .iter()
                    .any(|item| item.get("cache_control").is_some())
            })
    });
    let has_tool_marker = tools
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(|tool| tool.get("cache_control").is_some()));
    if !has_message_marker && !has_tool_marker {
        return None;
    }

    // prompt_cache_key is a stable routing bucket; OpenAI combines it with the
    // actual prefix hash. Keep Claude sessions on one bucket as their prefix grows.
    let session_id = session_id.map(str::trim).filter(|value| !value.is_empty());
    let key_material = if let Some(session_id) = session_id {
        json!({ "version": 2, "model": model, "session": session_id })
    } else {
        let message_prefix = first_cache_controlled_message_prefix(messages);
        let tool_prefix = cache_controlled_tool_prefix(tools, message_prefix.is_some());
        json!({
            "version": 2,
            "model": model,
            "messages": message_prefix,
            "tools": tool_prefix,
        })
    };
    let encoded = serde_json::to_vec(&key_material).ok()?;
    let digest = Sha256::digest(encoded);
    Some(format!("anthropic-cache-{}", &hex::encode(digest)[..32]))
}

fn first_cache_controlled_message_prefix(messages: &[Value]) -> Option<Vec<Value>> {
    let (message_index, content_index) =
        messages
            .iter()
            .enumerate()
            .find_map(|(message_index, message)| {
                message
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|content| {
                        content
                            .iter()
                            .enumerate()
                            .find(|(_, item)| item.get("cache_control").is_some())
                            .map(|(content_index, _)| (message_index, content_index))
                    })
            })?;

    let mut prefix = messages[..=message_index].to_vec();
    let content = prefix[message_index]
        .get_mut("content")
        .and_then(Value::as_array_mut)?;
    content.truncate(content_index + 1);
    remove_cache_control_from_content(&mut prefix);
    Some(prefix)
}

fn cache_controlled_tool_prefix(tools: Option<&Value>, include_all: bool) -> Option<Vec<Value>> {
    let tools = tools.and_then(Value::as_array)?;
    let last_marker = tools
        .iter()
        .enumerate()
        .filter(|(_, tool)| tool.get("cache_control").is_some())
        .map(|(index, _)| index)
        .next_back();
    let end = last_marker.or_else(|| include_all.then_some(tools.len().checked_sub(1)?))?;
    let mut prefix = tools[..=end].to_vec();
    for tool in &mut prefix {
        if let Some(object) = tool.as_object_mut() {
            object.remove("cache_control");
        }
    }
    Some(prefix)
}

fn remove_cache_control_from_content(messages: &mut [Value]) {
    for message in messages {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for item in content {
            if let Some(object) = item.as_object_mut() {
                object.remove("cache_control");
            }
        }
    }
}

fn supports_openai_explicit_prompt_cache(model: &str) -> bool {
    let Some(version) = model.strip_prefix("gpt-") else {
        return false;
    };
    let mut parts =
        version.split(|character: char| !character.is_ascii_digit() && character != '.');
    let mut numbers = parts.next().unwrap_or_default().split('.');
    let major = numbers.next().and_then(|value| value.parse::<u32>().ok());
    let minor = numbers.next().and_then(|value| value.parse::<u32>().ok());
    match (major, minor) {
        (Some(major), _) if major > 5 => true,
        (Some(5), Some(minor)) => minor >= 6,
        _ => false,
    }
}

fn is_openai_gpt_model(model: &str) -> bool {
    model.strip_prefix("gpt-").is_some_and(|version| {
        version
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    })
}

fn replace_anthropic_cache_controls(messages: &mut [Value]) -> usize {
    let mut replaced = 0;
    for message in messages {
        let Some(content) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for item in content {
            let Some(object) = item.as_object_mut() else {
                continue;
            };
            if object.remove("cache_control").is_some() {
                object.insert(
                    "prompt_cache_breakpoint".to_string(),
                    json!({ "mode": "explicit" }),
                );
                replaced += 1;
            }
        }
    }
    replaced
}

fn anthropic_tools_to_openai(value: Value) -> Option<Value> {
    let tools = value.as_array()?;
    let converted = tools
        .iter()
        .filter_map(anthropic_tool_to_openai)
        .collect::<Vec<_>>();
    (!converted.is_empty()).then_some(Value::Array(converted))
}

fn remove_anthropic_cache_control_from_tools(tools: &mut Value) {
    let Some(tools) = tools.as_array_mut() else {
        return;
    };
    for tool in tools {
        if let Some(object) = tool.as_object_mut() {
            object.remove("cache_control");
        }
    }
}

fn anthropic_tool_to_openai(tool: &Value) -> Option<Value> {
    if tool.get("type").and_then(Value::as_str) == Some("function")
        && tool.get("function").and_then(Value::as_object).is_some()
    {
        return Some(tool.clone());
    }

    let name = tool.get("name").and_then(Value::as_str)?;
    if name.is_empty() {
        return None;
    }
    let parameters = tool
        .get("input_schema")
        .filter(|schema| schema.as_object().is_some())
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    let mut function = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("parameters".to_string(), parameters),
    ]);
    if let Some(description) = tool.get("description").and_then(Value::as_str) {
        function.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Some(json!({
        "type": "function",
        "function": Value::Object(function),
    }))
}

fn anthropic_tool_choice_to_openai(value: &Value) -> Option<Value> {
    match value {
        Value::String(choice) if matches!(choice.as_str(), "none" | "auto" | "required") => {
            Some(Value::String(choice.clone()))
        }
        Value::Object(object) => match object.get("type").and_then(Value::as_str)? {
            "auto" => Some(Value::String("auto".to_string())),
            "none" => Some(Value::String("none".to_string())),
            "any" => Some(Value::String("required".to_string())),
            "tool" => object.get("name").and_then(Value::as_str).map(|name| {
                json!({
                    "type": "function",
                    "function": { "name": name },
                })
            }),
            _ => None,
        },
        _ => None,
    }
}

fn append_anthropic_message_as_openai_chat(
    messages: &mut Vec<Value>,
    tool_names_by_id: &mut HashMap<String, String>,
    role: &str,
    content: &Value,
) {
    let Value::Array(items) = content else {
        messages.push(json!({
            "role": role,
            "content": anthropic_content_to_openai_content(content),
        }));
        return;
    };

    let mut content_items = Vec::new();
    let mut tool_calls = Vec::new();

    for item in items {
        let Some(object) = item.as_object() else {
            continue;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("tool_use") => {
                let Some(id) = object.get("id").and_then(Value::as_str) else {
                    continue;
                };
                let Some(name) = object.get("name").and_then(Value::as_str) else {
                    continue;
                };
                tool_names_by_id.insert(id.to_string(), name.to_string());
                let arguments = object
                    .get("input")
                    .map_or_else(|| "{}".to_string(), json_string);
                tool_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": arguments,
                    },
                }));
            }
            Some("tool_result") => {
                let Some(tool_call_id) = object.get("tool_use_id").and_then(Value::as_str) else {
                    continue;
                };
                let mut tool_message = Map::from_iter([
                    ("role".to_string(), Value::String("tool".to_string())),
                    (
                        "tool_call_id".to_string(),
                        Value::String(tool_call_id.to_string()),
                    ),
                    (
                        "content".to_string(),
                        Value::String(anthropic_tool_result_content(item)),
                    ),
                ]);
                if let Some(name) = object
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| tool_names_by_id.get(tool_call_id).map(String::as_str))
                {
                    tool_message.insert("name".to_string(), Value::String(name.to_string()));
                }
                messages.push(Value::Object(tool_message));
            }
            Some("text" | "input_text") => {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    let mut text_item = Map::from_iter([
                        ("type".to_string(), Value::String("text".to_string())),
                        ("text".to_string(), Value::String(text.to_string())),
                    ]);
                    if let Some(cache_control) = object.get("cache_control") {
                        text_item.insert("cache_control".to_string(), cache_control.clone());
                    }
                    content_items.push(Value::Object(text_item));
                }
            }
            Some("image") => {
                if let Some(image) = anthropic_image_to_openai_image_url(item) {
                    content_items.push(image);
                }
            }
            _ => {}
        }
    }

    if !content_items.is_empty() || !tool_calls.is_empty() {
        let mut message = Map::from_iter([("role".to_string(), Value::String(role.to_string()))]);
        if !content_items.is_empty() {
            message.insert(
                "content".to_string(),
                simplify_openai_content_items(content_items),
            );
        } else if role == "assistant" {
            message.insert("content".to_string(), Value::String(String::new()));
        }
        if !tool_calls.is_empty() {
            message.insert("tool_calls".to_string(), Value::Array(tool_calls));
        }
        messages.push(Value::Object(message));
    }
}

fn anthropic_content_to_openai_content(content: &Value) -> Value {
    match content {
        Value::String(text) => Value::String(text.clone()),
        Value::Array(items) => {
            let converted = items
                .iter()
                .filter_map(|item| {
                    let object = item.as_object()?;
                    match object.get("type").and_then(Value::as_str) {
                        Some("text" | "input_text") => {
                            let text = object.get("text").and_then(Value::as_str)?;
                            let mut text_item = Map::from_iter([
                                ("type".to_string(), Value::String("text".to_string())),
                                ("text".to_string(), Value::String(text.to_string())),
                            ]);
                            if let Some(cache_control) = object.get("cache_control") {
                                text_item
                                    .insert("cache_control".to_string(), cache_control.clone());
                            }
                            Some(Value::Object(text_item))
                        }
                        Some("image") => anthropic_image_to_openai_image_url(item),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>();
            simplify_openai_content_items(converted)
        }
        _ => Value::String(String::new()),
    }
}

fn simplify_openai_content_items(items: Vec<Value>) -> Value {
    if items.is_empty() {
        return Value::String(String::new());
    }
    if items.len() == 1 {
        if let Some(object) = items[0].as_object() {
            if object.len() == 2 && object.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(text) = object.get("text").and_then(Value::as_str) {
                    return Value::String(text.to_string());
                }
            }
        }
    }
    Value::Array(items)
}

fn anthropic_image_to_openai_image_url(item: &Value) -> Option<Value> {
    let source = item.get("source")?;
    match source.get("type").and_then(Value::as_str) {
        Some("base64") => {
            let media_type = source.get("media_type").and_then(Value::as_str)?;
            let data = source.get("data").and_then(Value::as_str)?;
            Some(json!({
                "type": "image_url",
                "image_url": { "url": format!("data:{media_type};base64,{data}") },
            }))
        }
        Some("url") => {
            let url = source.get("url").and_then(Value::as_str)?;
            Some(json!({
                "type": "image_url",
                "image_url": { "url": url },
            }))
        }
        _ => None,
    }
}

fn anthropic_tool_result_content(item: &Value) -> String {
    let Some(content) = item.get("content") else {
        return String::new();
    };
    match content {
        Value::String(text) => text.clone(),
        Value::Array(items) => {
            let text = content_value_to_text(content, &["text"], false);
            if !text.is_empty() {
                text
            } else {
                json_string(&Value::Array(items.clone()))
            }
        }
        _ => json_string(content),
    }
}

fn json_string(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) async fn finish_anthropic_as_openai_chat(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    if ctx.streamed {
        return finish_bridge_stream(
            ctx,
            status,
            upstream_response,
            AnthropicSseToOpenAiChat::new,
            "ignored trailing upstream body read error after completed Anthropic fallback stream",
        );
    }
    finish_bridge_json(
        ctx,
        status,
        upstream_response,
        anthropic_response_to_openai_chat,
        "ignored trailing upstream body read error after parsing complete Anthropic fallback response",
    )
    .await
}
pub(super) fn anthropic_response_to_openai_chat(
    body: &[u8],
    fallback_model: &str,
) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let usage = value.get("usage");
    let input_tokens = anthropic_usage_input_tokens(usage);
    let output_tokens = anthropic_usage_output_tokens(usage);
    let (content, reasoning_content, tool_calls) =
        anthropic_content_to_openai_chat_message(value.get("content"));
    let mut message = Map::from_iter([
        ("role".to_string(), Value::String("assistant".to_string())),
        ("content".to_string(), Value::String(content)),
    ]);
    if !reasoning_content.is_empty() {
        message.insert(
            "reasoning_content".to_string(),
            Value::String(reasoning_content),
        );
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_string(), Value::Array(tool_calls));
    }
    let payload = json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("chatcmpl_anthropic_fallback"),
        "object": "chat.completion",
        "created": Utc::now().timestamp(),
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "choices": [{
            "index": 0,
            "message": Value::Object(message),
            "finish_reason": anthropic_stop_reason_to_openai(
                value.get("stop_reason").and_then(Value::as_str).unwrap_or_default()
            ),
        }],
        "usage": openai_usage_from_anthropic_usage(usage, input_tokens, output_tokens),
    });
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

fn anthropic_content_to_text(content: Option<&Value>) -> String {
    content
        .map(|value| content_value_to_text(value, &["text"], false))
        .unwrap_or_default()
}

fn anthropic_content_to_openai_chat_message(
    content: Option<&Value>,
) -> (String, String, Vec<Value>) {
    let Some(Value::Array(items)) = content else {
        return (
            anthropic_content_to_text(content),
            String::new(),
            Vec::new(),
        );
    };
    let mut text = String::new();
    let mut reasoning_content = String::new();
    let mut tool_calls = Vec::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(part) = item.get("text").and_then(Value::as_str) {
                    text.push_str(part);
                }
            }
            Some("thinking") => {
                if let Some(part) = item.get("thinking").and_then(Value::as_str) {
                    reasoning_content.push_str(part);
                }
            }
            Some("tool_use") => {
                if let Some(tool_call) = anthropic_tool_use_to_openai_chat_tool_call(item) {
                    tool_calls.push(tool_call);
                }
            }
            _ => {}
        }
    }
    (text, reasoning_content, tool_calls)
}

fn anthropic_tool_use_to_openai_chat_tool_call(item: &Value) -> Option<Value> {
    let id = item.get("id").and_then(Value::as_str)?;
    let name = item.get("name").and_then(Value::as_str)?;
    let arguments = item
        .get("input")
        .map_or_else(|| "{}".to_string(), Value::to_string);
    Some(json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments,
        },
    }))
}

pub(crate) fn anthropic_usage_input_tokens(usage: Option<&Value>) -> i64 {
    usage
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .saturating_add(
            usage
                .and_then(|usage| usage.get("cache_read_input_tokens"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
        )
}

pub(crate) fn anthropic_usage_output_tokens(usage: Option<&Value>) -> i64 {
    usage
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

pub(crate) fn anthropic_cache_creation_tokens(usage: Option<&Value>) -> i64 {
    usage
        .and_then(|usage| usage.get("cache_creation_input_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            let details = usage.and_then(|usage| usage.get("cache_creation"))?;
            let five_min = details
                .get("ephemeral_5m_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let one_hour = details
                .get("ephemeral_1h_input_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let total = five_min.saturating_add(one_hour);
            (total > 0).then_some(total)
        })
        .unwrap_or(0)
}

pub(super) fn openai_usage_from_anthropic_usage(
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
        "prompt_tokens": input_tokens,
        "completion_tokens": output_tokens,
        "total_tokens": input_tokens.saturating_add(output_tokens),
        "prompt_tokens_details": {
            "cached_tokens": cache_read,
            "cached_creation_tokens": cache_create,
        },
    })
}

pub(super) struct AnthropicSseToOpenAiChat {
    buffer: Vec<u8>,
    model: String,
    message_id: String,
    /// 首次创建时记录的 Unix 时间戳，整个流内所有 chunk 使用同一值以符合 OpenAI 规范。
    created_at: i64,
    started: bool,
    active_tool_calls: HashMap<i64, String>,
    stopped: bool,
    finish_reason: &'static str,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    cache_creation_input_tokens: i64,
}

impl AnthropicSseToOpenAiChat {
    pub(super) fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            message_id: "chatcmpl_anthropic_fallback".to_string(),
            created_at: Utc::now().timestamp(),
            started: false,
            active_tool_calls: HashMap::new(),
            stopped: false,
            finish_reason: "stop",
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Bytes {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let mut line = self.buffer.drain(..=index).collect::<Vec<_>>();
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            self.push_line(&line, &mut out);
        }
        Bytes::from(out)
    }

    fn push_line(&mut self, line: &[u8], out: &mut Vec<u8>) {
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let data = data.trim();
        if data.is_empty() {
            return;
        }
        if data == "[DONE]" {
            self.finish(out);
            return;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return;
        };
        self.observe_anthropic_event(&value, out);
    }

    fn observe_anthropic_event(&mut self, value: &Value, out: &mut Vec<u8>) {
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => self.start_from_message(value, out),
            Some("content_block_start") => self.start_content_block(value, out),
            Some("content_block_delta") => self.push_content_delta(value, out),
            Some("message_delta") => self.observe_message_delta(value),
            Some("message_stop") => self.finish(out),
            _ => {}
        }
    }

    fn start_from_message(&mut self, value: &Value, out: &mut Vec<u8>) {
        if self.started {
            return;
        }
        self.started = true;
        if let Some(message) = value.get("message") {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                self.message_id = id.to_string();
            }
            if let Some(model) = message.get("model").and_then(Value::as_str) {
                self.model = model.to_string();
            }
            self.observe_usage(message);
        }
        self.push_chunk(out, json!({ "role": "assistant" }), None, None);
    }

    fn start_content_block(&mut self, value: &Value, out: &mut Vec<u8>) {
        let Some(block) = value.get("content_block") else {
            return;
        };
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            return;
        }
        self.ensure_started(out);
        let index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
        let id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("toolu_anthropic_fallback")
            .to_string();
        let name = block
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("tool")
            .to_string();
        self.active_tool_calls.insert(index, String::new());
        self.push_chunk(
            out,
            json!({
                "tool_calls": [{
                    "index": index,
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": "",
                    },
                }],
            }),
            None,
            None,
        );
    }

    fn push_content_delta(&mut self, value: &Value, out: &mut Vec<u8>) {
        self.ensure_started(out);
        if let Some(partial_json) = value
            .get("delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("input_json_delta"))
            .and_then(|delta| delta.get("partial_json"))
            .and_then(Value::as_str)
        {
            // active_tool_calls 中的参数累积仅作记录，不做验证（暂无下游消费方）。
            // 真实参数已通过 OpenAI tool_call delta 直接转发给客户端。
            let index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
            let _ = (index, partial_json); // 显式标注意图：index/partial_json 已在上方 push_chunk 中直接输出
            self.push_chunk(
                out,
                json!({
                    "tool_calls": [{
                        "index": index,
                        "type": "function",
                        "function": { "arguments": partial_json },
                    }],
                }),
                None,
                None,
            );
            return;
        }
        if let Some(thinking) = value
            .get("delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("thinking_delta"))
            .and_then(|delta| delta.get("thinking"))
            .and_then(Value::as_str)
            .filter(|thinking| !thinking.is_empty())
        {
            self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(thinking));
            self.push_chunk(out, json!({ "reasoning_content": thinking }), None, None);
            return;
        }
        let Some(text) = value
            .get("delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("text_delta"))
            .and_then(|delta| delta.get("text"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        else {
            return;
        };
        self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(text));
        self.push_chunk(out, json!({ "content": text }), None, None);
    }

    fn observe_message_delta(&mut self, value: &Value) {
        if let Some(reason) = value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            self.finish_reason = anthropic_stop_reason_to_openai(reason);
        }
        self.observe_usage(value);
    }

    fn observe_usage(&mut self, value: &Value) {
        let Some(usage) = value.get("usage") else {
            return;
        };
        self.input_tokens = usage
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(self.input_tokens);
        self.output_tokens = usage
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(self.output_tokens);
        self.cached_input_tokens = usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(self.cached_input_tokens);
        self.cache_creation_input_tokens = usage
            .get("cache_creation_input_tokens")
            .and_then(Value::as_i64)
            .or_else(|| {
                let details = usage.get("cache_creation")?;
                let five_min = details
                    .get("ephemeral_5m_input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let one_hour = details
                    .get("ephemeral_1h_input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                let total = five_min.saturating_add(one_hour);
                (total > 0).then_some(total)
            })
            .unwrap_or(self.cache_creation_input_tokens);
    }

    fn ensure_started(&mut self, out: &mut Vec<u8>) {
        if !self.started {
            self.started = true;
            self.push_chunk(out, json!({ "role": "assistant" }), None, None);
        }
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.stopped {
            return;
        }
        self.ensure_started(out);
        self.stopped = true;
        self.push_chunk(
            out,
            json!({}),
            Some(self.finish_reason),
            Some(json!({
                "prompt_tokens": self
                    .input_tokens
                    .saturating_add(self.cached_input_tokens),
                "completion_tokens": self.output_tokens,
                "total_tokens": self
                    .input_tokens
                    .saturating_add(self.cached_input_tokens)
                    .saturating_add(self.output_tokens),
                "prompt_tokens_details": {
                    "cached_tokens": self.cached_input_tokens,
                    "cached_creation_tokens": self.cache_creation_input_tokens,
                },
            })),
        );
        out.extend_from_slice(b"data: [DONE]\n\n");
    }

    fn push_chunk(
        &self,
        out: &mut Vec<u8>,
        delta: Value,
        finish_reason: Option<&str>,
        usage: Option<Value>,
    ) {
        let payload = json!({
            "id": self.message_id,
            "object": "chat.completion.chunk",
            "created": self.created_at,
            "model": self.model,
            "choices": [{
                "index": 0,
                "delta": delta,
                "finish_reason": finish_reason,
            }],
            "usage": usage,
        });
        out.extend_from_slice(format!("data: {payload}\n\n").as_bytes());
    }
}

impl BridgeSseConverter for AnthropicSseToOpenAiChat {
    fn push(&mut self, chunk: &[u8]) -> Bytes {
        Self::push(self, chunk)
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        Self::finish(self, out);
    }

    fn stopped(&self) -> bool {
        self.stopped
    }
}
