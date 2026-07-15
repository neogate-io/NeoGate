use axum::http::{HeaderMap, StatusCode};
use bytes::Bytes;
use serde_json::Value;

use crate::{
    error::{AppError, AppResult},
    relay::selector::{SelectedUpstream, UpstreamProtocol},
};

use super::{
    compatible::COMPATIBLE_ADAPTER, AdapterErrorDisposition, PreparedHttpRetry,
    PreparedResponseImageGenerationRequest, PreparedUpstreamRequest, ProviderAdapter,
    ProviderCapabilities, RelayRoute,
};

pub(crate) static NEWAPI_ADAPTER: NewApiAdapter = NewApiAdapter;
const VARIATION_PROMPT: &str = "Create a variation of the input image.";

pub(crate) struct NewApiAdapter;

impl ProviderAdapter for NewApiAdapter {
    fn name(&self) -> &'static str {
        "newapi"
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        COMPATIBLE_ADAPTER.resolve_url(base_url, route)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            handles_image_stream_response: true,
            translates_response_image_generation: true,
        }
    }

    fn classify_http_error(
        &self,
        route: RelayRoute,
        status: StatusCode,
        body: &[u8],
    ) -> AdapterErrorDisposition {
        if route != RelayRoute::ImageVariations || status != StatusCode::BAD_REQUEST {
            return AdapterErrorDisposition::Default;
        }

        if is_variation_model_error(body) {
            AdapterErrorDisposition::Retryable
        } else {
            AdapterErrorDisposition::Default
        }
    }

    fn prepare_response_image_generation_request(
        &self,
        body: Bytes,
    ) -> AppResult<Option<PreparedResponseImageGenerationRequest>> {
        let request: Value = serde_json::from_slice(&body)?;
        let request = image_generation_request(&request)?;
        let model = request
            .get("model")
            .and_then(Value::as_str)
            .expect("validated image generation model")
            .to_string();
        Ok(Some(PreparedResponseImageGenerationRequest {
            body: Bytes::from(serde_json::to_vec(&request)?),
            model,
        }))
    }

    fn prepare_http_error_retry(
        &self,
        route: RelayRoute,
        status: StatusCode,
        error_body: &[u8],
        request_body: &Bytes,
        content_type: &axum::http::HeaderValue,
    ) -> AppResult<Option<PreparedHttpRetry>> {
        if route != RelayRoute::ImageVariations
            || status != StatusCode::BAD_REQUEST
            || !is_variation_model_error(error_body)
        {
            return Ok(None);
        }

        let content_type_text = content_type
            .to_str()
            .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?;
        let boundary = multipart_boundary(content_type_text)?;
        Ok(Some(PreparedHttpRetry {
            route: RelayRoute::ImageEdits,
            body: ensure_multipart_prompt(request_body, &boundary)?,
            content_type: content_type.clone(),
        }))
    }

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        client_headers: &HeaderMap,
        streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest> {
        COMPATIBLE_ADAPTER.prepare_openai_request(
            upstream,
            protocol,
            route,
            body,
            client_headers,
            streamed,
        )
    }
}

fn is_variation_model_error(error_body: &[u8]) -> bool {
    let body = String::from_utf8_lossy(error_body).to_ascii_lowercase();
    body.contains("new_api_error")
        && (body.contains("model name not specified")
            || body.contains("未指定模型名称")
            || body.contains("模型名称不能为空"))
}

fn multipart_boundary(content_type: &str) -> AppResult<String> {
    for part in content_type.split(';').skip(1) {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("boundary") {
            let boundary = value.trim().trim_matches('"');
            if !boundary.is_empty() {
                return Ok(boundary.to_string());
            }
        }
    }
    Err(AppError::BadRequest(
        "multipart/form-data boundary is required".to_string(),
    ))
}

fn ensure_multipart_prompt(body: &Bytes, boundary: &str) -> AppResult<Bytes> {
    if has_prompt_field(body) {
        return Ok(body.clone());
    }
    let closing_marker = format!("--{boundary}--").into_bytes();
    let Some(closing_offset) = find_last_bytes(body, &closing_marker) else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };

    let mut rewritten =
        Vec::with_capacity(body.len() + VARIATION_PROMPT.len() + boundary.len() + 80);
    rewritten.extend_from_slice(&body[..closing_offset]);
    if !rewritten.ends_with(b"\r\n") {
        rewritten.extend_from_slice(b"\r\n");
    }
    rewritten.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    rewritten.extend_from_slice(b"Content-Disposition: form-data; name=\"prompt\"\r\n\r\n");
    rewritten.extend_from_slice(VARIATION_PROMPT.as_bytes());
    rewritten.extend_from_slice(b"\r\n");
    rewritten.extend_from_slice(&body[closing_offset..]);
    Ok(Bytes::from(rewritten))
}

fn has_prompt_field(body: &[u8]) -> bool {
    body.windows(b"name=\"prompt\"".len())
        .any(|window| window == b"name=\"prompt\"")
        || body
            .windows(b"name=prompt".len())
            .any(|window| window == b"name=prompt")
}

fn find_last_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn image_generation_request(request: &Value) -> AppResult<Value> {
    let object = request.as_object().ok_or_else(|| {
        crate::error::AppError::BadRequest("request body must be an object".into())
    })?;
    let tool = object
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools
                .iter()
                .find(|tool| tool.get("type").and_then(Value::as_str) == Some("image_generation"))
        })
        .and_then(Value::as_object)
        .ok_or_else(|| {
            crate::error::AppError::BadRequest("image_generation tool is required".into())
        })?;
    let model = tool
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| {
            crate::error::AppError::BadRequest(
                "tools[].model is required for NewAPI image generation".into(),
            )
        })?;

    let mut prompt_parts = Vec::new();
    if let Some(instructions) = object.get("instructions") {
        collect_prompt_text(instructions, &mut prompt_parts);
    }
    if let Some(input) = object.get("input") {
        collect_prompt_text(input, &mut prompt_parts);
    }
    let prompt = prompt_parts
        .into_iter()
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if prompt.is_empty() {
        return Err(crate::error::AppError::BadRequest(
            "image_generation input must include text".into(),
        ));
    }

    let mut output = serde_json::Map::new();
    output.insert("model".into(), Value::String(model.to_string()));
    output.insert("prompt".into(), Value::String(prompt));
    for field in [
        "size",
        "quality",
        "output_format",
        "output_compression",
        "background",
        "moderation",
        "n",
    ] {
        if let Some(value) = tool.get(field) {
            output.insert(field.to_string(), value.clone());
        }
    }
    Ok(Value::Object(output))
}

fn collect_prompt_text(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(text) => output.push(text.clone()),
        Value::Array(items) => {
            for item in items {
                collect_prompt_text(item, output);
            }
        }
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                output.push(text.to_string());
            } else if let Some(content) = object.get("content") {
                collect_prompt_text(content, output);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_variation_model_error_as_retryable() {
        let english = br#"{"error":{"message":"Model name not specified, model name cannot be empty; type=new_api_error"}}"#;
        let chinese =
            r#"{"error":{"message":"未指定模型名称，模型名称不能为空; type=new_api_error"}}"#
                .as_bytes();

        for body in [english.as_slice(), chinese] {
            assert_eq!(
                NEWAPI_ADAPTER.classify_http_error(
                    RelayRoute::ImageVariations,
                    StatusCode::BAD_REQUEST,
                    body,
                ),
                AdapterErrorDisposition::Retryable
            );
        }
        assert_eq!(
            NEWAPI_ADAPTER.classify_http_error(
                RelayRoute::ImageEdits,
                StatusCode::BAD_REQUEST,
                chinese,
            ),
            AdapterErrorDisposition::Default
        );
    }

    #[test]
    fn retries_variation_as_edit_with_prompt() {
        let body = Bytes::from_static(
            b"--x\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-2\r\n--x\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n--x--\r\n",
        );
        let content_type = axum::http::HeaderValue::from_static("multipart/form-data; boundary=x");
        let error =
            r#"{"error":{"message":"未指定模型名称，模型名称不能为空; type=new_api_error"}}"#
                .as_bytes();

        let retry = NEWAPI_ADAPTER
            .prepare_http_error_retry(
                RelayRoute::ImageVariations,
                StatusCode::BAD_REQUEST,
                error,
                &body,
                &content_type,
            )
            .unwrap()
            .unwrap();
        let rewritten = String::from_utf8(retry.body.to_vec()).unwrap();

        assert_eq!(retry.route, RelayRoute::ImageEdits);
        assert!(rewritten.contains("name=\"model\""));
        assert!(rewritten.contains("name=\"image\"; filename=\"input.png\""));
        assert!(rewritten.contains("name=\"prompt\""));
        assert!(rewritten.contains(VARIATION_PROMPT));
        assert!(rewritten.ends_with("--x--\r\n"));
    }

    #[test]
    fn exposes_newapi_capabilities() {
        let capabilities = NEWAPI_ADAPTER.capabilities();

        assert!(capabilities.handles_image_stream_response);
        assert!(capabilities.translates_response_image_generation);
    }

    #[test]
    fn converts_response_image_tool_to_image_generation_request() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.5","input":"Draw a glass teapot","background":true,"store":true,"tools":[{"type":"image_generation","model":"gpt-image-2","size":"1536x1024","quality":"high","output_format":"webp","action":"generate","partial_images":2}]}"#,
        );
        let prepared = NEWAPI_ADAPTER
            .prepare_response_image_generation_request(body)
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_slice(&prepared.body).unwrap();

        assert_eq!(prepared.model, "gpt-image-2");
        assert_eq!(value["model"], "gpt-image-2");
        assert_eq!(value["prompt"], "Draw a glass teapot");
        assert_eq!(value["size"], "1536x1024");
        assert_eq!(value["quality"], "high");
        assert_eq!(value["output_format"], "webp");
        assert!(value.get("action").is_none());
        assert!(value.get("partial_images").is_none());
    }

    #[test]
    fn converts_structured_response_input_to_prompt() {
        let request = serde_json::json!({
            "model": "gpt-5.5",
            "instructions": "Use a clean editorial style.",
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "Draw a compact teapot."},
                    {"type": "input_image", "image_url": "https://example.com/input.png"}
                ]
            }],
            "tools": [{"type": "image_generation", "model": "gpt-image-2"}]
        });
        let value = image_generation_request(&request).unwrap();

        assert_eq!(value["model"], "gpt-image-2");
        assert_eq!(
            value["prompt"],
            "Use a clean editorial style.\nDraw a compact teapot."
        );
    }

    #[test]
    fn rejects_response_image_tool_without_model() {
        let body = Bytes::from_static(
            br#"{"model":"gpt-5.5","input":"Draw a glass teapot","tools":[{"type":"image_generation"}]}"#,
        );

        let err = NEWAPI_ADAPTER
            .prepare_response_image_generation_request(body)
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("tools[].model is required for NewAPI image generation"));
    }
}
