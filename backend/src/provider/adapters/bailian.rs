use axum::http::{HeaderMap, HeaderName, HeaderValue};
use bytes::Bytes;

use crate::{
    error::AppResult,
    relay::{
        bridge,
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute};

pub(crate) static BAILIAN_ADAPTER: BailianAdapter = BailianAdapter;

pub(crate) struct BailianAdapter;

const DASH_SCOPE_SSE_HEADER: &str = "x-dashscope-sse";

impl ProviderAdapter for BailianAdapter {
    fn name(&self) -> &'static str {
        "bailian"
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        match route {
            RelayRoute::Responses => bailian_responses_url(base_url),
            _ => upstream_url(base_url, route.path()),
        }
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
        let (route, body, response_mode) =
            if route == RelayRoute::Responses && upstream.responses_chat_fallback {
                (
                    RelayRoute::ChatCompletions,
                    bridge::openai_response_to_openai_chat(body)?,
                    AdapterResponseMode::OpenAiChatAsOpenAiResponse,
                )
            } else {
                (route, body, AdapterResponseMode::Passthrough)
            };
        let mut extra_headers = HeaderMap::new();
        if streamed {
            extra_headers.insert(
                HeaderName::from_static(DASH_SCOPE_SSE_HEADER),
                HeaderValue::from_static("enable"),
            );
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

fn bailian_responses_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/compatible-mode/v1") {
        return format!("{base}/responses");
    }
    if base.ends_with("/compatible-mode") {
        return format!("{base}/v1/responses");
    }
    upstream_url(base, RelayRoute::Responses.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::selector::SelectedUpstream;

    fn upstream(responses_chat_fallback: bool) -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "qwen".to_string(),
            channel_name: "qwen".to_string(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            responses_chat_fallback,
            secret: "sk-test".to_string(),
            account_id: None,
        }
    }

    #[test]
    fn bailian_responses_url_uses_compatible_mode_path() {
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                RelayRoute::Responses
            ),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
        );
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode",
                RelayRoute::Responses
            ),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/responses"
        );
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                RelayRoute::ChatCompletions
            ),
            "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
        );
    }

    #[test]
    fn bailian_native_responses_preserves_request_fields() {
        let body = Bytes::from_static(
            br#"{"model":"qwen","input":"hi","previous_response_id":"resp_1","store":true,"instructions":"keep"}"#,
        );
        let prepared = BAILIAN_ADAPTER
            .prepare_openai_request(
                &upstream(false),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body.clone(),
                &HeaderMap::new(),
                true,
            )
            .unwrap();

        assert_eq!(prepared.response_mode, AdapterResponseMode::Passthrough);
        assert_eq!(prepared.body, body);
        assert!(prepared.url.ends_with("/compatible-mode/v1/responses"));
        assert_eq!(
            prepared.extra_headers.get(DASH_SCOPE_SSE_HEADER).unwrap(),
            "enable"
        );
    }

    #[test]
    fn bailian_fallback_responses_converts_to_chat_completions() {
        let body =
            Bytes::from_static(br#"{"model":"glm-5.2","input":"hi","max_output_tokens":16}"#);
        let prepared = BAILIAN_ADAPTER
            .prepare_openai_request(
                &upstream(true),
                UpstreamProtocol::Openai,
                RelayRoute::Responses,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(
            prepared.response_mode,
            AdapterResponseMode::OpenAiChatAsOpenAiResponse
        );
        assert!(prepared
            .url
            .ends_with("/compatible-mode/v1/chat/completions"));
        assert_eq!(value["model"], "glm-5.2");
        assert_eq!(value["messages"][0]["role"], "user");
        assert_eq!(value["messages"][0]["content"], "hi");
        assert_eq!(value["max_tokens"], 16);
        assert!(value.get("input").is_none());
    }
}
