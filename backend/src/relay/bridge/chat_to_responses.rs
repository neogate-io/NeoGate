use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{error::AppResult, relay::RelayContext};

use super::{
    common::text_field_content_to_text,
    estimate_tokens,
    responses_common::{
        choice_usage_cached_tokens, drain_sse_lines, openai_response_message_item,
        openai_response_reasoning_item, openai_response_usage, push_sse_event, StreamingToolCall,
    },
    stream::{finish_bridge_json, finish_bridge_stream, BridgeSseConverter},
};

pub(crate) async fn finish_openai_chat_as_openai_response(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let warning_context = ReasoningMarkupWarningContext::from(&ctx);
    if ctx.streamed {
        return finish_bridge_stream(
            ctx,
            status,
            upstream_response,
            move |model| {
                OpenAiChatSseToOpenAiResponse::new_with_warning_context(model, warning_context)
            },
            "ignored trailing upstream body read error after completed OpenAI chat response fallback stream",
        );
    }
    finish_bridge_json(
        ctx,
        status,
        upstream_response,
        move |body, fallback_model| {
            warn_if_nonstream_reasoning_markup(body, &warning_context);
            openai_chat_response_to_openai_response(body, fallback_model)
        },
        "ignored trailing upstream body read error after parsing complete OpenAI chat response fallback",
    )
    .await
}

#[derive(Clone)]
struct ReasoningMarkupWarningContext {
    relay_trace_id: uuid::Uuid,
    provider: String,
    channel_id: crate::id::DbId,
    channel_name: String,
    channel_endpoint_id: crate::id::DbId,
    protocol: &'static str,
    model: String,
    path: &'static str,
    upstream_path: String,
    response_mode: &'static str,
}

impl From<&RelayContext> for ReasoningMarkupWarningContext {
    fn from(ctx: &RelayContext) -> Self {
        Self {
            relay_trace_id: ctx.relay_trace_id,
            provider: ctx.upstream.provider.clone(),
            channel_id: ctx.upstream.channel_id,
            channel_name: ctx.upstream.channel_name.clone(),
            channel_endpoint_id: ctx.upstream.channel_endpoint_id,
            protocol: ctx.protocol.as_str(),
            model: ctx.model.clone(),
            path: ctx.path,
            upstream_path: ctx
                .upstream_request_path
                .clone()
                .unwrap_or_else(|| ctx.path.to_string()),
            response_mode: ctx.upstream_response_mode.unwrap_or("passthrough"),
        }
    }
}

impl ReasoningMarkupWarningContext {
    fn warn(&self) {
        tracing::warn!(
            relay_trace_id = %self.relay_trace_id,
            provider = %self.provider,
            channel_id = self.channel_id,
            channel_name = %self.channel_name,
            channel_endpoint_id = self.channel_endpoint_id,
            protocol = self.protocol,
            model = %self.model,
            path = self.path,
            upstream_path = %self.upstream_path,
            response_mode = self.response_mode,
            "reasoning markup detected in assistant content during chat-to-responses fallback"
        );
    }
}

use super::reasoning_markup::{split_leading_reasoning_markup, LeadingReasoningMarkupParser};

fn warn_if_nonstream_reasoning_markup(body: &[u8], context: &ReasoningMarkupWarningContext) {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let content = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .map(text_field_content_to_text)
        .unwrap_or_default();
    if split_leading_reasoning_markup(&content).is_some() {
        context.warn();
    }
}

pub(super) fn openai_chat_response_to_openai_response(
    body: &[u8],
    fallback_model: &str,
) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(body)?;
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("resp_chat_fallback");
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback_model);
    let usage = value.get("usage");
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let message = choice.and_then(|choice| choice.get("message"));
    let finish_reason = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    let output = openai_chat_message_to_response_output(message, id);
    // 将 Chat Completions 的 finish_reason 映射到 Responses API 的 status / incomplete_details，
    // 让客户端能区分正常完成与截断/内容过滤。
    // "length" → max_output_tokens 截断；"content_filter" → content_filter 拦截；其余 → completed。
    let (status, incomplete_details) = chat_finish_reason_to_response_status(finish_reason);
    let mut payload = json!({
        "id": id,
        "object": "response",
        "created_at": value.get("created").and_then(Value::as_i64).unwrap_or(0),
        "status": status,
        "background": false,
        "model": model,
        "output": output,
        "usage": openai_response_usage(
            usage,
            openai_chat_usage_tokens(usage, "prompt_tokens"),
            openai_chat_usage_tokens(usage, "completion_tokens")
        ),
    });
    if let Some(details) = incomplete_details {
        payload["incomplete_details"] = details;
    }
    Ok(Bytes::from(serde_json::to_vec(&payload)?))
}

/// 将 Chat Completions 的 finish_reason 映射到 Responses API 的 (status, incomplete_details)。
fn chat_finish_reason_to_response_status(
    finish_reason: Option<&str>,
) -> (&'static str, Option<Value>) {
    match finish_reason {
        Some("length") => (
            "incomplete",
            Some(json!({ "reason": "max_output_tokens" })),
        ),
        Some("content_filter") => (
            "incomplete",
            Some(json!({ "reason": "content_filter" })),
        ),
        _ => ("completed", None),
    }
}

fn openai_chat_message_to_response_output(
    message: Option<&Value>,
    response_id: &str,
) -> Vec<Value> {
    let Some(message) = message else {
        return vec![openai_response_message_item(
            format!("msg_{response_id}"),
            String::new(),
            "completed",
        )];
    };
    let mut output = Vec::new();
    let structured_reasoning = reasoning_content(message);
    if let Some(reasoning) = structured_reasoning {
        output.push(openai_response_reasoning_item(
            format!("rs_{response_id}"),
            reasoning.to_string(),
            "completed",
        ));
    }
    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in tool_calls {
            if let Some(item) = openai_chat_tool_call_to_response_function_call(call) {
                output.push(item);
            }
        }
    }
    let mut content = message
        .get("content")
        .map(text_field_content_to_text)
        .unwrap_or_default();
    if let Some(parsed) = split_leading_reasoning_markup(&content) {
        if structured_reasoning.is_none() && !parsed.reasoning.is_empty() {
            output.insert(
                0,
                openai_response_reasoning_item(
                    format!("rs_{response_id}"),
                    parsed.reasoning,
                    "completed",
                ),
            );
        }
        content = parsed.content;
    }
    if !content.is_empty() || output.is_empty() {
        output.push(openai_response_message_item(
            format!("msg_{response_id}_{}", output.len()),
            content,
            "completed",
        ));
    }
    output
}

fn openai_chat_tool_call_to_response_function_call(call: &Value) -> Option<Value> {
    let id = call
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("call_chat");
    let function = call.get("function")?;
    let name = function.get("name").and_then(Value::as_str)?;
    let arguments = function
        .get("arguments")
        .and_then(Value::as_str)
        .unwrap_or("{}");
    Some(json!({
        "id": id,
        "type": "function_call",
        "status": "completed",
        "call_id": id,
        "name": name,
        "arguments": arguments,
    }))
}

fn openai_chat_usage_tokens(usage: Option<&Value>, field: &str) -> i64 {
    // 部分 OpenAI 兼容上游用 input_tokens/output_tokens 别名，需回退匹配，
    // 否则 token 记为 0 会退化为字节估算，与 chat→anthropic 路径口径不一致。
    let alias = match field {
        "prompt_tokens" => Some("input_tokens"),
        "completion_tokens" => Some("output_tokens"),
        _ => None,
    };
    usage
        .and_then(|usage| {
            usage
                .get(field)
                .or_else(|| alias.and_then(|alias| usage.get(alias)))
        })
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

pub(super) struct OpenAiChatSseToOpenAiResponse {
    buffer: Vec<u8>,
    model: String,
    response_id: String,
    output_item_id: String,
    completed_output: Vec<Value>,
    current_tool_calls: Vec<Option<StreamingToolCall>>,
    sequence_number: i64,
    response_started: bool,
    output_started: bool,
    content_started: bool,
    reasoning_started: bool,
    reasoning_finished: bool,
    stopped: bool,
    message_finished: bool,
    text: String,
    reasoning_text: String,
    leading_thinking_markup: LeadingReasoningMarkupParser,
    warning_context: Option<ReasoningMarkupWarningContext>,
    input_tokens: i64,
    output_tokens: i64,
    cached_input_tokens: i64,
    status: &'static str,
}

impl OpenAiChatSseToOpenAiResponse {
    #[cfg(test)]
    pub(super) fn new(model: String) -> Self {
        Self::new_inner(model, None)
    }

    fn new_with_warning_context(
        model: String,
        warning_context: ReasoningMarkupWarningContext,
    ) -> Self {
        Self::new_inner(model, Some(warning_context))
    }

    fn new_inner(model: String, warning_context: Option<ReasoningMarkupWarningContext>) -> Self {
        Self {
            buffer: Vec::new(),
            model,
            response_id: "resp_chat_fallback".to_string(),
            output_item_id: "msg_chat_fallback".to_string(),
            completed_output: Vec::new(),
            current_tool_calls: Vec::new(),
            sequence_number: 0,
            response_started: false,
            output_started: false,
            content_started: false,
            reasoning_started: false,
            reasoning_finished: false,
            stopped: false,
            message_finished: false,
            text: String::new(),
            reasoning_text: String::new(),
            leading_thinking_markup: LeadingReasoningMarkupParser::default(),
            warning_context,
            input_tokens: 0,
            output_tokens: 0,
            cached_input_tokens: 0,
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
        self.observe_chunk(&value, out);
    }

    fn observe_chunk(&mut self, value: &Value, out: &mut Vec<u8>) {
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            self.response_id = format!("resp_{id}");
            self.output_item_id = format!("msg_{id}");
        }
        if let Some(model) = value.get("model").and_then(Value::as_str) {
            self.model = model.to_string();
        }
        self.observe_usage(value);
        let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        else {
            return;
        };
        // Treat finish_reason "length" as completed instead of incomplete.
        // Emitting response.incomplete here makes Codex CLI abort with
        // "stream disconnected before completion", so always complete the stream.
        // "length" may still carry a final usage chunk below.
        if matches!(
            choice.get("finish_reason").and_then(Value::as_str),
            Some("tool_calls" | "function_call")
        ) {
            self.status = "completed";
        }
        let Some(delta) = choice.get("delta") else {
            return;
        };
        if let Some(reasoning) = reasoning_content(delta) {
            self.push_reasoning_delta(reasoning, out);
        }
        if let Some(content) = delta
            .get("content")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            let parsed = self.leading_thinking_markup.push(content);
            if parsed.detected {
                self.warn_reasoning_markup();
            }
            if let Some(reasoning) = parsed.reasoning {
                if !self.reasoning_started {
                    self.push_reasoning_delta(&reasoning, out);
                }
            }
            if let Some(content) = parsed.content {
                self.push_content_delta(&content, out);
            }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            self.flush_pending_content(out);
            for tool_call in tool_calls {
                self.push_tool_call_delta(tool_call, out);
            }
        }
    }

    fn observe_usage(&mut self, value: &Value) {
        let Some(usage) = value.get("usage") else {
            if let Some(cached_tokens) = choice_usage_cached_tokens(value) {
                self.cached_input_tokens = cached_tokens;
            }
            return;
        };
        self.input_tokens = usage
            .get("prompt_tokens")
            .or_else(|| usage.get("input_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(self.input_tokens);
        self.output_tokens = usage
            .get("completion_tokens")
            .or_else(|| usage.get("output_tokens"))
            .and_then(Value::as_i64)
            .unwrap_or(self.output_tokens);
        self.cached_input_tokens = usage
            .get("prompt_tokens_details")
            .and_then(|details| details.get("cached_tokens"))
            .or_else(|| {
                usage
                    .get("input_tokens_details")
                    .and_then(|details| details.get("cached_tokens"))
            })
            .or_else(|| usage.get("prompt_cache_hit_tokens"))
            .or_else(|| usage.get("cached_tokens"))
            .and_then(Value::as_i64)
            .or_else(|| choice_usage_cached_tokens(value))
            .unwrap_or(self.cached_input_tokens);
    }

    fn warn_reasoning_markup(&self) {
        if let Some(context) = &self.warning_context {
            context.warn();
        }
    }

    fn flush_pending_content(&mut self, out: &mut Vec<u8>) {
        let parsed = self.leading_thinking_markup.finish();
        if parsed.detected {
            self.warn_reasoning_markup();
        }
        if let Some(reasoning) = parsed.reasoning {
            if !self.reasoning_started {
                self.push_reasoning_delta(&reasoning, out);
            }
        }
        if let Some(content) = parsed.content {
            self.push_content_delta(&content, out);
        }
    }

    fn push_reasoning_delta(&mut self, reasoning: &str, out: &mut Vec<u8>) {
        self.ensure_response_started(out);
        if !self.reasoning_started {
            self.reasoning_started = true;
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "sequence_number": sequence_number,
                    "output_index": 0,
                    "item": {
                        "id": self.reasoning_item_id(),
                        "type": "reasoning",
                        "status": "in_progress",
                        "summary": [],
                    },
                }),
            );
            let sequence_number = self.next_sequence_number();
            self.push_event(
                out,
                "response.reasoning_summary_part.added",
                json!({
                    "type": "response.reasoning_summary_part.added",
                    "sequence_number": sequence_number,
                    "item_id": self.reasoning_item_id(),
                    "output_index": 0,
                    "summary_index": 0,
                    "part": { "type": "summary_text", "text": "" },
                }),
            );
        }
        self.reasoning_text.push_str(reasoning);
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.reasoning_summary_text.delta",
            json!({
                "type": "response.reasoning_summary_text.delta",
                "sequence_number": sequence_number,
                "item_id": self.reasoning_item_id(),
                "output_index": 0,
                "summary_index": 0,
                "delta": reasoning,
            }),
        );
    }

    fn push_content_delta(&mut self, content: &str, out: &mut Vec<u8>) {
        if self.reasoning_started && !self.reasoning_finished {
            self.finish_reasoning(out);
        }
        if self.current_tool_calls.iter().any(Option::is_some) {
            self.finish_tool_calls(out);
        }
        self.ensure_content_started(out);
        self.text.push_str(content);
        self.output_tokens = self.output_tokens.saturating_add(estimate_tokens(content));
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_text.delta",
            json!({
                "type": "response.output_text.delta",
                "sequence_number": sequence_number,
                "item_id": self.output_item_id,
                "output_index": self.message_output_index(),
                "content_index": 0,
                "delta": content,
            }),
        );
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
                    "output_index": self.message_output_index(),
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
                    "output_index": self.message_output_index(),
                    "content_index": 0,
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

    fn finish_reasoning(&mut self, out: &mut Vec<u8>) {
        if !self.reasoning_started || self.reasoning_finished {
            return;
        }
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
                "output_index": 0,
                "summary_index": 0,
                "text": self.reasoning_text,
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
                "item": openai_response_reasoning_item(item_id, self.reasoning_text.clone(), "completed"),
            }),
        );
    }

    fn push_tool_call_delta(&mut self, tool_call: &Value, out: &mut Vec<u8>) {
        // 文本与工具调用可在同一 assistant 回合共存：若文本已开始，先收尾 message item，
        // 再开 function_call item（对称于 push_content_delta 先 finish_tool_calls 的处理）。
        // 此前这里直接 return，导致 qwen「先输出文本、再输出工具调用」的工具调用被丢弃，
        // Codex 只收到文本、无 function_call，于是结束 agentic loop。
        self.finish_message_item(out);
        if self.reasoning_started && !self.reasoning_finished {
            self.finish_reasoning(out);
        }
        self.ensure_response_started(out);

        let index = tool_call
            .get("index")
            .and_then(Value::as_u64)
            .map_or(0, |value| value as usize);
        if self.current_tool_calls.len() <= index {
            self.current_tool_calls.resize_with(index + 1, || None);
        }

        let function = tool_call.get("function");
        let name = function
            .and_then(|function| function.get("name"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let id = tool_call
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());

        if self.current_tool_calls[index].is_none() {
            let fallback_id = format!("call_chat_fallback_{index}");
            let call_id = id.unwrap_or(&fallback_id).to_string();
            let tool_name = name.unwrap_or("tool").to_string();
            let output_index = self.next_output_index_for_new_item();
            let item_id = format!("fc_{call_id}");
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
                        "call_id": call_id,
                        "name": tool_name,
                        "arguments": "",
                    },
                }),
            );
            self.current_tool_calls[index] = Some(StreamingToolCall {
                output_index,
                item_id,
                call_id,
                name: tool_name,
                arguments: String::new(),
            });
        }

        let Some(current) = self.current_tool_calls[index].as_mut() else {
            return;
        };
        if let Some(id) = id {
            current.call_id = id.to_string();
        }
        if let Some(name) = name {
            current.name = name.to_string();
        }
        let arguments_delta = function
            .and_then(|function| function.get("arguments"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if arguments_delta.is_empty() {
            return;
        }
        current.arguments.push_str(arguments_delta);
        let item_id = current.item_id.clone();
        let output_index = current.output_index;
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "sequence_number": sequence_number,
                "item_id": item_id,
                "output_index": output_index,
                "delta": arguments_delta,
            }),
        );
    }

    fn finish_tool_calls(&mut self, out: &mut Vec<u8>) {
        let item_status = "completed";
        for index in 0..self.current_tool_calls.len() {
            let Some(tool_call) = self.current_tool_calls[index].take() else {
                continue;
            };
            let done_item = json!({
                "id": tool_call.item_id,
                "type": "function_call",
                "status": item_status,
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
    }

    fn finish_message_item(&mut self, out: &mut Vec<u8>) {
        if !self.output_started || self.message_finished {
            return;
        }
        self.message_finished = true;
        let sequence_number = self.next_sequence_number();
        self.push_event(
            out,
            "response.output_text.done",
            json!({
                "type": "response.output_text.done",
                "sequence_number": sequence_number,
                "item_id": self.output_item_id,
                "output_index": self.message_output_index(),
                "content_index": 0,
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
                "output_index": self.message_output_index(),
                "content_index": 0,
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
                "output_index": self.message_output_index(),
                "item": self.output_item_payload("completed"),
            }),
        );
        self.completed_output
            .push(self.output_item_payload("completed"));
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if self.stopped {
            return;
        }
        self.flush_pending_content(out);
        if self.current_tool_calls.iter().any(Option::is_some) {
            self.finish_tool_calls(out);
        }
        if self.reasoning_started && !self.reasoning_finished {
            self.finish_reasoning(out);
        }
        if !self.output_started && self.text.is_empty() && self.completed_output.is_empty() {
            self.ensure_content_started(out);
        }
        self.stopped = true;
        self.finish_message_item(out);
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
        let mut output = Vec::new();
        if status != "in_progress" {
            if self.reasoning_started {
                output.push(openai_response_reasoning_item(
                    self.reasoning_item_id(),
                    self.reasoning_text.clone(),
                    "completed",
                ));
            }
            output.extend(self.completed_output.clone());
        }
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
                })),
                self.input_tokens,
                self.output_tokens,
            ),
        })
    }

    fn output_item_payload(&self, status: &str) -> Value {
        openai_response_message_item(self.output_item_id.clone(), self.text.clone(), status)
    }

    fn message_output_index(&self) -> i64 {
        if self.reasoning_started {
            1
        } else {
            0
        }
    }

    fn reasoning_item_id(&self) -> String {
        format!("rs_{}", self.output_item_id)
    }

    fn next_output_index_for_new_item(&self) -> i64 {
        let mut index: i64 = 0;
        if self.reasoning_started {
            index = index.saturating_add(1);
        }
        if self.output_started {
            index = index.saturating_add(1);
        }
        index.saturating_add(
            self.current_tool_calls
                .iter()
                .filter(|tool_call| tool_call.is_some())
                .count() as i64,
        )
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

fn reasoning_content(value: &Value) -> Option<&str> {
    value
        .get("reasoning_content")
        .or_else(|| value.get("reasoning"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}

impl BridgeSseConverter for OpenAiChatSseToOpenAiResponse {
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

    #[test]
    fn chat_response_stream_finishes_incomplete_on_eof_without_done() {
        let mut converter = OpenAiChatSseToOpenAiResponse::new("NEO-GLM".to_string());

        converter.push(
            br#"data: {"id":"chatcmpl_1","model":"GLM-5.2","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}

"#,
        );
        converter.push(
            br#"data: {"id":"chatcmpl_1","model":"GLM-5.2","choices":[{"index":0,"delta":{},"finish_reason":"length"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}

"#,
        );

        let mut output = Vec::new();
        <OpenAiChatSseToOpenAiResponse as BridgeSseConverter>::finish(&mut converter, &mut output);
        let output = std::str::from_utf8(&output).unwrap();

        assert!(output.contains("event: response.completed"));
        assert!(output.contains(r#""status":"completed""#));
        assert!(!output.contains(r#""reason":"max_output_tokens""#));
        assert!(converter.stopped());
    }

    #[test]
    fn leading_thinking_markup_parser_handles_case_and_stream_boundaries() {
        let mut parser = LeadingReasoningMarkupParser::default();

        let first = parser.push("<THINK");
        assert!(first.reasoning.is_none());
        assert!(first.content.is_none());

        let second = parser.push("ING>internal");
        assert!(second.reasoning.is_none());
        assert!(second.content.is_none());

        let third = parser.push(" note</thinking>\nanswer");
        assert_eq!(third.reasoning.as_deref(), Some("internal note"));
        assert_eq!(third.content.as_deref(), Some("answer"));
        assert!(third.detected);
    }

    #[test]
    fn leading_thinking_markup_parser_preserves_normal_content_and_hides_unclosed_markup() {
        let mut normal = LeadingReasoningMarkupParser::default();
        let parsed = normal.push("ordinary <thinking>example</thinking>");
        assert_eq!(
            parsed.content.as_deref(),
            Some("ordinary <thinking>example</thinking>")
        );
        assert!(parsed.reasoning.is_none());

        let mut unclosed = LeadingReasoningMarkupParser::default();
        assert!(unclosed.push("<thinking>unfinished").content.is_none());
        let chunk = unclosed.finish();
        assert_eq!(chunk.reasoning.as_deref(), Some("unfinished"));
        assert_eq!(chunk.content, None);
        assert!(chunk.detected);
    }

    #[test]
    fn nonstream_leading_thinking_markup_becomes_reasoning() {
        let body = br#"{"id":"chatcmpl-1","model":"gpt-test","choices":[{"message":{"role":"assistant","content":"<thinking>**Inspecting**</thinking>\nVisible update"},"finish_reason":"stop"}]}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["output"][0]["type"], "reasoning");
        assert_eq!(value["output"][0]["summary"][0]["text"], "**Inspecting**");
        assert_eq!(value["output"][1]["content"][0]["text"], "Visible update");
    }

    #[test]
    fn nonstream_structured_reasoning_deduplicates_thinking_markup() {
        let body = br#"{"id":"chatcmpl-1","choices":[{"message":{"reasoning_content":"Structured reasoning","content":"<thinking>Duplicate reasoning</thinking>\nAnswer"}}]}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["output"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["output"][0]["summary"][0]["text"],
            "Structured reasoning"
        );
        assert_eq!(value["output"][1]["content"][0]["text"], "Answer");
    }

    #[test]
    fn nonstream_reasoning_alias_is_structured_reasoning() {
        let body = br#"{"id":"chatcmpl-1","choices":[{"message":{"reasoning":"Structured reasoning","content":"<think>Duplicate reasoning</think>\nAnswer"}}]}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["output"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["output"][0]["summary"][0]["text"],
            "Structured reasoning"
        );
        assert_eq!(value["output"][1]["content"][0]["text"], "Answer");
    }

    #[test]
    fn nonstream_preserves_openai_cached_tokens() {
        // chat 格式的缓存 token 在 prompt_tokens_details.cached_tokens 下，
        // 转换后必须映射到 input_tokens_details.cached_tokens，否则缓存输入被按全价计费。
        let body = br#"{"id":"chatcmpl-1","choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":1000,"completion_tokens":20,"prompt_tokens_details":{"cached_tokens":800}}}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["usage"]["input_tokens"], 1000);
        assert_eq!(value["usage"]["output_tokens"], 20);
        assert_eq!(value["usage"]["input_tokens_details"]["cached_tokens"], 800);
    }

    #[test]
    fn nonstream_supports_input_output_token_aliases() {
        // 部分兼容上游只给 input_tokens/output_tokens 别名，需正确读取而非归零。
        let body = br#"{"id":"chatcmpl-1","choices":[{"message":{"content":"hi"}}],"usage":{"input_tokens":300,"output_tokens":40}}"#;
        let converted = openai_chat_response_to_openai_response(body, "fallback").unwrap();
        let value: Value = serde_json::from_slice(&converted).unwrap();

        assert_eq!(value["usage"]["input_tokens"], 300);
        assert_eq!(value["usage"]["output_tokens"], 40);
    }

    #[test]
    fn streaming_leading_thinking_markup_uses_reasoning_events() {
        let mut converter = OpenAiChatSseToOpenAiResponse::new("gpt-test".to_string());
        let mut output = Vec::new();
        output.extend_from_slice(&converter.push(
            br#"data: {"choices":[{"delta":{"content":"<think"}}]}

"#,
        ));
        output.extend_from_slice(&converter.push(
            br#"data: {"choices":[{"delta":{"content":"ing>Private</thinking>\nPublic"}}]}

"#,
        ));
        <OpenAiChatSseToOpenAiResponse as BridgeSseConverter>::finish(&mut converter, &mut output);
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("response.reasoning_summary_text.delta"));
        assert!(output.contains(r#""delta":"Private""#));
        assert!(output.contains(r#""delta":"Public""#));
        assert!(!output.contains("<thinking>"));
        assert!(!output.contains("</thinking>"));
    }

    #[test]
    fn streaming_unclosed_thinking_markup_uses_reasoning_events() {
        let mut converter = OpenAiChatSseToOpenAiResponse::new("gpt-test".to_string());
        let mut output = converter
            .push(
                br#"data: {"choices":[{"delta":{"content":"<thinking>Private"}}]}

"#,
            )
            .to_vec();
        output.extend_from_slice(&converter.push(b"data: [DONE]\n\n"));
        let output = String::from_utf8(output).unwrap();

        assert!(output.contains("response.reasoning_summary_text.delta"));
        assert!(output.contains(r#""delta":"Private""#));
        assert!(!output.contains("<thinking>"));
    }
}
