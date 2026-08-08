use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{error::AppResult, relay::RelayContext};

use super::{
    anthropic_usage_input_tokens, anthropic_usage_output_tokens, content_value_to_text,
    estimate_tokens,
    responses_common::{
        drain_sse_lines, openai_response_message_item, openai_response_reasoning_item,
        openai_response_usage, push_sse_event, StreamingToolCall,
    },
    stream::{finish_bridge_json, finish_bridge_stream, BridgeSseConverter},
};

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
pub(super) fn anthropic_response_to_openai_response(
    body: &[u8],
    fallback_model: &str,
) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_anthropic_fallback");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    let usage = value.get("usage");
    let input_tokens = anthropic_usage_input_tokens(usage);
    let output_tokens = anthropic_usage_output_tokens(usage);
    let output = anthropic_content_to_openai_response_output(value.get("content"), id);
    // Treat stop_reason "max_tokens" as completed instead of incomplete.
    // Emitting response.incomplete for token-limit stops makes Codex CLI abort
    // with "stream disconnected before completion".
    let payload = json!({
        "id": id,
        "object": "response",
        "created_at": 0,
        "status": "completed",
        "background": false,
        "model": model,
        "output": output,
        "usage": openai_response_usage(usage, input_tokens, output_tokens),
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
    let mut reasoning_text = String::new();

    // 按原始块顺序遍历，在遇到 tool_use 时先 flush 已累积的 reasoning 和 text，
    // 保持 Anthropic 原始顺序（thinking → text → tool_use）。
    // 修复前：reasoning_text 统一在循环结束后追加，丢失了与 tool_use 之间的位置关系。
    for item in items {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(part) = item.get("text").and_then(Value::as_str) {
                    text.push_str(part);
                }
            }
            Some("thinking") => {
                if let Some(part) = item.get("thinking").and_then(Value::as_str) {
                    reasoning_text.push_str(part);
                }
            }
            Some("tool_use") => {
                // 在工具调用之前按顺序 flush 已累积的 reasoning 和 text
                if !reasoning_text.is_empty() {
                    output.push(openai_response_reasoning_item(
                        format!("rs_{response_id}_{}", output.len()),
                        std::mem::take(&mut reasoning_text),
                        "completed",
                    ));
                }
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
    // flush 循环结束后剩余的 reasoning 和 text
    if !reasoning_text.is_empty() {
        output.push(openai_response_reasoning_item(
            format!("rs_{response_id}_{}", output.len()),
            reasoning_text,
            "completed",
        ));
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

fn anthropic_tool_use_to_openai_response_function_call(item: &Value) -> Option<Value> {
    let id = item.get("id").and_then(Value::as_str)?;
    let name = item.get("name").and_then(Value::as_str)?;
    let arguments = item
        .get("input")
        .map_or_else(|| "{}".to_string(), Value::to_string);
    Some(json!({
        "id": format!("fc_{id}"),
        "type": "function_call",
        "status": "completed",
        "call_id": id,
        "name": name,
        "arguments": arguments,
    }))
}
pub(super) struct AnthropicSseToOpenAiResponse {
    buffer: Vec<u8>,
    model: String,
    response_id: String,
    output_item_id: String,
    message_output_index: i64,
    /// 单调递增的下一个 output_index。每创建一个新 output item（reasoning/message/
    /// tool_call）时分配当前值再自增，保证唯一。不能像旧实现那样只按存活标志推算——
    /// 已完成的 tool_use 块会移入 completed_output，导致后续并行 tool_use 复用同一
    /// index（都算成 0），下游按 output_index 索引时会覆盖前一个工具调用而丢失。
    next_output_index: i64,
    current_tool_call: Option<StreamingToolCall>,
    content_index: i64,
    sequence_number: i64,
    response_started: bool,
    output_started: bool,
    content_started: bool,
    reasoning_output_index: Option<i64>,
    reasoning_summary_started: bool,
    reasoning_finished: bool,
    completed_output: Vec<Value>,
    stopped: bool,
    reasoning_text: String,
    text: String,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    cache_creation_input_tokens: i64,
    status: &'static str,
}

impl AnthropicSseToOpenAiResponse {
    pub(super) fn new(model: String) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            response_id: "resp_anthropic_fallback".to_string(),
            output_item_id: "msg_anthropic_fallback".to_string(),
            message_output_index: 0,
            next_output_index: 0,
            current_tool_call: None,
            content_index: 0,
            sequence_number: 0,
            response_started: false,
            output_started: false,
            content_started: false,
            reasoning_output_index: None,
            reasoning_summary_started: false,
            reasoning_finished: false,
            completed_output: Vec::new(),
            stopped: false,
            reasoning_text: String::new(),
            text: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
            status: "completed",
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Bytes {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        for line in drain_sse_lines(&mut self.buffer) {
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
            Some("thinking") => self.ensure_reasoning_started(out),
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
        if let Some(thinking) = value
            .get("delta")
            .filter(|delta| delta.get("type").and_then(Value::as_str) == Some("thinking_delta"))
            .and_then(|delta| delta.get("thinking"))
            .and_then(Value::as_str)
            .filter(|thinking| !thinking.is_empty())
        {
            self.push_reasoning_delta(thinking, out);
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
                "output_index": self.message_output_index,
                "content_index": self.content_index,
                "delta": text,
            }),
        );
    }

    fn stop_content_block(&mut self, out: &mut Vec<u8>) {
        if self.current_tool_call.is_some() {
            self.finish_tool_call(out);
        }
        if self.reasoning_output_index.is_some() && !self.reasoning_finished {
            self.finish_reasoning(out);
        }
    }

    fn observe_message_delta(&mut self, value: &Value) {
        if let Some("tool_use") = value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            // stop_reason = "tool_use" 表示工具调用正常完成，状态应标记为 completed。
            // （与 max_tokens/end_turn 等其他停止原因区分：tool_use 是预期的中间停止，
            //  不是截断，因此标记 completed 而非 incomplete。）
            self.status = "completed";
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

    fn ensure_content_started(&mut self, out: &mut Vec<u8>) {
        self.ensure_response_started(out);
        if !self.output_started {
            self.output_started = true;
            self.message_output_index = self.next_output_index_for_new_item();
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": sequence_number,
                    "output_index": self.message_output_index,
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
                    "output_index": self.message_output_index,
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

    fn ensure_reasoning_started(&mut self, out: &mut Vec<u8>) {
        self.ensure_response_started(out);
        if self.reasoning_output_index.is_none() {
            if self.current_tool_call.is_some() {
                self.finish_tool_call(out);
            }
            let output_index = self.next_output_index_for_new_item();
            self.reasoning_output_index = Some(output_index);
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": sequence_number,
                    "output_index": output_index,
                    "item": {
                        "id": self.reasoning_item_id(),
                        "type": "reasoning",
                        "status": "in_progress",
                        "summary": [],
                    },
                }),
            );
        }
        if !self.reasoning_summary_started {
            self.reasoning_summary_started = true;
            let output_index = self.reasoning_output_index.unwrap_or(0);
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.reasoning_summary_part.added",
                json!({
                    "type": "response.reasoning_summary_part.added",
                    "sequence_number": sequence_number,
                    "item_id": self.reasoning_item_id(),
                    "output_index": output_index,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": "" },
                }),
            );
        }
    }

    fn push_reasoning_delta(&mut self, thinking: &str, out: &mut Vec<u8>) {
        self.ensure_reasoning_started(out);
        self.reasoning_text.push_str(thinking);
        self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(thinking));
        let output_index = self.reasoning_output_index.unwrap_or(0);
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "sequence_number": sequence_number,
                "item_id": self.reasoning_item_id(),
                "output_index": output_index,
                "summary_index": 0,
                "delta": thinking,
            }),
        );
    }

    fn finish_reasoning(&mut self, out: &mut Vec<u8>) {
        if self.reasoning_finished {
            return;
        }
        let Some(output_index) = self.reasoning_output_index else {
            return;
        };
        self.reasoning_finished = true;
        let item_id = self.reasoning_item_id();
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.reasoning_summary_text.done",
            json!({
                "type": "response.reasoning_summary_text.done",
                "sequence_number": sequence_number,
                "item_id": item_id,
                "output_index": output_index,
                "summary_index": 0,
                "text": self.reasoning_text,
            }),
        );
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.reasoning_summary_part.done",
            json!({
                "type": "response.reasoning_summary_part.done",
                "sequence_number": sequence_number,
                "item_id": item_id,
                "output_index": output_index,
                "summary_index": 0,
                "part": { "type": "summary_text", "text": self.reasoning_text },
            }),
        );
        let done_item =
            openai_response_reasoning_item(item_id, self.reasoning_text.clone(), "completed");
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "sequence_number": sequence_number,
                "output_index": output_index,
                "item": done_item,
            }),
        );
        self.completed_output.push(done_item);
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
        let output_index = self.next_output_index_for_new_item();
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
                output_index: self.next_output_index_for_new_item(),
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
        if self.reasoning_output_index.is_some() && !self.reasoning_finished {
            self.finish_reasoning(out);
        }
        if !self.output_started && !self.content_started && self.completed_output.is_empty() {
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
                    "output_index": self.message_output_index,
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
                    "output_index": self.message_output_index,
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
                    "output_index": self.message_output_index,
                    "item": self.output_item_payload("completed"),
                }),
            );
            self.completed_output
                .push(self.output_item_payload("completed"));
        }
        let event_type = "response.completed";
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
            "usage": openai_response_usage(
                Some(&json!({
                    "input_tokens": self.input_tokens,
                    "output_tokens": self.output_tokens,
                    "cache_read_input_tokens": self.cached_input_tokens,
                    "cache_creation_input_tokens": self.cache_creation_input_tokens,
                })),
                self
                    .input_tokens
                    .saturating_add(self.cached_input_tokens),
                self.output_tokens,
            ),
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

    fn reasoning_item_id(&self) -> String {
        format!("rs_{}", self.output_item_id)
    }

    fn next_output_index_for_new_item(&mut self) -> i64 {
        let index = self.next_output_index;
        self.next_output_index = self.next_output_index.saturating_add(1);
        index
    }

    fn push_event(&self, out: &mut Vec<u8>, event: &str, data: Value) {
        push_sse_event(out, event, &data);
    }

    fn next_sequence_number(&mut self) -> i64 {
        let sequence_number = self.sequence_number;
        self.sequence_number = self.sequence_number.saturating_add(1);
        sequence_number
    }
}

impl BridgeSseConverter for AnthropicSseToOpenAiResponse {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_output_indexes_for_added_items(output: &str) -> Vec<i64> {
        let mut indexes = Vec::new();
        for line in output.lines() {
            let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                continue;
            };
            let Ok(value) = serde_json::from_str::<Value>(data) else {
                continue;
            };
            if value.get("type").and_then(Value::as_str) == Some("response.output_item.added") {
                if let Some(index) = value.get("output_index").and_then(Value::as_i64) {
                    indexes.push(index);
                }
            }
        }
        indexes
    }

    #[test]
    fn parallel_tool_calls_get_unique_output_indexes() {
        // 两个连续 tool_use 块（前一个先 stop 再开下一个）必须获得不同的 output_index，
        // 否则下游按 output_index 索引会用第二个工具调用覆盖第一个，丢失工具调用。
        let mut converter = AnthropicSseToOpenAiResponse::new("claude".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\"}}\n\n",
        ));
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"search\"}}\n\n",
        ));
        out.extend_from_slice(
            &converter.push(b"data: {\"type\":\"content_block_stop\",\"index\":0}\n\n"),
        );
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_2\",\"name\":\"fetch\"}}\n\n",
        ));
        out.extend_from_slice(
            &converter.push(b"data: {\"type\":\"content_block_stop\",\"index\":1}\n\n"),
        );

        let output = String::from_utf8(out).unwrap();
        let indexes = collect_output_indexes_for_added_items(&output);
        assert_eq!(indexes, vec![0, 1], "两个工具调用应有唯一递增的 output_index");
    }

    #[test]
    fn message_after_tool_call_gets_distinct_output_index() {
        // tool_use 完成后再来 text 块，text 的 output_index 不能与已完成的工具调用冲突。
        let mut converter = AnthropicSseToOpenAiResponse::new("claude".to_string());
        let mut out = Vec::new();
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"model\":\"claude\"}}\n\n",
        ));
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"search\"}}\n\n",
        ));
        out.extend_from_slice(
            &converter.push(b"data: {\"type\":\"content_block_stop\",\"index\":0}\n\n"),
        );
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        ));
        out.extend_from_slice(&converter.push(
            b"data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"hi\"}}\n\n",
        ));

        let output = String::from_utf8(out).unwrap();
        let indexes = collect_output_indexes_for_added_items(&output);
        assert_eq!(indexes, vec![0, 1], "工具调用后的消息块应获得独立 output_index");
    }
}
