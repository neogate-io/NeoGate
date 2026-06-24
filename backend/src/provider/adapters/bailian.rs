use axum::http::{HeaderMap, HeaderName, HeaderValue};
use bytes::Bytes;

use crate::{
    error::AppResult,
    relay::{
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{
    AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute, ResponsesPolicy,
};

pub(crate) static BAILIAN_ADAPTER: BailianAdapter = BailianAdapter;

pub(crate) struct BailianAdapter;

const DASH_SCOPE_SSE_HEADER: &str = "x-dashscope-sse";

impl ProviderAdapter for BailianAdapter {
    fn name(&self) -> &'static str {
        "bailian"
    }

    fn responses_policy(&self) -> ResponsesPolicy {
        ResponsesPolicy::Native
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        match route {
            RelayRoute::OpenAiResponses => bailian_responses_url(base_url),
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
            response_mode: AdapterResponseMode::Passthrough,
        })
    }
}

fn bailian_responses_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if let Some(root) = base.strip_suffix("/compatible-mode/v1") {
        return format!("{root}/api/v2/apps/protocols/compatible-mode/v1/responses");
    }
    if let Some(root) = base.strip_suffix("/compatible-mode") {
        return format!("{root}/api/v2/apps/protocols/compatible-mode/v1/responses");
    }
    format!("{base}/api/v2/apps/protocols/compatible-mode/v1/responses")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{admin::channel::ResponsesCapability, relay::selector::SelectedUpstream};

    fn upstream(responses_capability: ResponsesCapability) -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "qwen".to_string(),
            channel_name: "qwen".to_string(),
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1".to_string(),
            responses_capability,
            responses_checked_at: None,
            secret: "sk-test".to_string(),
            account_id: None,
        }
    }

    #[test]
    fn bailian_responses_url_uses_app_protocol_path() {
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                RelayRoute::OpenAiResponses
            ),
            "https://dashscope.aliyuncs.com/api/v2/apps/protocols/compatible-mode/v1/responses"
        );
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                RelayRoute::OpenAiChatCompletions
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
                &upstream(ResponsesCapability::ChatFallback),
                UpstreamProtocol::Openai,
                RelayRoute::OpenAiResponses,
                body.clone(),
                &HeaderMap::new(),
                true,
            )
            .unwrap();

        assert_eq!(prepared.response_mode, AdapterResponseMode::Passthrough);
        assert_eq!(prepared.body, body);
        assert!(prepared
            .url
            .ends_with("/api/v2/apps/protocols/compatible-mode/v1/responses"));
        assert_eq!(
            prepared.extra_headers.get(DASH_SCOPE_SSE_HEADER).unwrap(),
            "enable"
        );
    }
}
