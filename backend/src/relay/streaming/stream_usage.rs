use axum::http::StatusCode;
use serde_json::Value;

use crate::billing::{
    estimate_claude_text_tokens, parse_usage_from_bytes, parse_usage_from_sse_data,
    TokenUsage,
};

use super::stream_error::{StreamErrorSummary, truncate_for_log};

pub(super) enum ResponseUsageParser {
    Sse(Box<StreamUsageParser>),
    Json {
        buffer: Option<Vec<u8>>,
        limit_bytes: usize,
    },
    Disabled,
}

impl ResponseUsageParser {
    pub(super) fn for_response(
        status: StatusCode,
        streamed: bool,
        path: &str,
        content_length: Option<u64>,
        limit_bytes: usize,
    ) -> Self {
        if !status.is_success() || path.starts_with("/v1/images/") {
            Self::Disabled
        } else if streamed {
            Self::Sse(Box::new(StreamUsageParser::new(limit_bytes)))
        } else {
            Self::Json {
                buffer: Some(Vec::with_capacity(json_usage_buffer_capacity(
                    content_length,
                    limit_bytes,
                ))),
                limit_bytes,
            }
        }
    }

    pub(super) fn observe(&mut self, chunk: &[u8]) {
        match self {
            Self::Sse(parser) => parser.observe(chunk),
            Self::Json {
                buffer,
                limit_bytes,
            } => {
                if let Some(bytes) = buffer {
                    if bytes.len().saturating_add(chunk.len()) <= *limit_bytes {
                        bytes.extend_from_slice(chunk);
                    } else {
                        tracing::warn!(
                            limit_bytes,
                            "non-streamed relay response exceeded usage parse buffer; skipping usage parse"
                        );
                        *buffer = None;
                    }
                }
            }
            Self::Disabled => {}
        }
    }

    pub(super) fn finish(&mut self) -> Option<TokenUsage> {
        match self {
            Self::Sse(parser) => parser.finish(),
            Self::Json { buffer, .. } => buffer
                .as_deref()
                .and_then(|bytes| parse_usage_from_bytes(bytes, false)),
            Self::Disabled => None,
        }
    }

    pub(super) fn response_complete(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.completed)
            || matches!(self, Self::Json { buffer: Some(bytes), .. } if json_body_is_complete(bytes))
    }

    pub(super) fn response_failed(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.failed)
    }

    pub(super) fn saw_meaningful_output(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.saw_meaningful_output)
    }

    pub(super) fn responses_terminal_usage_present(&self) -> bool {
        matches!(self, Self::Sse(parser) if parser.responses_terminal_usage_present)
    }

    pub(super) fn responses_terminal_response_id(&self) -> Option<&str> {
        match self {
            Self::Sse(parser) => parser.responses_terminal_response_id.as_deref(),
            Self::Json { .. } | Self::Disabled => None,
        }
    }

    pub(super) fn estimated_responses_output(&self) -> ResponsesOutputEstimate {
        match self {
            Self::Sse(parser) => parser.estimated_responses_output(),
            Self::Json { .. } | Self::Disabled => ResponsesOutputEstimate::default(),
        }
    }

    pub(super) fn stream_error_summary(&self) -> Option<StreamErrorSummary> {
        match self {
            Self::Sse(parser) => parser.last_error.clone(),
            Self::Json { .. } | Self::Disabled => None,
        }
    }

    /// Returns a short human-readable summary of the last observed stream
    /// signal so we can include it in the "stream ended before terminal SSE
    /// event" warning. Helps distinguish an upstream that emitted an
    /// unrecognized terminal type from one that simply hung up mid-stream.
    pub(super) fn last_signal_summary(&self) -> Option<String> {
        match self {
            Self::Sse(parser) => {
                if parser.saw_done {
                    return Some("data:[DONE]".to_string());
                }
                signal_summary(parser.last_event.as_deref(), parser.last_type.as_deref())
            }
            Self::Json {
                buffer: Some(bytes),
                ..
            } => json_body_is_complete(bytes)
                .then(|| "json-body-complete".to_string())
                .or(Some("json-body-incomplete".to_string())),
            Self::Json { buffer: None, .. } => Some("json-buffer-overflow".to_string()),
            Self::Disabled => None,
        }
    }

    pub(super) fn previous_signal_summary(&self) -> Option<String> {
        match self {
            Self::Sse(parser) => signal_summary(
                parser.previous_event.as_deref(),
                parser.previous_type.as_deref(),
            ),
            Self::Json { .. } | Self::Disabled => None,
        }
    }
}

pub(super) fn signal_summary(event: Option<&str>, data_type: Option<&str>) -> Option<String> {
    match (event, data_type) {
        (Some(event), Some(data_type)) => Some(format!("event:{event} data_type:{data_type}")),
        (Some(event), None) => Some(format!("event:{event}")),
        (None, Some(data_type)) => Some(format!("data_type:{data_type}")),
        (None, None) => None,
    }
}

pub(super) fn json_usage_buffer_capacity(content_length: Option<u64>, limit_bytes: usize) -> usize {
    content_length
        .and_then(|length| usize::try_from(length).ok())
        .map_or(0, |length| length.min(limit_bytes))
}

pub(super) fn json_body_is_complete(bytes: &[u8]) -> bool {
    serde_json::from_slice::<Value>(bytes).is_ok()
}

#[derive(Default)]
pub(super) struct ParsedLine {
    pub(super) usage: Option<TokenUsage>,
    pub(super) event: Option<String>,
    pub(super) data: Option<String>,
    pub(super) data_type: Option<String>,
    pub(super) responses_terminal_usage_present: bool,
    pub(super) responses_terminal_response_id: Option<String>,
    pub(super) responses_output_delta: Option<String>,
    pub(super) responses_function_arguments_delta: Option<String>,
    pub(super) responses_reasoning_delta: Option<String>,
    pub(super) meaningful_output: bool,
    pub(super) completed: bool,
    pub(super) failed: bool,
    pub(super) done: bool,
}

pub(crate) struct StreamUsageParser {
    pub(super) buffered: Vec<u8>,
    pub(super) latest: Option<TokenUsage>,
    pub(super) completed: bool,
    pub(super) skipping_oversized_line: bool,
    pub(super) limit_bytes: usize,
    pub(super) last_event: Option<String>,
    pub(super) last_type: Option<String>,
    pub(super) previous_event: Option<String>,
    pub(super) previous_type: Option<String>,
    pub(super) saw_done: bool,
    pub(super) failed: bool,
    pub(super) last_error: Option<StreamErrorSummary>,
    pub(super) responses_terminal_usage_present: bool,
    pub(super) responses_terminal_response_id: Option<String>,
    pub(super) responses_output_text: String,
    pub(super) responses_function_arguments: String,
    pub(super) responses_reasoning: String,
    pub(super) saw_meaningful_output: bool,
}

impl StreamUsageParser {
    pub(crate) fn new(limit_bytes: usize) -> Self {
        Self {
            buffered: Vec::new(),
            latest: None,
            completed: false,
            skipping_oversized_line: false,
            limit_bytes,
            last_event: None,
            last_type: None,
            previous_event: None,
            previous_type: None,
            saw_done: false,
            failed: false,
            last_error: None,
            responses_terminal_usage_present: false,
            responses_terminal_response_id: None,
            responses_output_text: String::new(),
            responses_function_arguments: String::new(),
            responses_reasoning: String::new(),
            saw_meaningful_output: false,
        }
    }

    pub(crate) fn observe(&mut self, chunk: &[u8]) {
        if self.skipping_oversized_line {
            if let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') {
                self.skipping_oversized_line = false;
                self.observe(&chunk[offset + 1..]);
            }
            return;
        }
        if self.buffered.len().saturating_add(chunk.len()) > self.limit_bytes {
            tracing::debug!(
                limit_bytes = self.limit_bytes,
                "streamed relay response line exceeded usage parse buffer; skipping oversized line"
            );
            self.buffered.clear();
            if let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') {
                self.observe(&chunk[offset + 1..]);
            } else {
                self.skipping_oversized_line = true;
            }
            return;
        }

        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let line_end = consumed + offset;
            let mut line = &self.buffered[consumed..line_end];
            if matches!(line.last(), Some(b'\r')) {
                line = &line[..line.len() - 1];
            }
            let parsed = Self::parse_line(line);
            self.observe_parsed_line(parsed);
            consumed = line_end + 1;
        }
        if consumed == self.buffered.len() {
            self.buffered.clear();
        } else if consumed > 0 {
            self.buffered.drain(..consumed);
        }
    }

    pub(super) fn observe_line(&mut self, line: &[u8]) {
        let parsed = Self::parse_line(line);
        self.observe_parsed_line(parsed);
    }

    pub(super) fn observe_parsed_line(&mut self, parsed: ParsedLine) {
        if parsed.meaningful_output {
            self.saw_meaningful_output = true;
        }
        if parsed.responses_terminal_usage_present {
            self.responses_terminal_usage_present = true;
        }
        if parsed.responses_terminal_response_id.is_some() {
            self.responses_terminal_response_id = parsed.responses_terminal_response_id.clone();
        }
        if let Some(delta) = parsed.responses_output_delta.as_deref() {
            self.responses_output_text.push_str(delta);
        }
        if let Some(delta) = parsed.responses_function_arguments_delta.as_deref() {
            self.responses_function_arguments.push_str(delta);
        }
        if let Some(delta) = parsed.responses_reasoning_delta.as_deref() {
            self.responses_reasoning.push_str(delta);
        }
        if let Some(usage) = parsed.usage {
            match &mut self.latest {
                Some(latest) => merge_token_usage(latest, usage),
                None => self.latest = Some(usage),
            }
        }
        if parsed.failed && !self.failed {
            self.previous_event.clone_from(&self.last_event);
            self.previous_type.clone_from(&self.last_type);
        }
        if let Some(event) = parsed.event {
            self.last_event = Some(event);
        }
        if let Some(data_type) = parsed.data_type {
            self.last_type = Some(data_type);
        }
        if parsed.done {
            self.saw_done = true;
        }
        if parsed.completed {
            self.completed = true;
        }
        if parsed.failed {
            self.failed = true;
        }
        if let Some(data) = parsed.data.as_deref() {
            let is_error_data = parsed.failed || self.last_event.as_deref() == Some("error");
            if is_error_data {
                self.last_error = Some(StreamErrorSummary::from_sse_data(data));
            }
        }
    }

    pub(super) fn parse_line(line: &[u8]) -> ParsedLine {
        let Ok(line) = std::str::from_utf8(line) else {
            return ParsedLine::default();
        };
        if let Some(event) = line.strip_prefix("event:").map(str::trim) {
            return ParsedLine {
                event: Some(event.to_string()),
                completed: stream_event_is_terminal(event),
                failed: stream_event_is_failure(event),
                ..Default::default()
            };
        }
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return ParsedLine::default();
        };
        if data.is_empty() {
            return ParsedLine::default();
        }
        if data == "[DONE]" {
            return ParsedLine {
                done: true,
                completed: true,
                ..Default::default()
            };
        }
        let data_type = sse_data_type_name(data);
        let usage = parse_usage_from_sse_data(data);
        let responses_terminal = data_type
            .as_deref()
            .is_some_and(responses_event_is_terminal);
        let (responses_output_delta, responses_function_arguments_delta, responses_reasoning_delta) =
            responses_deltas_from_sse_data(data, data_type.as_deref());
        let meaningful_output = sse_data_has_meaningful_output(data, data_type.as_deref());
        // Reuse the parsed data type for terminal classification.
        let completed = data_type.as_deref().is_some_and(stream_event_is_terminal);
        let failed = data_type.as_deref().is_some_and(stream_event_is_failure);
        ParsedLine {
            responses_terminal_usage_present: responses_terminal && usage.is_some(),
            responses_terminal_response_id: responses_terminal
                .then(|| response_id_from_sse_data(data))
                .flatten(),
            usage,
            data: Some(data.to_string()),
            data_type,
            responses_output_delta,
            responses_function_arguments_delta,
            responses_reasoning_delta,
            meaningful_output,
            completed,
            failed,
            ..Default::default()
        }
    }

    pub(crate) fn finish(&mut self) -> Option<TokenUsage> {
        if self.skipping_oversized_line {
            return self.latest;
        }
        if !self.buffered.is_empty() {
            let line = std::mem::take(&mut self.buffered);
            self.observe_line(&line);
        }
        self.latest
    }

    pub(super) fn estimated_responses_output(&self) -> ResponsesOutputEstimate {
        let text_tokens = estimate_claude_text_tokens(&self.responses_output_text);
        let function_arguments_tokens =
            estimate_claude_text_tokens(&self.responses_function_arguments);
        let reasoning_tokens = estimate_claude_text_tokens(&self.responses_reasoning);
        ResponsesOutputEstimate {
            output_tokens: text_tokens
                .saturating_add(function_arguments_tokens)
                .saturating_add(reasoning_tokens),
            reasoning_tokens,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ResponsesOutputEstimate {
    pub(super) output_tokens: i64,
    pub(super) reasoning_tokens: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TokenUsageState {
    Missing,
    AllZero,
    Present,
}

impl TokenUsageState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::AllZero => "all_zero",
            Self::Present => "present",
        }
    }
}

pub(super) fn token_usage_state(usage: Option<TokenUsage>) -> TokenUsageState {
    let Some(usage) = usage else {
        return TokenUsageState::Missing;
    };
    let meaningful = usage.input_tokens > 0
        || usage.output_tokens > 0
        || usage.cached_input_tokens.unwrap_or(0) > 0
        || usage.cache_creation_input_tokens.unwrap_or(0) > 0
        || usage.cache_creation_input_tokens_5m.unwrap_or(0) > 0
        || usage.cache_creation_input_tokens_1h.unwrap_or(0) > 0
        || usage.reasoning_output_tokens.unwrap_or(0) > 0
        || usage.audio_input_tokens.unwrap_or(0) > 0
        || usage.audio_output_tokens.unwrap_or(0) > 0;
    if meaningful {
        TokenUsageState::Present
    } else {
        TokenUsageState::AllZero
    }
}

pub(super) fn responses_usage_with_estimate(
    observed: Option<TokenUsage>,
    input_tokens_estimate: i64,
    output: ResponsesOutputEstimate,
) -> (Option<TokenUsage>, bool) {
    if output.output_tokens <= 0 {
        return (observed, false);
    }
    let mut usage = observed.unwrap_or(TokenUsage {
        input_tokens: 0,
        output_tokens: 0,
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5m: None,
        cache_creation_input_tokens_1h: None,
        reasoning_output_tokens: None,
        audio_input_tokens: None,
        audio_output_tokens: None,
    });
    let estimate_input = usage.input_tokens <= 0;
    let estimate_output = usage.output_tokens <= 0;
    if !estimate_input && !estimate_output {
        return (Some(usage), false);
    }
    if estimate_input {
        usage.input_tokens = input_tokens_estimate.max(0);
    }
    if estimate_output {
        usage.output_tokens = output.output_tokens;
        if usage.reasoning_output_tokens.unwrap_or(0) <= 0 && output.reasoning_tokens > 0 {
            usage.reasoning_output_tokens = Some(output.reasoning_tokens);
        }
    }
    (Some(usage), true)
}

pub(super) fn merge_token_usage(current: &mut TokenUsage, incoming: TokenUsage) {
    // SSE providers may put cache details in an early event and final output usage later.
    // Merge fields so the later partial update cannot discard previously reported usage.
    current.input_tokens = current.input_tokens.max(incoming.input_tokens);
    current.output_tokens = current.output_tokens.max(incoming.output_tokens);
    merge_optional_usage(
        &mut current.cached_input_tokens,
        incoming.cached_input_tokens,
    );
    merge_optional_usage(
        &mut current.cache_creation_input_tokens,
        incoming.cache_creation_input_tokens,
    );
    merge_optional_usage(
        &mut current.cache_creation_input_tokens_5m,
        incoming.cache_creation_input_tokens_5m,
    );
    merge_optional_usage(
        &mut current.cache_creation_input_tokens_1h,
        incoming.cache_creation_input_tokens_1h,
    );
    merge_optional_usage(
        &mut current.reasoning_output_tokens,
        incoming.reasoning_output_tokens,
    );
    merge_optional_usage(&mut current.audio_input_tokens, incoming.audio_input_tokens);
    merge_optional_usage(
        &mut current.audio_output_tokens,
        incoming.audio_output_tokens,
    );
}

pub(super) fn merge_optional_usage(current: &mut Option<i64>, incoming: Option<i64>) {
    if let Some(incoming) = incoming {
        *current = Some(current.map_or(incoming, |current| current.max(incoming)));
    }
}

pub(super) fn stream_event_is_terminal(event: &str) -> bool {
    matches!(
        event,
        "message_stop"
            | "response.completed"
            | "response.done"
            | "response.incomplete"
            | "response.failed"
            | "response.cancelled"
            | "response.canceled"
            | "error"
    )
}

pub(super) fn responses_event_is_terminal(event: &str) -> bool {
    matches!(
        event,
        "response.completed"
            | "response.done"
            | "response.incomplete"
            | "response.failed"
            | "response.cancelled"
            | "response.canceled"
    )
}

pub(super) fn stream_event_is_failure(event: &str) -> bool {
    matches!(event, "error" | "response.failed")
}

pub(super) fn sse_data_type_name(data: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;
    value
        .get("type")
        .and_then(|type_| type_.as_str())
        .map(ToString::to_string)
}

pub(super) fn response_id_from_sse_data(data: &str) -> Option<String> {
    let value: Value = serde_json::from_str(data).ok()?;
    value
        .get("response")
        .and_then(|response| response.get("id"))
        .and_then(Value::as_str)
        .map(|id| truncate_for_log(id, 256))
}

pub(super) fn responses_deltas_from_sse_data(
    data: &str,
    data_type: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(data_type) = data_type else {
        return (None, None, None);
    };
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return (None, None, None);
    };
    let delta = value
        .get("delta")
        .and_then(Value::as_str)
        .map(str::to_owned);
    match data_type {
        "response.output_text.delta" => (delta, None, None),
        "response.function_call_arguments.delta" => (None, delta, None),
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            (None, None, delta)
        }
        _ => (None, None, None),
    }
}

pub(super) fn sse_data_has_meaningful_output(data: &str, data_type: Option<&str>) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        return false;
    };

    if matches!(
        data_type,
        Some(
            "response.output_text.delta"
                | "response.function_call_arguments.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
        )
    ) && value
        .get("delta")
        .and_then(Value::as_str)
        .is_some_and(|delta| !delta.is_empty())
    {
        return true;
    }

    if data_type == Some("content_block_delta") {
        return value.get("delta").is_some_and(|delta| {
            ["text", "thinking", "partial_json"]
                .iter()
                .any(|field| nonempty_json_string(delta.get(*field)))
        });
    }

    value
        .get("choices")
        .and_then(Value::as_array)
        .is_some_and(|choices| {
            choices.iter().any(|choice| {
                nonempty_json_string(choice.get("text"))
                    || choice.get("delta").is_some_and(|delta| {
                        ["content", "reasoning", "reasoning_content"]
                            .iter()
                            .any(|field| nonempty_json_string(delta.get(*field)))
                            || delta
                                .get("tool_calls")
                                .and_then(Value::as_array)
                                .is_some_and(|tool_calls| {
                                    tool_calls.iter().any(|tool_call| {
                                        nonempty_json_string(
                                            tool_call
                                                .get("function")
                                                .and_then(|function| function.get("arguments")),
                                        )
                                    })
                                })
                    })
            })
        })
}

pub(super) fn nonempty_json_string(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
}
