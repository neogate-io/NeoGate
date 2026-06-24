use axum::http::{header::ACCEPT, HeaderMap, HeaderValue};
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

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: route.path().to_string(),
            body,
            extra_headers,
            response_mode: AdapterResponseMode::Passthrough,
        })
    }
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
}
