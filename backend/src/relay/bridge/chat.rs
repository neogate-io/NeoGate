use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use crate::{
    error::{AppError, AppResult},
    relay::RelayContext,
};

use super::{
    anthropic_stop_reason_to_openai, anthropic_thinking_to_openai_reasoning_effort,
    content_value_to_text, estimate_tokens, finish_reason_to_anthropic,
    openai_reasoning_to_anthropic_thinking,
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

const OPENAI_TO_ANTHROPIC_DROP_FIELDS: &[&str] = &[
    "frequency_penalty",
    "logit_bias",
    "logprobs",
    "metadata",
    "n",
    "parallel_tool_calls",
    "presence_penalty",
    "response_format",
    "seed",
    "service_tier",
    "stream_options",
    "top_logprobs",
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

    object.insert("messages".to_string(), Value::Array(messages));
    rename_field(object, "stop_sequences", "stop");
    let has_tools = if let Some(tools) = object.remove("tools").and_then(anthropic_tools_to_openai)
    {
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

fn rename_field(object: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
    }
}

fn anthropic_tools_to_openai(value: Value) -> Option<Value> {
    let tools = value.as_array()?;
    let converted = tools
        .iter()
        .filter_map(anthropic_tool_to_openai)
        .collect::<Vec<_>>();
    (!converted.is_empty()).then_some(Value::Array(converted))
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
                    .map(json_string)
                    .unwrap_or_else(|| "{}".to_string());
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

pub(crate) fn openai_chat_to_anthropic_messages(body: Bytes) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;

    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;
    let openai_messages = object
        .remove("messages")
        .ok_or_else(|| AppError::BadRequest("messages is required".to_string()))?;
    let mut system = Vec::new();
    let mut messages = Vec::new();

    for message in openai_messages
        .as_array()
        .ok_or_else(|| AppError::BadRequest("messages must be an array".to_string()))?
    {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("message role is required".to_string()))?;
        match role {
            "system" | "developer" => {
                if let Some(content) = openai_chat_content_to_anthropic_content(
                    message.get("content").unwrap_or(&Value::Null),
                ) {
                    push_system_content(&mut system, content);
                }
            }
            "assistant" => {
                if let Some(message) = openai_chat_assistant_to_anthropic(message) {
                    messages.push(message);
                }
            }
            "tool" => {
                if let Some(message) = openai_chat_tool_to_anthropic(message) {
                    messages.push(message);
                }
            }
            _ => {
                if let Some(content) = openai_chat_content_to_anthropic_content(
                    message.get("content").unwrap_or(&Value::Null),
                ) {
                    messages.push(json!({ "role": "user", "content": content }));
                }
            }
        }
    }

    object.insert("messages".to_string(), Value::Array(messages));
    if !system.is_empty() {
        object.insert("system".to_string(), Value::Array(system));
    }
    rename_field(object, "max_completion_tokens", "max_tokens");
    rename_field(object, "stop", "stop_sequences");
    if let Some(tools) = object
        .remove("tools")
        .and_then(openai_chat_tools_to_anthropic)
    {
        object.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = object
        .remove("tool_choice")
        .and_then(|value| openai_chat_tool_choice_to_anthropic(&value))
    {
        object.insert("tool_choice".to_string(), tool_choice);
    }
    openai_reasoning_to_anthropic_thinking(object);
    for &key in OPENAI_TO_ANTHROPIC_DROP_FIELDS {
        object.remove(key);
    }

    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn push_system_content(system: &mut Vec<Value>, content: Value) {
    match content {
        Value::String(text) if !text.is_empty() => {
            system.push(json!({ "type": "text", "text": text }));
        }
        Value::Array(items) => {
            for item in items {
                system.push(item);
            }
        }
        _ => {}
    }
}

fn openai_chat_assistant_to_anthropic(message: &Value) -> Option<Value> {
    let mut content = Vec::new();
    if let Some(converted) =
        openai_chat_content_to_anthropic_content(message.get("content").unwrap_or(&Value::Null))
    {
        append_anthropic_content_items(&mut content, converted);
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            if let Some(tool_use) = openai_chat_tool_call_to_anthropic(tool_call) {
                content.push(tool_use);
            }
        }
    }
    (!content.is_empty()).then(|| json!({ "role": "assistant", "content": content }))
}

fn openai_chat_tool_to_anthropic(message: &Value) -> Option<Value> {
    let id = message.get("tool_call_id").and_then(Value::as_str)?;
    let content = message
        .get("content")
        .map(openai_chat_content_to_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "...".to_string());
    Some(json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": id,
            "content": content,
        }],
    }))
}

fn openai_chat_tool_call_to_anthropic(tool_call: &Value) -> Option<Value> {
    let function = tool_call.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?;
    let id = tool_call
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("call_openai_fallback");
    let input = function
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
        .unwrap_or_else(|| json!({}));
    Some(json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input,
    }))
}

fn append_anthropic_content_items(items: &mut Vec<Value>, content: Value) {
    match content {
        Value::String(text) if !text.is_empty() => {
            items.push(json!({ "type": "text", "text": text }));
        }
        Value::Array(content_items) => {
            items.extend(content_items);
        }
        _ => {}
    }
}

fn openai_chat_content_to_anthropic_content(content: &Value) -> Option<Value> {
    match content {
        Value::String(text) if !text.is_empty() => Some(Value::String(text.clone())),
        Value::Array(items) => {
            let converted = items
                .iter()
                .filter_map(openai_chat_content_item_to_anthropic)
                .collect::<Vec<_>>();
            (!converted.is_empty()).then_some(Value::Array(converted))
        }
        _ => None,
    }
}

fn openai_chat_content_item_to_anthropic(item: &Value) -> Option<Value> {
    let object = item.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("text" | "input_text") => {
            let text = object
                .get("text")
                .or_else(|| object.get("input_text"))
                .and_then(Value::as_str)?;
            let mut converted = Map::from_iter([
                ("type".to_string(), Value::String("text".to_string())),
                ("text".to_string(), Value::String(text.to_string())),
            ]);
            if let Some(cache_control) = object.get("cache_control") {
                converted.insert("cache_control".to_string(), cache_control.clone());
            }
            Some(Value::Object(converted))
        }
        Some("image_url") => openai_chat_image_to_anthropic_image(item),
        _ => None,
    }
}

fn openai_chat_image_to_anthropic_image(item: &Value) -> Option<Value> {
    let image_url = item.get("image_url")?;
    let url = match image_url {
        Value::String(url) => url.as_str(),
        Value::Object(object) => object.get("url").and_then(Value::as_str)?,
        _ => return None,
    };
    let data_url = url.strip_prefix("data:")?;
    let (media_type, data) = data_url.split_once(";base64,")?;
    Some(json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": media_type,
            "data": data,
        },
    }))
}

fn openai_chat_content_to_text(value: &Value) -> String {
    content_value_to_text(value, &["text", "input_text", "output_text"], true)
}

fn openai_chat_tools_to_anthropic(value: Value) -> Option<Value> {
    let tools = value.as_array()?;
    let converted = tools
        .iter()
        .filter_map(openai_chat_tool_schema_to_anthropic)
        .collect::<Vec<_>>();
    (!converted.is_empty()).then_some(Value::Array(converted))
}

fn openai_chat_tool_schema_to_anthropic(tool: &Value) -> Option<Value> {
    let function = tool.get("function").or(Some(tool))?;
    let name = function.get("name").and_then(Value::as_str)?;
    if name.is_empty() {
        return None;
    }
    let input_schema = function
        .get("parameters")
        .filter(|schema| schema.as_object().is_some())
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    let mut converted = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("input_schema".to_string(), input_schema),
    ]);
    if let Some(description) = function.get("description").and_then(Value::as_str) {
        converted.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Some(Value::Object(converted))
}

fn openai_chat_tool_choice_to_anthropic(value: &Value) -> Option<Value> {
    match value {
        Value::String(choice) => match choice.as_str() {
            "auto" => Some(json!({ "type": "auto" })),
            "none" => Some(json!({ "type": "none" })),
            "required" => Some(json!({ "type": "any" })),
            _ => None,
        },
        Value::Object(object) => match object.get("type").and_then(Value::as_str)? {
            "auto" => Some(json!({ "type": "auto" })),
            "none" => Some(json!({ "type": "none" })),
            "required" => Some(json!({ "type": "any" })),
            "function" => object
                .get("function")
                .and_then(|function| function.get("name"))
                .or_else(|| object.get("name"))
                .and_then(Value::as_str)
                .map(|name| json!({ "type": "tool", "name": name })),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) async fn finish_chat_as_anthropic(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    if ctx.streamed {
        return finish_bridge_stream(
            ctx,
            status,
            upstream_response,
            OpenAiChatSseToAnthropic::new,
            "ignored trailing upstream body read error after completed OpenAI fallback stream",
        );
    }
    finish_bridge_json(
        ctx,
        status,
        upstream_response,
        chat_response_to_anthropic,
        "ignored trailing upstream body read error after parsing complete OpenAI fallback response",
    )
    .await
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

fn chat_response_to_anthropic(body: &[u8], fallback_model: &str) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    let stop_reason = finish_reason_to_anthropic(finish_reason.unwrap_or_default());
    let content = choice
        .and_then(|choice| choice.get("message"))
        .map(openai_chat_message_to_anthropic_content)
        .filter(|content| !content.is_empty())
        .unwrap_or_else(|| vec![json!({ "type": "text", "text": "" })]);
    let usage = value.get("usage");
    let input_tokens = usage
        .and_then(|usage| usage.get("prompt_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = usage
        .and_then(|usage| usage.get("completion_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let payload = json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("msg_openai_fallback"),
        "type": "message",
        "role": "assistant",
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "content": content,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": anthropic_usage_from_openai_usage(usage, input_tokens, output_tokens),
    });
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

fn openai_chat_message_to_anthropic_content(message: &Value) -> Vec<Value> {
    let mut content = Vec::new();
    if let Some(reasoning) = message
        .get("reasoning_content")
        .or_else(|| message.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|reasoning| !reasoning.is_empty())
    {
        content.push(json!({ "type": "thinking", "thinking": reasoning }));
    }
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(json!({ "type": "text", "text": text }));
        }
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let function = tool_call.get("function");
            let input = function
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
                .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
                .unwrap_or_else(|| json!({}));
            content.push(json!({
                "type": "tool_use",
                "id": tool_call
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("call_openai_fallback"),
                "name": function
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                "input": input,
            }));
        }
    }
    content
}

fn anthropic_usage_from_openai_usage(
    usage: Option<&Value>,
    input_tokens: i64,
    output_tokens: i64,
) -> Value {
    let mut object = Map::from_iter([
        (
            "input_tokens".to_string(),
            Value::Number(input_tokens.into()),
        ),
        (
            "output_tokens".to_string(),
            Value::Number(output_tokens.into()),
        ),
    ]);
    let input_details = usage
        .and_then(|usage| usage.get("prompt_tokens_details"))
        .or_else(|| usage.and_then(|usage| usage.get("input_tokens_details")));
    if let Some(cached_tokens) = input_details
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_i64)
    {
        object.insert(
            "cache_read_input_tokens".to_string(),
            Value::Number(cached_tokens.into()),
        );
    }
    if let Some(cache_creation_tokens) = input_details
        .and_then(|details| details.get("cached_creation_tokens"))
        .and_then(Value::as_i64)
        .or_else(|| {
            usage
                .and_then(|usage| usage.get("cache_creation_input_tokens"))
                .and_then(Value::as_i64)
        })
    {
        object.insert(
            "cache_creation_input_tokens".to_string(),
            Value::Number(cache_creation_tokens.into()),
        );
    }
    Value::Object(object)
}

fn anthropic_response_to_openai_chat(body: &[u8], fallback_model: &str) -> AppResult<Bytes> {
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
        "created": 0,
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
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_string());
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
        .saturating_add(anthropic_cache_creation_tokens(usage))
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

struct OpenAiChatSseToAnthropic {
    buffer: Vec<u8>,
    model: String,
    message_id: String,
    started: bool,
    content_started: bool,
    open_block: Option<AnthropicOpenBlock>,
    tool_block_indices: HashMap<i64, usize>,
    next_block_index: usize,
    stopped: bool,
    stop_reason: &'static str,
    input_tokens: i64,
    output_tokens: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnthropicOpenBlock {
    Thinking(usize),
    Text(usize),
    Tool,
}

impl OpenAiChatSseToAnthropic {
    fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            message_id: "msg_openai_fallback".to_string(),
            started: false,
            content_started: false,
            open_block: None,
            tool_block_indices: HashMap::new(),
            next_block_index: 0,
            stopped: false,
            stop_reason: "end_turn",
            input_tokens: 0,
            output_tokens: 0,
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
        self.observe_openai_chunk(&value, out);
    }

    fn observe_openai_chunk(&mut self, value: &Value, out: &mut Vec<u8>) {
        self.ensure_message_start(value, out);

        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            self.observe_openai_usage(value);
            return;
        };
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.stop_reason = finish_reason_to_anthropic(reason);
        }
        let delta = choice.get("delta");
        if let Some(tool_calls) = delta
            .and_then(|delta| delta.get("tool_calls"))
            .and_then(Value::as_array)
            .filter(|tool_calls| !tool_calls.is_empty())
        {
            self.observe_openai_tool_calls(tool_calls, out);
        } else if let Some(reasoning) = delta
            .and_then(|delta| {
                delta
                    .get("reasoning_content")
                    .or_else(|| delta.get("reasoning"))
            })
            .and_then(Value::as_str)
            .filter(|reasoning| !reasoning.is_empty())
        {
            self.ensure_thinking_block(out);
            self.output_tokens = self
                .output_tokens
                .saturating_add(estimate_tokens(reasoning));
            push_anthropic_sse(
                out,
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": self.current_thinking_index(),
                    "delta": { "type": "thinking_delta", "thinking": reasoning },
                }),
            );
        } else if let Some(text) = delta
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            self.ensure_text_block(out);
            self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(text));
            push_anthropic_sse(
                out,
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": self.current_text_index(),
                    "delta": { "type": "text_delta", "text": text },
                }),
            );
        }
        self.observe_openai_usage(value);
    }

    fn observe_openai_tool_calls(&mut self, tool_calls: &[Value], out: &mut Vec<u8>) {
        self.close_text_block(out);
        self.open_block = Some(AnthropicOpenBlock::Tool);
        for (position, tool_call) in tool_calls.iter().enumerate() {
            let stream_index = tool_call
                .get("index")
                .and_then(Value::as_i64)
                .unwrap_or(position as i64);
            let block_index = if let Some(block_index) = self.tool_block_indices.get(&stream_index)
            {
                *block_index
            } else {
                let block_index = self.next_block_index;
                self.next_block_index += 1;
                self.tool_block_indices.insert(stream_index, block_index);
                let function = tool_call.get("function");
                push_anthropic_sse(
                    out,
                    "content_block_start",
                    json!({
                        "type": "content_block_start",
                        "index": block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": tool_call
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("call_openai_fallback"),
                            "name": function
                                .and_then(|function| function.get("name"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown"),
                            "input": {},
                        },
                    }),
                );
                block_index
            };

            if let Some(arguments) = tool_call
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
                .filter(|arguments| !arguments.is_empty())
            {
                push_anthropic_sse(
                    out,
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": block_index,
                        "delta": { "type": "input_json_delta", "partial_json": arguments },
                    }),
                );
            }
        }
    }

    fn observe_openai_usage(&mut self, value: &Value) {
        if let Some(usage) = value.get("usage") {
            self.input_tokens = usage
                .get("prompt_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(self.input_tokens);
            self.output_tokens = usage
                .get("completion_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(self.output_tokens);
        }
    }

    fn ensure_message_start(&mut self, value: &Value, out: &mut Vec<u8>) {
        if self.started {
            return;
        }
        self.started = true;
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            self.message_id = id.to_string();
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        push_anthropic_sse(
            out,
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.message_id,
                    "type": "message",
                    "role": "assistant",
                    "model": self.model,
                    "content": [],
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 },
                }
            }),
        );
    }

    fn ensure_text_block(&mut self, out: &mut Vec<u8>) {
        if matches!(self.open_block, Some(AnthropicOpenBlock::Text(_))) {
            return;
        }
        self.close_thinking_block(out);
        self.close_tool_blocks(out);
        self.content_started = true;
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.open_block = Some(AnthropicOpenBlock::Text(index));
        push_anthropic_sse(
            out,
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "text", "text": "" },
            }),
        );
    }

    fn ensure_thinking_block(&mut self, out: &mut Vec<u8>) {
        if matches!(self.open_block, Some(AnthropicOpenBlock::Thinking(_))) {
            return;
        }
        self.close_text_block(out);
        self.close_tool_blocks(out);
        self.content_started = true;
        let index = self.next_block_index;
        self.next_block_index += 1;
        self.open_block = Some(AnthropicOpenBlock::Thinking(index));
        push_anthropic_sse(
            out,
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": { "type": "thinking", "thinking": "" },
            }),
        );
    }

    fn current_text_index(&self) -> usize {
        match self.open_block {
            Some(AnthropicOpenBlock::Text(index)) => index,
            _ => 0,
        }
    }

    fn current_thinking_index(&self) -> usize {
        match self.open_block {
            Some(AnthropicOpenBlock::Thinking(index)) => index,
            _ => 0,
        }
    }

    fn close_text_block(&mut self, out: &mut Vec<u8>) {
        if let Some(AnthropicOpenBlock::Text(index)) = self.open_block {
            push_anthropic_sse(
                out,
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            );
            self.open_block = None;
        }
    }

    fn close_thinking_block(&mut self, out: &mut Vec<u8>) {
        if let Some(AnthropicOpenBlock::Thinking(index)) = self.open_block {
            push_anthropic_sse(
                out,
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            );
            self.open_block = None;
        }
    }

    fn close_tool_blocks(&mut self, out: &mut Vec<u8>) {
        if self.tool_block_indices.is_empty() {
            return;
        }
        let mut indices = self
            .tool_block_indices
            .values()
            .copied()
            .collect::<Vec<_>>();
        indices.sort_unstable();
        for index in indices {
            push_anthropic_sse(
                out,
                "content_block_stop",
                json!({ "type": "content_block_stop", "index": index }),
            );
        }
        self.tool_block_indices.clear();
        if self.open_block == Some(AnthropicOpenBlock::Tool) {
            self.open_block = None;
        }
    }

    fn close_open_blocks(&mut self, out: &mut Vec<u8>) {
        self.close_thinking_block(out);
        self.close_text_block(out);
        self.close_tool_blocks(out);
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if !self.started {
            self.ensure_message_start(&json!({}), out);
        }
        if self.open_block.is_none() && self.next_block_index == 0 {
            self.ensure_text_block(out);
        }
        self.close_open_blocks(out);
        push_anthropic_sse(
            out,
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": { "stop_reason": self.stop_reason, "stop_sequence": null },
                "usage": {
                    "input_tokens": self.input_tokens,
                    "output_tokens": self.output_tokens,
                },
            }),
        );
        push_anthropic_sse(out, "message_stop", json!({ "type": "message_stop" }));
    }
}

impl BridgeSseConverter for OpenAiChatSseToAnthropic {
    fn push(&mut self, chunk: &[u8]) -> Bytes {
        Self::push(self, chunk)
    }

    fn stopped(&self) -> bool {
        self.stopped
    }
}

struct AnthropicSseToOpenAiChat {
    buffer: Vec<u8>,
    model: String,
    message_id: String,
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
    fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            message_id: "chatcmpl_anthropic_fallback".to_string(),
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
            let index = value.get("index").and_then(Value::as_i64).unwrap_or(0);
            if let Some(arguments) = self.active_tool_calls.get_mut(&index) {
                arguments.push_str(partial_json);
            }
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
                    .saturating_add(self.cached_input_tokens)
                    .saturating_add(self.cache_creation_input_tokens),
                "completion_tokens": self.output_tokens,
                "total_tokens": self
                    .input_tokens
                    .saturating_add(self.cached_input_tokens)
                    .saturating_add(self.cache_creation_input_tokens)
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
            "created": 0,
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

    fn stopped(&self) -> bool {
        self.stopped
    }
}

fn push_anthropic_sse(out: &mut Vec<u8>, event: &str, data: Value) {
    out.extend_from_slice(format!("event: {event}\n").as_bytes());
    out.extend_from_slice(format!("data: {data}\n\n").as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_anthropic_message_request_to_openai_chat() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","system":"Be terse.","messages":[{"role":"user","content":[{"type":"text","text":"Reply OK"}]}],"max_tokens":16}"#,
        );

        let converted = messages_to_openai_chat(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "GLM-5.1");
        assert_eq!(value["max_tokens"], 16);
        assert_eq!(value["messages"][0]["role"], "system");
        assert_eq!(value["messages"][0]["content"], "Be terse.");
        assert_eq!(value["messages"][1]["role"], "user");
        assert_eq!(value["messages"][1]["content"], "Reply OK");
        assert!(value.get("system").is_none());
    }

    #[test]
    fn converts_anthropic_tools_to_openai_functions_and_filters_invalid_items() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[{"role":"user","content":"use tool"}],"max_tokens":16,"tools":[{"name":"lookup","description":"Lookup data","input_schema":{"type":"object","properties":{"q":{"type":"string"}}}},"bad",{"type":"text"},{"type":"function","function":{"name":"already_openai","parameters":{"type":"object"}}}],"tool_choice":{"type":"tool","name":"lookup"},"stop_sequences":["END"],"thinking":{"type":"enabled","budget_tokens":128},"metadata":{"user_id":"u1"}}"#,
        );

        let converted = messages_to_openai_chat(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["tools"].as_array().unwrap().len(), 2);
        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "lookup");
        assert_eq!(value["tools"][0]["function"]["description"], "Lookup data");
        assert_eq!(
            value["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
        assert_eq!(value["tools"][1]["function"]["name"], "already_openai");
        assert_eq!(value["tool_choice"]["type"], "function");
        assert_eq!(value["tool_choice"]["function"]["name"], "lookup");
        assert_eq!(value["stop"][0], "END");
        assert!(value.get("stop_sequences").is_none());
        assert!(value.get("thinking").is_none());
        assert!(value.get("metadata").is_none());
    }

    #[test]
    fn preserves_cache_control_and_tool_messages_when_converting_anthropic_to_openai() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","system":[{"type":"text","text":"Be terse.","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":[{"type":"text","text":"Use lookup","cache_control":{"type":"ephemeral"}}]},{"role":"assistant","content":[{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"q":"NeoGate"}}]},{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"found"}]}],"max_tokens":16}"#,
        );

        let converted = messages_to_openai_chat(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(
            value["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(
            value["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(value["messages"][2]["role"], "assistant");
        assert_eq!(value["messages"][2]["tool_calls"][0]["id"], "toolu_1");
        assert_eq!(
            value["messages"][2]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(
            value["messages"][2]["tool_calls"][0]["function"]["arguments"],
            r#"{"q":"NeoGate"}"#
        );
        assert_eq!(value["messages"][3]["role"], "tool");
        assert_eq!(value["messages"][3]["tool_call_id"], "toolu_1");
        assert_eq!(value["messages"][3]["name"], "lookup");
        assert_eq!(value["messages"][3]["content"], "found");
    }

    #[test]
    fn converts_openai_chat_response_to_anthropic_message() {
        let body = br#"{"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":1}}"#;

        let converted = chat_response_to_anthropic(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["id"], "chatcmpl-1");
        assert_eq!(value["type"], "message");
        assert_eq!(value["role"], "assistant");
        assert_eq!(value["model"], "GLM-5.1");
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][0]["text"], "OK");
        assert_eq!(value["usage"]["input_tokens"], 8);
        assert_eq!(value["usage"]["output_tokens"], 1);
    }

    #[test]
    fn converts_openai_reasoning_content_to_anthropic_thinking() {
        let body = br#"{"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"message":{"role":"assistant","reasoning_content":"Thinking.","content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":3}}"#;

        let converted = chat_response_to_anthropic(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["content"][0]["type"], "thinking");
        assert_eq!(value["content"][0]["thinking"], "Thinking.");
        assert_eq!(value["content"][1]["type"], "text");
        assert_eq!(value["content"][1]["text"], "OK");
    }

    #[test]
    fn converts_openai_tool_call_response_to_anthropic_tool_use() {
        let body = br#"{"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"message":{"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"NeoGate\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":6}}}"#;

        let converted = chat_response_to_anthropic(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["content"][0]["type"], "tool_use");
        assert_eq!(value["content"][0]["id"], "call_1");
        assert_eq!(value["content"][0]["name"], "lookup");
        assert_eq!(value["content"][0]["input"]["q"], "NeoGate");
        assert_eq!(value["stop_reason"], "tool_use");
        assert_eq!(value["usage"]["cache_read_input_tokens"], 6);
    }

    #[test]
    fn converts_openai_chat_request_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","messages":[{"role":"system","content":"Be terse."},{"role":"developer","content":"No markdown."},{"role":"user","content":"Reply OK"}],"max_completion_tokens":16,"stop":["END"],"stream_options":{"include_usage":true}}"#,
        );

        let converted = openai_chat_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["max_tokens"], 16);
        assert_eq!(value["system"][0]["text"], "Be terse.");
        assert_eq!(value["system"][1]["text"], "No markdown.");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "Reply OK");
        assert_eq!(value["stop_sequences"][0], "END");
        assert!(value.get("max_completion_tokens").is_none());
        assert!(value.get("stream_options").is_none());
    }

    #[test]
    fn converts_openai_chat_reasoning_to_anthropic_thinking() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","messages":[{"role":"user","content":"Reply OK"}],"max_completion_tokens":16,"reasoning_effort":"high","top_p":0.9}"#,
        );

        let converted = openai_chat_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["thinking"]["budget_tokens"], 4096);
        assert_eq!(value["max_tokens"], 4097);
        assert_eq!(value["temperature"], 1.0);
        assert!(value.get("reasoning_effort").is_none());
        assert!(value.get("top_p").is_none());
    }

    #[test]
    fn converts_openai_chat_reasoning_max_tokens_to_anthropic_thinking() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","messages":[{"role":"user","content":"Reply OK"}],"max_completion_tokens":4096,"reasoning":{"enabled":true,"max_tokens":2048}}"#,
        );

        let converted = openai_chat_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["thinking"]["budget_tokens"], 2048);
        assert_eq!(value["max_tokens"], 4096);
        assert!(value.get("reasoning").is_none());
    }

    #[test]
    fn converts_openai_chat_tools_and_tool_history_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","messages":[{"role":"user","content":[{"type":"text","text":"Lookup weather","cache_control":{"type":"ephemeral"}}]},{"role":"assistant","content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"city\":\"Shanghai\"}"}}]},{"role":"tool","tool_call_id":"call_1","content":"Sunny"}],"tools":[{"type":"function","function":{"name":"lookup","description":"Lookup weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}}],"tool_choice":{"type":"function","function":{"name":"lookup"}},"parallel_tool_calls":true,"max_completion_tokens":16}"#,
        );

        let converted = openai_chat_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["tools"][0]["name"], "lookup");
        assert_eq!(value["tools"][0]["description"], "Lookup weather");
        assert_eq!(
            value["tools"][0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
        assert_eq!(value["tool_choice"]["type"], "tool");
        assert_eq!(value["tool_choice"]["name"], "lookup");
        assert_eq!(
            value["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(value["messages"][1]["role"], "assistant");
        assert_eq!(value["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(value["messages"][1]["content"][0]["id"], "call_1");
        assert_eq!(
            value["messages"][1]["content"][0]["input"]["city"],
            "Shanghai"
        );
        assert_eq!(value["messages"][2]["role"], "user");
        assert_eq!(value["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(value["messages"][2]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(value["messages"][2]["content"][0]["content"], "Sunny");
        assert!(value.get("parallel_tool_calls").is_none());
    }

    #[test]
    fn converts_anthropic_message_response_to_openai_chat() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":1,"cache_read_input_tokens":6,"cache_creation":{"ephemeral_5m_input_tokens":2}}}"#;

        let converted = anthropic_response_to_openai_chat(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["id"], "msg-1");
        assert_eq!(value["object"], "chat.completion");
        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["choices"][0]["message"]["role"], "assistant");
        assert_eq!(value["choices"][0]["message"]["content"], "OK");
        assert_eq!(value["choices"][0]["finish_reason"], "stop");
        assert_eq!(value["usage"]["prompt_tokens"], 16);
        assert_eq!(value["usage"]["completion_tokens"], 1);
        assert_eq!(value["usage"]["total_tokens"], 17);
        assert_eq!(value["usage"]["prompt_tokens_details"]["cached_tokens"], 6);
        assert_eq!(
            value["usage"]["prompt_tokens_details"]["cached_creation_tokens"],
            2
        );
    }

    #[test]
    fn converts_anthropic_thinking_response_to_openai_reasoning_content() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"thinking","thinking":"Thinking."},{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":3}}"#;

        let converted = anthropic_response_to_openai_chat(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(
            value["choices"][0]["message"]["reasoning_content"],
            "Thinking."
        );
        assert_eq!(value["choices"][0]["message"]["content"], "OK");
    }

    #[test]
    fn converts_anthropic_tool_use_response_to_openai_chat_tool_calls() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"text","text":"Checking."},{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"city":"Shanghai"}}],"stop_reason":"tool_use","usage":{"input_tokens":8,"output_tokens":3}}"#;

        let converted = anthropic_response_to_openai_chat(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["choices"][0]["message"]["content"], "Checking.");
        assert_eq!(
            value["choices"][0]["message"]["tool_calls"][0]["id"],
            "toolu_1"
        );
        assert_eq!(
            value["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(
            value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            r#"{"city":"Shanghai"}"#
        );
        assert_eq!(value["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn converts_openai_chat_stream_to_anthropic_events() {
        let mut converter = OpenAiChatSseToAnthropic::new("GLM-5.1".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"delta":{"content":"O"},"finish_reason":null}]}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"choices":[{"delta":{"content":"K"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":1}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(b"data: [DONE]\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: message_start"));
        assert!(text.contains("event: content_block_start"));
        assert!(text.contains(r#""text":"O""#));
        assert!(text.contains(r#""text":"K""#));
        assert!(text.contains("event: message_delta"));
        assert!(text.contains(r#""input_tokens":8"#));
        assert!(text.contains(r#""output_tokens":1"#));
        assert!(text.contains("event: message_stop"));
    }

    #[test]
    fn converts_openai_reasoning_stream_to_anthropic_thinking_events() {
        let mut converter = OpenAiChatSseToAnthropic::new("GLM-5.1".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"delta":{"reasoning_content":"Think"},"finish_reason":null}]}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"choices":[{"delta":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":3}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(b"data: [DONE]\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains(r#""type":"thinking""#));
        assert!(text.contains(r#""type":"thinking_delta""#));
        assert!(text.contains(r#""thinking":"Think""#));
        assert!(text.contains(r#""type":"text_delta""#));
        assert!(text.contains(r#""text":"OK""#));
    }

    #[test]
    fn converts_openai_tool_call_stream_to_anthropic_tool_use_events() {
        let mut converter = OpenAiChatSseToAnthropic::new("GLM-5.1".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"q\""}}]},"finish_reason":null}]}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":\"NeoGate\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":1}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(b"data: [DONE]\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: content_block_start"));
        assert!(text.contains(r#""type":"tool_use""#));
        assert!(text.contains(r#""id":"call_1""#));
        assert!(text.contains(r#""name":"lookup""#));
        assert!(text.contains(r#""type":"input_json_delta""#));
        assert!(text.contains(r#""partial_json":"{\"q\"""#));
        assert!(text.contains(r#""partial_json":":\"NeoGate\"}""#));
        assert!(text.contains(r#""stop_reason":"tool_use""#));
        assert!(text.contains("event: message_stop"));
    }

    #[test]
    fn converts_anthropic_stream_to_openai_chat_chunks() {
        let mut converter = AnthropicSseToOpenAiChat::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0,"cache_read_input_tokens":6}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"O"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"K"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1,"cache_creation_input_tokens":2}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains(r#""object":"chat.completion.chunk""#));
        assert!(text.contains(r#""role":"assistant""#));
        assert!(text.contains(r#""content":"O""#));
        assert!(text.contains(r#""content":"K""#));
        assert!(text.contains(r#""finish_reason":"stop""#));
        assert!(text.contains(r#""prompt_tokens":16"#));
        assert!(text.contains(r#""completion_tokens":1"#));
        assert!(text.contains(r#""cached_tokens":6"#));
        assert!(text.contains(r#""cached_creation_tokens":2"#));
        assert!(text.contains("data: [DONE]"));
    }

    #[test]
    fn converts_anthropic_stream_thinking_to_openai_reasoning_chunks() {
        let mut converter = AnthropicSseToOpenAiChat::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Think"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains(r#""reasoning_content":"Think""#));
        assert!(text.contains("data: [DONE]"));
    }

    #[test]
    fn converts_anthropic_stream_tool_use_to_openai_chat_tool_calls() {
        let mut converter = AnthropicSseToOpenAiChat::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Shanghai\"}"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains(r#""tool_calls""#));
        assert!(text.contains(r#""id":"toolu_1""#));
        assert!(text.contains(r#""name":"lookup""#));
        assert!(text.contains(r#""arguments":"{\"city\":"#));
        assert!(text.contains(r#""arguments":"\"Shanghai\"}""#));
        assert!(text.contains(r#""finish_reason":"tool_calls""#));
        assert!(text.contains("data: [DONE]"));
    }
}
