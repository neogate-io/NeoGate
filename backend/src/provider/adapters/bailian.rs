use axum::http::{HeaderMap, HeaderName, HeaderValue};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{
    error::{AppError, AppResult},
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
const DASH_SCOPE_ASYNC_HEADER: &str = "x-dashscope-async";
const BAILIAN_VIDEO_SYNTHESIS_PATH: &str = "/services/aigc/video-generation/video-synthesis";

impl ProviderAdapter for BailianAdapter {
    fn name(&self) -> &'static str {
        "bailian"
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        match route {
            RelayRoute::Responses => bailian_responses_url(base_url),
            RelayRoute::Videos => bailian_video_synthesis_url(base_url),
            _ => upstream_url(base_url, route.path()),
        }
    }

    fn resolve_bound_url(&self, base_url: &str, path: &str) -> (String, String) {
        if let Some(task_id) = bailian_task_id_from_openai_video_path(path) {
            let log_path = format!("/api/v1/tasks/{task_id}");
            return (bailian_task_url(base_url, task_id), log_path);
        }
        (upstream_url(base_url, path), path.to_string())
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
        let (route, body, response_mode) = if route == RelayRoute::Videos {
            (
                RelayRoute::Videos,
                openai_video_to_bailian(body)?,
                AdapterResponseMode::Passthrough,
            )
        } else if route == RelayRoute::Responses && upstream.responses_chat_fallback {
            (
                RelayRoute::ChatCompletions,
                bridge::openai_response_to_openai_chat(body)?,
                AdapterResponseMode::OpenAiChatAsOpenAiResponse,
            )
        } else {
            (route, body, AdapterResponseMode::Passthrough)
        };
        let mut extra_headers = HeaderMap::new();
        if route == RelayRoute::Videos {
            extra_headers.insert(
                HeaderName::from_static(DASH_SCOPE_ASYNC_HEADER),
                HeaderValue::from_static("enable"),
            );
        }
        if streamed {
            extra_headers.insert(
                HeaderName::from_static(DASH_SCOPE_SSE_HEADER),
                HeaderValue::from_static("enable"),
            );
        }

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: match route {
                RelayRoute::Videos => {
                    format!("/api/v1{BAILIAN_VIDEO_SYNTHESIS_PATH}")
                }
                _ => route.path().to_string(),
            },
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

fn bailian_video_synthesis_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let native_base = base
        .strip_suffix("/compatible-mode/v1")
        .or_else(|| base.strip_suffix("/compatible-mode"))
        .unwrap_or(base);
    if native_base.ends_with("/api/v1") {
        return upstream_url(native_base, BAILIAN_VIDEO_SYNTHESIS_PATH);
    }
    upstream_url(
        &format!("{native_base}/api/v1"),
        BAILIAN_VIDEO_SYNTHESIS_PATH,
    )
}

fn bailian_task_url(base_url: &str, task_id: &str) -> String {
    let base = base_url.trim_end_matches('/');
    let native_base = base
        .strip_suffix("/compatible-mode/v1")
        .or_else(|| base.strip_suffix("/compatible-mode"))
        .unwrap_or(base);
    if native_base.ends_with("/api/v1") {
        return upstream_url(native_base, &format!("/tasks/{task_id}"));
    }
    upstream_url(
        &format!("{native_base}/api/v1"),
        &format!("/tasks/{task_id}"),
    )
}

fn bailian_task_id_from_openai_video_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/v1/videos/")?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest)
}

fn openai_video_to_bailian(body: Bytes) -> AppResult<Bytes> {
    let value: Value = serde_json::from_slice(&body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let input = value
        .as_object()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;
    let model = input
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    let prompt = input
        .get("prompt")
        .or_else(|| input.get("input").and_then(|value| value.get("prompt")))
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or_else(|| AppError::BadRequest("prompt is required".to_string()))?;

    let mut parameters = serde_json::Map::new();
    if let Some(resolution) = input
        .get("resolution")
        .or_else(|| input.get("size"))
        .and_then(Value::as_str)
        .and_then(normalize_bailian_video_resolution)
    {
        parameters.insert("resolution".to_string(), Value::String(resolution));
    }
    if let Some(ratio) = input
        .get("ratio")
        .or_else(|| input.get("aspect_ratio"))
        .and_then(Value::as_str)
        .filter(|ratio| !ratio.trim().is_empty())
    {
        parameters.insert("ratio".to_string(), Value::String(ratio.to_string()));
    }
    if let Some(duration) = input
        .get("duration")
        .or_else(|| input.get("seconds"))
        .and_then(Value::as_i64)
        .filter(|duration| *duration > 0)
    {
        parameters.insert("duration".to_string(), json!(duration));
    }

    let mut output = json!({
        "model": model,
        "input": {
            "prompt": prompt
        }
    });
    if !parameters.is_empty() {
        output["parameters"] = Value::Object(parameters);
    }
    Ok(Bytes::from(serde_json::to_vec(&output)?))
}

fn normalize_bailian_video_resolution(value: &str) -> Option<String> {
    let normalized = value.trim().to_ascii_uppercase().replace(['X', '*'], "x");
    if normalized == "720P" || normalized.ends_with("x720") || normalized.contains("720") {
        return Some("720P".to_string());
    }
    if normalized == "1080P" || normalized.ends_with("x1080") || normalized.contains("1080") {
        return Some("1080P".to_string());
    }
    None
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
            affinity: None,
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
    fn bailian_video_url_uses_native_async_task_path() {
        assert_eq!(
            BAILIAN_ADAPTER.resolve_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1",
                RelayRoute::Videos
            ),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/video-generation/video-synthesis"
        );
        assert_eq!(
            BAILIAN_ADAPTER
                .resolve_url("https://dashscope.aliyuncs.com/api/v1", RelayRoute::Videos),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/video-generation/video-synthesis"
        );
    }

    #[test]
    fn bailian_video_get_uses_native_task_path() {
        let (url, log_path) = BAILIAN_ADAPTER.resolve_bound_url(
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "/v1/videos/task_123",
        );

        assert_eq!(url, "https://dashscope.aliyuncs.com/api/v1/tasks/task_123");
        assert_eq!(log_path, "/api/v1/tasks/task_123");
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

    #[test]
    fn bailian_videos_converts_openai_video_create_to_native_task() {
        let body = Bytes::from_static(
            br#"{"model":"happyhorse-1.1-t2v","prompt":"ping","resolution":"720P","seconds":3}"#,
        );
        let prepared = BAILIAN_ADAPTER
            .prepare_openai_request(
                &upstream(false),
                UpstreamProtocol::Openai,
                RelayRoute::Videos,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(
            prepared.url,
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/video-generation/video-synthesis"
        );
        assert_eq!(
            prepared.extra_headers.get(DASH_SCOPE_ASYNC_HEADER).unwrap(),
            "enable"
        );
        assert_eq!(value["model"], "happyhorse-1.1-t2v");
        assert_eq!(value["input"]["prompt"], "ping");
        assert_eq!(value["parameters"]["resolution"], "720P");
        assert_eq!(value["parameters"]["duration"], 3);
    }
}
