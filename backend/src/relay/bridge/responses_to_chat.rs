use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};

use super::common::{openai_response_content_to_text, remove_fields, rename_field};

pub(crate) fn openai_response_to_openai_chat(body: Bytes) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;
    if object
        .get("background")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(AppError::BadRequest(
            "responses chat fallback does not support background=true".to_string(),
        ));
    }
    if object.get("previous_response_id").is_some() {
        return Err(AppError::BadRequest(
            "responses chat fallback does not support previous_response_id".to_string(),
        ));
    }
    let input = object
        .remove("input")
        .ok_or_else(|| AppError::BadRequest("input is required".to_string()))?;
    let (mut system, mut messages) = openai_response_input_to_chat_messages(&input)?;
    // Codex 把系统指令放在顶层 instructions 字段（不在 input 里），需回填成 system 消息，
    // 否则上游 chat 接口收不到系统提示，多轮后行为漂移。
    if let Some(instructions) = object
        .remove("instructions")
        .and_then(|value| {
            value
                .as_str()
                .map(str::trim)
                .filter(|trimmed| !trimmed.is_empty())
                .map(str::to_string)
        })
    {
        system.push(instructions);
    }
    if !system.is_empty() {
        messages.insert(
            0,
            json!({
                "role": "system",
                "content": system.join("\n"),
            }),
        );
    }
    object.insert("messages".to_string(), Value::Array(messages));
    rename_field(object, "max_output_tokens", "max_tokens");
    if let Some(tools) = object
        .remove("tools")
        .and_then(openai_response_tools_to_openai_chat)
    {
        object.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = object
        .remove("tool_choice")
        .and_then(|value| openai_response_tool_choice_to_openai_chat(&value))
    {
        object.insert("tool_choice".to_string(), tool_choice);
    }
    remove_fields(object, &["include", "metadata", "store", "truncation"]);
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}
fn openai_response_input_to_chat_messages(input: &Value) -> AppResult<(Vec<String>, Vec<Value>)> {
    match input {
        Value::String(text) => Ok((Vec::new(), vec![json!({ "role": "user", "content": text })])),
        Value::Array(items) => {
            let mut system = Vec::new();
            let mut messages: Vec<Value> = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    AppError::BadRequest("input items must be JSON objects".to_string())
                })?;
                match object.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        let Some(name) = object.get("name").and_then(Value::as_str) else {
                            continue;
                        };
                        let id = object
                            .get("call_id")
                            .or_else(|| object.get("id"))
                            .and_then(Value::as_str);
                        let arguments = object
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        let tool_call = json!({
                            "id": id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments,
                            },
                        });
                        // 同一 assistant 回合的并行 function_call 必须合并进同一条消息的
                        // tool_calls 数组，否则上游 chat 接口会收到一连串空 assistant 消息。
                        let append_to_prev = matches!(
                            messages.last(),
                            Some(prev) if prev.get("role").and_then(Value::as_str)
                                == Some("assistant")
                                && prev.get("tool_calls").is_some()
                        );
                        if append_to_prev {
                            if let Some(prev) = messages.last_mut() {
                                if let Some(tool_calls) =
                                    prev.get_mut("tool_calls").and_then(Value::as_array_mut)
                                {
                                    tool_calls.push(tool_call);
                                }
                            }
                        } else {
                            messages.push(json!({
                                "role": "assistant",
                                "content": null,
                                "tool_calls": [tool_call],
                            }));
                        }
                        continue;
                    }
                    Some("function_call_output") => {
                        if let Some(message) = openai_response_function_output_to_openai_chat(item)
                        {
                            messages.push(message);
                        }
                        continue;
                    }
                    _ => {}
                }
                let role = object.get("role").and_then(Value::as_str).unwrap_or("user");
                let content = object
                    .get("content")
                    .map(openai_response_content_to_text)
                    .unwrap_or_default();
                if content.is_empty() {
                    continue;
                }
                match role {
                    "system" | "developer" => system.push(content),
                    "assistant" => messages.push(json!({
                        "role": "assistant",
                        "content": content,
                    })),
                    _ => messages.push(json!({
                        "role": "user",
                        "content": content,
                    })),
                }
            }
            if messages.is_empty() {
                return Err(AppError::BadRequest(
                    "input must contain at least one user or assistant message".to_string(),
                ));
            }
            Ok((system, messages))
        }
        _ => Err(AppError::BadRequest(
            "input must be a string or an array".to_string(),
        )),
    }
}
fn openai_response_tools_to_openai_chat(value: Value) -> Option<Value> {
    let tools = value.as_array()?;
    let converted = tools
        .iter()
        .filter_map(openai_response_tool_to_openai_chat)
        .collect::<Vec<_>>();
    (!converted.is_empty()).then_some(Value::Array(converted))
}

fn openai_response_tool_to_openai_chat(tool: &Value) -> Option<Value> {
    let object = tool.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("function") | None => {}
        _ => return None,
    }
    let name = object.get("name").and_then(Value::as_str)?;
    if name.is_empty() {
        return None;
    }
    let parameters = object
        .get("parameters")
        .filter(|schema| schema.as_object().is_some())
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    let mut function = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("parameters".to_string(), parameters),
    ]);
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        function.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Some(json!({
        "type": "function",
        "function": function,
    }))
}
fn openai_response_tool_choice_to_openai_chat(value: &Value) -> Option<Value> {
    match value {
        Value::String(choice) => Some(Value::String(choice.to_string())),
        Value::Object(object) => match object.get("type").and_then(Value::as_str)? {
            "auto" | "none" | "required" => object.get("type").cloned(),
            "function" => object.get("name").and_then(Value::as_str).map(|name| {
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
fn openai_response_function_output_to_openai_chat(item: &Value) -> Option<Value> {
    let id = item.get("call_id").and_then(Value::as_str)?;
    let content = item
        .get("output")
        .or_else(|| item.get("content"))
        .map(openai_response_content_to_text)
        .unwrap_or_default();
    Some(json!({
        "role": "tool",
        "tool_call_id": id,
        "content": content,
    }))
}
