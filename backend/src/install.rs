use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};

use crate::{
    error::{AppError, AppResult},
    AppState,
};

const INSTALL_TEMPLATE: &str = include_str!("../templates/install.template");

pub fn router() -> Router<Arc<AppState>> {
    Router::new().route("/install", get(install_script))
}

pub fn bootstrap_router() -> Router {
    Router::new().route("/install", get(bootstrap_install_script))
}

async fn install_script(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let origin = state
        .config
        .public_base_url
        .clone()
        .or_else(|| inferred_public_base_url(&headers))
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_response(&origin)
}

async fn bootstrap_install_script(headers: HeaderMap) -> AppResult<Response> {
    let origin = inferred_public_base_url(&headers)
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_response(&origin)
}

fn install_response(origin: &str) -> AppResult<Response> {
    let script = render_install_script(origin);
    let mut response = script.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/x-shellscript; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

fn render_install_script(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    INSTALL_TEMPLATE
        .replace("__NEOGATE_DEFAULT_BASE_URL__", &format!("{origin}/v1"))
        .replace("__NEOGATE_INSTALL_ORIGIN__", origin)
}

pub fn inferred_public_base_url(headers: &HeaderMap) -> Option<String> {
    let host = header_first_value(headers, "x-forwarded-host")
        .or_else(|| header_first_value(headers, "host"))?;
    let proto = header_first_value(headers, "x-forwarded-proto")
        .or_else(|| header_first_value(headers, "x-forwarded-scheme"))
        .unwrap_or_else(|| "http".to_string());
    let proto = proto.to_ascii_lowercase();

    if proto != "http" && proto != "https" {
        return None;
    }
    if host.contains('/') || host.contains('\\') || host.trim().is_empty() {
        return None;
    }

    Some(format!("{proto}://{}", host.trim().trim_end_matches('/')))
}

fn header_first_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()?
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{to_bytes, Body},
        http::{HeaderMap, Request, StatusCode},
    };
    use tower::ServiceExt;

    use super::*;
    use crate::app::tests::test_state;

    #[test]
    fn infers_public_base_url_from_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "https".parse().unwrap());
        headers.insert("x-forwarded-host", "dev.moligate.com".parse().unwrap());

        assert_eq!(
            inferred_public_base_url(&headers).as_deref(),
            Some("https://dev.moligate.com")
        );
    }

    #[test]
    fn falls_back_to_host_when_forwarded_host_is_missing() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "localhost:8080".parse().unwrap());

        assert_eq!(
            inferred_public_base_url(&headers).as_deref(),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn rejects_invalid_forwarded_proto() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "javascript".parse().unwrap());
        headers.insert("x-forwarded-host", "dev.moligate.com".parse().unwrap());

        assert_eq!(inferred_public_base_url(&headers), None);
    }

    #[test]
    fn renders_install_script_with_origin() {
        let script = render_install_script("https://dev.moligate.com/");

        assert!(script.contains("DEFAULT_BASE_URL=\"https://dev.moligate.com/v1\""));
        assert!(script.contains("curl -fsSL https://dev.moligate.com/install | bash"));
        assert!(!script.contains("__NEOGATE_DEFAULT_BASE_URL__"));
        assert!(!script.contains("__NEOGATE_INSTALL_ORIGIN__"));
    }

    #[tokio::test]
    async fn install_route_returns_shell_script() {
        let state = test_state();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/install")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/x-shellscript; charset=utf-8"
        );

        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let script = String::from_utf8(body.to_vec()).unwrap();
        assert!(script.starts_with("#!/usr/bin/env bash"));
        assert!(script.contains("DEFAULT_BASE_URL=\"http://localhost:8080/v1\""));
    }

    #[tokio::test]
    async fn bootstrap_install_route_uses_forwarded_headers() {
        let app = bootstrap_router();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/install")
                    .header("x-forwarded-proto", "https")
                    .header("x-forwarded-host", "dev.moligate.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let script = String::from_utf8(body.to_vec()).unwrap();
        assert!(script.contains("DEFAULT_BASE_URL=\"https://dev.moligate.com/v1\""));
    }
}
