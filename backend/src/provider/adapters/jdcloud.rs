use axum::http::{header::ACCEPT, HeaderMap, HeaderValue};
use bytes::Bytes;
use serde_json::Value;

use crate::{
    error::{AppError, AppResult},
    relay::{
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{
    AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute, ResponsesPolicy,
};

pub(crate) static JDCLOUD_ADAPTER: JdcloudAdapter = JdcloudAdapter;

pub(crate) struct JdcloudAdapter;

impl ProviderAdapter for JdcloudAdapter {
    fn name(&self) -> &'static str {
        "jdcloud"
    }

    fn responses_policy(&self) -> ResponsesPolicy {
        ResponsesPolicy::Native
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
        let body = jdcloud_chat_cache_body(body, route)?;

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: route.path().to_string(),
            body,
            extra_headers,
            response_mode: AdapterResponseMode::Passthrough,
        })
    }
}

fn jdcloud_chat_cache_body(body: Bytes, route: RelayRoute) -> AppResult<Bytes> {
    let Some(prompt_cache_key) = prompt_cache_key_from_body(&body)? else {
        return Ok(body);
    };
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let Some(object) = value.as_object_mut() else {
        return Ok(body);
    };
    if route == RelayRoute::OpenAiChatCompletions {
        reorder_system_cache_prefix(object.get_mut("messages"));
    }
    let has_session_id = object
        .get("session_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    if !has_session_id {
        object.insert("session_id".to_string(), Value::String(prompt_cache_key));
    }
    Ok(Bytes::from(serde_json::to_vec(&value)?))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{admin::channel::ResponsesCapability, relay::selector::SelectedUpstream};

    fn upstream() -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "jdcloud".to_string(),
            channel_name: "jdcloud".to_string(),
            base_url: "https://agentrs.jd.com/api/saas/openai-u/v1".to_string(),
            responses_capability: ResponsesCapability::Native,
            responses_checked_at: None,
            secret: "sk-test".to_string(),
            account_id: None,
        }
    }

    #[test]
    fn jdcloud_urls_use_openai_u_v1_base() {
        assert_eq!(
            JDCLOUD_ADAPTER.resolve_url(
                "https://agentrs.jd.com/api/saas/openai-u/v1",
                RelayRoute::OpenAiResponses
            ),
            "https://agentrs.jd.com/api/saas/openai-u/v1/responses"
        );
        assert_eq!(
            JDCLOUD_ADAPTER.resolve_url(
                "https://agentrs.jd.com/api/saas/openai-u/v1",
                RelayRoute::OpenAiChatCompletions
            ),
            "https://agentrs.jd.com/api/saas/openai-u/v1/chat/completions"
        );
    }

    #[test]
    fn jdcloud_responses_preserves_multiturn_fields() {
        let body = Bytes::from_static(
            br#"{"model":"deepseek-v3.2","input":"hi","previous_response_id":"resp_1","store":true,"instructions":"keep"}"#,
        );
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::OpenAiResponses,
                body.clone(),
                &HeaderMap::new(),
                false,
            )
            .unwrap();

        assert_eq!(prepared.response_mode, AdapterResponseMode::Passthrough);
        assert_eq!(prepared.body, body);
        assert!(prepared.url.ends_with("/openai-u/v1/responses"));
        assert!(prepared.extra_headers.is_empty());
    }

    #[test]
    fn jdcloud_streaming_requests_accept_event_stream() {
        let body = Bytes::from_static(br#"{"model":"deepseek-v3.2","input":"hi","stream":true}"#);
        let prepared = JDCLOUD_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::OpenAiResponses,
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
                RelayRoute::OpenAiChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["prompt_cache_key"], "anthropic-cache-1");
        assert_eq!(value["session_id"], "anthropic-cache-1");
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
                RelayRoute::OpenAiChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["prompt_cache_key"], "anthropic-cache-1");
        assert_eq!(value["session_id"], "client-session");
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
                RelayRoute::OpenAiChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();
        let content = value["messages"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["text"], "stable-a");
        assert_eq!(content[1]["text"], "stable-b");
        assert_eq!(content.len(), 2);
        let user_content = value["messages"][1]["content"].as_array().unwrap();
        assert_eq!(user_content[0], "hi");
        assert_eq!(user_content[1]["text"], "volatile");
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
                RelayRoute::OpenAiChatCompletions,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();
        let content = value["messages"][0]["content"].as_array().unwrap();

        assert_eq!(content[0]["text"], "stable");
        assert_eq!(content[1]["text"], "volatile");
    }
}
