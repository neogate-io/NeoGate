use serde_json::Value;

use crate::billing::TokenUsage;

const TASK_RESULT_USAGE_BUFFER_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct AnthropicResultsUsageParser {
    buffered: Vec<u8>,
    total: Option<TokenUsage>,
    disabled: bool,
}

impl AnthropicResultsUsageParser {
    pub(crate) fn observe(&mut self, chunk: &[u8]) {
        if self.disabled {
            return;
        }
        if self.buffered.len().saturating_add(chunk.len()) > TASK_RESULT_USAGE_BUFFER_BYTES {
            tracing::warn!("anthropic batch results usage parse buffer exceeded limit");
            self.buffered.clear();
            self.disabled = true;
            return;
        }
        self.buffered.extend_from_slice(chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let end = consumed + offset;
            let mut line = self.buffered[consumed..end].to_vec();
            if matches!(line.last(), Some(b'\r')) {
                line.pop();
            }
            self.observe_line(&line);
            consumed = end + 1;
        }
        if consumed > 0 {
            self.buffered.drain(..consumed);
        }
    }

    pub(crate) fn finish(mut self) -> Option<TokenUsage> {
        if !self.disabled && !self.buffered.is_empty() {
            let line = std::mem::take(&mut self.buffered);
            self.observe_line(&line);
        }
        self.total
    }

    fn observe_line(&mut self, line: &[u8]) {
        let Some(usage) = anthropic_result_line_usage(line) else {
            return;
        };
        let total = self.total.get_or_insert(TokenUsage {
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
        total.input_tokens = total.input_tokens.saturating_add(usage.input_tokens);
        total.output_tokens = total.output_tokens.saturating_add(usage.output_tokens);
    }
}

#[cfg(test)]
pub(crate) fn anthropic_results_usage(body: &[u8]) -> Option<TokenUsage> {
    let mut parser = AnthropicResultsUsageParser::default();
    parser.observe(body);
    parser.finish()
}

fn anthropic_result_line_usage(line: &[u8]) -> Option<TokenUsage> {
    if line.iter().all(|byte| byte.is_ascii_whitespace()) {
        return None;
    }
    let value = serde_json::from_slice::<Value>(line).ok()?;
    let message = value
        .get("result")
        .and_then(|result| result.get("message"))
        .unwrap_or(&value);
    let usage = message.get("usage")?;
    Some(TokenUsage {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_i64)
            .unwrap_or(0),
        cached_input_tokens: None,
        cache_creation_input_tokens: None,
        cache_creation_input_tokens_5m: None,
        cache_creation_input_tokens_1h: None,
        reasoning_output_tokens: None,
        audio_input_tokens: None,
        audio_output_tokens: None,
    })
}
