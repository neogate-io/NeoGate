use axum::http::{header::ACCEPT, HeaderMap, HeaderName, HeaderValue};
use bytes::Bytes;
use serde_json::Value;

use crate::{
    error::{AppError, AppResult},
    relay::{
        bridge,
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute};

pub(crate) static JDCLOUD_ADAPTER: JdcloudAdapter = JdcloudAdapter;

pub(crate) struct JdcloudAdapter;

impl ProviderAdapter for JdcloudAdapter {
    fn name(&self) -> &'static str {
        "jdcloud"
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        upstream_url(base_url, route.path())
    }

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        _protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        _client_headers: &HeaderMap,
        streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest> {
        let mut extra_headers = HeaderMap::new();
        if streamed {
            extra_headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        }
        let (route, body, response_mode) = if route == RelayRoute::Responses {
            let route = RelayRoute::ChatCompletions;
            (
                route,
                jdcloud_chat_body(bridge::openai_response_to_openai_chat(body)?, route)?,
                AdapterResponseMode::OpenAiChatAsOpenAiResponse,
            )
        } else {
            (
                route,
                jdcloud_chat_body(body, route)?,
                AdapterResponseMode::Passthrough,
            )
        };
        if let Some(session_id) = session_id_from_body(&body)? {
            if let Ok(value) = HeaderValue::from_str(&session_id) {
                extra_headers.insert(HeaderName::from_static("session_id"), value);
            }
        }

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: route.path().to_string(),
            body,
            extra_headers,
            response_mode,
        })
    }
}

fn jdcloud_chat_body(body: Bytes, route: RelayRoute) -> AppResult<Bytes> {
    let prompt_cache_key = prompt_cache_key_from_body(&body)?;
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let Some(object) = value.as_object_mut() else {
        return Ok(body);
    };
    if route == RelayRoute::ChatCompletions {
        if prompt_cache_key.is_some() {
            reorder_system_cache_prefix(object.get_mut("messages"));
        }
        sanitize_chat_messages(object.get_mut("messages"));
        sanitize_tools(object.get_mut("tools"));
        object.remove("stream_options");
    }
    if let Some(prompt_cache_key) = prompt_cache_key {
        let has_session_id = object
            .get("session_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|value| !value.is_empty());
        if !has_session_id {
            object.insert("session_id".to_string(), Value::String(prompt_cache_key));
        }
        object.remove("prompt_cache_key");
    }
    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn sanitize_chat_messages(messages: Option<&mut Value>) {
    let Some(Value::Array(messages)) = messages else {
        return;
    };
    for message in messages {
        let Some(message) = message.as_object_mut() else {
            continue;
        };
        if let Some(content) = message.get_mut("content") {
            sanitize_chat_content(content);
        }
    }
}

fn sanitize_chat_content(content: &mut Value) {
    let Value::Array(items) = content else {
        return;
    };
    for item in items.iter_mut() {
        if let Some(object) = item.as_object_mut() {
            object.remove("cache_control");
        }
    }
    if let Some(text) = text_only_content(items) {
        *content = Value::String(text);
    }
}

fn text_only_content(items: &[Value]) -> Option<String> {
    let mut parts = Vec::new();
    for item in items {
        match item {
            Value::String(text) => parts.push(text.clone()),
            Value::Object(object) if object.get("type").and_then(Value::as_str) == Some("text") => {
                parts.push(object.get("text").and_then(Value::as_str)?.to_string());
            }
            _ => return None,
        }
    }
    Some(parts.join("\n"))
}

fn sanitize_tools(tools: Option<&mut Value>) {
    let Some(Value::Array(tools)) = tools else {
        return;
    };
    for tool in tools {
        let Some(tool) = tool.as_object_mut() else {
            continue;
        };
        tool.remove("cache_control");
    }
}

fn reorder_system_cache_prefix(messages: Option<&mut Value>) {
    let Some(Value::Array(messages)) = messages else {
        return;
    };
    let Some(user_index) = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"))
    else {
        return;
    };
    let Some(first_message) = messages.first_mut() else {
        return;
    };
    let Some(first_message) = first_message.as_object_mut() else {
        return;
    };
    if first_message.get("role").and_then(Value::as_str) != Some("system") {
        return;
    }
    let Some(Value::Array(content)) = first_message.get_mut("content") else {
        return;
    };
    let Some(first_cache_index) = content.iter().position(has_cache_control) else {
        return;
    };
    if first_cache_index == 0 {
        return;
    }

    let leading = content.drain(..first_cache_index).collect::<Vec<_>>();
    append_system_tail_to_user(&mut messages[user_index], leading);
}

fn append_system_tail_to_user(user_message: &mut Value, leading: Vec<Value>) {
    let Some(user_message) = user_message.as_object_mut() else {
        return;
    };
    append_content_items(
        user_message
            .entry("content")
            .or_insert(Value::String(String::new())),
        leading,
    );
}

fn append_content_items(content: &mut Value, mut items: Vec<Value>) {
    match content {
        Value::Array(existing) => {
            existing.append(&mut items);
        }
        Value::String(text) => {
            let mut combined = Vec::new();
            if !text.is_empty() {
                combined.push(Value::String(std::mem::take(text)));
            }
            combined.append(&mut items);
            *content = Value::Array(combined);
        }
        _ => {
            *content = Value::Array(items);
        }
    }
}

fn has_cache_control(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("cache_control"))
}

fn prompt_cache_key_from_body(body: &[u8]) -> AppResult<Option<String>> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    Ok(value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

fn session_id_from_body(body: &[u8]) -> AppResult<Option<String>> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    Ok(value
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::selector::SelectedUpstream;

    fn upstream() -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "jdcloud".to_string(),
            channel_name: "jdcloud".to_string(),
            base_url: "https://agentrs.jd.com/api/saas/openai-u/v1".to_string(),
            responses_chat_fallback: false,
            secret: "sk-test".to_string(),
            account_id: None,
        }
    }

    #[test]
    fn jdcloud_urls_use_openai_u_v1_base() {
        assert_eq!(
            JDCLOUD_ADAPTER.resolve_url(
                "https://agentrs.jd.com/api/saas/openai-u/v1",
                RelayRoute::Responses
            ),
            "https://agentrs.jd.com/api/saas/openai-u/v1/responses"
        );
        assert_eq!(
            JDCLOUD_ADAPTER.resolve_url(
                "https://agentrs.jd.com/api/saas/openai-u/v1",
                RelayRoute::ChatCompletions
            ),
            "https://agentrs.jd.com/api/saas/openai-u/v1/chat/completions"
        );
    }

    #[test]
    fn jdcloud_responses_use_chat_fallback() {
        let body = Bytes::from_static(
            br#"{"model":"deepseek-v3.2","input":"hi","max_output_tokens":16,"previous_response_id":"resp_1","store":true,"instructions":"keep"}"#,
        );
        let err = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("responses chat fallback does not support previous_response_id"));
    }

    #[test]
    fn jdcloud_simple_responses_convert_to_chat_completions() {
        let body =
            Bytes::from_static(br#"{"model":"deepseek-v3.2","input":"hi","max_output_tokens":16}"#);
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(
            prepared.response_mode,
            AdapterResponseMode::OpenAiChatAsOpenAiResponse
        );
        assert!(prepared.url.ends_with("/openai-u/v1/chat/completions"));
        assert!(prepared.extra_headers.get(ACCEPT).is_none());
        assert_eq!(value["model"], "deepseek-v3.2");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "hi");
        assert_eq!(value["max_tokens"], 16);
        assert!(value.get("input").is_none());
        assert!(value.get("max_output_tokens").is_none());
    }

    #[test]
    fn jdcloud_streaming_requests_accept_event_stream() {
        let body = Bytes::from_static(br#"{"model":"deepseek-v3.2","input":"hi","stream":true}"#);
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body,
                &HeaderMap::new(),
                true,
            )
            .unwrap();

        assert_eq!(
            prepared.extra_headers.get(ACCEPT).unwrap(),
            "text/event-stream"
        );
    }

    #[test]
    fn jdcloud_copies_prompt_cache_key_to_session_id_body_field() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[],"prompt_cache_key":"anthropic-cache-1"}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(value.get("prompt_cache_key").is_none());
        assert_eq!(value["session_id"], "anthropic-cache-1");
        assert_eq!(
            prepared.extra_headers.get("session_id").unwrap(),
            "anthropic-cache-1"
        );
    }

    #[test]
    fn jdcloud_preserves_existing_session_id_body_field() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[],"prompt_cache_key":"anthropic-cache-1","session_id":"client-session"}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert!(value.get("prompt_cache_key").is_none());
        assert_eq!(value["session_id"], "client-session");
        assert_eq!(
            prepared.extra_headers.get("session_id").unwrap(),
            "client-session"
        );
    }

    #[test]
    fn jdcloud_sanitizes_anthropic_cache_extensions_from_chat_body() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[{"role":"system","content":[{"type":"text","text":"stable","cache_control":{"type":"ephemeral"}}]},{"role":"user","content":[{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}},{"type":"text","text":"there"}]}],"tools":[{"type":"function","function":{"name":"lookup","parameters":{"type":"object"}},"cache_control":{"type":"ephemeral"}}],"prompt_cache_key":"anthropic-cache-1","stream_options":{"include_usage":true}}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["messages"][0]["content"], "stable");
        assert_eq!(value["messages"][1]["content"], "hi\nthere");
        assert!(!value["messages"][0]["content"]
            .to_string()
            .contains("cache_control"));
        assert!(value["tools"][0].get("cache_control").is_none());
        assert!(value.get("prompt_cache_key").is_none());
        assert!(value.get("stream_options").is_none());
        assert_eq!(value["session_id"], "anthropic-cache-1");
        assert_eq!(
            prepared.extra_headers.get("session_id").unwrap(),
            "anthropic-cache-1"
        );
    }

    #[test]
    fn jdcloud_keeps_session_id_stable_for_matching_cache_blocks() {
        let first = bridge::messages_to_openai_chat(Bytes::from_static(
            br#"{"model":"GLM-5.1","system":[{"type":"text","text":"Stable system","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":"Fresh question A"}],"max_tokens":16}"#,
        ))
        .unwrap();
        let second = bridge::messages_to_openai_chat(Bytes::from_static(
            br#"{"model":"GLM-5.1","system":[{"type":"text","text":"Stable system","cache_control":{"type":"ephemeral"}}],"messages":[{"role":"user","content":"Fresh question B"}],"max_tokens":16}"#,
        ))
        .unwrap();

        let first = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                first,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let second = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                second,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let first_value: Value = serde_json::from_slice(&first.body).unwrap();
        let second_value: Value = serde_json::from_slice(&second.body).unwrap();

        assert_eq!(first_value["session_id"], second_value["session_id"]);
        assert_eq!(
            first.extra_headers.get("session_id"),
            second.extra_headers.get("session_id")
        );
        assert_eq!(first_value["messages"][0]["content"], "Stable system");
        assert_eq!(first_value["messages"][1]["content"], "Fresh question A");
        assert_eq!(second_value["messages"][1]["content"], "Fresh question B");
    }

    #[test]
    fn jdcloud_moves_uncached_system_prefix_to_last_user_message() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[{"role":"system","content":[{"type":"text","text":"volatile"},{"type":"text","text":"stable-a","cache_control":{"type":"ephemeral"}},{"type":"text","text":"stable-b","cache_control":{"type":"ephemeral"}}]},{"role":"user","content":"hi"}],"prompt_cache_key":"anthropic-cache-1"}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["messages"][0]["content"], "stable-a\nstable-b");
        assert_eq!(value["messages"][1]["content"], "hi\nvolatile");
        assert_eq!(value["session_id"], "anthropic-cache-1");
        assert!(value.get("prompt_cache_key").is_none());
    }

    #[test]
    fn jdcloud_leaves_system_cache_prefix_when_already_first() {
        let body = Bytes::from_static(
            br#"{"model":"GLM-5.1","messages":[{"role":"system","content":[{"type":"text","text":"stable","cache_control":{"type":"ephemeral"}},{"type":"text","text":"volatile"}]}],"prompt_cache_key":"anthropic-cache-1"}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::ChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["messages"][0]["content"], "stable\nvolatile");
    }

    #[test]
    fn jdcloud_responses_fallback_sanitizes_chat_request_body() {
        let body = Bytes::from_static(
            br#"{"model":"deepseek-v3.2","input":[{"role":"user","content":[{"type":"input_text","text":"hi"}]}],"max_output_tokens":16,"prompt_cache_key":"trace-1","stream":true}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body,
                &HeaderMap::new(),
                true,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(prepared.log_path, "/v1/chat/completions");
        assert!(prepared.url.ends_with("/openai-u/v1/chat/completions"));
        assert_eq!(value["messages"][0]["content"], "hi");
        assert_eq!(value["session_id"], "trace-1");
        assert_eq!(prepared.extra_headers.get("session_id").unwrap(), "trace-1");
        assert!(value.get("prompt_cache_key").is_none());
        assert!(value.get("stream_options").is_none());
    }
}
