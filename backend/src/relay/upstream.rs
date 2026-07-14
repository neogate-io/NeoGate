use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use bytes::Bytes;
use serde_json::{json, Value};

use crate::{
    admin::credentials::refresh_openai_runtime_credential,
    config::DEFAULT_ANTHROPIC_VERSION,
    error::{AppError, AppResult, UpstreamErrorKind, UpstreamRequestError},
    provider::adapters::{adapter_for_endpoint, PreparedUpstreamRequest},
    AppState,
};

use super::selector::{SelectedUpstream, UpstreamProtocol};
use super::streaming::RelayContext;

const ANTHROPIC_CLI_PASSTHROUGH_HEADERS: &[&str] = &[
    "anthropic-beta",
    "anthropic-version",
    "x-stainless-arch",
    "x-stainless-lang",
    "x-stainless-os",
    "x-stainless-package-version",
    "x-stainless-retry-count",
    "x-stainless-runtime",
    "x-stainless-runtime-version",
    "x-stainless-timeout",
    "user-agent",
    "x-app",
    "anthropic-dangerous-direct-browser-access",
];

const CODEX_CLI_PASSTHROUGH_HEADERS: &[&str] = &[
    "originator",
    "session_id",
    "user-agent",
    "x-codex-beta-features",
    "x-codex-turn-metadata",
];

pub(crate) async fn forward_openai(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    body: Bytes,
    path: &str,
) -> AppResult<reqwest::Response> {
    forward_openai_with_headers(state, upstream, protocol, body, path, &HeaderMap::new()).await
}

pub(crate) async fn forward_openai_with_headers(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    body: Bytes,
    path: &str,
    headers: &HeaderMap,
) -> AppResult<reqwest::Response> {
    if protocol == UpstreamProtocol::OpenAiOauth {
        return forward_openai_oauth(state, upstream, body, path).await;
    }
    ensure_openai_protocol(protocol)?;
    let url = upstream_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, protocol, path, || {
        let request = state
            .http
            .post(url.clone())
            .bearer_auth(&upstream.secret)
            .header("content-type", "application/json")
            .body(body.clone());
        apply_openai_codex_passthrough_headers(request, headers, &body)
    })
    .await
}

pub(crate) async fn forward_prepared_openai(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    headers: &HeaderMap,
    prepared: PreparedUpstreamRequest,
) -> AppResult<reqwest::Response> {
    if protocol == UpstreamProtocol::OpenAiOauth {
        return forward_openai_oauth(state, upstream, prepared.body, &prepared.log_path).await;
    }
    ensure_openai_protocol(protocol)?;
    send_upstream_request(state, upstream, protocol, &prepared.log_path, || {
        let mut request = state
            .http
            .post(prepared.url.clone())
            .bearer_auth(&upstream.secret)
            .header("content-type", "application/json")
            .body(prepared.body.clone());
        for (name, value) in &prepared.extra_headers {
            request = request.header(name, value.clone());
        }
        apply_openai_codex_passthrough_headers(request, headers, &prepared.body)
    })
    .await
}

pub(crate) async fn forward_openai_with_content_type(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    body: Bytes,
    path: &str,
    content_type: HeaderValue,
    accept_event_stream: bool,
) -> AppResult<reqwest::Response> {
    ensure_openai_protocol(protocol)?;
    let url = upstream_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, protocol, path, || {
        let mut request = state
            .http
            .post(url.clone())
            .bearer_auth(&upstream.secret)
            .header("content-type", content_type.clone())
            .body(body.clone());
        if accept_event_stream {
            request = request.header("accept", "text/event-stream");
        }
        request
    })
    .await
}

fn ensure_openai_protocol(protocol: UpstreamProtocol) -> AppResult<()> {
    if protocol == UpstreamProtocol::Openai {
        return Ok(());
    }
    Err(AppError::BadRequest(format!(
        "{} cannot be sent with OpenAI forwarding",
        protocol.as_str()
    )))
}

fn apply_openai_codex_passthrough_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
    body: &[u8],
) -> reqwest::RequestBuilder {
    let mut has_session_id = false;
    for &header in CODEX_CLI_PASSTHROUGH_HEADERS {
        let Some(value) = headers.get(header) else {
            continue;
        };
        if header.eq_ignore_ascii_case("session_id") {
            has_session_id = true;
        }
        request = request.header(header, value.clone());
    }

    for &header in ANTHROPIC_CLI_PASSTHROUGH_HEADERS {
        if let Some(value) = headers.get(header) {
            request = request.header(header, value.clone());
        }
    }

    if !has_session_id {
        if let Some(prompt_cache_key) = prompt_cache_key_from_body(body) {
            request = request.header("session_id", prompt_cache_key);
        }
    }

    request
}

fn prompt_cache_key_from_body(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("prompt_cache_key")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn forward_openai_oauth(
    state: &AppState,
    upstream: &SelectedUpstream,
    body: Bytes,
    path: &str,
) -> AppResult<reqwest::Response> {
    if path != "/v1/responses" {
        return Err(AppError::BadRequest(
            "openai_oauth only supports /v1/responses".to_string(),
        ));
    }
    let account_id = upstream.account_id.as_deref().ok_or_else(|| {
        AppError::BadRequest("令牌文件缺少 ChatGPT account_id，请重新上传有效凭证".to_string())
    })?;
    let body = codex_compatible_responses_body(body)?;
    let url = format!("{}/responses", upstream.base_url.trim_end_matches('/'));

    let response =
        send_openai_oauth_request(state, upstream, &url, account_id, body.clone()).await?;
    if response.status() != StatusCode::UNAUTHORIZED {
        return Ok(response);
    }

    let Some(credential_id) = upstream.credential_id else {
        return Ok(response);
    };
    drop(response);
    let refreshed = refresh_openai_runtime_credential(state, credential_id).await?;
    state
        .selector
        .invalidate_refreshed_credential(credential_id)
        .await;
    let refreshed_account_id = refreshed.account_id.as_deref().unwrap_or(account_id);
    let refreshed_upstream = SelectedUpstream {
        secret: refreshed.access_token,
        account_id: Some(refreshed_account_id.to_string()),
        ..upstream.clone()
    };
    send_openai_oauth_request(state, &refreshed_upstream, &url, refreshed_account_id, body).await
}

pub(crate) async fn forward_openai_bound(
    state: &AppState,
    upstream: &SelectedUpstream,
    method: Method,
    path: &str,
    body: Option<Bytes>,
) -> AppResult<reqwest::Response> {
    let adapter = adapter_for_endpoint(&upstream.provider, &upstream.base_url);
    let (url, log_path) = adapter.resolve_bound_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, UpstreamProtocol::Openai, &log_path, || {
        let mut request = state
            .http
            .request(method.clone(), url.clone())
            .bearer_auth(&upstream.secret);
        if let Some(body) = body.clone() {
            request = request
                .header("content-type", "application/json")
                .body(body);
        }
        request
    })
    .await
}

async fn send_openai_oauth_request(
    state: &AppState,
    upstream: &SelectedUpstream,
    url: &str,
    account_id: &str,
    body: Bytes,
) -> AppResult<reqwest::Response> {
    send_upstream_request(
        state,
        upstream,
        UpstreamProtocol::OpenAiOauth,
        "/responses",
        || {
            state
                .http
                .post(url)
                .bearer_auth(&upstream.secret)
                .header("content-type", "application/json")
                .header("accept", "text/event-stream")
                .header("connection", "Keep-Alive")
                .header("originator", "codex_cli_rs")
                .header("chatgpt-account-id", account_id)
                .header("user-agent", "codex_cli_rs/0.118.0 (NeoGate; openai_oauth)")
                .body(body.clone())
        },
    )
    .await
}

fn codex_compatible_responses_body(body: Bytes) -> AppResult<Bytes> {
    let mut value: Value = serde_json::from_slice(&body)?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".to_string()))?;

    if let Some(input) = object.get_mut("input") {
        if let Some(text) = input.as_str() {
            *input = json!([{ "role": "user", "content": text }]);
        }
        convert_system_roles(input);
    }

    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            if let Some(tool_type) = tool.get_mut("type").and_then(|value| value.as_str()) {
                if tool_type == "web_search_preview" || tool_type == "web_search_preview_2025_03_11"
                {
                    tool["type"] = Value::String("web_search".to_string());
                }
            }
        }
    }

    object.insert("stream".to_string(), Value::Bool(true));
    object.insert("store".to_string(), Value::Bool(false));
    object.insert("parallel_tool_calls".to_string(), Value::Bool(true));
    object.insert(
        "include".to_string(),
        Value::Array(vec![Value::String(
            "reasoning.encrypted_content".to_string(),
        )]),
    );

    for key in [
        "max_output_tokens",
        "max_completion_tokens",
        "temperature",
        "top_p",
        "truncation",
        "user",
        "stream_options",
        "previous_response_id",
        "prompt_cache_retention",
        "safety_identifier",
    ] {
        object.remove(key);
    }

    Ok(Bytes::from(serde_json::to_vec(&value)?))
}

fn convert_system_roles(value: &mut Value) {
    match value {
        Value::Array(items) => {
            for item in items {
                convert_system_roles(item);
            }
        }
        Value::Object(object) => {
            if object.get("role").and_then(Value::as_str) == Some("system") {
                object.insert("role".to_string(), Value::String("developer".to_string()));
            }
            for value in object.values_mut() {
                convert_system_roles(value);
            }
        }
        _ => {}
    }
}

pub(crate) async fn forward_anthropic(
    state: &AppState,
    headers: &HeaderMap,
    upstream: &SelectedUpstream,
    body: Bytes,
) -> AppResult<reqwest::Response> {
    let url = upstream_url(&upstream.base_url, "/v1/messages");
    let anthropic_version = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_ANTHROPIC_VERSION)
        .to_string();
    let anthropic_beta = headers
        .get("anthropic-beta")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    send_upstream_request(
        state,
        upstream,
        UpstreamProtocol::Anthropic,
        "/v1/messages",
        || {
            let mut request = state
                .http
                .post(url.clone())
                .header("x-api-key", &upstream.secret)
                .header("anthropic-version", &anthropic_version)
                .header("content-type", "application/json")
                .body(body.clone());

            if let Some(beta) = &anthropic_beta {
                request = request.header("anthropic-beta", beta);
            }

            apply_anthropic_cli_passthrough_headers(request, headers)
        },
    )
    .await
}

pub(crate) async fn forward_anthropic_bound(
    state: &AppState,
    headers: &HeaderMap,
    upstream: &SelectedUpstream,
    method: Method,
    path: &str,
    body: Option<Bytes>,
) -> AppResult<reqwest::Response> {
    let url = upstream_url(&upstream.base_url, path);
    let anthropic_version = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or(DEFAULT_ANTHROPIC_VERSION)
        .to_string();
    let anthropic_beta = headers
        .get("anthropic-beta")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    send_upstream_request(state, upstream, UpstreamProtocol::Anthropic, path, || {
        let mut request = state
            .http
            .request(method.clone(), url.clone())
            .header("x-api-key", &upstream.secret)
            .header("anthropic-version", &anthropic_version);
        if let Some(beta) = &anthropic_beta {
            request = request.header("anthropic-beta", beta);
        }
        if let Some(body) = body.clone() {
            request = request
                .header("content-type", "application/json")
                .body(body);
        }
        apply_anthropic_cli_passthrough_headers(request, headers)
    })
    .await
}

fn apply_anthropic_cli_passthrough_headers(
    mut request: reqwest::RequestBuilder,
    headers: &HeaderMap,
) -> reqwest::RequestBuilder {
    for name in ANTHROPIC_CLI_PASSTHROUGH_HEADERS {
        if let Some(value) = headers.get(*name) {
            request = request.header(*name, value.clone());
        }
    }
    request
}

async fn send_upstream_request<F>(
    state: &AppState,
    upstream: &SelectedUpstream,
    _protocol: UpstreamProtocol,
    _path: &str,
    build: F,
) -> AppResult<reqwest::Response>
where
    F: FnOnce() -> reqwest::RequestBuilder,
{
    match tokio::time::timeout(
        state.config.http.upstream_timeout,
        build().header("accept-encoding", "identity").send(),
    )
    .await
    {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(err)) => Err(AppError::Reqwest(err)),
        Err(_) => Err(AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Timeout,
            upstream.provider.clone(),
            format!(
                "response headers were not received within {} seconds",
                state.config.http.upstream_timeout.as_secs()
            ),
        ))),
    }
}

pub(crate) fn relay_upstream_error(ctx: &RelayContext, err: AppError) -> AppError {
    match err {
        AppError::Reqwest(err) => AppError::UpstreamRequest(UpstreamRequestError::from_reqwest(
            ctx.upstream.provider.clone(),
            &err,
        )),
        err => err,
    }
}

pub(crate) fn log_relay_upstream_failure(ctx: &RelayContext, err: &AppError) {
    let latency_ms = ctx.started.elapsed().as_millis() as i64;
    match err {
        AppError::UpstreamRequest(upstream_error) => {
            let error_kind = upstream_error.kind.type_code();
            let reason =
                upstream_request_failure_reason(upstream_error.kind, &upstream_error.detail);
            let detail = upstream_request_failure_detail(reason);
            let client_response = json!({
                "error": {
                    "message": upstream_error.kind.user_message(),
                    "code": error_kind,
                    "upstream": upstream_error.provider,
                    "retryable": upstream_error.retryable,
                }
            })
            .to_string();
            if upstream_error.status().is_server_error() {
                tracing::error!(
                    channel = %ctx.upstream.channel_name,
                    channel_id = ctx.upstream.channel_id,
                    channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                    channel_key_id = ?ctx.upstream.channel_key_id,
                    credential_id = ?ctx.upstream.credential_id,
                    provider = %ctx.upstream.provider,
                    protocol = ctx.protocol.as_str(),
                    model = %ctx.model,
                    path = ctx.path,
                    upstream = %ctx.upstream.base_url,
                    status = upstream_error.status().as_u16(),
                    client_status = upstream_error.status().as_u16(),
                    latency_ms,
                    error_kind,
                    reason,
                    detail,
                    retryable = upstream_error.retryable,
                    client_response = %client_response,
                    "upstream request failed"
                );
            } else {
                tracing::warn!(
                    channel = %ctx.upstream.channel_name,
                    channel_id = ctx.upstream.channel_id,
                    channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                    channel_key_id = ?ctx.upstream.channel_key_id,
                    credential_id = ?ctx.upstream.credential_id,
                    provider = %ctx.upstream.provider,
                    protocol = ctx.protocol.as_str(),
                    model = %ctx.model,
                    path = ctx.path,
                    upstream = %ctx.upstream.base_url,
                    status = upstream_error.status().as_u16(),
                    client_status = upstream_error.status().as_u16(),
                    latency_ms,
                    error_kind,
                    reason,
                    detail,
                    retryable = upstream_error.retryable,
                    client_response = %client_response,
                    "upstream request failed"
                );
            }
            tracing::debug!(
                channel_id = ctx.upstream.channel_id,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                provider = %ctx.upstream.provider,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                path = ctx.path,
                upstream = %ctx.upstream.base_url,
                error_kind,
                reason,
                source_error = %upstream_error.detail,
                "upstream request failure detail"
            );
        }
        err => {
            tracing::error!(
                provider = %ctx.upstream.provider,
                channel_id = ctx.upstream.channel_id,
                channel_name = %ctx.upstream.channel_name,
                channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                channel_key_id = ?ctx.upstream.channel_key_id,
                credential_id = ?ctx.upstream.credential_id,
                protocol = ctx.protocol.as_str(),
                model = %ctx.model,
                path = ctx.path,
                base_url = %ctx.upstream.base_url,
                latency_ms,
                error = %err,
                "relay request failed"
            );
        }
    }
}

fn upstream_request_failure_reason(kind: UpstreamErrorKind, detail: &str) -> &'static str {
    let detail = detail.to_ascii_lowercase();
    match kind {
        UpstreamErrorKind::Timeout => "timeout",
        UpstreamErrorKind::Tls
            if detail.contains("close_notify")
                || detail.contains("unexpectedeof")
                || detail.contains("unexpected eof") =>
        {
            "tls_unexpected_eof"
        }
        UpstreamErrorKind::Tls => "tls_error",
        UpstreamErrorKind::Dns => "dns_resolution_failed",
        UpstreamErrorKind::Connect => "connect_failed",
        UpstreamErrorKind::Request => "request_error",
    }
}

fn upstream_request_failure_detail(reason: &str) -> &'static str {
    match reason {
        "timeout" => "upstream did not respond before the request timed out",
        "tls_unexpected_eof" => "peer closed the TLS connection before sending close_notify",
        "tls_error" => "TLS connection to upstream failed",
        "dns_resolution_failed" => "upstream hostname could not be resolved",
        "connect_failed" => "could not connect to upstream",
        "request_error" => "upstream request failed before a response was received",
        _ => "upstream request failed before a response was received",
    }
}

pub(crate) fn upstream_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if is_versioned_openai_compatible_base(base) && path.starts_with("/v1/") {
        format!("{}{}", base, &path[3..])
    } else {
        format!("{base}{path}")
    }
}

fn is_versioned_openai_compatible_base(base_url: &str) -> bool {
    let last_segment = base_url.rsplit('/').next().unwrap_or_default();
    if last_segment == "openai" {
        return true;
    }

    matches!(
        last_segment,
        "v1" | "v2" | "v3" | "v4" | "v1beta" | "v1beta1"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_url_avoids_double_v1() {
        assert_eq!(
            upstream_url("https://api.openai.com/v1", "/v1/chat/completions"),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn upstream_url_appends_path_to_root_base() {
        assert_eq!(
            upstream_url("https://api.openai.com", "/v1/responses"),
            "https://api.openai.com/v1/responses"
        );
    }

    #[test]
    fn upstream_url_keeps_provider_specific_api_versions() {
        assert_eq!(
            upstream_url(
                "https://open.bigmodel.cn/api/paas/v4",
                "/v1/chat/completions"
            ),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            upstream_url(
                "https://generativelanguage.googleapis.com/v1beta/openai",
                "/v1/chat/completions"
            ),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
    }

    #[test]
    fn upstream_request_failure_reason_identifies_tls_unexpected_eof() {
        assert_eq!(
            upstream_request_failure_reason(
                UpstreamErrorKind::Tls,
                "peer closed connection without sending TLS close_notify"
            ),
            "tls_unexpected_eof"
        );
        assert_eq!(
            upstream_request_failure_detail("tls_unexpected_eof"),
            "peer closed the TLS connection before sending close_notify"
        );
    }
}
