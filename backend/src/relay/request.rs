use std::borrow::Cow;

use bytes::Bytes;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};

#[derive(Clone, Copy)]
pub(super) enum BodyKind {
    OpenaiChat,
    OpenaiResponses,
    Anthropic,
}

#[derive(Debug)]
pub(super) struct RelayRequestMeta {
    pub(super) model: String,
    pub(super) stream: bool,
    pub(super) background: bool,
    pub(super) store: Option<bool>,
}

pub(super) struct PreparedRelayBody {
    pub(super) body: Bytes,
    pub(super) meta: RelayRequestMeta,
    pub(super) output_tokens: i64,
}

#[derive(Deserialize)]
struct RequestProbe<'a> {
    #[serde(borrow)]
    model: Option<Cow<'a, str>>,
    stream: Option<bool>,
    background: Option<bool>,
    store: Option<bool>,
    max_completion_tokens: Option<i64>,
    max_tokens: Option<i64>,
    max_output_tokens: Option<i64>,
    stream_options: Option<StreamOptionsProbe>,
}

#[derive(Deserialize)]
struct StreamOptionsProbe {
    include_usage: Option<bool>,
}

pub(super) fn prepare_relay_body(
    body: Bytes,
    kind: BodyKind,
    default_output_tokens: i64,
) -> AppResult<PreparedRelayBody> {
    let probe: RequestProbe<'_> = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let meta = request_meta_from_probe(&probe)?;
    let (output_tokens, has_output_limit) = output_limit_from_probe(&probe, kind)
        .map(|tokens| (tokens, true))
        .unwrap_or((default_output_tokens, false));
    let needs_stream_usage = meta.stream
        && matches!(kind, BodyKind::OpenaiChat | BodyKind::OpenaiResponses)
        && !openai_stream_usage_included(&probe);
    let changed = !has_output_limit || needs_stream_usage;
    if !changed {
        return Ok(PreparedRelayBody {
            body,
            meta,
            output_tokens,
        });
    }

    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    if !has_output_limit {
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

fn request_meta_from_probe(probe: &RequestProbe<'_>) -> AppResult<RelayRequestMeta> {
    let model = probe
        .model
        .as_deref()
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    if model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    Ok(RelayRequestMeta {
        model: model.to_string(),
        stream: probe.stream.unwrap_or(false),
        background: probe.background.unwrap_or(false),
        store: probe.store,
    })
}

fn output_limit_from_probe(probe: &RequestProbe<'_>, kind: BodyKind) -> Option<i64> {
    let tokens = match kind {
        BodyKind::OpenaiChat => probe.max_completion_tokens.or(probe.max_tokens),
        BodyKind::OpenaiResponses => probe.max_output_tokens,
        BodyKind::Anthropic => probe.max_tokens,
    }?;
    (tokens > 0).then_some(tokens)
}

fn openai_stream_usage_included(probe: &RequestProbe<'_>) -> bool {
    probe
        .stream_options
        .as_ref()
        .and_then(|options| options.include_usage)
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
    let keys: &[&str] = match kind {
        BodyKind::OpenaiChat => &["max_completion_tokens", "max_tokens"],
        BodyKind::OpenaiResponses => &["max_output_tokens"],
        BodyKind::Anthropic => &["max_tokens"],
    };
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
}
