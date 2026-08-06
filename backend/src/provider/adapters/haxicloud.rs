use std::sync::Arc;

use axum::{
    extract::{OriginalUri, State},
    http::{HeaderMap, Method},
    response::Response,
    routing::{get, post, put},
    Router,
};
use bytes::Bytes;
use serde_json::{Map, Value};

use super::{AdapterResponseMode, PreparedUpstreamRequest, ProviderAdapter, RelayRoute};
use crate::{
    auth::UserAuth,
    error::{AppError, AppResult},
    relay::{
        forward_openai_bound_with_headers, raw_upstream_response,
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
    AppState,
};

pub(crate) static HAXICLOUD_ADAPTER: HaxicloudAdapter = HaxicloudAdapter;
pub(crate) struct HaxicloudAdapter;
const TASKS_PATH: &str = "/contents/generations/tasks";
const HAXICLOUD_HOST: &str = "token.haxicloud.com";
const ASSET_ROUTING_MODEL: &str = "dreamina-seedance-2-0-260128";

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/asset-groups", get(proxy_get).post(proxy_post))
        .route("/v1/asset-groups/{id}", put(proxy_put).delete(proxy_delete))
        .route("/v1/assets", post(proxy_post))
        .route("/v1/assets/upload", post(proxy_post))
        .route("/v1/assets/sync-upload", post(proxy_post))
        .route("/v1/assets/sync", post(proxy_post))
        .route("/v1/assets/importable", get(proxy_get))
        .route("/v1/assets/import", post(proxy_post))
        .route("/v1/assets/{id}", get(proxy_get).delete(proxy_delete))
        .route("/v1/realperson/agreement", post(proxy_post))
        .route("/v1/realperson/session", post(proxy_post))
        .route("/v1/realperson/result", post(proxy_post))
}

async fn proxy_get(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    uri: OriginalUri,
    headers: HeaderMap,
) -> AppResult<Response> {
    proxy(state, auth, uri, headers, Method::GET, Bytes::new()).await
}

async fn proxy_post(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    uri: OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    proxy(state, auth, uri, headers, Method::POST, body).await
}

async fn proxy_put(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    uri: OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    proxy(state, auth, uri, headers, Method::PUT, body).await
}

async fn proxy_delete(
    state: State<Arc<AppState>>,
    auth: UserAuth,
    uri: OriginalUri,
    headers: HeaderMap,
) -> AppResult<Response> {
    proxy(state, auth, uri, headers, Method::DELETE, Bytes::new()).await
}

async fn proxy(
    State(state): State<Arc<AppState>>,
    _auth: UserAuth,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    method: Method,
    body: Bytes,
) -> AppResult<Response> {
    let upstream = state
        .selector
        .select_matching_endpoint(
            &state.db.pool,
            &state.secrets,
            UpstreamProtocol::Openai,
            ASSET_ROUTING_MODEL,
            |channel| {
                super::adapter_for_endpoint(
                    &channel.provider,
                    &channel.base_url,
                    channel.adapter_hint.as_deref(),
                )
                .name()
                    == "haxicloud"
            },
        )
        .await
        .map_err(|err| match err {
            AppError::UpstreamUnavailable(_) => AppError::UpstreamUnavailable(
                "no available HaxiCloud channel for asset or real-person APIs".to_string(),
            ),
            err => err,
        })?;

    let path = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or_else(|| uri.path());
    let response = forward_openai_bound_with_headers(
        &state,
        &upstream,
        method,
        path,
        (!body.is_empty()).then_some(body),
        &headers,
    )
    .await?;
    raw_upstream_response(response).await
}

pub(crate) fn matches_base_url(base_url: &str) -> bool {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case(HAXICLOUD_HOST))
        })
        .unwrap_or(false)
}

impl ProviderAdapter for HaxicloudAdapter {
    fn name(&self) -> &'static str {
        "haxicloud"
    }

    fn prepares_video_request(&self, _model: &str) -> bool {
        true
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        if route == RelayRoute::Videos {
            tasks_url(base_url)
        } else {
            upstream_url(base_url, route.path())
        }
    }

    fn resolve_bound_url(&self, base_url: &str, path: &str) -> (String, String) {
        if let Some(id) = path
            .strip_prefix("/v1/videos/")
            .filter(|id| !id.is_empty() && !id.contains('/'))
        {
            return (
                format!("{}/{}", tasks_url(base_url), id),
                format!("{TASKS_PATH}/{id}"),
            );
        }
        (upstream_url(base_url, path), path.to_string())
    }

    fn prepare_openai_request(
        &self,
        upstream: &SelectedUpstream,
        _protocol: UpstreamProtocol,
        route: RelayRoute,
        body: Bytes,
        headers: &HeaderMap,
        _streamed: bool,
    ) -> AppResult<PreparedUpstreamRequest> {
        if route != RelayRoute::Videos {
            return Ok(PreparedUpstreamRequest {
                url: self.resolve_url(&upstream.base_url, route),
                log_path: route.path().to_string(),
                body,
                extra_headers: HeaderMap::new(),
                response_mode: AdapterResponseMode::Passthrough,
            });
        }
        let content_type = headers
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/json");
        if !content_type
            .to_ascii_lowercase()
            .starts_with("application/json")
        {
            return Err(AppError::BadRequest(
                "Haxicloud Seedance video upstream requires application/json requests".into(),
            ));
        }
        let input: Value = serde_json::from_slice(&body)
            .map_err(|e| AppError::BadRequest(format!("invalid json: {e}")))?;
        let object = input
            .as_object()
            .ok_or_else(|| AppError::BadRequest("request body must be a JSON object".into()))?;
        let model = object
            .get("model")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| AppError::BadRequest("model is required".into()))?;
        let prompt = object
            .get("prompt")
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
            .or_else(|| content_prompt(object))
            .ok_or_else(|| AppError::BadRequest("prompt is required".into()))?;
        let mut output = object.clone();
        output.insert("model".into(), Value::String(model.into()));
        output.insert("prompt".into(), Value::String(prompt.into()));
        if !output.contains_key("duration") {
            if let Some(seconds) = object
                .get("seconds")
                .and_then(|value| {
                    value
                        .as_i64()
                        .or_else(|| value.as_str()?.parse::<i64>().ok())
                })
                .filter(|v| *v > 0)
            {
                output.insert("duration".into(), Value::from(seconds));
            }
        }
        // The HaxiCloud API accepts `duration` as a number, not OpenAI's
        // compatibility `seconds` field.
        output.remove("seconds");
        if output.contains_key("ratio") && output.contains_key("resolution") {
            output.remove("size");
        }
        append_images(object, &mut output);
        Ok(PreparedUpstreamRequest {
            url: tasks_url(&upstream.base_url),
            log_path: TASKS_PATH.into(),
            body: Bytes::from(serde_json::to_vec(&Value::Object(output))?),
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        })
    }

    fn normalize_response_body(&self, route: RelayRoute, body: Bytes) -> AppResult<Bytes> {
        if route != RelayRoute::Videos {
            return Ok(body);
        }
        let value: Value = serde_json::from_slice(&body)?;
        let Some(data) = value.get("data").and_then(Value::as_object) else {
            return Ok(body);
        };
        let mut payload = data
            .get("data")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(|| data.clone());
        if let Some(task_id) = data.get("task_id").or_else(|| data.get("id")) {
            payload.insert("id".into(), task_id.clone());
            payload.insert("task_id".into(), task_id.clone());
        }
        if let Some(status) = data.get("status") {
            payload.entry("status").or_insert_with(|| status.clone());
        }
        if let Some(url) = data.get("result_url") {
            payload
                .entry("output")
                .or_insert_with(|| serde_json::json!({"video_url": url}));
        }
        Ok(Bytes::from(serde_json::to_vec(&Value::Object(payload))?))
    }
}

fn tasks_url(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    // Channels are often configured with the OpenAI-compatible `/v1` suffix,
    // while HaxiCloud's native video endpoint is rooted at `/api/v3`.
    let base = base.strip_suffix("/v1").unwrap_or(base);
    if base.ends_with("/api/v3") {
        upstream_url(base, TASKS_PATH)
    } else {
        format!("{base}/api/v3{TASKS_PATH}")
    }
}

fn content_prompt(input: &Map<String, Value>) -> Option<&str> {
    input
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|item| {
            (item.get("type").and_then(Value::as_str) == Some("text"))
                .then(|| item.get("text").and_then(Value::as_str))
                .flatten()
                .filter(|v| !v.trim().is_empty())
        })
}

fn append_images(input: &Map<String, Value>, output: &mut Map<String, Value>) {
    let mut urls = Vec::new();
    for key in ["image", "input_reference"] {
        if let Some(url) = input
            .get(key)
            .and_then(Value::as_str)
            .filter(|v| !v.trim().is_empty())
        {
            urls.push(url.to_string());
        }
    }
    if let Some(images) = input.get("images").and_then(Value::as_array) {
        urls.extend(
            images
                .iter()
                .filter_map(Value::as_str)
                .filter(|v| !v.trim().is_empty())
                .map(str::to_string),
        );
    }
    if urls.is_empty() {
        return;
    }
    let content = output
        .entry("content")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(content) = content.as_array_mut() {
        for url in urls {
            content.push(serde_json::json!({"type":"image_url","image_url":{"url":url},"role":"reference_image"}));
        }
    }
    for key in ["image", "input_reference", "images"] {
        output.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn upstream() -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 1,
            channel_key_id: None,
            credential_id: None,
            provider: "custom".into(),
            channel_name: "haxicloud".into(),
            base_url: "https://token.haxicloud.com".into(),
            responses_chat_fallback: false,
            secret: "sk".into(),
            account_id: None,
            adapter_hint: None,
            affinity: None,
        }
    }
    #[test]
    fn converts_prompt_and_seconds() {
        let body = Bytes::from_static(br#"{"model":"dreamina-seedance-2-0-260128","content":[{"type":"text","text":"walk"}],"size":"1280x720","seconds":5,"ratio":"16:9","resolution":"480p"}"#);
        let prepared = HAXICLOUD_ADAPTER
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
        assert_eq!(value["prompt"], "walk");
        assert_eq!(value["duration"], 5);
        assert!(value.get("seconds").is_none());
        assert!(value.get("size").is_none());
        assert_eq!(value["ratio"], "16:9");
        assert_eq!(value["resolution"], "480p");
    }

    #[test]
    fn task_url_ignores_openai_compatible_v1_suffix() {
        assert_eq!(
            tasks_url("https://token.haxicloud.com/v1"),
            "https://token.haxicloud.com/api/v3/contents/generations/tasks"
        );
        assert_eq!(
            tasks_url("https://token.haxicloud.com/api/v3"),
            "https://token.haxicloud.com/api/v3/contents/generations/tasks"
        );
    }

    #[test]
    fn task_lookup_uses_documented_native_path() {
        let (url, log_path) = HAXICLOUD_ADAPTER
            .resolve_bound_url("https://token.haxicloud.com/v1", "/v1/videos/task_123");
        assert_eq!(
            url,
            "https://token.haxicloud.com/api/v3/contents/generations/tasks/task_123"
        );
        assert_eq!(log_path, "/contents/generations/tasks/task_123");
    }

    #[test]
    fn asset_paths_preserve_v1_and_query_parameters() {
        let (url, log_path) = HAXICLOUD_ADAPTER.resolve_bound_url(
            "https://token.haxicloud.com/v1",
            "/v1/assets/importable?target_group_id=20",
        );
        assert_eq!(
            url,
            "https://token.haxicloud.com/v1/assets/importable?target_group_id=20"
        );
        assert_eq!(log_path, "/v1/assets/importable?target_group_id=20");
    }

    #[test]
    fn unwraps_haxicloud_response() {
        let body = Bytes::from_static(br#"{"code":"success","data":{"task_id":"outer","status":"SUCCESS","data":{"status":"completed","output":{"video_url":"https://example.com/a.mp4"}}}}"#);
        let value: Value = serde_json::from_slice(
            &HAXICLOUD_ADAPTER
                .normalize_response_body(RelayRoute::Videos, body)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["id"], "outer");
        assert_eq!(value["status"], "completed");
        assert_eq!(value["output"]["video_url"], "https://example.com/a.mp4");
    }
}
