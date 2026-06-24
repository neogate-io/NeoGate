use axum::http::HeaderMap;
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

pub(crate) static COMPATIBLE_ADAPTER: CompatibleAdapter = CompatibleAdapter;

pub(crate) struct CompatibleAdapter;

impl ProviderAdapter for CompatibleAdapter {
    fn name(&self) -> &'static str {
        "compatible"
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
        _streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest> {
        let (route, body, response_mode) = if route == RelayRoute::OpenAiResponses
            && upstream.responses_chat_fallback
        {
            (
                RelayRoute::OpenAiChatCompletions,
                bridge::openai_response_to_openai_chat(body)?,
                AdapterResponseMode::OpenAiChatAsOpenAiResponse,
            )
        } else {
            (route, body, AdapterResponseMode::Passthrough)
        };

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: route.path().to_string(),
            body,
            extra_headers: HeaderMap::new(),
            response_mode,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_url_matches_upstream_url() {
        assert_eq!(
            COMPATIBLE_ADAPTER.resolve_url(
                "https://api.openai.com/v1",
                RelayRoute::OpenAiChatCompletions
            ),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            COMPATIBLE_ADAPTER.resolve_url("https://api.openai.com", RelayRoute::OpenAiResponses),
            "https://api.openai.com/v1/responses"
        );
    }
}
