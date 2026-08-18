use bytes::Bytes;
use serde_json::Value;

use super::stream_error::normalize_bare_sse_error;

pub(super) struct SseRewriteOutput {
    pub(super) chunk: Bytes,
    pub(super) bare_error: bool,
}

pub(super) enum SseLineRewrite {
    Line(Vec<u8>),
    BareError(Bytes),
}

pub(super) struct SseStreamRewriter {
    pub(super) buffered: Vec<u8>,
    pub(super) external_model: Option<String>,
    pub(super) path: &'static str,
    pub(super) limit_bytes: usize,
    pub(super) skipping_oversized_line: bool,
    pub(super) finished: bool,
}

impl SseStreamRewriter {
    pub(super) fn new(external_model: Option<String>, path: &'static str, limit_bytes: usize) -> Self {
        Self {
            buffered: Vec::new(),
            external_model,
            path,
            limit_bytes,
            skipping_oversized_line: false,
            finished: false,
        }
    }

    pub(super) fn rewrite_chunk(&mut self, chunk: Bytes) -> SseRewriteOutput {
        if chunk.is_empty() || self.finished {
            return SseRewriteOutput {
                chunk: Bytes::new(),
                bare_error: false,
            };
        }
        let mut output = Vec::with_capacity(self.buffered.len().saturating_add(chunk.len()));
        let chunk = if self.skipping_oversized_line {
            let Some(offset) = chunk.iter().position(|byte| *byte == b'\n') else {
                return SseRewriteOutput {
                    chunk,
                    bare_error: false,
                };
            };
            output.extend_from_slice(&chunk[..=offset]);
            self.skipping_oversized_line = false;
            chunk.slice(offset + 1..)
        } else {
            chunk
        };
        self.buffered.extend_from_slice(&chunk);
        let mut consumed = 0;
        while let Some(offset) = self.buffered[consumed..]
            .iter()
            .position(|byte| *byte == b'\n')
        {
            let line_end = consumed + offset;
            match self.rewrite_line(&self.buffered[consumed..line_end]) {
                SseLineRewrite::Line(line) => {
                    output.extend_from_slice(&line);
                    output.push(b'\n');
                }
                SseLineRewrite::BareError(frame) => {
                    output.extend_from_slice(&frame);
                    self.buffered.clear();
                    self.finished = true;
                    return SseRewriteOutput {
                        chunk: Bytes::from(output),
                        bare_error: true,
                    };
                }
            }
            consumed = line_end + 1;
        }
        if consumed == self.buffered.len() {
            self.buffered.clear();
        } else if consumed > 0 {
            self.buffered.drain(..consumed);
        }

        if self.buffered.len() > self.limit_bytes {
            output.extend_from_slice(&self.buffered);
            self.buffered.clear();
            self.skipping_oversized_line = true;
            return SseRewriteOutput {
                chunk: Bytes::from(output),
                bare_error: false,
            };
        }

        if let Some(frame) = normalize_bare_sse_error(&self.buffered, self.path) {
            output.extend_from_slice(&frame);
            self.buffered.clear();
            self.finished = true;
            return SseRewriteOutput {
                chunk: Bytes::from(output),
                bare_error: true,
            };
        }

        SseRewriteOutput {
            chunk: Bytes::from(output),
            bare_error: false,
        }
    }

    pub(super) fn finish(&mut self) -> Option<SseRewriteOutput> {
        if self.finished {
            return None;
        }
        self.finished = true;
        if self.buffered.is_empty() {
            return None;
        }
        let line = std::mem::take(&mut self.buffered);
        Some(match self.rewrite_line(&line) {
            SseLineRewrite::Line(line) => SseRewriteOutput {
                chunk: Bytes::from(line),
                bare_error: false,
            },
            SseLineRewrite::BareError(frame) => SseRewriteOutput {
                chunk: frame,
                bare_error: true,
            },
        })
    }

    pub(super) fn rewrite_line(&self, line: &[u8]) -> SseLineRewrite {
        let (line, cr) = match line.strip_suffix(b"\r") {
            Some(line) => (line, true),
            None => (line, false),
        };
        if let Some(frame) = normalize_bare_sse_error(line, self.path) {
            return SseLineRewrite::BareError(frame);
        }
        let Some(external_model) = self.external_model.as_deref() else {
            let mut output = line.to_vec();
            if cr {
                output.push(b'\r');
            }
            return SseLineRewrite::Line(output);
        };
        let Some(rewritten) = rewrite_sse_data_model(line, external_model) else {
            let mut output = line.to_vec();
            if cr {
                output.push(b'\r');
            }
            return SseLineRewrite::Line(output);
        };
        let mut output = rewritten.into_bytes();
        if cr {
            output.push(b'\r');
        }
        SseLineRewrite::Line(output)
    }
}

pub(super) fn rewrite_sse_data_model(line: &[u8], external_model: &str) -> Option<String> {
    let line = std::str::from_utf8(line).ok()?;
    let rest = line.strip_prefix("data:")?;
    let leading_len = rest.len() - rest.trim_start().len();
    let leading = &rest[..leading_len];
    let data = rest[leading_len..].trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let mut value = serde_json::from_str::<Value>(data).ok()?;
    let object = value.as_object_mut()?;
    if !object.contains_key("model") {
        return None;
    }
    object.insert(
        "model".to_string(),
        Value::String(external_model.to_string()),
    );
    serde_json::to_string(&value)
        .ok()
        .map(|json| format!("data:{leading}{json}"))
}

