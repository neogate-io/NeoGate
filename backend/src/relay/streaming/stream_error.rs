use bytes::Bytes;
use serde_json::Value;

use super::DOWNSTREAM_STREAM_ERROR_MESSAGE;

pub(super) fn downstream_stream_error_frame(path: &str) -> Bytes {
    let data = match path {
        "/v1/messages" => serde_json::json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
            },
        }),
        "/v1/responses" => serde_json::json!({
            "type": "error",
            "code": "server_error",
            "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
            "param": null,
        }),
        _ => serde_json::json!({
            "error": {
                "message": DOWNSTREAM_STREAM_ERROR_MESSAGE,
                "type": "server_error",
                "param": null,
                "code": "upstream_stream_error",
            },
        }),
    };
    encode_downstream_stream_error(path, data)
}

pub(super) fn normalize_bare_sse_error(line: &[u8], path: &str) -> Option<Bytes> {
    let line = std::str::from_utf8(line).ok()?.trim();
    if line.is_empty()
        || line.starts_with("data:")
        || line.starts_with("event:")
        || line.starts_with(':')
    {
        return None;
    }
    let mut data = serde_json::from_str::<Value>(line).ok()?;
    let object = data.as_object_mut()?;
    let type_name = object.get("type").and_then(Value::as_str);
    let nested_error = object.get("error").and_then(Value::as_object);
    let nested_error_has_details = nested_error.is_some_and(|error| {
        ["message", "type", "code"]
            .iter()
            .any(|field| error.get(*field).is_some_and(Value::is_string))
    });
    let typed_error = type_name == Some("error");
    let nested_error_envelope =
        nested_error_has_details && matches!(type_name, None | Some("error"));
    let top_level_error = typed_error
        && (object.get("message").is_some_and(Value::is_string)
            || object.get("code").is_some_and(Value::is_string));
    let valid_for_path = match path {
        "/v1/messages" => nested_error_envelope,
        "/v1/responses" => top_level_error,
        _ => nested_error_envelope,
    };
    if !valid_for_path {
        return None;
    }
    if path == "/v1/messages" {
        object
            .entry("type".to_string())
            .or_insert_with(|| Value::String("error".to_string()));
    }
    Some(encode_downstream_stream_error(path, data))
}

pub(super) fn encode_downstream_stream_error(path: &str, data: Value) -> Bytes {
    let data = serde_json::to_string(&data).expect("stream error payload is serializable");
    if matches!(path, "/v1/messages" | "/v1/responses") {
        Bytes::from(format!("event: error\ndata: {data}\n\n"))
    } else {
        Bytes::from(format!("data: {data}\n\n"))
    }
}


#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct StreamErrorSummary {
    pub(super) response_id: Option<String>,
    pub(super) request_id: Option<String>,
    pub(super) error_type: Option<String>,
    pub(super) error_code: Option<String>,
    pub(super) error_message: Option<String>,
    pub(super) raw: Option<String>,
}

impl StreamErrorSummary {
    pub(super) fn from_sse_data(data: &str) -> Self {
        let raw = Some(truncate_for_log(data, 1000));
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            return Self {
                response_id: None,
                request_id: None,
                error_type: None,
                error_code: None,
                error_message: Some(truncate_for_log(data, 500)),
                raw,
            };
        };
        let response_error = value
            .get("response")
            .and_then(|response| response.get("error"));
        let response_id = value
            .get("response")
            .and_then(|response| string_field(response, "id"))
            .or_else(|| string_field(&value, "response_id"))
            .map(|value| truncate_for_log(&value, 256));
        let request_id = string_field(&value, "request_id")
            .or_else(|| string_field(&value, "request-id"))
            .map(|value| truncate_for_log(&value, 256));
        let error = response_error
            .or_else(|| value.get("error"))
            .unwrap_or(&value);
        let error_type = string_field(error, "type")
            .or_else(|| string_field(&value, "type"))
            .map(|value| truncate_for_log(&value, 128));
        let error_code = string_field(error, "code")
            .or_else(|| string_field(&value, "code"))
            .map(|value| truncate_for_log(&value, 128));
        let error_message = string_field(error, "message")
            .or_else(|| string_field(&value, "message"))
            .or_else(|| string_field(error, "msg"))
            .or_else(|| string_field(&value, "msg"))
            .map(|value| truncate_for_log(&value, 500));

        Self {
            response_id,
            request_id,
            error_type,
            error_code,
            error_message,
            raw,
        }
    }

    pub(super) fn to_error_summary(&self) -> String {
        let mut summary = String::from("upstream stream ended with SSE error event");
        if let Some(code) = self.error_code.as_deref() {
            summary.push_str(" code=");
            summary.push_str(code);
        }
        if let Some(error_type) = self.error_type.as_deref() {
            summary.push_str(" type=");
            summary.push_str(error_type);
        }
        if let Some(message) = self.error_message.as_deref() {
            summary.push_str(": ");
            summary.push_str(message);
        }
        summary
    }
}

/// 把 SSE error 的 code/type/message 拼成小写字符串，供 `is_model_error_text` 做关键词
/// 匹配。code 和 type 也参与拼接，因为有些上游把 `model_not_found` 放在 code 而非
/// message 里。
pub(super) fn sse_error_lowered(summary: &StreamErrorSummary) -> String {
    [
        summary.error_code.as_deref(),
        summary.error_type.as_deref(),
        summary.error_message.as_deref(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}

pub(super) fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|value| match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    })
}

pub(super) fn truncate_for_log(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index >= limit {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}
