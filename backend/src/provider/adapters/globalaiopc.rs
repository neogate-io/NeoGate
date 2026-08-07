use axum::http::HeaderMap;
use bytes::Bytes;
use serde_json::{Map, Value};

use crate::{
    error::{AppError, AppResult},
    relay::{
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
};

use super::{
    AdapterResponseMode, AssetCreateRequest, AssetType, NormalizedAsset, PreparedUpstreamRequest,
    ProviderAdapter, RelayRoute,
};

pub(crate) static GLOBALAIOPC_ADAPTER: GlobalAiOpcAdapter = GlobalAiOpcAdapter;

pub(crate) struct GlobalAiOpcAdapter;

#[derive(Clone, Copy)]
struct DiscountModel {
    base: &'static str,
    resolution: Option<&'static str>,
    with_video_ref: bool,
}

const GLOBALAIOPC_HOST: &str = "apillm.globalaiopc.com";
const VIDEO_BASE_URL: &str = "https://zcbservice.aizfw.cn/kyyReactApiServer";
const DISCOUNT_PREFIX: &str = "sd_2.0_discount";
const FAST_DISCOUNT_PREFIX: &str = "sd_2.0_fast_discount";

pub(crate) fn matches_base_url(base_url: &str) -> bool {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case(GLOBALAIOPC_HOST))
        })
        .unwrap_or(false)
}

impl GlobalAiOpcAdapter {
    fn video_base_path(path: &str) -> String {
        format!("{VIDEO_BASE_URL}{path}")
    }

    fn passthrough(
        upstream: &SelectedUpstream,
        route: RelayRoute,
        body: Bytes,
    ) -> PreparedUpstreamRequest {
        PreparedUpstreamRequest {
            url: upstream_url(&upstream.base_url, route.path()),
            log_path: route.path().to_string(),
            body,
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        }
    }
}

impl DiscountModel {
    fn parse(model: &str) -> Option<Self> {
        let (model, with_video_ref) = model
            .strip_suffix("_with_video_ref")
            .map_or((model, false), |model| (model, true));
        let (base, suffix) = [FAST_DISCOUNT_PREFIX, DISCOUNT_PREFIX]
            .into_iter()
            .find_map(|base| model.strip_prefix(base).map(|suffix| (base, suffix)))?;
        let resolution = match suffix {
            "" => None,
            "_480p" => Some("480p"),
            "_720p" => Some("720p"),
            "_1080p" => Some("1080p"),
            _ => return None,
        };
        Some(Self {
            base,
            resolution,
            with_video_ref,
        })
    }

    fn is_fast(self) -> bool {
        self.base == FAST_DISCOUNT_PREFIX
    }
}

impl ProviderAdapter for GlobalAiOpcAdapter {
    fn name(&self) -> &'static str {
        "globalaiopc"
    }

    fn prepares_video_request(&self, model: &str) -> bool {
        DiscountModel::parse(model).is_some()
    }

    fn supports_assets(&self, model: &str) -> bool {
        DiscountModel::parse(model).is_some()
    }

    fn prepare_asset_create_request(
        &self,
        _upstream: &SelectedUpstream,
        _model: &str,
        request: &AssetCreateRequest,
    ) -> AppResult<PreparedUpstreamRequest> {
        let mut body = Map::new();
        body.insert(
            "assetType".to_string(),
            Value::String(global_asset_type(request.asset_type).to_string()),
        );
        body.insert("url".to_string(), Value::String(request.url.clone()));
        if let Some(name) = &request.name {
            body.insert("name".to_string(), Value::String(name.clone()));
        }
        Ok(PreparedUpstreamRequest {
            url: Self::video_base_path("/asset/seedance2/assetUpload"),
            log_path: "/asset/seedance2/assetUpload".to_string(),
            body: Bytes::from(serde_json::to_vec(&Value::Object(body))?),
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        })
    }

    fn prepare_asset_detail_request(
        &self,
        _upstream: &SelectedUpstream,
        _model: &str,
        upstream_asset_id: &str,
    ) -> AppResult<PreparedUpstreamRequest> {
        Ok(PreparedUpstreamRequest {
            url: Self::video_base_path("/asset/seedance2/assetDetail"),
            log_path: "/asset/seedance2/assetDetail".to_string(),
            body: Bytes::from(serde_json::to_vec(&serde_json::json!({
                "assetId": upstream_asset_id,
            }))?),
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        })
    }

    fn normalize_asset_response(&self, body: Bytes) -> AppResult<NormalizedAsset> {
        let value: Value = serde_json::from_slice(&body)?;
        let object = asset_payload(&value).ok_or_else(|| {
            AppError::BadRequest("GlobalAI asset response must contain an asset object".into())
        })?;
        let upstream_asset_id = object
            .get("assetId")
            .or_else(|| object.get("asset_id"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                AppError::BadRequest("GlobalAI asset response is missing assetId".into())
            })?;
        let asset_type = object
            .get("assetType")
            .or_else(|| object.get("asset_type"))
            .and_then(Value::as_str)
            .and_then(AssetType::parse)
            .ok_or_else(|| {
                AppError::BadRequest("GlobalAI asset response has invalid assetType".into())
            })?;
        let status = normalize_asset_status(
            object
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("NONE"),
        );
        let error_message = object
            .get("errorMessage")
            .or_else(|| object.get("error_message"))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        Ok(NormalizedAsset {
            upstream_asset_id: upstream_asset_id.to_string(),
            asset_type,
            status,
            name: object
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            error_message,
        })
    }

    fn format_asset_reference(
        &self,
        _asset_type: AssetType,
        upstream_asset_id: &str,
    ) -> AppResult<String> {
        if upstream_asset_id.trim().is_empty() {
            return Err(AppError::BadRequest("upstream asset id is empty".into()));
        }
        Ok(format!("assetId://{upstream_asset_id}"))
    }

    fn resolve_url(&self, base_url: &str, route: RelayRoute) -> String {
        upstream_url(base_url, route.path())
    }

    fn resolve_video_task_url(
        &self,
        base_url: &str,
        path: &str,
        model: Option<&str>,
    ) -> (String, String) {
        let Some(_) = model.and_then(DiscountModel::parse) else {
            return (upstream_url(base_url, path), path.to_string());
        };
        let Some(id) = super::openai_video_task_id(path) else {
            return (upstream_url(base_url, path), path.to_string());
        };
        (
            Self::video_base_path(&format!("/v1/result/{id}")),
            format!("/v1/result/{id}"),
        )
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
            return Ok(Self::passthrough(upstream, route, body));
        }
        let content_type = headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("application/json");
        if !content_type
            .to_ascii_lowercase()
            .starts_with("application/json")
        {
            return Err(AppError::BadRequest(
                "GlobalAI OPC Seedance discount video upstream requires application/json requests"
                    .to_string(),
            ));
        }
        let input: Value = serde_json::from_slice(&body)
            .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
        let object = input.as_object().ok_or_else(|| {
            AppError::BadRequest("video request body must be a JSON object".into())
        })?;
        let model = object
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .ok_or_else(|| AppError::BadRequest("model is required".into()))?;
        let Some(model) = DiscountModel::parse(model) else {
            return Ok(Self::passthrough(upstream, route, body));
        };

        let output = discount_request(object, model)?;
        Ok(PreparedUpstreamRequest {
            url: Self::video_base_path("/v1/seedance-discount/videos"),
            log_path: "/v1/seedance-discount/videos".to_string(),
            body: Bytes::from(serde_json::to_vec(&output)?),
            extra_headers: HeaderMap::new(),
            response_mode: AdapterResponseMode::Passthrough,
        })
    }

    fn normalize_response_body(&self, route: RelayRoute, body: Bytes) -> AppResult<Bytes> {
        if route != RelayRoute::Videos {
            return Ok(body);
        }
        let mut value: Value = serde_json::from_slice(&body)?;
        let Some(object) = value.as_object_mut() else {
            return Ok(body);
        };
        if let Some(status) = object
            .get_mut("status")
            .and_then(|value| value.as_str())
            .map(str::to_ascii_lowercase)
        {
            object.insert("status".to_string(), Value::String(status));
        }
        if let Some(total_tokens) = object.get("totalTokens").and_then(value_as_i64) {
            object.insert(
                "usage".to_string(),
                serde_json::json!({
                    "input_tokens": total_tokens,
                    "output_tokens": 0,
                    "total_tokens": total_tokens,
                }),
            );
        }
        Ok(Bytes::from(serde_json::to_vec(&value)?))
    }

    fn video_content_url(
        &self,
        model: Option<&str>,
        metadata: &Value,
        fallback_status: &str,
    ) -> AppResult<Option<reqwest::Url>> {
        if model.and_then(DiscountModel::parse).is_none() {
            return Ok(None);
        }
        let status = metadata
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or(fallback_status)
            .to_ascii_lowercase();
        if !matches!(status.as_str(), "completed" | "succeeded" | "success") {
            if matches!(
                status.as_str(),
                "failed" | "cancelled" | "canceled" | "expired"
            ) {
                return Err(AppError::BadRequest(format!(
                    "video content is unavailable because the task ended with status {status}"
                )));
            }
            return Err(AppError::BadRequest(
                "video content is not available until the task completes".to_string(),
            ));
        }
        let video_url = metadata
            .get("video_url")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AppError::BadRequest("completed video response is missing video_url".into())
            })?;
        let url = reqwest::Url::parse(video_url)
            .map_err(|_| AppError::BadRequest("upstream video_url is invalid".into()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(AppError::BadRequest(
                "upstream video_url must use http or https".into(),
            ));
        }
        Ok(Some(url))
    }
}

fn asset_payload(value: &Value) -> Option<&Map<String, Value>> {
    let object = value.as_object()?;
    if object.contains_key("assetId") || object.contains_key("asset_id") {
        return Some(object);
    }
    for key in ["data", "result", "output", "asset"] {
        if let Some(payload) = object.get(key).and_then(asset_payload) {
            return Some(payload);
        }
    }
    None
}

fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str()?.trim().parse().ok())
}

fn global_asset_type(asset_type: AssetType) -> &'static str {
    match asset_type {
        AssetType::Image => "Image",
        AssetType::Video => "Video",
        AssetType::Audio => "Audio",
    }
}

fn normalize_asset_status(status: &str) -> String {
    match status.to_ascii_uppercase().as_str() {
        "NONE" => "queued".to_string(),
        "UPLOADING" | "PROCESSING" => "processing".to_string(),
        "ACTIVE" => "active".to_string(),
        "FAILED" => "failed".to_string(),
        "EXPIRED" => "expired".to_string(),
        "DELETED" => "deleted".to_string(),
        _ => status.to_ascii_lowercase(),
    }
}

fn discount_request(
    input: &Map<String, Value>,
    model: DiscountModel,
) -> AppResult<Map<String, Value>> {
    for key in [
        "callback_url",
        "priority",
        "safety_identifier",
        "service_tier",
        "execution_expires_after",
        "frames",
        "camera_fixed",
        "watermark",
        "draft",
    ] {
        if input.contains_key(key) {
            return Err(AppError::BadRequest(format!(
                "GlobalAI OPC discount video does not support field {key}"
            )));
        }
    }
    let resolution = input
        .get("resolution")
        .and_then(Value::as_str)
        .map(normalize_resolution)
        .or_else(|| {
            input
                .get("size")
                .and_then(Value::as_str)
                .map(normalize_resolution)
        })
        .unwrap_or_else(|| model.resolution.unwrap_or("480p").to_string());
    let allowed = if model.is_fast() {
        ["480p", "720p"].as_slice()
    } else {
        ["480p", "720p", "1080p"].as_slice()
    };
    if !allowed.contains(&resolution.as_str()) {
        return Err(AppError::BadRequest(format!(
            "GlobalAI OPC model {} does not support resolution {resolution}",
            model.base
        )));
    }
    let upstream_model = format!(
        "{}_{resolution}{}",
        model.base,
        if has_video_reference(input) || model.with_video_ref {
            "_with_video_ref"
        } else {
            ""
        }
    );
    let content = build_content(input)?;
    let duration = positive_integer(input, "duration")
        .or_else(|| positive_integer(input, "seconds"))
        .unwrap_or(5);
    if !(4..=15).contains(&duration) {
        return Err(AppError::BadRequest(
            "GlobalAI OPC video duration must be between 4 and 15 seconds".into(),
        ));
    }
    let ratio = input.get("ratio").and_then(Value::as_str).unwrap_or("16:9");
    if !["16:9", "9:16", "1:1", "4:3", "3:4", "21:9", "adaptive"].contains(&ratio) {
        return Err(AppError::BadRequest(format!(
            "unsupported GlobalAI OPC video ratio: {ratio}"
        )));
    }
    validate_content(&content)?;

    let mut output = Map::new();
    output.insert("model".into(), Value::String(upstream_model));
    output.insert("resolution".into(), Value::String(resolution));
    output.insert("ratio".into(), Value::String(ratio.to_string()));
    output.insert("duration".into(), Value::from(duration));
    for key in ["generate_audio", "return_last_frame", "tools", "seed"] {
        if let Some(value) = input.get(key) {
            output.insert(key.to_string(), value.clone());
        }
    }
    output.insert("content".into(), Value::Array(content));
    Ok(output)
}

fn build_content(input: &Map<String, Value>) -> AppResult<Vec<Value>> {
    let mut content = input
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(prompt) = input
        .get("prompt")
        .and_then(Value::as_str)
        .filter(|prompt| !prompt.trim().is_empty())
    {
        let has_text = content
            .iter()
            .any(|item| item.get("type").and_then(Value::as_str) == Some("text"));
        if !has_text {
            content.insert(0, serde_json::json!({"type":"text", "text":prompt}));
        }
    }
    if let Some(reference) = input.get("input_reference") {
        let image_url = reference
            .as_str()
            .or_else(|| reference.as_object()?.get("image_url")?.as_str())
            .filter(|url| !url.trim().is_empty())
            .ok_or_else(|| {
                AppError::BadRequest("GlobalAI OPC input_reference requires an image URL".into())
            })?;
        content.push(serde_json::json!({
            "type": "image_url",
            "role": "reference_image",
            "image_url": {"url": image_url}
        }));
    }
    Ok(content)
}

fn validate_content(content: &[Value]) -> AppResult<()> {
    if !content.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("text")
            && item
                .get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
    }) {
        return Err(AppError::BadRequest(
            "GlobalAI OPC video content must include a text prompt".into(),
        ));
    }
    let roles: Vec<&str> = content
        .iter()
        .filter_map(|item| item.get("role").and_then(Value::as_str))
        .collect();
    let has_first_last = roles.contains(&"first_frame") || roles.contains(&"last_frame");
    let has_reference = roles.iter().any(|role| {
        matches!(
            *role,
            "reference_image" | "reference_video" | "reference_audio"
        )
    });
    if has_first_last && has_reference {
        return Err(AppError::BadRequest(
            "GlobalAI OPC first/last-frame and multimodal reference scenes cannot be mixed".into(),
        ));
    }
    let has_audio = content.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("audio_url")
            || item.get("role").and_then(Value::as_str) == Some("reference_audio")
    });
    let has_image_or_video = content.iter().any(|item| {
        matches!(
            item.get("type").and_then(Value::as_str),
            Some("image_url") | Some("video_url")
        )
    });
    if has_audio && !has_image_or_video {
        return Err(AppError::BadRequest(
            "GlobalAI OPC reference audio must be accompanied by an image or video".into(),
        ));
    }
    for item in content {
        for key in ["image_url", "video_url", "audio_url"] {
            if let Some(value) = item
                .get(key)
                .and_then(Value::as_object)
                .and_then(|value| value.get("url"))
                .and_then(Value::as_str)
            {
                if value.starts_with("data:") {
                    return Err(AppError::BadRequest(
                        "GlobalAI OPC video image/audio inputs do not support base64 URLs".into(),
                    ));
                }
                if let Some(asset_id) = value.strip_prefix("assetId://") {
                    if asset_id.trim().is_empty() {
                        return Err(AppError::BadRequest(
                            "GlobalAI OPC asset reference must include an asset id".into(),
                        ));
                    }
                    continue;
                }
                let url = reqwest::Url::parse(value).map_err(|_| {
                    AppError::BadRequest(
                        "GlobalAI OPC video inputs must use public http or https URLs".into(),
                    )
                })?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(AppError::BadRequest(
                        "GlobalAI OPC video inputs must use public http or https URLs".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn has_video_reference(input: &Map<String, Value>) -> bool {
    input
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|content| {
            content.iter().any(|item| {
                item.get("role").and_then(Value::as_str) == Some("reference_video")
                    || item.get("type").and_then(Value::as_str) == Some("video_url")
            })
        })
}

fn positive_integer(input: &Map<String, Value>, key: &str) -> Option<i64> {
    input
        .get(key)
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        .filter(|value| *value > 0)
}

fn normalize_resolution(value: &str) -> String {
    let value = value.to_ascii_lowercase().replace([' ', '_'], "");
    if value.contains("1080") {
        "1080p".into()
    } else if value.contains("720") {
        "720p".into()
    } else if value.contains("480") {
        "480p".into()
    } else {
        value
    }
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
            provider: "openai".into(),
            channel_name: "globalaiopc".into(),
            base_url: "http://apillm.globalaiopc.com/gw_llm_power".into(),
            adapter_hint: None,
            responses_chat_fallback: false,
            secret: "test-key".into(),
            account_id: None,
            affinity: None,
        }
    }

    #[test]
    fn matches_only_globalaiopc_host() {
        assert!(matches_base_url(
            "http://apillm.globalaiopc.com/gw_llm_power"
        ));
        assert!(!matches_base_url(
            "http://globalaiopc.com.example.com/gw_llm_power"
        ));
    }

    #[test]
    fn converts_discount_request_and_infers_resolution() {
        let value = discount_request(
            serde_json::json!({"model":"sd_2.0_fast_discount","prompt":"walk","seconds":5,"size":"1280x720"}).as_object().unwrap(),
            DiscountModel::parse("sd_2.0_fast_discount").unwrap(),
        ).unwrap();
        assert_eq!(value["model"], "sd_2.0_fast_discount_720p");
        assert_eq!(value["duration"], 5);
        assert_eq!(value["content"][0]["type"], "text");
    }

    #[test]
    fn converts_openai_json_input_reference_to_global_content() {
        let value = discount_request(
            serde_json::json!({
                "model": "sd_2.0_discount",
                "prompt": "walk",
                "input_reference": {"image_url": "https://example.com/ref.png"}
            })
            .as_object()
            .unwrap(),
            DiscountModel::parse("sd_2.0_discount").unwrap(),
        )
        .unwrap();

        assert_eq!(value["content"][1]["type"], "image_url");
        assert_eq!(
            value["content"][1]["image_url"]["url"],
            "https://example.com/ref.png"
        );
        assert_eq!(value["content"][1]["role"], "reference_image");
    }

    #[test]
    fn prepares_asset_create_requests_for_all_asset_types() {
        for (asset_type, expected) in [
            (AssetType::Image, "Image"),
            (AssetType::Video, "Video"),
            (AssetType::Audio, "Audio"),
        ] {
            let prepared = GLOBALAIOPC_ADAPTER
                .prepare_asset_create_request(
                    &upstream(),
                    "sd_2.0_discount",
                    &AssetCreateRequest {
                        asset_type,
                        url: "https://cdn.example.com/input".into(),
                        name: Some("reference".into()),
                    },
                )
                .unwrap();
            assert_eq!(
                prepared.url,
                "https://zcbservice.aizfw.cn/kyyReactApiServer/asset/seedance2/assetUpload"
            );
            assert_eq!(prepared.log_path, "/asset/seedance2/assetUpload");
            let body: Value = serde_json::from_slice(&prepared.body).unwrap();
            assert_eq!(body["assetType"], expected);
            assert_eq!(body["url"], "https://cdn.example.com/input");
            assert_eq!(body["name"], "reference");
        }
    }

    #[test]
    fn prepares_asset_detail_request() {
        let prepared = GLOBALAIOPC_ADAPTER
            .prepare_asset_detail_request(&upstream(), "sd_2.0_discount", "upstream-asset")
            .unwrap();
        assert_eq!(
            prepared.url,
            "https://zcbservice.aizfw.cn/kyyReactApiServer/asset/seedance2/assetDetail"
        );
        assert_eq!(prepared.log_path, "/asset/seedance2/assetDetail");
        let body: Value = serde_json::from_slice(&prepared.body).unwrap();
        assert_eq!(body["assetId"], "upstream-asset");
    }

    #[test]
    fn normalizes_total_tokens_and_status() {
        let body = GLOBALAIOPC_ADAPTER
            .normalize_response_body(
                RelayRoute::Videos,
                Bytes::from_static(br#"{"status":"COMPLETED","totalTokens":123}"#),
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["status"], "completed");
        assert_eq!(value["usage"]["total_tokens"], 123);
    }

    #[test]
    fn normalizes_string_total_tokens_for_billing() {
        let body = GLOBALAIOPC_ADAPTER
            .normalize_response_body(
                RelayRoute::Videos,
                Bytes::from_static(br#"{"status":"completed","totalTokens":"40594"}"#),
            )
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["usage"]["input_tokens"], 40594);
        assert_eq!(value["usage"]["output_tokens"], 0);
        assert_eq!(value["usage"]["total_tokens"], 40594);
    }

    #[test]
    fn maps_discount_task_lookup_to_fixed_video_service() {
        let (url, log_path) = GLOBALAIOPC_ADAPTER.resolve_video_task_url(
            "http://apillm.globalaiopc.com/gw_llm_power",
            "/v1/videos/video_123",
            Some("sd_2.0_fast_discount"),
        );
        assert_eq!(
            url,
            "https://zcbservice.aizfw.cn/kyyReactApiServer/v1/result/video_123"
        );
        assert_eq!(log_path, "/v1/result/video_123");
    }

    #[test]
    fn exposes_completed_discount_video_as_unauthenticated_content_url() {
        let source = GLOBALAIOPC_ADAPTER
            .video_content_url(
                Some("sd_2.0_discount"),
                &serde_json::json!({
                    "status": "completed",
                    "video_url": "https://cdn.example.com/video.mp4"
                }),
                "processing",
            )
            .unwrap();
        let url = source.expect("expected unauthenticated video URL");
        assert_eq!(url.as_str(), "https://cdn.example.com/video.mp4");
    }

    #[test]
    fn rejects_discount_video_content_before_completion() {
        let error = GLOBALAIOPC_ADAPTER
            .video_content_url(
                Some("sd_2.0_fast_discount"),
                &serde_json::json!({"status": "processing"}),
                "queued",
            )
            .unwrap_err();
        assert!(error.to_string().contains("until the task completes"));
        assert!(GLOBALAIOPC_ADAPTER
            .video_content_url(
                Some("unrelated-video-model"),
                &serde_json::json!({}),
                "queued",
            )
            .unwrap()
            .is_none());
    }

    #[test]
    fn accepts_internal_asset_references_but_rejects_other_non_public_urls() {
        let asset_reference = serde_json::json!({
            "model": "sd_2.0_discount",
            "content": [
                {"type":"text", "text":"walk"},
                {"type":"image_url", "role":"reference_image", "image_url":{"url":"assetId://asset-1"}}
            ]
        });
        assert!(discount_request(
            asset_reference.as_object().unwrap(),
            DiscountModel::parse("sd_2.0_discount").unwrap(),
        )
        .is_ok());

        let non_public = serde_json::json!({
            "model": "sd_2.0_discount",
            "content": [
                {"type":"text", "text":"walk"},
                {"type":"image_url", "role":"reference_image", "image_url":{"url":"ftp://example.com/image.png"}}
            ]
        });
        assert!(discount_request(
            non_public.as_object().unwrap(),
            DiscountModel::parse("sd_2.0_discount").unwrap(),
        )
        .unwrap_err()
        .to_string()
        .contains("public http or https"));
    }

    #[test]
    fn rejects_fast_1080p() {
        let fast_1080p = serde_json::json!({
            "model": "sd_2.0_fast_discount",
            "resolution": "1080p",
            "prompt": "walk"
        });
        assert!(discount_request(
            fast_1080p.as_object().unwrap(),
            DiscountModel::parse("sd_2.0_fast_discount").unwrap()
        )
        .is_err());
    }

    #[test]
    fn normalizes_asset_responses_and_formats_references() {
        for (upstream, expected) in [
            ("NONE", "queued"),
            ("UPLOADING", "processing"),
            ("PROCESSING", "processing"),
            ("ACTIVE", "active"),
            ("FAILED", "failed"),
            ("EXPIRED", "expired"),
            ("DELETED", "deleted"),
        ] {
            let normalized = GLOBALAIOPC_ADAPTER
                .normalize_asset_response(Bytes::from(
                    serde_json::to_vec(&serde_json::json!({
                        "assetId": "upstream-asset",
                        "assetType": "Image",
                        "status": upstream,
                        "errorMessage": "upload failed"
                    }))
                    .unwrap(),
                ))
                .unwrap();
            assert_eq!(normalized.status, expected);
            assert_eq!(normalized.asset_type, AssetType::Image);
            assert_eq!(normalized.error_message.as_deref(), Some("upload failed"));
        }
        assert_eq!(
            GLOBALAIOPC_ADAPTER
                .format_asset_reference(AssetType::Video, "upstream-asset")
                .unwrap(),
            "assetId://upstream-asset"
        );
    }

    #[test]
    fn normalizes_wrapped_asset_response() {
        let body = Bytes::from_static(
            br#"{"code":200,"data":{"asset_id":"wrapped-asset","asset_type":"Image","status":"ACTIVE","error_message":null}}"#,
        );
        let normalized = GLOBALAIOPC_ADAPTER.normalize_asset_response(body).unwrap();
        assert_eq!(normalized.upstream_asset_id, "wrapped-asset");
        assert_eq!(normalized.asset_type, AssetType::Image);
        assert_eq!(normalized.status, "active");
        assert_eq!(normalized.error_message, None);
    }

    #[test]
    fn discount_model_parser_rejects_unknown_suffixes() {
        assert!(DiscountModel::parse("sd_2.0_discount").is_some());
        assert!(DiscountModel::parse("sd_2.0_discount_720p_with_video_ref").is_some());
        assert!(DiscountModel::parse("sd_2.0_discount_unknown").is_none());
        assert!(DiscountModel::parse("sd_2.0_fast_discount_1080p").is_some());
    }
}
