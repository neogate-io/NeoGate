use axum::http::{HeaderValue, StatusCode};

pub(crate) const PROVIDER_CODE: &str = "newapi";
const IMAGE_VARIATION_PATH: &str = "/v1/images/variations";

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
}
