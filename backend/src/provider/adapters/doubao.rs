use axum::http::{HeaderMap, HeaderValue};
use bytes::Bytes;
use serde_json::{json, Map, Value};

use crate::{
    error::{AppError, AppResult},
    relay::{
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute};

pub(crate) static DOUBAO_ADAPTER: DoubaoAdapter = DoubaoAdapter;

pub(crate) struct DoubaoAdapter;

const SEEDANCE_TASKS_PATH: &str = "/contents/generations/tasks";

impl ProviderAdapter for DoubaoAdapter {
    fn name(&self) -> &'static str {
        "doubao"
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        match route {
            RelayRoute::Videos => seedance_tasks_url(base_url),
            _ => upstream_url(base_url, route.path()),
        }
    }

    fn resolve_bound_url(&self, base_url: &str, path: &str) -> (String, String) {
        if let Some(task_id) = seedance_task_id_from_openai_video_path(path) {
            let log_path = format!("{SEEDANCE_TASKS_PATH}/{task_id}");
            return (seedance_task_url(base_url, task_id), log_path);
        }
        (upstream_url(base_url, path), path.to_string())
    }

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        _protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        client_headers: &HeaderMap,
        _streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest> {
        let body = if route == RelayRoute::Videos {
            ensure_json_content_type(client_headers)?;
            openai_video_to_seedance(body)?
        } else {
            body
        };

        Ok(PreparedUpstreamRequest {
            url: self.resolve_url(&upstream.base_url, route),
            log_path: match route {
                RelayRoute::Videos => SEEDANCE_TASKS_PATH.to_string(),
                _ => route.path().to_string(),
            },
            body,
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        })
    }
}

fn ensure_json_content_type(headers: &HeaderMap) -> AppResult<()> {
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let content_type = content_type
        .to_str()
        .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?;
    if content_type
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        return Ok(());
    }
    Err(AppError::BadRequest(
        "Seedance native video upstream requires application/json requests".to_string(),
    ))
}

fn seedance_tasks_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/api/v3") {
        return upstream_url(base, SEEDANCE_TASKS_PATH);
    }
    format!("{base}/api/v3{SEEDANCE_TASKS_PATH}")
}

fn seedance_task_url(base_url: &str, task_id: &str) -> String {
    format!("{}/{}", seedance_tasks_url(base_url), task_id)
}

fn seedance_task_id_from_openai_video_path(path: &str) -> Option<&str> {
    let task_id = path.strip_prefix("/v1/videos/")?;
    if task_id.is_empty() || task_id.contains('/') {
        return None;
    }
    Some(task_id)
}

fn openai_video_to_seedance(body: Bytes) -> AppResult<Bytes> {
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

    let mut output = Map::new();
    output.insert("model".to_string(), Value::String(model.to_string()));
    merge_metadata(input, &mut output)?;
    copy_seedance_fields(input, &mut output);
    copy_seconds_as_duration(input, &mut output);
    let prompt = input.get("prompt").and_then(Value::as_str);
    merge_content(
        input,
        &mut output,
        prompt.is_some_and(|prompt| !prompt.trim().is_empty()),
    );

    if let Some(prompt) = prompt {
        append_text_content(&mut output, prompt);
    }
    append_image_content(input, &mut output);

    Ok(Bytes::from(serde_json::to_vec(&Value::Object(output))?))
}

fn merge_metadata(input: &Map<String, Value>, output: &mut Map<String, Value>) -> AppResult<()> {
    let Some(metadata) = input.get("metadata") else {
        return Ok(());
    };
    let metadata = metadata
        .as_object()
        .ok_or_else(|| AppError::BadRequest("metadata must be a JSON object".to_string()))?;
    for (key, value) in metadata {
        output.insert(key.clone(), value.clone());
    }
    Ok(())
}

fn copy_seedance_fields(input: &Map<String, Value>, output: &mut Map<String, Value>) {
    for key in [
        "callback_url",
        "return_last_frame",
        "service_tier",
        "execution_expires_after",
        "generate_audio",
        "draft",
        "tools",
        "resolution",
        "ratio",
        "duration",
        "frames",
        "seed",
        "camera_fixed",
        "watermark",
        "style",
    ] {
        if let Some(value) = input.get(key) {
            output.insert(key.to_string(), value.clone());
        }
    }
}

fn copy_seconds_as_duration(input: &Map<String, Value>, output: &mut Map<String, Value>) {
    if output.contains_key("duration") {
        return;
    }
    let seconds = input
        .get("seconds")
        .and_then(|value| {
            value
                .as_i64()
                .or_else(|| value.as_str()?.parse::<i64>().ok())
        })
        .filter(|seconds| *seconds > 0);
    if let Some(seconds) = seconds {
        output.insert("duration".to_string(), json!(seconds));
    }
}

fn merge_content(input: &Map<String, Value>, output: &mut Map<String, Value>, replace_text: bool) {
    let content = input
        .get("content")
        .or_else(|| output.get("content"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| !replace_text || item.get("type").and_then(Value::as_str) != Some("text"))
        .collect::<Vec<_>>();
    output.insert("content".to_string(), Value::Array(content));
}

fn append_text_content(output: &mut Map<String, Value>, prompt: &str) {
    if prompt.trim().is_empty() {
        return;
    }
    content_array(output).push(json!({
        "type": "text",
        "text": prompt,
    }));
}

fn append_image_content(input: &Map<String, Value>, output: &mut Map<String, Value>) {
    for image_url in image_urls(input) {
        content_array(output).push(json!({
            "type": "image_url",
            "image_url": {
                "url": image_url,
            },
            "role": "reference_image",
        }));
    }
}

fn image_urls(input: &Map<String, Value>) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(image) = input.get("image").and_then(Value::as_str) {
        urls.push(image.to_string());
    }
    if let Some(image) = input.get("input_reference").and_then(Value::as_str) {
        urls.push(image.to_string());
    }
    if let Some(images) = input.get("images").and_then(Value::as_array) {
        for image in images {
            if let Some(image) = image.as_str() {
                urls.push(image.to_string());
            }
        }
    }
    urls.retain(|url| !url.trim().is_empty());
    urls
}

fn content_array(output: &mut Map<String, Value>) -> &mut Vec<Value> {
    output
        .entry("content".to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("content is maintained as an array")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream() -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "doubao".to_string(),
            channel_name: "doubao".to_string(),
            base_url: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
            responses_chat_fallback: false,
            secret: "sk-test".to_string(),
            account_id: None,
            adapter_hint: None,
            affinity: None,
        }
    }

    #[test]
    fn seedance_url_handles_api_v3_and_root_bases() {
        assert_eq!(
            seedance_tasks_url("https://ark.cn-beijing.volces.com/api/v3"),
            "https://ark.cn-beijing.volces.com/api/v3/contents/generations/tasks"
        );
        assert_eq!(
            seedance_tasks_url("https://token.haxicloud.com"),
            "https://token.haxicloud.com/api/v3/contents/generations/tasks"
        );
    }

    #[test]
    fn remaps_openai_video_get_to_seedance_task_url() {
        let (url, log_path) = DOUBAO_ADAPTER.resolve_bound_url(
            "https://ark.cn-beijing.volces.com/api/v3",
            "/v1/videos/task_123",
        );
        assert_eq!(
            url,
            "https://ark.cn-beijing.volces.com/api/v3/contents/generations/tasks/task_123"
        );
        assert_eq!(log_path, "/contents/generations/tasks/task_123");
    }

    #[test]
    fn converts_openai_video_json_to_seedance_content_request() {
        let body = Bytes::from_static(
            br#"{"model":"doubao-seedance-2-0-260128","prompt":"walk by sea","seconds":"5","image":"https://example.com/a.png","ratio":"16:9","resolution":"480p","metadata":{"watermark":false}}"#,
        );
        let prepared = DOUBAO_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Videos,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(prepared.log_path, "/contents/generations/tasks");
        assert_eq!(value["model"], "doubao-seedance-2-0-260128");
        assert_eq!(value["duration"], 5);
        assert_eq!(value["ratio"], "16:9");
        assert_eq!(value["resolution"], "480p");
        assert_eq!(value["watermark"], false);
        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][0]["text"], "walk by sea");
        assert_eq!(value["content"][1]["type"], "image_url");
        assert_eq!(
            value["content"][1]["image_url"]["url"],
            "https://example.com/a.png"
        );
    }

    #[test]
    fn preserves_native_seedance_content_without_prompt() {
        let body = Bytes::from_static(
            br#"{"model":"dreamina-seedance-2-0-260128","content":[{"type":"text","text":"native prompt"},{"type":"image_url","image_url":{"url":"https://example.com/ref.png"}}],"duration":5}"#,
        );
        let prepared = DOUBAO_ADAPTER
            .prepare_openai_request(
                &upstream(),
                UpstreamProtocol::Openai,
                RelayRoute::Videos,
                body,
                &HeaderMap::new(),
                false,
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(value["content"][0]["type"], "text");
        assert_eq!(value["content"][0]["text"], "native prompt");
        assert_eq!(value["content"][1]["type"], "image_url");
    }
}
