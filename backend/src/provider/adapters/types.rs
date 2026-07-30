use axum::http::{HeaderMap, HeaderValue, StatusCode};
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
    ResponsesCompact,
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
            Self::ResponsesCompact => "/v1/responses/compact",
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProviderCapabilities {
    pub(crate) handles_image_stream_response: bool,
    pub(crate) translates_response_image_generation: bool,
}

/// 上游对 Anthropic 协议各端点的支持能力。
///
/// Anthropic 路径（`UpstreamProtocol::Anthropic`）不走 `ProviderAdapter`——上游接受
/// 标准 Anthropic 格式，无需 body 改写。不同上游的差异只体现在“实现了哪些端点”。
/// 例如 new-api 的 Anthropic 中继只实现了 `/v1/messages`，缺少 count_tokens 与
/// batches 系列端点。这里集中声明各端点能力，供 handler 决定是本地兜底还是返回
/// 明确的“上游不支持”错误，而不是把上游的 404 透传给客户端。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnthropicCapabilities {
    /// 是否支持 `POST /v1/messages/count_tokens`
    pub(crate) supports_count_tokens: bool,
    /// 是否支持 `/v1/messages/batches` 系列端点
    pub(crate) supports_batch: bool,
}

impl Default for AnthropicCapabilities {
    fn default() -> Self {
        // 默认按真 Anthropic 上游处理：全部支持。
        Self {
            supports_count_tokens: true,
            supports_batch: true,
        }
    }
}

impl AnthropicCapabilities {
    /// 根据 channel_endpoint.adapter_hint 推断上游能力。
    /// 目前仅 new-api（hint == "newapi"）已知缺少 count_tokens 与 batches。
    pub(crate) fn for_adapter_hint(hint: Option<&str>) -> Self {
        let is_newapi = hint.is_some_and(|hint| hint.eq_ignore_ascii_case("newapi"));
        if is_newapi {
            Self {
                supports_count_tokens: false,
                supports_batch: false,
            }
        } else {
            Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum AdapterErrorDisposition {
    #[default]
    Default,
    Retryable,
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

#[derive(Debug, Clone)]
pub(crate) struct PreparedHttpRetry {
    pub(crate) route: RelayRoute,
    pub(crate) body: Bytes,
    pub(crate) content_type: HeaderValue,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedResponseImageGenerationRequest {
    pub(crate) body: Bytes,
    pub(crate) model: String,
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

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::default()
    }

    fn classify_http_error(
        &self,
        _route: RelayRoute,
        _status: StatusCode,
        _body: &[u8],
    ) -> AdapterErrorDisposition {
        AdapterErrorDisposition::Default
    }

    fn prepare_response_image_generation_request(
        &self,
        _body: Bytes,
    ) -> AppResult<Option<PreparedResponseImageGenerationRequest>> {
        Ok(None)
    }

    fn prepare_http_error_retry(
        &self,
        _route: RelayRoute,
        _status: StatusCode,
        _error_body: &[u8],
        _request_body: &Bytes,
        _content_type: &HeaderValue,
    ) -> AppResult<Option<PreparedHttpRetry>> {
        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newapi_lacks_count_tokens_and_batch() {
        let caps = AnthropicCapabilities::for_adapter_hint(Some("newapi"));
        assert!(!caps.supports_count_tokens);
        assert!(!caps.supports_batch);
    }

    #[test]
    fn adapter_hint_match_is_case_insensitive() {
        let caps = AnthropicCapabilities::for_adapter_hint(Some("NewApi"));
        assert!(!caps.supports_count_tokens);
        assert!(!caps.supports_batch);
    }

    #[test]
    fn unknown_and_absent_hints_default_to_full_support() {
        for hint in [None, Some("openai"), Some("anthropic"), Some("doubao")] {
            let caps = AnthropicCapabilities::for_adapter_hint(hint);
            assert!(caps.supports_count_tokens, "hint={hint:?}");
            assert!(caps.supports_batch, "hint={hint:?}");
        }
    }
}
