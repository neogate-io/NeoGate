use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use bytes::Bytes;
use serde_json::{json, Value};
use tokio::time::Duration;

use crate::{
    admin::credentials::refresh_openai_runtime_credential,
    error::{AppError, AppResult, UpstreamErrorKind, UpstreamRequestError},
    AppState,
};

use super::selector::{SelectedUpstream, UpstreamProtocol};
use super::streaming::RelayContext;

const UPSTREAM_MAX_TRANSPORT_ATTEMPTS: usize = 2;
const UPSTREAM_RETRY_BACKOFF: Duration = Duration::from_millis(250);

pub(super) async fn forward_openai(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    body: Bytes,
    path: &str,
) -> AppResult<reqwest::Response> {
    if protocol == UpstreamProtocol::OpenAiOauth {
        return forward_openai_oauth(state, upstream, body, path).await;
    }
    let url = upstream_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, UpstreamProtocol::Openai, path, || {
        state
            .http
            .post(url.clone())
            .bearer_auth(&upstream.secret)
            .header("content-type", "application/json")
            .body(body.clone())
    })
    .await
}

pub(super) async fn forward_openai_with_content_type(
    state: &AppState,
    upstream: &SelectedUpstream,
    body: Bytes,
    path: &str,
    content_type: HeaderValue,
    accept_event_stream: bool,
) -> AppResult<reqwest::Response> {
    let url = upstream_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, UpstreamProtocol::Openai, path, || {
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
    state.selector.invalidate().await;
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
    let url = upstream_url(&upstream.base_url, path);
    send_upstream_request(state, upstream, UpstreamProtocol::Openai, path, || {
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

pub(super) async fn forward_anthropic(
    state: &AppState,
    headers: &HeaderMap,
    upstream: &SelectedUpstream,
    body: Bytes,
) -> AppResult<reqwest::Response> {
    let url = upstream_url(&upstream.base_url, "/v1/messages");
    let anthropic_version = headers
        .get("anthropic-version")
        .and_then(|value| value.to_str().ok())
        .unwrap_or(&state.config.anthropic_version)
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

            request
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
        .unwrap_or(&state.config.anthropic_version)
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
        request
    })
    .await
}

async fn send_upstream_request<F>(
    state: &AppState,
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    path: &str,
    mut build: F,
) -> AppResult<reqwest::Response>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let send = async {
        for attempt in 1..=UPSTREAM_MAX_TRANSPORT_ATTEMPTS {
            match build().send().await {
                Ok(response) => return Ok(response),
                Err(err)
                    if attempt < UPSTREAM_MAX_TRANSPORT_ATTEMPTS
                        && should_retry_transport_error(&err) =>
                {
                    let kind = classify_reqwest_error(&err);
                    tracing::warn!(
                        "{}",
                        format_upstream_transport_retry_log(
                            upstream,
                            protocol,
                            path,
                            kind,
                            attempt,
                            format!("{err:?}")
                        )
                    );
                    tokio::time::sleep(UPSTREAM_RETRY_BACKOFF).await;
                }
                Err(err) => return Err(err),
            }
        }
        unreachable!("upstream attempt loop always returns")
    };

    match tokio::time::timeout(state.config.upstream_timeout, send).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(err)) => Err(AppError::Reqwest(err)),
        Err(_) => Err(AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Timeout,
            upstream.provider.clone(),
            format!(
                "response headers were not received within {} seconds",
                state.config.upstream_timeout.as_secs()
            ),
        ))),
    }
}

pub(super) fn relay_upstream_error(ctx: &RelayContext, err: AppError) -> AppError {
    match err {
        AppError::Reqwest(err) => AppError::UpstreamRequest(UpstreamRequestError::new(
            classify_reqwest_error(&err),
            ctx.upstream.provider.clone(),
            format!("{err:?}"),
        )),
        err => err,
    }
}

pub(super) fn log_relay_upstream_failure(ctx: &RelayContext, err: &AppError) {
    let latency_ms = ctx.started.elapsed().as_millis() as i64;
    match err {
        AppError::UpstreamRequest(upstream_error) => {
            let client_response = json!({
                "error": {
                    "message": upstream_error.kind.user_message(),
                    "code": upstream_error.kind.type_code(),
                    "upstream": upstream_error.provider,
                    "retryable": upstream_error.retryable,
                }
            })
            .to_string();
            let line = format_relay_upstream_failure_log(
                ctx,
                upstream_error.kind.type_code(),
                upstream_error.retryable,
                upstream_error.status(),
                latency_ms,
                &upstream_error.detail,
                &client_response,
            );
            if upstream_error.status().is_server_error() {
                tracing::error!("{line}");
            } else {
                tracing::warn!("{line}");
            }
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

fn format_upstream_transport_retry_log(
    upstream: &SelectedUpstream,
    protocol: UpstreamProtocol,
    path: &str,
    kind: UpstreamErrorKind,
    attempt: usize,
    error: String,
) -> String {
    format!(
        "retry upstream transport error | channel={}({}) endpoint={} key={} credential={} provider={} protocol={} path={} upstream={} error_kind={} retryable=true attempt={}/{} error={}",
        upstream.channel_name,
        upstream.channel_id,
        upstream.channel_endpoint_id,
        optional_id(upstream.channel_key_id),
        optional_id(upstream.credential_id),
        upstream.provider,
        protocol.as_str(),
        path,
        upstream.base_url,
        kind.type_code(),
        attempt,
        UPSTREAM_MAX_TRANSPORT_ATTEMPTS,
        error
    )
}

fn format_relay_upstream_failure_log(
    ctx: &RelayContext,
    error_kind: &str,
    retryable: bool,
    status: StatusCode,
    latency_ms: i64,
    error: &str,
    client_response: &str,
) -> String {
    format!(
        "upstream request failed | channel={}({}) endpoint={} key={} credential={} provider={} protocol={} model={} path={} upstream={} status={} latency={}ms error_kind={} retryable={} error={} client_status={} client_response={}",
        ctx.upstream.channel_name,
        ctx.upstream.channel_id,
        ctx.upstream.channel_endpoint_id,
        optional_id(ctx.upstream.channel_key_id),
        optional_id(ctx.upstream.credential_id),
        ctx.upstream.provider,
        ctx.protocol.as_str(),
        ctx.model,
        ctx.path,
        ctx.upstream.base_url,
        status.as_u16(),
        latency_ms,
        error_kind,
        retryable,
        error,
        status.as_u16(),
        client_response
    )
}

fn optional_id(id: Option<i64>) -> String {
    id.map(|id| id.to_string())
        .unwrap_or_else(|| "none".to_string())
}

fn should_retry_transport_error(err: &reqwest::Error) -> bool {
    if err.is_connect() {
        return true;
    }

    let details = format!("{err:?}").to_ascii_lowercase();
    details.contains("tls")
        || details.contains("dns")
        || details.contains("unexpectedeof")
        || details.contains("connection reset")
        || details.contains("connection closed")
}

fn classify_reqwest_error(err: &reqwest::Error) -> UpstreamErrorKind {
    if err.is_timeout() {
        return UpstreamErrorKind::Timeout;
    }

    let details = format!("{err:?}").to_ascii_lowercase();
    if details.contains("tls") {
        UpstreamErrorKind::Tls
    } else if details.contains("dns") || details.contains("resolve") {
        UpstreamErrorKind::Dns
    } else if err.is_connect() {
        UpstreamErrorKind::Connect
    } else {
        UpstreamErrorKind::Request
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
}
