use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::error::{AppError, AppResult};

use super::{
    common::{openai_response_content_to_text, remove_fields, rename_field},
    openai_reasoning_to_anthropic_thinking,
};

pub(crate) fn openai_response_to_anthropic_messages(body: Bytes) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;

    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;
    let input = object
        .remove("input")
        .ok_or_else(|| AppError::BadRequest("input is required".to_string()))?;
    let cache_control = openai_response_cache_control(object);
    let (system, mut messages) = openai_response_input_to_anthropic_messages(&input)?;
    let mut system = (!system.is_empty()).then(|| Value::String(system.join("\n")));
    if let Some(cache_control) = cache_control.as_ref() {
        apply_anthropic_cache_control(system.as_mut(), &mut messages, cache_control);
    }
    object.insert("messages".to_string(), Value::Array(messages));
    if let Some(system) = system {
        object.insert("system".to_string(), system);
    }
    rename_field(object, "max_output_tokens", "max_tokens");
    if let Some(tools) = object
        .remove("tools")
        .and_then(openai_response_tools_to_anthropic)
    {
        object.insert("tools".to_string(), tools);
    }
    if let Some(tool_choice) = object
        .remove("tool_choice")
        .and_then(|value| openai_response_tool_choice_to_anthropic(&value))
    {
        object.insert("tool_choice".to_string(), tool_choice);
    }
    openai_reasoning_to_anthropic_thinking(object);
    remove_fields(
        object,
        &[
            "background",
            "include",
            "instructions",
            "metadata",
            "parallel_tool_calls",
            "previous_response_id",
            "prompt",
            "prompt_cache_key",
            "prompt_cache_retention",
            "service_tier",
            "store",
            "text",
            "top_logprobs",
            "truncation",
            "user",
        ],
    );

    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn openai_response_cache_control(object: &Map<String, Value>) -> Option<Value> {
    (object.contains_key("prompt_cache_key") || object.contains_key("prompt_cache_retention"))
        .then(|| json!({ "type": "ephemeral" }))
}
fn apply_anthropic_cache_control(
    system: Option<&mut Value>,
    messages: &mut [Value],
    cache_control: &Value,
) {
    let mut applied = 0;
    if let Some(system) = system {
        if add_cache_control_to_content(system, cache_control) {
            applied += 1;
        }
    }

    let message_count = messages.len();
    let final_index = message_count.checked_sub(1);
    for (index, message) in messages.iter_mut().enumerate().rev() {
        if applied >= 4 {
            break;
        }
        if Some(index) == final_index && message_count > 1 {
            continue;
        }
        if add_cache_control_to_message(message, cache_control) {
            applied += 1;
            break;
        }
    }

    if applied == 0 {
        if let Some(message) = messages.last_mut() {
            add_cache_control_to_message(message, cache_control);
        }
    }
}

fn add_cache_control_to_message(message: &mut Value, cache_control: &Value) -> bool {
    let Some(content) = message
        .as_object_mut()
        .and_then(|message| message.get_mut("content"))
    else {
        return false;
    };
    add_cache_control_to_content(content, cache_control)
}

fn add_cache_control_to_content(content: &mut Value, cache_control: &Value) -> bool {
    match content {
        Value::String(text) if !text.is_empty() => {
            *content = Value::Array(vec![json!({
                "type": "text",
                "text": text,
                "cache_control": cache_control,
            })]);
            true
        }
        Value::Array(items) => {
            for item in items.iter_mut().rev() {
                let Some(object) = item.as_object_mut() else {
                    continue;
                };
                if object.get("type").and_then(Value::as_str) != Some("text") {
                    continue;
                }
                if object
                    .get("text")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    continue;
                }
                object.insert("cache_control".to_string(), cache_control.clone());
                return true;
            }
            false
        }
        _ => false,
    }
}

fn openai_response_input_to_anthropic_messages(
    input: &Value,
) -> AppResult<(Vec<String>, Vec<Value>)> {
    match input {
        Value::String(text) => Ok((Vec::new(), vec![json!({ "role": "user", "content": text })])),
        Value::Array(items) => {
            let mut system = Vec::new();
            let mut messages = Vec::new();
            for item in items {
                let object = item.as_object().ok_or_else(|| {
                    AppError::BadRequest("input items must be JSON objects".to_string())
                })?;
                match object.get("type").and_then(Value::as_str) {
                    Some("function_call") => {
                        if let Some(message) = openai_response_function_call_to_anthropic(item) {
                            messages.push(message);
                        }
                        continue;
                    }
                    Some("function_call_output") => {
                        if let Some(message) = openai_response_function_output_to_anthropic(item) {
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
                    "assistant" => {
                        messages.push(json!({ "role": "assistant", "content": content }));
                    }
                    _ => messages.push(json!({ "role": "user", "content": content })),
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

fn openai_response_tools_to_anthropic(value: Value) -> Option<Value> {
    let tools = value.as_array()?;
    let converted = tools
        .iter()
        .filter_map(openai_response_tool_to_anthropic)
        .collect::<Vec<_>>();
    (!converted.is_empty()).then_some(Value::Array(converted))
}
fn openai_response_tool_to_anthropic(tool: &Value) -> Option<Value> {
    let object = tool.as_object()?;
    match object.get("type").and_then(Value::as_str) {
        Some("function") | None => {}
        _ => return None,
    }
    let name = object.get("name").and_then(Value::as_str)?;
    if name.is_empty() {
        return None;
    }
    let input_schema = object
        .get("parameters")
        .filter(|schema| schema.as_object().is_some())
        .cloned()
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
    let mut converted = Map::from_iter([
        ("name".to_string(), Value::String(name.to_string())),
        ("input_schema".to_string(), input_schema),
    ]);
    if let Some(description) = object.get("description").and_then(Value::as_str) {
        converted.insert(
            "description".to_string(),
            Value::String(description.to_string()),
        );
    }
    Some(Value::Object(converted))
}

fn openai_response_tool_choice_to_anthropic(value: &Value) -> Option<Value> {
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
            "function" => object.get("name").and_then(Value::as_str).map(|name| {
                json!({
                    "type": "tool",
                    "name": name,
                })
            }),
            _ => None,
        },
        _ => None,
    }
}

fn openai_response_function_call_to_anthropic(item: &Value) -> Option<Value> {
    let name = item.get("name").and_then(Value::as_str)?;
    let id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)?;
    let input = item
        .get("arguments")
        .and_then(Value::as_str)
        .and_then(|arguments| serde_json::from_str::<Value>(arguments).ok())
        .unwrap_or_else(|| json!({}));
    Some(json!({
        "role": "assistant",
        "content": [{
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input,
        }],
    }))
}

fn openai_response_function_output_to_anthropic(item: &Value) -> Option<Value> {
    let id = item.get("call_id").and_then(Value::as_str)?;
    let output = item
        .get("output")
        .or_else(|| item.get("content"))
        .map(openai_response_content_to_text)
        .unwrap_or_default();
    Some(json!({
        "role": "user",
        "content": [{
            "type": "tool_result",
            "tool_use_id": id,
            "content": output,
        }],
    }))
}
#[cfg(test)]
mod response_tests {
    use super::super::{
        anthropic_to_responses::{
            anthropic_response_to_openai_response, AnthropicSseToOpenAiResponse,
        },
        chat_to_responses::{
            openai_chat_response_to_openai_response, OpenAiChatSseToOpenAiResponse,
        },
        responses_to_chat::openai_response_to_openai_chat,
        stream::BridgeSseConverter,
    };
    use super::*;

    #[test]
    fn converts_openai_response_request_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","input":[{"role":"developer","content":[{"type":"input_text","text":"Be terse."}]},{"role":"user","content":[{"type":"input_text","text":"Reply OK"}]}],"max_output_tokens":16,"store":false,"reasoning":{"effort":"low"}}"#,
        );

        let converted = openai_response_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["max_tokens"], 1281);
        assert_eq!(value["system"], "Be terse.");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "Reply OK");
        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["thinking"]["budget_tokens"], 1280);
        assert!(value.get("input").is_none());
        assert!(value.get("max_output_tokens").is_none());
        assert!(value.get("store").is_none());
        assert!(value.get("reasoning").is_none());
    }

    #[test]
    fn converts_openai_response_request_to_openai_chat() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","input":"Reply OK","max_output_tokens":16,"temperature":0.2}"#,
        );
        let converted = openai_response_to_openai_chat(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();
        assert_eq!(value["model"], "GLM-5.1");
        assert_eq!(value["max_tokens"], 16);
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "Reply OK");
        assert!(value.get("input").is_none());
        assert!(value.get("max_output_tokens").is_none());
    }

    #[test]
    fn converts_openai_response_tools_and_tool_history_to_openai_chat() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","input":[{"role":"user","content":[{"type":"input_text","text":"Lookup weather"}]},{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"city\":\"Shanghai\"}"},{"type":"function_call_output","call_id":"call_1","output":"Sunny"}],"tools":[{"type":"function","name":"lookup","description":"Lookup weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}],"tool_choice":{"type":"function","name":"lookup"},"parallel_tool_calls":true,"max_output_tokens":16}"#,
        );

        let converted = openai_response_to_openai_chat(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["tools"][0]["type"], "function");
        assert_eq!(value["tools"][0]["function"]["name"], "lookup");
        assert_eq!(
            value["tools"][0]["function"]["parameters"]["properties"]["city"]["type"],
            "string"
        );
        assert_eq!(value["tool_choice"]["type"], "function");
        assert_eq!(value["tool_choice"]["function"]["name"], "lookup");
        assert_eq!(value["parallel_tool_calls"], true);
        assert_eq!(value["messages"][1]["role"], "assistant");
        assert_eq!(value["messages"][1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            value["messages"][1]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(
            value["messages"][1]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"Shanghai\"}"
        );
        assert_eq!(value["messages"][2]["role"], "tool");
        assert_eq!(value["messages"][2]["tool_call_id"], "call_1");
        assert_eq!(value["messages"][2]["content"], "Sunny");
    }

    #[test]
    fn converts_openai_chat_response_to_openai_response() {
        let body = br#"{"id":"chatcmpl-1","created":123,"model":"GLM-5.1","choices":[{"finish_reason":"stop","message":{"role":"assistant","reasoning_content":"Think.","content":"OK"}}],"usage":{"prompt_tokens":4,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":1}}}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();
        assert_eq!(value["id"], "chatcmpl-1");
        assert_eq!(value["object"], "response");
        assert_eq!(value["model"], "GLM-5.1");
        assert_eq!(value["status"], "completed");
        assert_eq!(value["output"][0]["type"], "reasoning");
        assert_eq!(value["output"][1]["type"], "message");
        assert_eq!(value["output"][1]["content"][0]["text"], "OK");
        assert_eq!(value["usage"]["input_tokens"], 4);
        assert_eq!(value["usage"]["output_tokens"], 3);
    }

    #[test]
    fn converts_openai_chat_stream_tool_calls_to_openai_response_events() {
        let mut converter = OpenAiChatSseToOpenAiResponse::new("GLM-5.1".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","created":123,"model":"GLM-5.1","choices":[{"delta":{"role":"assistant"}}]}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"lookup","arguments":"{\"city\":"}}]}}]}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"id":"chatcmpl-1","model":"GLM-5.1","choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Shanghai\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":6}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(b"data: [DONE]\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: response.output_item.added"));
        assert!(text.contains(r#""type":"function_call""#));
        assert!(text.contains(r#""call_id":"call_1""#));
        assert!(text.contains(r#""name":"lookup""#));
        assert!(text.contains("event: response.function_call_arguments.delta"));
        assert!(text.contains("event: response.function_call_arguments.done"));
        assert!(text.contains(r#""arguments":"{\"city\":\"Shanghai\"}""#));
        assert!(text.contains("event: response.completed"));
        assert!(text.contains(r#""cached_tokens":6"#));
    }

    #[test]
    fn converts_openai_response_reasoning_max_tokens_to_anthropic_thinking() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","input":"Reply OK","max_output_tokens":4096,"reasoning":{"enabled":true,"max_tokens":2048}}"#,
        );

        let converted = openai_response_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["thinking"]["type"], "enabled");
        assert_eq!(value["thinking"]["budget_tokens"], 2048);
        assert_eq!(value["max_tokens"], 4096);
        assert!(value.get("reasoning").is_none());
    }

    #[test]
    fn converts_openai_response_tools_and_tool_history_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","input":[{"role":"user","content":[{"type":"input_text","text":"Lookup weather"}]},{"type":"function_call","call_id":"call_1","name":"lookup","arguments":"{\"city\":\"Shanghai\"}"},{"type":"function_call_output","call_id":"call_1","output":"Sunny"}],"tools":[{"type":"function","name":"lookup","description":"Lookup weather","parameters":{"type":"object","properties":{"city":{"type":"string"}}}}],"tool_choice":{"type":"function","name":"lookup"},"max_output_tokens":16}"#,
        );

        let converted = openai_response_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["tools"][0]["name"], "lookup");
        assert_eq!(value["tools"][0]["description"], "Lookup weather");
        assert_eq!(
            value["tools"][0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
        assert_eq!(value["tool_choice"]["type"], "tool");
        assert_eq!(value["tool_choice"]["name"], "lookup");
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
    }

    #[test]
    fn converts_openai_prompt_cache_key_to_anthropic_cache_control() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","prompt_cache_key":"trace-1","prompt_cache_retention":"24h","input":[{"role":"developer","content":[{"type":"input_text","text":"Stable instructions"}]},{"role":"user","content":[{"type":"input_text","text":"Stable context"}]},{"role":"user","content":[{"type":"input_text","text":"Fresh question"}]}],"max_output_tokens":16}"#,
        );

        let converted = openai_response_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert!(value.get("prompt_cache_key").is_none());
        assert!(value.get("prompt_cache_retention").is_none());
        assert_eq!(value["system"][0]["type"], "text");
        assert_eq!(value["system"][0]["text"], "Stable instructions");
        assert_eq!(value["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(value["messages"][0]["content"][0]["type"], "text");
        assert_eq!(value["messages"][0]["content"][0]["text"], "Stable context");
        assert_eq!(
            value["messages"][0]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
        assert_eq!(value["messages"][1]["content"], "Fresh question");
    }

    #[test]
    fn converts_anthropic_message_response_to_openai_response() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":1}}"#;

        let converted = anthropic_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["id"], "msg-1");
        assert_eq!(value["object"], "response");
        assert_eq!(value["status"], "completed");
        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["output"][0]["type"], "message");
        assert_eq!(value["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(value["output"][0]["content"][0]["text"], "OK");
        assert_eq!(value["usage"]["input_tokens"], 8);
        assert_eq!(value["usage"]["output_tokens"], 1);
        assert_eq!(value["usage"]["total_tokens"], 9);
    }

    #[test]
    fn converts_anthropic_thinking_response_to_openai_reasoning_output() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"thinking","thinking":"Thinking."},{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":3}}"#;

        let converted = anthropic_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["output"][0]["type"], "reasoning");
        assert_eq!(value["output"][0]["summary"][0]["text"], "Thinking.");
        assert_eq!(value["output"][1]["type"], "message");
        assert_eq!(value["output"][1]["content"][0]["text"], "OK");
    }

    #[test]
    fn converts_anthropic_tool_use_response_to_openai_function_call() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"text","text":"Checking."},{"type":"tool_use","id":"toolu_1","name":"lookup","input":{"city":"Shanghai"}}],"stop_reason":"tool_use","usage":{"input_tokens":8,"output_tokens":3}}"#;

        let converted = anthropic_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["status"], "completed");
        assert_eq!(value["output"][0]["type"], "message");
        assert_eq!(value["output"][0]["content"][0]["text"], "Checking.");
        assert_eq!(value["output"][1]["type"], "function_call");
        assert_eq!(value["output"][1]["call_id"], "toolu_1");
        assert_eq!(value["output"][1]["name"], "lookup");
        assert_eq!(value["output"][1]["arguments"], r#"{"city":"Shanghai"}"#);
    }

    #[test]
    fn converts_anthropic_stream_to_openai_response_events() {
        let mut converter = AnthropicSseToOpenAiResponse::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"O"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"K"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: response.created"));
        assert!(text.contains("event: response.output_text.delta"));
        assert!(text.contains(r#""delta":"O""#));
        assert!(text.contains(r#""delta":"K""#));
        assert!(text.contains("event: response.completed"));
        assert!(text.contains(r#""text":"OK""#));
        assert!(text.contains(r#""input_tokens":8"#));
        assert!(text.contains(r#""output_tokens":1"#));
    }

    #[test]
    fn converts_anthropic_stream_thinking_to_openai_response_reasoning_events() {
        let mut converter = AnthropicSseToOpenAiResponse::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Think"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_stop","index":0}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: response.reasoning_summary_text.delta"));
        assert!(text.contains(r#""delta":"Think""#));
        assert!(text.contains("event: response.reasoning_summary_text.done"));
        assert!(text.contains(r#""type":"reasoning""#));
        assert!(text.contains(r#""text":"Think""#));
        assert!(text.contains("event: response.completed"));
    }

    #[test]
    fn converts_anthropic_stream_tool_use_to_openai_response_events() {
        let mut converter = AnthropicSseToOpenAiResponse::new("claude-sonnet-4".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_start","message":{"id":"msg-1","model":"claude-sonnet-4","usage":{"input_tokens":8,"output_tokens":0}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"lookup","input":{}}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"Shanghai\"}"}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"content_block_stop","index":0}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        out.extend_from_slice(&converter.push(br#"data: {"type":"message_stop"}"#));
        out.extend_from_slice(&converter.push(b"\n\n"));
        let text = String::from_utf8(out).unwrap();

        assert!(text.contains("event: response.output_item.added"));
        assert!(text.contains(r#""type":"function_call""#));
        assert!(text.contains(r#""call_id":"toolu_1""#));
        assert!(text.contains(r#""name":"lookup""#));
        assert!(text.contains("event: response.function_call_arguments.delta"));
        assert!(text.contains("event: response.function_call_arguments.done"));
        assert!(text.contains(r#""arguments":"{\"city\":\"Shanghai\"}""#));
        assert!(text.contains("event: response.completed"));
    }
}
