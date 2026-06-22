use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::{
    error::{AppError, AppResult},
    relay::RelayContext,
};

use super::{
    anthropic_stop_reason_to_openai, content_value_to_text, estimate_tokens,
    finish_reason_to_anthropic,
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
    "tool_choice",
    "tools",
    "top_logprobs",
    "user",
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
    if let Some(system) = object.remove("system") {
        messages.push(json!({
            "role": "system",
            "content": content_to_text(&system),
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
        messages.push(json!({
            "role": role,
            "content": content_to_text(content),
        }));
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
        let content = message
            .get("content")
            .map(content_to_text)
            .unwrap_or_default();
        match role {
            "system" | "developer" => {
                if !content.is_empty() {
                    system.push(content);
                }
            }
            "assistant" => messages.push(json!({ "role": "assistant", "content": content })),
            _ => messages.push(json!({ "role": "user", "content": content })),
        }
    }

    object.insert("messages".to_string(), Value::Array(messages));
    if !system.is_empty() {
        object.insert("system".to_string(), Value::String(system.join("\n")));
    }
    rename_field(object, "max_completion_tokens", "max_tokens");
    rename_field(object, "stop", "stop_sequences");
    for &key in OPENAI_TO_ANTHROPIC_DROP_FIELDS {
        object.remove(key);
    }

    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn content_to_text(value: &Value) -> String {
    content_value_to_text(value, &["text"], false)
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
    let text = choice
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    let stop_reason = finish_reason_to_anthropic(finish_reason.unwrap_or_default());
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
        "content": [{"type": "text", "text": text}],
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
        }
    });
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

fn anthropic_response_to_openai_chat(body: &[u8], fallback_model: &str) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let input_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("input_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let output_tokens = value
        .get("usage")
        .and_then(|usage| usage.get("output_tokens"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let payload = json!({
        "id": value.get("id").and_then(Value::as_str).unwrap_or("chatcmpl_anthropic_fallback"),
        "object": "chat.completion",
        "created": 0,
        "model": value.get("model").and_then(Value::as_str).unwrap_or(fallback_model),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": anthropic_content_to_text(value.get("content")),
            },
            "finish_reason": anthropic_stop_reason_to_openai(
                value.get("stop_reason").and_then(Value::as_str).unwrap_or_default()
            ),
        }],
        "usage": {
            "prompt_tokens": input_tokens,
            "completion_tokens": output_tokens,
            "total_tokens": input_tokens.saturating_add(output_tokens),
        },
    });
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

fn anthropic_content_to_text(content: Option<&Value>) -> String {
    content
        .map(|value| content_value_to_text(value, &["text"], false))
        .unwrap_or_default()
}

struct OpenAiChatSseToAnthropic {
    buffer: Vec<u8>,
    model: String,
    message_id: String,
    started: bool,
    content_started: bool,
    stopped: bool,
    stop_reason: &'static str,
    input_tokens: i64,
    output_tokens: i64,
}

impl OpenAiChatSseToAnthropic {
    fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            message_id: "msg_openai_fallback".to_string(),
            started: false,
            content_started: false,
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
        if let Some(text) = choice
            .get("delta")
            .and_then(|delta| delta.get("content"))
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
        {
            self.ensure_content_start(out);
            self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(text));
            push_anthropic_sse(
                out,
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": 0,
                    "delta": { "type": "text_delta", "text": text },
                }),
            );
        }
        self.observe_openai_usage(value);
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

    fn ensure_content_start(&mut self, out: &mut Vec<u8>) {
        if self.content_started {
            return;
        }
        self.content_started = true;
        push_anthropic_sse(
            out,
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": 0,
                "content_block": { "type": "text", "text": "" },
            }),
        );
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.stopped {
            return;
        }
        self.stopped = true;
        if !self.started {
            self.ensure_message_start(&json!({}), out);
        }
        self.ensure_content_start(out);
        push_anthropic_sse(
            out,
            "content_block_stop",
            json!({ "type": "content_block_stop", "index": 0 }),
        );
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
    stopped: bool,
    finish_reason: &'static str,
    input_tokens: i64,
    output_tokens: i64,
}

impl AnthropicSseToOpenAiChat {
    fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            message_id: "chatcmpl_anthropic_fallback".to_string(),
            started: false,
            stopped: false,
            finish_reason: "stop",
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
        self.observe_anthropic_event(&value, out);
    }

    fn observe_anthropic_event(&mut self, value: &Value, out: &mut Vec<u8>) {
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => self.start_from_message(value, out),
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

    fn push_content_delta(&mut self, value: &Value, out: &mut Vec<u8>) {
        self.ensure_started(out);
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
                "prompt_tokens": self.input_tokens,
                "completion_tokens": self.output_tokens,
                "total_tokens": self.input_tokens.saturating_add(self.output_tokens),
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
    fn converts_openai_chat_request_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","messages":[{"role":"system","content":"Be terse."},{"role":"developer","content":"No markdown."},{"role":"user","content":"Reply OK"}],"max_completion_tokens":16,"stop":["END"],"stream_options":{"include_usage":true}}"#,
        );

        let converted = openai_chat_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["max_tokens"], 16);
        assert_eq!(value["system"], "Be terse.\nNo markdown.");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "Reply OK");
        assert_eq!(value["stop_sequences"][0], "END");
        assert!(value.get("max_completion_tokens").is_none());
        assert!(value.get("stream_options").is_none());
    }

    #[test]
    fn converts_anthropic_message_response_to_openai_chat() {
        let body = br#"{"id":"msg-1","model":"claude-sonnet-4","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":8,"output_tokens":1}}"#;

        let converted = anthropic_response_to_openai_chat(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["id"], "msg-1");
        assert_eq!(value["object"], "chat.completion");
        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["choices"][0]["message"]["role"], "assistant");
        assert_eq!(value["choices"][0]["message"]["content"], "OK");
        assert_eq!(value["choices"][0]["finish_reason"], "stop");
        assert_eq!(value["usage"]["prompt_tokens"], 8);
        assert_eq!(value["usage"]["completion_tokens"], 1);
        assert_eq!(value["usage"]["total_tokens"], 9);
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
    fn converts_anthropic_stream_to_openai_chat_chunks() {
        let mut converter = AnthropicSseToOpenAiChat::new("claude-sonnet-4".to_string());
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

        assert!(text.contains(r#""object":"chat.completion.chunk""#));
        assert!(text.contains(r#""role":"assistant""#));
        assert!(text.contains(r#""content":"O""#));
        assert!(text.contains(r#""content":"K""#));
        assert!(text.contains(r#""finish_reason":"stop""#));
        assert!(text.contains(r#""prompt_tokens":8"#));
        assert!(text.contains(r#""completion_tokens":1"#));
        assert!(text.contains("data: [DONE]"));
    }
}
