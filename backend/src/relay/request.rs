use bytes::Bytes;
use serde_json::Value;

use super::affinity::{
    anthropic_messages_affinity_key_from_value, openai_responses_affinity_key_from_value,
    ChannelAffinityKey,
};
use crate::error::{AppError, AppResult};

#[derive(Clone, Copy)]
pub(crate) enum BodyKind {
    OpenaiChat,
    OpenaiJson,
    OpenaiResponses,
    OpenaiResponsesCompact,
    Anthropic,
}

#[derive(Debug)]
pub(crate) struct RelayRequestMeta {
    pub(crate) model: String,
    pub(crate) stream: bool,
    pub(crate) background: bool,
    pub(crate) store: Option<bool>,
    pub(crate) request_params: RelayRequestParams,
    pub(crate) channel_affinity_key: Option<ChannelAffinityKey>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RelayRequestParams {
    pub(crate) max_tokens: Option<i64>,
    pub(crate) temperature: Option<f64>,
    pub(crate) top_p: Option<f64>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) reasoning_max_tokens: Option<i64>,
    pub(crate) tool_count: Option<i64>,
    pub(crate) tool_choice: Option<String>,
    pub(crate) response_format: Option<String>,
    pub(crate) parallel_tool_calls: Option<bool>,
    pub(crate) store: Option<bool>,
    pub(crate) background: Option<bool>,
    pub(crate) image_count: Option<i64>,
    pub(crate) image_size: Option<String>,
    pub(crate) image_quality: Option<String>,
    pub(crate) image_style: Option<String>,
    pub(crate) video_size: Option<String>,
    pub(crate) video_seconds: Option<i64>,
}

impl RelayRequestParams {
    pub(crate) fn image(
        image_count: i64,
        size: Option<String>,
        quality: Option<String>,
        style: Option<String>,
    ) -> Self {
        Self {
            image_count: Some(image_count),
            image_size: size.map(|value| safe_log_label(&value)),
            image_quality: quality.map(|value| safe_log_label(&value)),
            image_style: style.map(|value| safe_log_label(&value)),
            ..Self::default()
        }
    }

    pub(crate) fn video(size: Option<String>, seconds: Option<i64>) -> Self {
        Self {
            video_size: size.map(|value| safe_log_label(&value)),
            video_seconds: seconds.filter(|value| *value > 0),
            ..Self::default()
        }
    }
}

pub(crate) struct PreparedRelayBody {
    pub(crate) body: Bytes,
    pub(crate) meta: RelayRequestMeta,
    pub(crate) output_tokens: i64,
}

pub(crate) fn prepare_relay_body(
    body: Bytes,
    kind: BodyKind,
    default_output_tokens: i64,
) -> AppResult<PreparedRelayBody> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let meta = request_meta_from_value(&value, kind)?;
    let inserts_output_limit = inserts_output_limit(kind);
    let (output_tokens, has_output_limit) = match output_limit_from_value(&value, kind) {
        Some(tokens) => (tokens, true),
        None if reserves_default_output_tokens(kind) => (default_output_tokens, false),
        None => (0, true),
    };
    let needs_stream_usage = meta.stream
        && matches!(kind, BodyKind::OpenaiChat)
        && !openai_stream_usage_included_value(&value);
    let changed = (inserts_output_limit && !has_output_limit) || needs_stream_usage;
    if !changed {
        return Ok(PreparedRelayBody {
            body,
            meta,
            output_tokens,
        });
    }

    if inserts_output_limit && !has_output_limit {
        ensure_output_limit(&mut value, kind, default_output_tokens)?;
    }
    if needs_stream_usage {
        ensure_openai_stream_usage(&mut value)?;
    }
    let body = Bytes::from(serde_json::to_vec(&value)?);
    Ok(PreparedRelayBody {
        body,
        meta,
        output_tokens,
    })
}

pub(crate) fn rewrite_relay_body_model(
    body: Bytes,
    kind: BodyKind,
    target_model: &str,
) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a json object".to_string()))?;
    if !object.contains_key("model") {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    object.insert("model".to_string(), Value::String(target_model.to_string()));
    if matches!(
        kind,
        BodyKind::OpenaiResponses | BodyKind::OpenaiResponsesCompact
    ) {
        // Affinity keys include the requested model. The relay now routes on the target model,
        // so downstream affinity must follow the body sent upstream.
    }
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn request_meta_from_value(value: &Value, kind: BodyKind) -> AppResult<RelayRequestMeta> {
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    if model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    let channel_affinity_key = match kind {
        BodyKind::OpenaiResponses | BodyKind::OpenaiResponsesCompact => {
            openai_responses_affinity_key_from_value(model, value)
        }
        BodyKind::Anthropic => anthropic_messages_affinity_key_from_value(model, value),
        BodyKind::OpenaiChat | BodyKind::OpenaiJson => None,
    };
    Ok(RelayRequestMeta {
        model: model.to_string(),
        stream: value
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        background: value
            .get("background")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        store: value.get("store").and_then(Value::as_bool),
        request_params: request_params_from_value(value, kind),
        channel_affinity_key,
    })
}

fn request_params_from_value(value: &Value, kind: BodyKind) -> RelayRequestParams {
    RelayRequestParams {
        max_tokens: output_limit_from_value(value, kind),
        temperature: value.get("temperature").and_then(Value::as_f64),
        top_p: value.get("top_p").and_then(Value::as_f64),
        reasoning_effort: reasoning_effort_from_value(value),
        reasoning_max_tokens: reasoning_max_tokens_from_value(value),
        tool_count: value
            .get("tools")
            .and_then(Value::as_array)
            .map(|tools| tools.len() as i64),
        tool_choice: value.get("tool_choice").and_then(tool_choice_summary),
        response_format: response_format_summary(value),
        parallel_tool_calls: value.get("parallel_tool_calls").and_then(Value::as_bool),
        store: value.get("store").and_then(Value::as_bool),
        background: value.get("background").and_then(Value::as_bool),
        image_count: None,
        image_size: None,
        image_quality: None,
        image_style: None,
        video_size: None,
        video_seconds: None,
    }
}

fn reasoning_effort_from_value(value: &Value) -> Option<String> {
    value
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("reasoning")
                .and_then(|reasoning| reasoning.get("effort"))
                .and_then(Value::as_str)
        })
        .map(safe_log_label)
        .or_else(|| {
            let enabled = value
                .get("reasoning")
                .and_then(|reasoning| reasoning.get("enabled"))
                .and_then(Value::as_bool)
                .or_else(|| {
                    value
                        .get("thinking")
                        .and_then(|thinking| thinking.get("type"))
                        .and_then(Value::as_str)
                        .map(|kind| kind == "enabled")
                })?;
            Some(if enabled { "enabled" } else { "disabled" }.to_string())
        })
}

fn reasoning_max_tokens_from_value(value: &Value) -> Option<i64> {
    value
        .get("reasoning")
        .and_then(|reasoning| {
            reasoning
                .get("max_tokens")
                .or_else(|| reasoning.get("budget_tokens"))
        })
        .or_else(|| {
            value
                .get("thinking")
                .and_then(|thinking| thinking.get("budget_tokens"))
        })
        .and_then(Value::as_i64)
        .filter(|tokens| *tokens > 0)
}

fn tool_choice_summary(value: &Value) -> Option<String> {
    if let Some(choice) = value.as_str() {
        return Some(safe_log_label(choice));
    }
    let object = value.as_object()?;
    let kind = object.get("type").and_then(Value::as_str)?;
    if kind == "function" {
        let name = object
            .get("function")
            .and_then(|function| function.get("name"))
            .or_else(|| object.get("name"))
            .and_then(Value::as_str)
            .map(safe_log_label);
        return Some(match name {
            Some(name) if !name.is_empty() => format!("function:{name}"),
            _ => "function".to_string(),
        });
    }
    if kind == "tool" {
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .map(safe_log_label);
        return Some(match name {
            Some(name) if !name.is_empty() => format!("tool:{name}"),
            _ => "tool".to_string(),
        });
    }
    Some(safe_log_label(kind))
}

fn response_format_summary(value: &Value) -> Option<String> {
    value
        .get("response_format")
        .and_then(|format| format.get("type").or(Some(format)))
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("text")
                .and_then(|text| text.get("format"))
                .and_then(|format| format.get("type").or(Some(format)))
                .and_then(Value::as_str)
        })
        .map(safe_log_label)
}

pub(crate) fn safe_log_label(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'))
        .take(80)
        .collect()
}

fn output_limit_from_value(value: &Value, kind: BodyKind) -> Option<i64> {
    output_limit_keys(kind)
        .iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_i64))
        .find(|tokens| *tokens > 0)
}

fn reserves_default_output_tokens(kind: BodyKind) -> bool {
    matches!(
        kind,
        BodyKind::OpenaiChat | BodyKind::OpenaiResponses | BodyKind::Anthropic
    )
}

fn inserts_output_limit(kind: BodyKind) -> bool {
    matches!(kind, BodyKind::OpenaiChat | BodyKind::Anthropic)
}

fn openai_stream_usage_included_value(value: &Value) -> bool {
    value
        .get("stream_options")
        .and_then(|options| options.get("include_usage"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn ensure_output_limit(
    value: &mut Value,
    kind: BodyKind,
    default_output_tokens: i64,
) -> AppResult<(i64, bool)> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a json object".to_string()))?;
    let keys = output_limit_keys(kind);
    for key in keys {
        if let Some(tokens) = object.get(*key).and_then(Value::as_i64) {
            if tokens > 0 {
                return Ok((tokens, false));
            }
        }
    }
    let insert_key = keys[0];
    object.insert(
        insert_key.to_string(),
        Value::Number(serde_json::Number::from(default_output_tokens)),
    );
    Ok((default_output_tokens, true))
}

fn output_limit_keys(kind: BodyKind) -> &'static [&'static str] {
    match kind {
        BodyKind::OpenaiChat => &["max_completion_tokens", "max_tokens"],
        BodyKind::OpenaiJson => &[],
        BodyKind::OpenaiResponses => &["max_output_tokens"],
        BodyKind::OpenaiResponsesCompact => &[],
        BodyKind::Anthropic => &["max_tokens"],
    }
}

fn ensure_openai_stream_usage(value: &mut Value) -> AppResult<bool> {
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a json object".to_string()))?;
    let stream_options = object
        .entry("stream_options")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let stream_options = stream_options
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("stream_options must be an object".to_string()))?;
    if stream_options
        .get("include_usage")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Ok(false);
    }
    stream_options.insert("include_usage".to_string(), Value::Bool(true));
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_body_bytes_when_no_rewrite_is_needed() {
        let body = Bytes::from_static(br#"{"model":"gpt-4.1","max_tokens":128}"#);

        let prepared = prepare_relay_body(body.clone(), BodyKind::OpenaiChat, 4096).unwrap();

        assert_eq!(prepared.body, body);
        assert_eq!(prepared.meta.model, "gpt-4.1");
        assert!(!prepared.meta.stream);
        assert!(!prepared.meta.background);
        assert_eq!(prepared.output_tokens, 128);
    }

    #[test]
    fn adds_default_output_limit_when_missing() {
        let body = Bytes::from_static(br#"{"model":"gpt-4.1"}"#);

        let prepared = prepare_relay_body(body, BodyKind::OpenaiChat, 2048).unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["max_completion_tokens"], 2048);
        assert_eq!(prepared.output_tokens, 2048);
    }

    #[test]
    fn adds_openai_stream_usage_for_streaming_requests() {
        let body = Bytes::from_static(br#"{"model":"gpt-4.1","stream":true,"max_tokens":128}"#);

        let prepared = prepare_relay_body(body, BodyKind::OpenaiChat, 4096).unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["stream_options"]["include_usage"], true);
        assert!(prepared.meta.stream);
    }

    #[test]
    fn does_not_add_stream_usage_for_openai_responses() {
        let body = Bytes::from_static(br#"{"model":"gpt-5","stream":true}"#);

        let prepared = prepare_relay_body(body.clone(), BodyKind::OpenaiResponses, 4096).unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(value.get("stream_options").is_none());
        assert!(value.get("max_output_tokens").is_none());
        assert_eq!(prepared.body, body);
        assert_eq!(prepared.output_tokens, 4096);
        assert!(prepared.meta.stream);
    }

    #[test]
    fn keeps_openai_responses_compact_body_without_output_limit_rewrite() {
        let body =
            Bytes::from_static(br#"{"model":"gpt-5","input":[{"role":"user","content":"hello"}]}"#);

        let prepared =
            prepare_relay_body(body.clone(), BodyKind::OpenaiResponsesCompact, 4096).unwrap();

        assert_eq!(prepared.body, body);
        assert_eq!(prepared.meta.model, "gpt-5");
        assert_eq!(prepared.output_tokens, 0);
        assert!(prepared.meta.request_params.max_tokens.is_none());
    }

    #[test]
    fn keeps_openai_json_body_without_output_limit_rewrite() {
        let body = Bytes::from_static(
            br#"{"model":"text-embedding-3-small","input":"hello","encoding_format":"float"}"#,
        );

        let prepared = prepare_relay_body(body.clone(), BodyKind::OpenaiJson, 4096).unwrap();

        assert_eq!(prepared.body, body);
        assert_eq!(prepared.meta.model, "text-embedding-3-small");
        assert_eq!(prepared.output_tokens, 0);
    }

    #[test]
    fn extracts_openai_responses_affinity_key_from_prepared_meta() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","prompt_cache_key":"trace-1","max_output_tokens":128}"#,
        );

        let prepared = prepare_relay_body(body, BodyKind::OpenaiResponses, 4096).unwrap();
        let key = prepared.meta.channel_affinity_key.expect("affinity key");

        assert_eq!(key.rule, "openai_responses_prompt_cache_key");
        assert_eq!(key.value, "trace-1");
    }

    #[test]
    fn extracts_safe_request_params_without_prompt_content() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5","input":"do not log this","max_output_tokens":128,"temperature":0.7,"top_p":0.9,"reasoning":{"effort":"high","max_tokens":64},"tools":[{"type":"function","name":"lookup","parameters":{"type":"object"}}],"tool_choice":{"type":"function","name":"lookup"},"response_format":{"type":"json_schema"},"parallel_tool_calls":true,"store":false}"#,
        );

        let prepared = prepare_relay_body(body, BodyKind::OpenaiResponses, 4096).unwrap();
        let params = prepared.meta.request_params;

        assert_eq!(params.max_tokens, Some(128));
        assert_eq!(params.temperature, Some(0.7));
        assert_eq!(params.top_p, Some(0.9));
        assert_eq!(params.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(params.reasoning_max_tokens, Some(64));
        assert_eq!(params.tool_count, Some(1));
        assert_eq!(params.tool_choice.as_deref(), Some("function:lookup"));
        assert_eq!(params.response_format.as_deref(), Some("json_schema"));
        assert_eq!(params.parallel_tool_calls, Some(true));
        assert_eq!(params.store, Some(false));
    }

    #[test]
    fn extracts_anthropic_affinity_key_from_prepared_meta() {
        let body = Bytes::from_static(
            br#"{"model":"claude-sonnet-4","metadata":{"user_id":"user-1"},"max_tokens":128}"#,
        );

        let prepared = prepare_relay_body(body, BodyKind::Anthropic, 4096).unwrap();
        let key = prepared.meta.channel_affinity_key.expect("affinity key");

        assert_eq!(key.rule, "anthropic_messages_metadata_user_id");
        assert_eq!(key.value, "user-1");
    }
}
