use axum::http::{HeaderValue, StatusCode};
use bytes::Bytes;

use crate::error::{AppError, AppResult};

pub(crate) const PROVIDER_CODE: &str = "newapi";
const IMAGE_VARIATION_PATH: &str = "/v1/images/variations";
const IMAGE_EDIT_PATH: &str = "/v1/images/edits";
const VARIATION_PROMPT: &str = "Create a variation of the input image.";

pub(crate) fn is_newapi_provider(provider: &str) -> bool {
    provider.eq_ignore_ascii_case(PROVIDER_CODE)
}

pub(crate) fn should_retry_image_variation(provider: &str, path: &str) -> bool {
    is_newapi_provider(provider) && path == IMAGE_VARIATION_PATH
}

pub(crate) fn should_wrap_image_stream(provider: &str, stream: bool, path: &str) -> bool {
    is_newapi_provider(provider)
        && stream
        && matches!(path, "/v1/images/generations" | "/v1/images/edits")
}

pub(crate) fn is_event_stream(content_type: &HeaderValue) -> bool {
    content_type
        .to_str()
        .map(|value| value.to_ascii_lowercase().contains("text/event-stream"))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub(crate) struct ImageRetryRequest {
    pub(crate) path: &'static str,
    pub(crate) body: Bytes,
    pub(crate) content_type: HeaderValue,
}

pub(crate) fn should_retry_variation_as_edit(
    path: &str,
    status: StatusCode,
    error_body: &[u8],
) -> bool {
    if path != IMAGE_VARIATION_PATH || status != StatusCode::BAD_REQUEST {
        return false;
    }
    let body = String::from_utf8_lossy(error_body).to_ascii_lowercase();
    body.contains("new_api_error") && body.contains("model name not specified")
}

pub(crate) fn variation_as_edit_request(
    body: &Bytes,
    content_type: &HeaderValue,
) -> AppResult<ImageRetryRequest> {
    let content_type_text = content_type
        .to_str()
        .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?;
    let boundary = multipart_boundary(content_type_text)?;
    let body = ensure_multipart_prompt(body, &boundary)?;
    Ok(ImageRetryRequest {
        path: IMAGE_EDIT_PATH,
        body,
        content_type: content_type.clone(),
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_newapi_variation_model_error() {
        let body = br#"{"error":{"message":"Model name not specified, model name cannot be empty; type=new_api_error"}}"#;

        assert!(should_retry_variation_as_edit(
            IMAGE_VARIATION_PATH,
            StatusCode::BAD_REQUEST,
            body
        ));
        assert!(!should_retry_variation_as_edit(
            "/v1/images/edits",
            StatusCode::BAD_REQUEST,
            body
        ));
    }

    #[test]
    fn rewrites_variation_multipart_as_edit_with_prompt() {
        let body = Bytes::from_static(
            b"--x\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-2\r\n--x\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG\r\n--x--\r\n",
        );
        let content_type = HeaderValue::from_static("multipart/form-data; boundary=x");

        let request = variation_as_edit_request(&body, &content_type).unwrap();
        let rewritten = String::from_utf8(request.body.to_vec()).unwrap();

        assert_eq!(request.path, IMAGE_EDIT_PATH);
        assert!(rewritten.contains("name=\"model\""));
        assert!(rewritten.contains("name=\"image\"; filename=\"input.png\""));
        assert!(rewritten.contains("name=\"prompt\""));
        assert!(rewritten.contains(VARIATION_PROMPT));
        assert!(rewritten.ends_with("--x--\r\n"));
    }
}
