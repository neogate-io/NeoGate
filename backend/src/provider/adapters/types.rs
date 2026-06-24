use axum::http::HeaderMap;
use bytes::Bytes;

use crate::{
    error::AppResult,
    relay::selector::{SelectedUpstream, UpstreamProtocol},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum RelayRoute {
    OpenAiChatCompletions,
    OpenAiResponses,
    OpenAiEmbeddings,
    OpenAiModerations,
    OpenAiImageGenerations,
    OpenAiImageEdits,
    OpenAiImageVariations,
}

impl RelayRoute {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "/v1/chat/completions",
            Self::OpenAiResponses => "/v1/responses",
            Self::OpenAiEmbeddings => "/v1/embeddings",
            Self::OpenAiModerations => "/v1/moderations",
            Self::OpenAiImageGenerations => "/v1/images/generations",
            Self::OpenAiImageEdits => "/v1/images/edits",
            Self::OpenAiImageVariations => "/v1/images/variations",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdapterResponseMode {
    Passthrough,
    OpenAiChatAsOpenAiResponse,
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
    #[allow(dead_code)]
    fn name(&self) -> &'static str;

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String;

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        client_headers: &HeaderMap,
        streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest>;
}
