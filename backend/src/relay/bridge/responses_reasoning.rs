use axum::{http::StatusCode, response::Response};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{error::AppResult, relay::RelayContext};

use super::{
    reasoning_markup::{split_leading_reasoning_markup, LeadingReasoningMarkupParser},
    responses_common::openai_response_reasoning_item,
    stream::{finish_bridge_json, finish_bridge_stream, BridgeSseConverter},
};

const MAX_PENDING_SSE_BYTES: usize = 128 * 1024;

pub(crate) async fn finish_openai_response_with_reasoning_normalization(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
) -> AppResult<Response> {
    let log_context = ReasoningMarkupLogContext::from(&ctx);
    if ctx.streamed {
        let rewrite_model = ctx.external_model != ctx.model;
        return finish_bridge_stream(
            ctx,
            status,
            upstream_response,
            move |model| NativeResponsesSseNormalizer::new(model, rewrite_model, Some(log_context)),
            "ignored trailing upstream body read error after completed normalized Responses stream",
        );
    }
    finish_bridge_json(
        ctx,
        status,
        upstream_response,
        move |body, external_model| {
            normalize_response_json(body, external_model, Some(&log_context))
        },
        "ignored trailing upstream body read error after parsing complete normalized Responses response",
    )
    .await
}

#[derive(Clone)]
struct ReasoningMarkupLogContext {
    relay_trace_id: uuid::Uuid,
    provider: String,
    channel_id: crate::id::DbId,
    channel_name: String,
    channel_endpoint_id: crate::id::DbId,
    protocol: &'static str,
    model: String,
    upstream_path: String,
}

impl From<&RelayContext> for ReasoningMarkupLogContext {
    fn from(ctx: &RelayContext) -> Self {
        Self {
            relay_trace_id: ctx.relay_trace_id,
            provider: ctx.upstream.provider.clone(),
            channel_id: ctx.upstream.channel_id,
            channel_name: ctx.upstream.channel_name.clone(),
            channel_endpoint_id: ctx.upstream.channel_endpoint_id,
            protocol: ctx.protocol.as_str(),
            model: ctx.model.clone(),
            upstream_path: ctx
                .upstream_request_path
                .clone()
                .unwrap_or_else(|| ctx.path.to_string()),
        }
    }
}

impl ReasoningMarkupLogContext {
    fn normalized(&self, tag: &str, streamed: bool, structured_reasoning_present: bool) {
        tracing::warn!(
            relay_trace_id = %self.relay_trace_id,
            provider = %self.provider,
            channel_id = self.channel_id,
            channel_name = %self.channel_name,
            channel_endpoint_id = self.channel_endpoint_id,
            protocol = self.protocol,
            model = %self.model,
            upstream_path = %self.upstream_path,
            source_protocol = "openai_responses",
            tag,
            streamed,
            structured_reasoning_present,
            "normalized reasoning markup in assistant content"
        );
    }
}

#[derive(Debug, Clone)]
struct NormalizedMarkup {
    tag: &'static str,
    structured_reasoning_present: bool,
}

fn normalize_response_json(
    body: &[u8],
    external_model: &str,
    log_context: Option<&ReasoningMarkupLogContext>,
) -> AppResult<Bytes> {
    let mut value: Value = match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(_) => return Ok(Bytes::copy_from_slice(body)),
    };
    let normalized = normalize_response_value(&mut value);
    if let Some(object) = value.as_object_mut() {
        if object.contains_key("model") {
            object.insert(
                "model".to_string(),
                Value::String(external_model.to_string()),
            );
        }
    }
    if let Some(normalized) = normalized.as_ref() {
        if let Some(context) = log_context {
            context.normalized(
                normalized.tag,
                false,
                normalized.structured_reasoning_present,
            );
        }
    }
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn normalize_response_value(value: &mut Value) -> Option<NormalizedMarkup> {
    let output = value.get_mut("output")?.as_array_mut()?;
    let structured_reasoning_present = output
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("reasoning"));

    for message_index in 0..output.len() {
        let item = &mut output[message_index];
        if item.get("type").and_then(Value::as_str) != Some("message")
            || item.get("role").and_then(Value::as_str) != Some("assistant")
        {
            continue;
        }
        let message_id = item
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("message")
            .to_string();
        let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        let Some(text_part) = content
            .iter_mut()
            .find(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
        else {
            continue;
        };
        let text = text_part.get("text").and_then(Value::as_str)?.to_string();
        let parsed = split_leading_reasoning_markup(&text)?;
        text_part["text"] = Value::String(parsed.content.clone());
        let inserted_reasoning = !structured_reasoning_present && !parsed.reasoning.is_empty();
        let normalized = NormalizedMarkup {
            tag: parsed.tag,
            structured_reasoning_present,
        };
        if inserted_reasoning {
            output.insert(
                message_index,
                openai_response_reasoning_item(
                    format!("rs_markup_{message_id}"),
                    parsed.reasoning,
                    "completed",
                ),
            );
        }
        return Some(normalized);
    }
    None
}

#[derive(Debug)]
struct SseFrame {
    raw: Vec<u8>,
    value: Option<Value>,
}

impl SseFrame {
    fn event_type(&self) -> Option<&str> {
        self.value
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
    }
}

#[derive(Debug)]
struct ActiveNormalization {
    reasoning: String,
    visible_content: String,
    message_id: String,
    insertion_index: i64,
    content_index: i64,
    shift_indexes: bool,
}

#[derive(Debug)]
enum StreamMode {
    Undecided,
    Passthrough,
    Normalizing(ActiveNormalization),
}

struct NativeResponsesSseNormalizer {
    buffer: Vec<u8>,
    pending: Vec<SseFrame>,
    pending_bytes: usize,
    parser: LeadingReasoningMarkupParser,
    mode: StreamMode,
    structured_reasoning_seen: bool,
    external_model: String,
    rewrite_model: bool,
    next_sequence_number: i64,
    stopped: bool,
    log_context: Option<ReasoningMarkupLogContext>,
}

impl NativeResponsesSseNormalizer {
    fn new(
        external_model: String,
        rewrite_model: bool,
        log_context: Option<ReasoningMarkupLogContext>,
    ) -> Self {
        Self {
            buffer: Vec::new(),
            pending: Vec::new(),
            pending_bytes: 0,
            parser: LeadingReasoningMarkupParser::default(),
            mode: StreamMode::Undecided,
            structured_reasoning_seen: false,
            external_model,
            rewrite_model,
            next_sequence_number: 0,
            stopped: false,
            log_context,
        }
    }

    fn push(&mut self, chunk: &[u8]) -> Bytes {
        self.buffer.extend_from_slice(chunk);
        let mut out = Vec::new();
        for raw in drain_sse_frames(&mut self.buffer) {
            self.push_frame(parse_sse_frame(raw), &mut out);
        }
        Bytes::from(out)
    }

    fn push_frame(&mut self, frame: SseFrame, out: &mut Vec<u8>) {
        let terminal = is_terminal_event(frame.event_type());
        self.structured_reasoning_seen |= frame
            .event_type()
            .is_some_and(|event| event.starts_with("response.reasoning_"))
            || frame.value.as_ref().is_some_and(|value| {
                value
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("reasoning")
            });
        if terminal {
            self.stopped = true;
        }
        match &mut self.mode {
            StreamMode::Passthrough => self.push_passthrough_frame(frame, out),
            StreamMode::Normalizing(_) => self.push_normalized_frame(frame, out),
            StreamMode::Undecided => {
                let starts_message = frame.value.as_ref().is_some_and(|value| {
                    value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
                        && value
                            .get("item")
                            .and_then(|item| item.get("type"))
                            .and_then(Value::as_str)
                            == Some("message")
                });
                let delta = frame
                    .value
                    .as_ref()
                    .filter(|value| {
                        value.get("type").and_then(Value::as_str)
                            == Some("response.output_text.delta")
                    })
                    .and_then(|value| value.get("delta"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if self.pending.is_empty() && !starts_message && delta.is_none() {
                    self.observe_passthrough_sequence(&frame);
                    self.push_passthrough_frame(frame, out);
                    return;
                }
                self.pending_bytes = self.pending_bytes.saturating_add(frame.raw.len());
                self.pending.push(frame);
                if self.pending_bytes > MAX_PENDING_SSE_BYTES {
                    self.mode = StreamMode::Passthrough;
                    self.flush_pending_raw(out);
                    return;
                }
                let Some(delta) = delta else {
                    if terminal {
                        let parsed = self.parser.finish();
                        if parsed.detected {
                            self.begin_normalization(
                                parsed.reasoning.unwrap_or_default(),
                                parsed.content,
                                parsed.tag.unwrap_or("<thinking>"),
                                out,
                            );
                        } else {
                            self.mode = StreamMode::Passthrough;
                            self.flush_pending_raw(out);
                        }
                    }
                    return;
                };
                let parsed = self.parser.push(&delta);
                if parsed.detected {
                    self.begin_normalization(
                        parsed.reasoning.unwrap_or_default(),
                        parsed.content,
                        parsed.tag.unwrap_or("<thinking>"),
                        out,
                    );
                } else if parsed.content.is_some() {
                    self.mode = StreamMode::Passthrough;
                    self.flush_pending_raw(out);
                }
            }
        }
    }

    fn begin_normalization(
        &mut self,
        reasoning: String,
        content: Option<String>,
        tag: &'static str,
        out: &mut Vec<u8>,
    ) {
        let structured_reasoning_present = self.structured_reasoning_seen;
        let message_frame = self.pending.iter().find_map(|frame| {
            let value = frame.value.as_ref()?;
            if value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
                && value
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("message")
            {
                Some(value)
            } else {
                None
            }
        });
        let insertion_index = message_frame
            .and_then(|value| value.get("output_index"))
            .and_then(Value::as_i64)
            .or_else(|| {
                self.pending
                    .iter()
                    .find_map(|frame| frame.value.as_ref()?.get("output_index")?.as_i64())
            })
            .unwrap_or(0);
        let message_id = message_frame
            .and_then(|value| value.get("item"))
            .and_then(|item| item.get("id"))
            .and_then(Value::as_str)
            .or_else(|| {
                self.pending
                    .iter()
                    .find_map(|frame| frame.value.as_ref()?.get("item_id")?.as_str())
            })
            .unwrap_or("message")
            .to_string();
        let content_index = self
            .pending
            .iter()
            .find_map(|frame| {
                let value = frame.value.as_ref()?;
                (value.get("type").and_then(Value::as_str) == Some("response.output_text.delta"))
                    .then(|| value.get("content_index").and_then(Value::as_i64))
                    .flatten()
            })
            .unwrap_or(0);
        let shift_indexes = !structured_reasoning_present && !reasoning.is_empty();
        let active = ActiveNormalization {
            reasoning,
            visible_content: content.unwrap_or_default(),
            message_id: message_id.clone(),
            insertion_index,
            content_index,
            shift_indexes,
        };
        if let Some(context) = &self.log_context {
            context.normalized(tag, true, structured_reasoning_present);
        }
        self.mode = StreamMode::Normalizing(active);

        let pending = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        let mut inserted = false;
        for frame in pending {
            let is_target_message_start = frame.value.as_ref().is_some_and(|value| {
                value.get("type").and_then(Value::as_str) == Some("response.output_item.added")
                    && value.get("output_index").and_then(Value::as_i64) == Some(insertion_index)
                    && value
                        .get("item")
                        .and_then(|item| item.get("type"))
                        .and_then(Value::as_str)
                        == Some("message")
            });
            if is_target_message_start && shift_indexes && !inserted {
                self.push_reasoning_lifecycle(out);
                inserted = true;
            }
            let is_target_delta = frame.value.as_ref().is_some_and(|value| {
                value.get("type").and_then(Value::as_str) == Some("response.output_text.delta")
                    && (value.get("item_id").and_then(Value::as_str) == Some(&message_id)
                        || value.get("output_index").and_then(Value::as_i64)
                            == Some(insertion_index))
            });
            if is_target_delta {
                continue;
            }
            self.push_normalized_frame(frame, out);
        }
        if shift_indexes && !inserted {
            self.push_reasoning_lifecycle(out);
        }
        let visible = match &self.mode {
            StreamMode::Normalizing(active) => active.visible_content.clone(),
            _ => String::new(),
        };
        if !visible.is_empty() {
            let (message_id, output_index, content_index) = match &self.mode {
                StreamMode::Normalizing(active) => (
                    active.message_id.clone(),
                    active.insertion_index + i64::from(active.shift_indexes),
                    active.content_index,
                ),
                _ => return,
            };
            self.push_event(
                out,
                json!({
                    "type": "response.output_text.delta",
                    "item_id": message_id,
                    "output_index": output_index,
                    "content_index": content_index,
                    "delta": visible,
                }),
            );
        }
    }

    fn push_reasoning_lifecycle(&mut self, out: &mut Vec<u8>) {
        let StreamMode::Normalizing(active) = &self.mode else {
            return;
        };
        let reasoning = active.reasoning.clone();
        let item_id = format!("rs_markup_{}", active.message_id);
        let output_index = active.insertion_index;
        let events = [
            json!({"type":"response.output_item.added","output_index":output_index,"item":{"id":item_id,"type":"reasoning","status":"in_progress","summary":[]}}),
            json!({"type":"response.reasoning_summary_part.added","item_id":item_id,"output_index":output_index,"summary_index":0,"part":{"type":"summary_text","text":""}}),
            json!({"type":"response.reasoning_summary_text.delta","item_id":item_id,"output_index":output_index,"summary_index":0,"delta":reasoning}),
            json!({"type":"response.reasoning_summary_text.done","item_id":item_id,"output_index":output_index,"summary_index":0,"text":reasoning}),
            json!({"type":"response.reasoning_summary_part.done","item_id":item_id,"output_index":output_index,"summary_index":0,"part":{"type":"summary_text","text":reasoning}}),
            json!({"type":"response.output_item.done","output_index":output_index,"item":openai_response_reasoning_item(item_id, reasoning, "completed")}),
        ];
        for event in events {
            self.push_event(out, event);
        }
    }

    fn push_normalized_frame(&mut self, mut frame: SseFrame, out: &mut Vec<u8>) {
        let Some(mut value) = frame.value.take() else {
            out.extend_from_slice(&frame.raw);
            return;
        };
        let Some((message_id, insertion_index, shift_indexes, visible_content, reasoning)) =
            (match &self.mode {
                StreamMode::Normalizing(active) => Some((
                    active.message_id.clone(),
                    active.insertion_index,
                    active.shift_indexes,
                    active.visible_content.clone(),
                    active.reasoning.clone(),
                )),
                _ => None,
            })
        else {
            out.extend_from_slice(&frame.raw);
            return;
        };
        if shift_indexes {
            shift_output_index(&mut value, insertion_index);
        }
        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .map(str::to_string);
        let message_output_index = insertion_index + i64::from(shift_indexes);
        let targets_message = value.get("item_id").and_then(Value::as_str) == Some(&message_id)
            || value
                .get("item")
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                == Some(&message_id)
            || value.get("output_index").and_then(Value::as_i64) == Some(message_output_index);
        match event_type.as_deref() {
            Some("response.output_text.delta") if targets_message => {
                if let Some(delta) = value.get("delta").and_then(Value::as_str) {
                    if let StreamMode::Normalizing(active) = &mut self.mode {
                        active.visible_content.push_str(delta);
                    }
                }
            }
            Some("response.output_text.done") if targets_message => {
                value["text"] = Value::String(visible_content.clone());
            }
            Some("response.content_part.done")
                if targets_message && value.get("part").is_some() =>
            {
                value["part"]["text"] = Value::String(visible_content.clone());
            }
            Some("response.output_item.done") if targets_message => {
                replace_item_output_text(&mut value["item"], &visible_content);
            }
            Some(
                "response.completed"
                | "response.done"
                | "response.incomplete"
                | "response.failed"
                | "response.cancelled"
                | "response.canceled",
            ) => {
                if let Some(response) = value.get_mut("response") {
                    normalize_terminal_response(
                        response,
                        &message_id,
                        insertion_index,
                        shift_indexes,
                        &reasoning,
                        &visible_content,
                    );
                }
            }
            _ => {}
        }
        self.push_event(out, value);
    }

    fn push_event(&mut self, out: &mut Vec<u8>, mut value: Value) {
        if self.rewrite_model {
            rewrite_response_model_in_event(&mut value, &self.external_model);
        }
        value["sequence_number"] = Value::from(self.next_sequence_number);
        self.next_sequence_number = self.next_sequence_number.saturating_add(1);
        let event = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message")
            .to_string();
        out.extend_from_slice(b"event: ");
        out.extend_from_slice(event.as_bytes());
        out.extend_from_slice(b"\ndata: ");
        serde_json::to_writer(&mut *out, &value)
            .expect("serializing JSON value to Vec cannot fail");
        out.extend_from_slice(b"\n\n");
    }

    fn flush_pending_raw(&mut self, out: &mut Vec<u8>) {
        self.pending_bytes = 0;
        for frame in std::mem::take(&mut self.pending) {
            self.push_passthrough_frame(frame, out);
        }
    }

    fn observe_passthrough_sequence(&mut self, frame: &SseFrame) {
        if let Some(sequence_number) = frame
            .value
            .as_ref()
            .and_then(|value| value.get("sequence_number"))
            .and_then(Value::as_i64)
        {
            self.next_sequence_number = self
                .next_sequence_number
                .max(sequence_number.saturating_add(1));
        }
    }

    fn push_passthrough_frame(&self, mut frame: SseFrame, out: &mut Vec<u8>) {
        if !self.rewrite_model {
            out.extend_from_slice(&frame.raw);
            return;
        }
        let Some(mut value) = frame.value.take() else {
            out.extend_from_slice(&frame.raw);
            return;
        };
        rewrite_response_model_in_event(&mut value, &self.external_model);
        let event = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("message");
        out.extend_from_slice(b"event: ");
        out.extend_from_slice(event.as_bytes());
        out.extend_from_slice(b"\ndata: ");
        serde_json::to_writer(&mut *out, &value)
            .expect("serializing JSON value to Vec cannot fail");
        out.extend_from_slice(b"\n\n");
    }

    fn finish(&mut self, out: &mut Vec<u8>) {
        if matches!(self.mode, StreamMode::Undecided) {
            let parsed = self.parser.finish();
            if parsed.detected {
                self.begin_normalization(
                    parsed.reasoning.unwrap_or_default(),
                    parsed.content,
                    parsed.tag.unwrap_or("<thinking>"),
                    out,
                );
            } else {
                self.flush_pending_raw(out);
            }
        }
        if !self.buffer.is_empty() {
            out.extend_from_slice(&std::mem::take(&mut self.buffer));
        }
    }
}

impl BridgeSseConverter for NativeResponsesSseNormalizer {
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

fn replace_item_output_text(item: &mut Value, text: &str) {
    let Some(content) = item.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    if let Some(part) = content
        .iter_mut()
        .find(|part| part.get("type").and_then(Value::as_str) == Some("output_text"))
    {
        part["text"] = Value::String(text.to_string());
    }
}

fn normalize_terminal_response(
    response: &mut Value,
    message_id: &str,
    insertion_index: i64,
    insert_reasoning: bool,
    reasoning: &str,
    visible_content: &str,
) {
    if normalize_response_value(response).is_some() {
        return;
    }
    let Some(output) = response.get_mut("output").and_then(Value::as_array_mut) else {
        return;
    };
    if let Some(message) = output.iter_mut().find(|item| {
        item.get("type").and_then(Value::as_str) == Some("message")
            && item.get("id").and_then(Value::as_str) == Some(message_id)
    }) {
        replace_item_output_text(message, visible_content);
    }
    let reasoning_id = format!("rs_markup_{message_id}");
    if insert_reasoning
        && !reasoning.is_empty()
        && !output.iter().any(|item| {
            item.get("id").and_then(Value::as_str) == Some(&reasoning_id)
                || item.get("type").and_then(Value::as_str) == Some("reasoning")
        })
    {
        let index = usize::try_from(insertion_index)
            .unwrap_or_default()
            .min(output.len());
        output.insert(
            index,
            openai_response_reasoning_item(reasoning_id, reasoning.to_string(), "completed"),
        );
    }
}

fn rewrite_response_model_in_event(value: &mut Value, external_model: &str) {
    if let Some(response) = value.get_mut("response").and_then(Value::as_object_mut) {
        if response.contains_key("model") {
            response.insert(
                "model".to_string(),
                Value::String(external_model.to_string()),
            );
        }
    }
}

fn shift_output_index(value: &mut Value, insertion_index: i64) {
    let Some(index) = value.get("output_index").and_then(Value::as_i64) else {
        return;
    };
    if index >= insertion_index {
        value["output_index"] = Value::from(index.saturating_add(1));
    }
}

fn is_terminal_event(event: Option<&str>) -> bool {
    matches!(
        event,
        Some(
            "response.completed"
                | "response.done"
                | "response.failed"
                | "response.incomplete"
                | "response.cancelled"
                | "response.canceled"
        )
    )
}

fn parse_sse_frame(raw: Vec<u8>) -> SseFrame {
    let text = String::from_utf8_lossy(&raw);
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    let value = (!data.is_empty() && data != "[DONE]")
        .then(|| serde_json::from_str(&data).ok())
        .flatten();
    SseFrame { raw, value }
}

fn drain_sse_frames(buffer: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut frames = Vec::new();
    loop {
        let lf = buffer.windows(2).position(|window| window == b"\n\n");
        let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
        let (index, delimiter_len) = match (lf, crlf) {
            (Some(lf), Some(crlf)) if lf <= crlf => (lf, 2),
            (Some(_), Some(crlf)) => (crlf, 4),
            (Some(lf), None) => (lf, 2),
            (None, Some(crlf)) => (crlf, 4),
            (None, None) => break,
        };
        frames.push(buffer.drain(..index + delimiter_len).collect());
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonstream_normalizes_markup_and_deduplicates_reasoning() {
        let mut response = json!({
            "id":"resp_1",
            "output":[
                {"id":"rs_1","type":"reasoning","summary":[{"type":"summary_text","text":"structured"}]},
                {"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"\n<think>duplicate</think>\nanswer"}]}
            ]
        });
        let normalized = normalize_response_value(&mut response).unwrap();
        assert!(normalized.structured_reasoning_present);
        assert_eq!(response["output"].as_array().unwrap().len(), 2);
        assert_eq!(response["output"][1]["content"][0]["text"], "answer");
    }

    #[test]
    fn nonstream_normalizes_unclosed_markup() {
        let mut response = json!({
            "id":"resp_1",
            "output":[
                {"id":"msg_1","type":"message","role":"assistant","content":[{"type":"output_text","text":"<thinking>private plan"}]}
            ]
        });
        let normalized = normalize_response_value(&mut response).unwrap();

        assert!(!normalized.structured_reasoning_present);
        assert_eq!(response["output"][0]["type"], "reasoning");
        assert_eq!(response["output"][0]["summary"][0]["text"], "private plan");
        assert_eq!(response["output"][1]["content"][0]["text"], "");
    }

    #[test]
    fn stream_normalizes_split_markup_and_terminal_snapshot() {
        let mut normalizer = NativeResponsesSseNormalizer::new("gpt-test".to_string(), false, None);
        let input = concat!(
            "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"id\":\"resp_1\",\"output\":[]}}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":2,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"\\n <think\"}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":3,\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\">plan</think>\\nanswer\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":4,\"response\":{\"id\":\"resp_1\",\"output\":[{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"\\n <think>plan</think>\\nanswer\"}]}]}}\n\n"
        );
        let output = normalizer.push(input.as_bytes());
        let output = String::from_utf8(output.to_vec()).unwrap();
        assert!(output.contains("response.reasoning_summary_text.delta"));
        assert!(output.contains("\"delta\":\"plan\""));
        assert!(output.contains("\"delta\":\"answer\""));
        assert!(output.contains("\"output_index\":1"));
        assert!(!output.contains("<think>"));
    }

    #[test]
    fn stream_without_markup_preserves_content_and_rewrites_model() {
        let mut normalizer =
            NativeResponsesSseNormalizer::new("external-model".to_string(), true, None);
        let input = concat!(
            "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"model\":\"upstream-model\",\"output\":[]}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"delta\":\"ordinary answer\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":2,\"response\":{\"model\":\"upstream-model\",\"output\":[]}}\n\n"
        );
        let output = String::from_utf8(normalizer.push(input.as_bytes()).to_vec()).unwrap();
        assert!(output.contains("ordinary answer"));
        assert!(output.contains("external-model"));
        assert!(!output.contains("upstream-model"));
        assert!(!output.contains("response.reasoning_summary_text.delta"));
    }

    #[test]
    fn stream_deduplicates_existing_structured_reasoning() {
        let mut normalizer = NativeResponsesSseNormalizer::new("gpt-test".to_string(), false, None);
        let input = concat!(
            "event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"output\":[]}}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":1,\"output_index\":0,\"item\":{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[]}}\n\n",
            "event: response.reasoning_summary_text.delta\ndata: {\"type\":\"response.reasoning_summary_text.delta\",\"sequence_number\":2,\"item_id\":\"rs_1\",\"output_index\":0,\"delta\":\"structured\"}\n\n",
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":3,\"output_index\":1,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":4,\"item_id\":\"msg_1\",\"output_index\":1,\"delta\":\"<thinking>duplicate</thinking>\\nanswer\"}\n\n",
            "event: response.completed\ndata: {\"type\":\"response.completed\",\"sequence_number\":5,\"response\":{\"output\":[{\"id\":\"rs_1\",\"type\":\"reasoning\",\"summary\":[{\"type\":\"summary_text\",\"text\":\"structured\"}]},{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"<thinking>duplicate</thinking>\\nanswer\"}]}]}}\n\n"
        );
        let output = String::from_utf8(normalizer.push(input.as_bytes()).to_vec()).unwrap();
        assert!(output.contains("structured"));
        assert!(output.contains("\"delta\":\"answer\""));
        assert!(!output.contains("rs_markup_"));
        assert!(!output.contains("<thinking>"));
        assert!(!output.contains("\"output_index\":2"));
    }

    #[test]
    fn stream_normalizes_unclosed_markup_on_terminal_event() {
        let mut normalizer = NativeResponsesSseNormalizer::new("gpt-test".to_string(), false, None);
        let input = concat!(
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":0,\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\" \\n<THINK>unfinished\"}\n\n",
            "event: response.incomplete\ndata: {\"type\":\"response.incomplete\",\"sequence_number\":2,\"response\":{\"output\":[]}}\n\n"
        );
        let output = String::from_utf8(normalizer.push(input.as_bytes()).to_vec()).unwrap();
        assert!(!output.contains("<THINK>unfinished"));
        assert!(output.contains("response.reasoning_summary_text.delta"));
        assert!(output.contains("\"delta\":\"unfinished\""));
    }

    #[test]
    fn stream_normalizes_unclosed_markup_when_transport_ends() {
        let mut normalizer = NativeResponsesSseNormalizer::new("gpt-test".to_string(), false, None);
        let input = concat!(
            "event: response.output_item.added\ndata: {\"type\":\"response.output_item.added\",\"sequence_number\":0,\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[]}}\n\n",
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"sequence_number\":1,\"item_id\":\"msg_1\",\"output_index\":0,\"delta\":\"<thinking>unfinished\"}\n\n"
        );
        let mut output = normalizer.push(input.as_bytes()).to_vec();
        normalizer.finish(&mut output);
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("<thinking>unfinished"));
        assert!(output.contains("\"delta\":\"unfinished\""));
    }

    #[test]
    fn stream_does_not_buffer_events_before_message() {
        let mut normalizer = NativeResponsesSseNormalizer::new("gpt-test".to_string(), false, None);
        let created = b"event: response.created\ndata: {\"type\":\"response.created\",\"sequence_number\":0,\"response\":{\"output\":[]}}\n\n";
        assert_eq!(normalizer.push(created), Bytes::copy_from_slice(created));
    }
}
