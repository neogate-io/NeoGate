use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::{
    error::{AppError, AppResult},
    relay::RelayContext,
};

use super::{
    content_value_to_text, estimate_tokens,
    stream::{finish_bridge_json, finish_bridge_stream, BridgeSseConverter},
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
    let (system, messages) = openai_response_input_to_anthropic_messages(&input)?;
    object.insert("messages".to_string(), Value::Array(messages));
    if !system.is_empty() {
        object.insert("system".to_string(), Value::String(system.join("\n")));
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
    for key in [
        "background",
        "include",
        "instructions",
        "metadata",
        "parallel_tool_calls",
        "previous_response_id",
        "prompt",
        "reasoning",
        "service_tier",
        "store",
        "text",
        "top_logprobs",
        "truncation",
        "user",
    ] {
        object.remove(key);
    }

    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn rename_field(object: &mut Map<String, Value>, from: &str, to: &str) {
    if let Some(value) = object.remove(from) {
        object.insert(to.to_string(), value);
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

fn openai_response_content_to_text(value: &Value) -> String {
    content_value_to_text(value, &["input_text", "output_text", "text"], true)
}

pub(crate) async fn finish_anthropic_as_openai_response(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    if ctx.streamed {
        return finish_bridge_stream(
            ctx,
            status,
            upstream_response,
            AnthropicSseToOpenAiResponse::new,
            "ignored trailing upstream body read error after completed Anthropic response fallback stream",
        );
    }
    finish_bridge_json(
        ctx,
        status,
        upstream_response,
        anthropic_response_to_openai_response,
        "ignored trailing upstream body read error after parsing complete Anthropic response fallback",
    )
    .await
}

fn anthropic_response_to_openai_response(body: &[u8], fallback_model: &str) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_anthropic_fallback");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
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
    let output = anthropic_content_to_openai_response_output(value.get("content"), id);
    let status = if value.get("stop_reason").and_then(Value::as_str) == Some("max_tokens") {
        "incomplete"
    } else {
        "completed"
    };
    let payload = json!({
        "id": id,
        "object": "response",
        "created_at": 0,
        "status": status,
        "background": false,
        "model": model,
        "output": output,
        "usage": openai_response_usage(input_tokens, output_tokens),
    });
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

fn anthropic_content_to_openai_response_output(
    content: Option<&Value>,
    response_id: &str,
) -> Vec<Value> {
    let Some(Value::Array(items)) = content else {
        return vec![openai_response_message_item(
            format!("msg_{response_id}"),
            anthropic_content_to_text(content),
            "completed",
        )];
    };

    let mut output = Vec::new();
    let mut text = String::new();
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(part) = item.get("text").and_then(Value::as_str) {
                    text.push_str(part);
                }
            }
            Some("tool_use") => {
                if !text.is_empty() {
                    output.push(openai_response_message_item(
                        format!("msg_{response_id}_{}", output.len()),
                        std::mem::take(&mut text),
                        "completed",
                    ));
                }
                if let Some(tool_call) = anthropic_tool_use_to_openai_response_function_call(item) {
                    output.push(tool_call);
                }
            }
            _ => {}
        }
    }
    if !text.is_empty() || output.is_empty() {
        output.push(openai_response_message_item(
            format!("msg_{response_id}_{}", output.len()),
            text,
            "completed",
        ));
    }
    output
}

fn anthropic_content_to_text(content: Option<&Value>) -> String {
    content
        .map(|value| content_value_to_text(value, &["text"], false))
        .unwrap_or_default()
}

fn openai_response_message_item(id: String, text: String, status: &str) -> Value {
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

fn anthropic_tool_use_to_openai_response_function_call(item: &Value) -> Option<Value> {
    let id = item.get("id").and_then(Value::as_str)?;
    let name = item.get("name").and_then(Value::as_str)?;
    let arguments = item
        .get("input")
        .map(Value::to_string)
        .unwrap_or_else(|| "{}".to_string());
    Some(json!({
        "id": format!("fc_{id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": id,
        "name": name,
        "arguments": arguments,
    }))
}

fn openai_response_usage(input_tokens: i64, output_tokens: i64) -> Value {
    json!({
        "input_tokens": input_tokens,
        "output_tokens": output_tokens,
        "total_tokens": input_tokens.saturating_add(output_tokens),
        "input_tokens_details": { "cached_tokens": 0 },
        "output_tokens_details": { "reasoning_tokens": 0 },
    })
}

struct AnthropicSseToOpenAiResponse {
    buffer: Vec<u8>,
    model: String,
    response_id: String,
    output_item_id: String,
    current_tool_call: Option<StreamingToolCall>,
    content_index: i64,
    sequence_number: i64,
    response_started: bool,
    output_started: bool,
    content_started: bool,
    completed_output: Vec<Value>,
    stopped: bool,
    text: String,
    input_tokens: i64,
    output_tokens: i64,
    status: &'static str,
}

impl AnthropicSseToOpenAiResponse {
    fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            response_id: "resp_anthropic_fallback".to_string(),
            output_item_id: "msg_anthropic_fallback".to_string(),
            current_tool_call: None,
            content_index: 0,
            sequence_number: 0,
            response_started: false,
            output_started: false,
            content_started: false,
            completed_output: Vec::new(),
            stopped: false,
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            status: "completed",
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
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return;
        };
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
            Some("content_block_stop") => self.stop_content_block(out),
            Some("message_delta") => self.observe_message_delta(value),
            Some("message_stop") => self.finish(out),
            _ => {}
        }
    }

    fn start_from_message(&mut self, value: &Value, _out: &mut Vec<u8>) {
        if let Some(message) = value.get("message") {
            if let Some(id) = message.get("id").and_then(Value::as_str) {
                self.response_id = format!("resp_{id}");
                self.output_item_id = id.to_string();
            }
            if let Some(model) = message.get("model").and_then(Value::as_str) {
                self.model = model.to_string();
            }
            self.observe_usage(message);
        }
    }

    fn start_content_block(&mut self, value: &Value, out: &mut Vec<u8>) {
        let Some(block) = value.get("content_block") else {
            return;
        };
        match block.get("type").and_then(Value::as_str) {
            Some("tool_use") => self.start_tool_call(block, out),
            Some("text") => self.ensure_content_started(out),
            _ => {}
        }
    }

    fn push_content_delta(&mut self, value: &Value, out: &mut Vec<u8>) {
        if let Some(partial_json) = value
            .get("delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("input_json_delta"))
            .and_then(|delta| delta.get("partial_json"))
            .and_then(Value::as_str)
        {
            self.push_tool_arguments_delta(partial_json, out);
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
        self.ensure_content_started(out);
        self.text.push_str(text);
        self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(text));
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "sequence_number": sequence_number,
                "item_id": self.output_item_id,
                "output_index": 0,
                "content_index": self.content_index,
                "delta": text,
            }),
        );
    }

    fn stop_content_block(&mut self, out: &mut Vec<u8>) {
        if self.current_tool_call.is_some() {
            self.finish_tool_call(out);
        }
    }

    fn observe_message_delta(&mut self, value: &Value) {
        match value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            Some("max_tokens") => self.status = "incomplete",
            Some("tool_use") => self.status = "completed",
            _ => {}
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

    fn ensure_content_started(&mut self, out: &mut Vec<u8>) {
        self.ensure_response_started(out);
        if !self.output_started {
            self.output_started = true;
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": sequence_number,
                    "output_index": 0,
                    "item": {
                        "id": self.output_item_id,
                        "type": "message",
                        "status": "in_progress",
                        "role": "assistant",
                        "content": [],
                    },
                }),
            );
        }
        if !self.content_started {
            self.content_started = true;
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.content_part.added",
                json!({
                    "type": "response.content_part.added",
                    "sequence_number": sequence_number,
                    "item_id": self.output_item_id,
                    "output_index": 0,
                    "content_index": self.content_index,
                    "part": {
                        "type": "output_text",
                        "text": "",
                        "annotations": [],
                    },
                }),
            );
        }
    }

    fn ensure_response_started(&mut self, out: &mut Vec<u8>) {
        if self.response_started {
            return;
        }
        self.response_started = true;
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": sequence_number,
                "response": self.response_payload("in_progress"),
            }),
        );
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "sequence_number": sequence_number,
                "response": self.response_payload("in_progress"),
            }),
        );
    }

    fn start_tool_call(&mut self, block: &Value, out: &mut Vec<u8>) {
        self.ensure_response_started(out);
        if self.current_tool_call.is_some() {
            self.finish_tool_call(out);
        }
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
        let output_index = if self.output_started { 1 } else { 0 };
        let item_id = format!("fc_{id}");
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "sequence_number": sequence_number,
                "output_index": output_index,
                "item": {
                    "id": item_id,
                    "type": "function_call",
                    "status": "in_progress",
                    "call_id": id,
                    "name": name,
                    "arguments": "",
                },
            }),
        );
        self.current_tool_call = Some(StreamingToolCall {
            output_index,
            item_id,
            call_id: id,
            name,
            arguments: String::new(),
        });
    }

    fn push_tool_arguments_delta(&mut self, partial_json: &str, out: &mut Vec<u8>) {
        if self.current_tool_call.is_none() {
            self.current_tool_call = Some(StreamingToolCall {
                output_index: if self.output_started { 1 } else { 0 },
                item_id: "fc_toolu_anthropic_fallback".to_string(),
                call_id: "toolu_anthropic_fallback".to_string(),
                name: "tool".to_string(),
                arguments: String::new(),
            });
        }
        let Some(tool_call) = self.current_tool_call.as_mut() else {
            return;
        };
        tool_call.arguments.push_str(partial_json);
        let item_id = tool_call.item_id.clone();
        let output_index = tool_call.output_index;
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "sequence_number": sequence_number,
                "item_id": item_id,
                "output_index": output_index,
                "delta": partial_json,
            }),
        );
    }

    fn finish_tool_call(&mut self, out: &mut Vec<u8>) {
        let Some(tool_call) = self.current_tool_call.take() else {
            return;
        };
        let done_item = json!({
            "id": tool_call.item_id,
            "type": "function_call",
            "status": "completed",
            "call_id": tool_call.call_id,
            "name": tool_call.name,
            "arguments": tool_call.arguments,
        });
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.function_call_arguments.done",
            json!({
                "type": "response.function_call_arguments.done",
                "sequence_number": sequence_number,
                "item_id": done_item["id"],
                "output_index": tool_call.output_index,
                "arguments": done_item["arguments"],
            }),
        );
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": sequence_number,
                "output_index": tool_call.output_index,
                "item": done_item,
            }),
        );
        self.completed_output.push(done_item);
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.stopped {
            return;
        }
        if self.current_tool_call.is_some() {
            self.finish_tool_call(out);
        }
        if !self.output_started && !self.content_started {
            self.ensure_content_started(out);
        }
        self.stopped = true;
        if self.output_started {
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "sequence_number": sequence_number,
                    "item_id": self.output_item_id,
                    "output_index": 0,
                    "content_index": self.content_index,
                    "text": self.text,
                }),
            );
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "sequence_number": sequence_number,
                    "item_id": self.output_item_id,
                    "output_index": 0,
                    "content_index": self.content_index,
                    "part": {
                        "type": "output_text",
                        "text": self.text,
                        "annotations": [],
                    },
                }),
            );
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "sequence_number": sequence_number,
                    "output_index": 0,
                    "item": self.output_item_payload("completed"),
                }),
            );
            self.completed_output
                .push(self.output_item_payload("completed"));
        }
        let event_type = if self.status == "completed" {
            "response.completed"
        } else {
            "response.incomplete"
        };
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            event_type,
            json!({
                "type": event_type,
                "sequence_number": sequence_number,
                "response": self.response_payload(self.status),
            }),
        );
    }

    fn response_payload(&self, status: &str) -> Value {
        let output = if status == "in_progress" {
            Vec::new()
        } else {
            self.completed_output.clone()
        };
        json!({
            "id": self.response_id,
            "object": "response",
            "created_at": 0,
            "status": status,
            "background": false,
            "model": self.model,
            "output": output,
            "usage": openai_response_usage(self.input_tokens, self.output_tokens),
        })
    }

    fn output_item_payload(&self, status: &str) -> Value {
        json!({
            "id": self.output_item_id,
            "type": "message",
            "status": status,
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": self.text,
                "annotations": [],
            }],
        })
    }

    fn push_event(&self, out: &mut Vec<u8>, event: &str, data: Value) {
        out.extend_from_slice(format!("event: {event}\n").as_bytes());
        out.extend_from_slice(format!("data: {data}\n\n").as_bytes());
    }

    fn next_sequence_number(&mut self) -> i64 {
        let sequence_number = self.sequence_number;
        self.sequence_number = self.sequence_number.saturating_add(1);
        sequence_number
    }
}

struct StreamingToolCall {
    output_index: i64,
    item_id: String,
    call_id: String,
    name: String,
    arguments: String,
}

impl BridgeSseConverter for AnthropicSseToOpenAiResponse {
    fn push(&mut self, chunk: &[u8]) -> Bytes {
        Self::push(self, chunk)
    }

    fn stopped(&self) -> bool {
        self.stopped
    }
}

#[cfg(test)]
mod response_tests {
    use super::*;

    #[test]
    fn converts_openai_response_request_to_anthropic_messages() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","input":[{"role":"developer","content":[{"type":"input_text","text":"Be terse."}]},{"role":"user","content":[{"type":"input_text","text":"Reply OK"}]}],"max_output_tokens":16,"store":false,"reasoning":{"effort":"low"}}"#,
        );

        let converted = openai_response_to_anthropic_messages(body).unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["model"], "claude-sonnet-4");
        assert_eq!(value["max_tokens"], 16);
        assert_eq!(value["system"], "Be terse.");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "Reply OK");
        assert!(value.get("input").is_none());
        assert!(value.get("max_output_tokens").is_none());
        assert!(value.get("store").is_none());
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
