use axum::http::HeaderMap;
use bytes::Bytes;
use serde_json::Value;

use crate::{
    error::AppResult,
    relay::selector::{SelectedUpstream, UpstreamProtocol},
};

use super::{
    compatible::COMPATIBLE_ADAPTER, PreparedResponseImageGenerationRequest,
    PreparedUpstreamRequest, ProviderAdapter, ProviderCapabilities, RelayRoute,
};

pub(crate) static NEWAPI_ADAPTER: NewApiAdapter = NewApiAdapter;

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
    let mut images = Vec::new();
    if let Some(input) = object.get("input") {
        collect_input_images(input, &mut images);
    }
    let action = tool.get("action").and_then(Value::as_str).unwrap_or("auto");
    let is_edit = action == "edit" || (action == "auto" && !images.is_empty());
    if is_edit {
        if images.is_empty() {
            return Err(crate::error::AppError::BadRequest(
                "image_generation action=edit requires an input_image".into(),
            ));
        }
        output.insert("images".into(), Value::Array(images));
        if let Some(value) = tool.get("input_fidelity") {
            output.insert("input_fidelity".into(), value.clone());
        }
        if let Some(value) = tool.get("input_image_mask") {
            output.insert("mask".into(), value.clone());
        }
    }
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

fn collect_input_images(value: &Value, output: &mut Vec<Value>) {
    match value {
        Value::Array(items) => {
            for item in items {
                collect_input_images(item, output);
            }
        }
        Value::Object(object) => {
            if object.get("type").and_then(Value::as_str) == Some("input_image") {
                if let Some(image_url) = object.get("image_url").and_then(Value::as_str) {
                    output.push(serde_json::json!({"image_url": image_url}));
                } else if let Some(file_id) = object.get("file_id").and_then(Value::as_str) {
                    output.push(serde_json::json!({"file_id": file_id}));
                }
            } else if let Some(content) = object.get("content") {
                collect_input_images(content, output);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "tools": [{
                "type": "image_generation",
                "model": "gpt-image-2",
                "action": "edit",
                "input_fidelity": "high",
                "input_image_mask": {"file_id": "file-mask"}
            }]
        });
        let value = image_generation_request(&request).unwrap();

        assert_eq!(value["model"], "gpt-image-2");
        assert_eq!(
            value["prompt"],
            "Use a clean editorial style.\nDraw a compact teapot."
        );
        assert_eq!(
            value["images"][0]["image_url"],
            "https://example.com/input.png"
        );
        assert_eq!(value["input_fidelity"], "high");
        assert_eq!(value["mask"]["file_id"], "file-mask");
        assert!(value.get("input_image_mask").is_none());
    }

    #[test]
    fn rejects_edit_without_an_input_image() {
        let request = serde_json::json!({
            "input": "Cut out the dog.",
            "tools": [{"type": "image_generation", "model": "gpt-image-2", "action": "edit"}]
        });

        let err = image_generation_request(&request).unwrap_err();

        assert!(err
            .to_string()
            .contains("action=edit requires an input_image"));
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
