use axum::http::HeaderMap;
use bytes::Bytes;

use crate::{
    error::AppResult,
    relay::selector::{SelectedUpstream, UpstreamProtocol},
    relay::upstream_url,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// Keep the complete OpenAI-compatible route set in one place so provider
// adapters can opt in without changing the shared trait surface.
#[allow(dead_code)]
pub(crate) enum RelayRoute {
    ChatCompletions,
    Responses,
    Embeddings,
    Moderations,
    ImageGenerations,
    ImageEdits,
    ImageVariations,
    Videos,
}

impl RelayRoute {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "/v1/chat/completions",
            Self::Responses => "/v1/responses",
            Self::Embeddings => "/v1/embeddings",
            Self::Moderations => "/v1/moderations",
            Self::ImageGenerations => "/v1/images/generations",
            Self::ImageEdits => "/v1/images/edits",
            Self::ImageVariations => "/v1/images/variations",
            Self::Videos => "/v1/videos",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterResponseMode {
    Passthrough,
    OpenAiChatAsOpenAiResponse,
}

impl AdapterResponseMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::OpenAiChatAsOpenAiResponse => "openai_chat_as_openai_response",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedUpstreamRequest {
    pub(crate) url: String,
    pub(crate) log_path: String,
    pub(crate) body: Bytes,
    pub(crate) extra_headers: HeaderMap,
    pub(crate) response_mode: AdapterResponseMode,
}

pub(crate) trait ProviderAdapter: Sync {
    // Useful in adapter diagnostics and tests even when a call site only needs
    // the prepared upstream request.
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String;

    fn resolve_bound_url(&self, base_url: &str, path: &str) -> (String, String) {
        (upstream_url(base_url, path), path.to_string())
    }

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        client_headers: &HeaderMap,
        streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest>;

    fn normalize_response_body(&self, _route: RelayRoute, body: Bytes) -> AppResult<Bytes> {
        Ok(body)
    }
}
