use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method},
    response::Response,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use futures_util::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::video::{self, VideoBillingInput, VideoBillingMetadata},
    error::{reqwest_status, AppError, AppResult},
    provider::adapters::{adapter_for_endpoint, RelayRoute},
    relay::{
        describe_upstream_http_failure, finish_task_json_response, forward_openai_bound,
        forward_openai_video_task_bound, forward_openai_with_content_type, forward_prepared_openai,
        raw_upstream_response, read_upstream_error_body, record_upstream_http_failure,
        record_upstream_transport_failure_for_failover, release_empty_hold,
        reserve_billable_credit, reserve_credit, respond_upstream_http_failure,
        response_from_bytes, rewrite_relay_body_model, selector::AttemptedUpstream,
        should_failover_upstream_failure, BodyKind, RelayBody, RelayContext, RelayRequestParams,
    },
    task::{
        billing as task_billing,
        upstream::{self as upstream_task, NewUpstreamTask, UpstreamTaskType},
    },
    AppState,
};

use super::{
    assets::resolve_video_asset_request,
    content_type_header, json_string_field, log_relay_transport_failover,
    multipart::{
        multipart_boundary, multipart_files, multipart_text_fields, rewrite_multipart_model_field,
        safe_multipart_log_label,
    },
    positive_i64_field, positive_i64_text, required_json_string_field, select_upstream_excluding,
};

const VIDEO_CREATE_PATH: &str = "/v1/videos";

#[derive(Debug, Clone)]
struct VideoRequestMeta {
    model: String,
    content_type: HeaderValue,
    request_params: RelayRequestParams,
    video_billing_input: VideoBillingInput,
    is_json: bool,
}

pub(crate) async fn openai_videos(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    relay_openai_video_create(state, auth, headers, body).await
}

pub(crate) async fn openai_video(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(video_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) =
        upstream_task::fetch_task_for_auth(&state, &auth, UpstreamTaskType::OpenAiVideo, &video_id)
            .await?;
    let path = format!("/v1/videos/{video_id}");
    let response = forward_openai_video_task_bound(
        &state,
        &upstream,
        Method::GET,
        &path,
        task.upstream_model.as_deref(),
    )
    .await?;
    finish_task_json_response(state, auth, task, response).await
}

pub(crate) async fn openai_video_content(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(video_id): Path<String>,
) -> AppResult<Response> {
    let (task, upstream) =
        upstream_task::fetch_task_for_auth(&state, &auth, UpstreamTaskType::OpenAiVideo, &video_id)
            .await?;
    let adapter = adapter_for_endpoint(
        &upstream.provider,
        &upstream.base_url,
        upstream.adapter_hint.as_deref(),
    );
    if let Some(url) = adapter.video_content_url(
        task.upstream_model.as_deref(),
        &task.upstream_metadata,
        &task.status,
    )? {
        return proxy_video_content_url(&state, url).await;
    }
    let path = format!("/v1/videos/{video_id}/content");
    let response = forward_openai_bound(&state, &upstream, Method::GET, &path, None).await?;
    raw_upstream_response(response).await
}

async fn proxy_video_content_url(state: &AppState, url: reqwest::Url) -> AppResult<Response> {
    let response = state.http.get(url).send().await?;
    let status = reqwest_status(response.status());
    if !status.is_success() {
        return Err(AppError::UpstreamUnavailable(format!(
            "video content CDN returned {}",
            status.as_u16()
        )));
    }
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("video/mp4"));
    let content_length = response.headers().get(header::CONTENT_LENGTH).cloned();
    let stream = response
        .bytes_stream()
        .map(|result| result.map_err(std::io::Error::other));
    let mut builder = Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type);
    if let Some(content_length) = content_length {
        builder = builder.header(header::CONTENT_LENGTH, content_length);
    }
    builder
        .body(Body::from_stream(stream))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

async fn relay_openai_video_create(
    state: Arc<AppState>,
    auth: UserAuth,
    headers: HeaderMap,
    body: Bytes,
) -> AppResult<Response> {
    let meta = video_request_meta(&headers, &body)?;
    let resolved =
        crate::project::models::resolve_project_model(&state.db.pool, auth.project_id, &meta.model)
            .await?;
    let content_type_text = meta.content_type.to_str().unwrap_or("");
    let model_body = if resolved.target_model == meta.model {
        body.clone()
    } else if content_type_text
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        rewrite_relay_body_model(body.clone(), BodyKind::OpenaiJson, &resolved.target_model)?
    } else {
        rewrite_multipart_model_field(&body, content_type_text, &resolved.target_model)?
    };
    let asset_resolution = if meta.is_json {
        resolve_video_asset_request(&state, &auth, &resolved.target_model, model_body.clone())
            .await?
    } else {
        None
    };
    let upstream_body = asset_resolution
        .as_ref()
        .map(|resolved| resolved.body.clone())
        .unwrap_or(model_body);
    let bound_asset_upstream = asset_resolution
        .as_ref()
        .map(|resolved| resolved.upstream.clone());
    let user_key_model_credit_account =
        auth.model_credit_account(&resolved.external_model).cloned();
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let relay_trace_id = Uuid::new_v4();
    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();

    loop {
        let started = Instant::now();
        let (protocol, upstream) = if let Some(upstream) = bound_asset_upstream.clone() {
            (crate::relay::selector::UpstreamProtocol::Openai, upstream)
        } else {
            select_upstream_excluding(
                &state,
                VIDEO_CREATE_PATH,
                &resolved.target_model,
                resolved.target_channel_id,
                None,
                &attempted_upstreams,
            )
            .await?
        };
        attempted_upstreams.push(AttemptedUpstream::from(&upstream));
        let price = state
            .billing
            .price_for(
                &state.db.pool,
                upstream.channel_id,
                &resolved.target_model,
                &auth.user_group,
            )
            .await?;
        let prepared_video_billing = video::prepare_seedance_video_billing(
            &upstream.provider,
            &resolved.target_model,
            &price,
            &meta.video_billing_input,
        )?;
        let hold = if let Some(prepared) = &prepared_video_billing {
            reserve_billable_credit(
                &state,
                &auth,
                user_key_model_credit_account.as_ref(),
                prepared.estimated_micros,
            )
            .await?
        } else {
            reserve_credit(
                &state,
                &auth,
                user_key_model_credit_account.as_ref(),
                &upstream_body,
                state.billing.default_output_tokens(),
                &price,
            )
            .await?
        };
        let mut ctx = RelayContext {
            state: Arc::clone(&state),
            auth: auth.clone(),
            upstream: upstream.clone(),
            protocol,
            method: "POST",
            path: VIDEO_CREATE_PATH,
            model: resolved.target_model.clone(),
            external_model: resolved.external_model.clone(),
            upstream_model: resolved.target_model.clone(),
            routing: resolved.routing.clone(),
            streamed: false,
            price,
            hold,
            user_key_model_credit_account: user_key_model_credit_account.clone(),
            started,
            channel_affinity_key: None,
            relay_trace_id,
            relay_attempt: attempted_upstreams.len() as i32,
            relay_final: false,
            request_body_bytes: upstream_body.len(),
            request_input_tokens_estimate: crate::billing::estimate_input_tokens(&upstream_body),
            request_params: meta.request_params.clone(),
            request_permit: None,
            upstream_request_path: Some(RelayRoute::Videos.path().to_string()),
            upstream_response_mode: None,
        };
        let adapter = adapter_for_endpoint(
            &ctx.upstream.provider,
            &ctx.upstream.base_url,
            ctx.upstream.adapter_hint.as_deref(),
        );
        let response = if meta.is_json || adapter.prepares_video_request(&resolved.target_model) {
            let prepared = match adapter.prepare_openai_request(
                &ctx.upstream,
                protocol,
                RelayRoute::Videos,
                upstream_body.clone(),
                &headers,
                false,
            ) {
                Ok(prepared) => prepared,
                Err(err) => {
                    release_empty_hold(
                        &ctx.state,
                        ctx.hold,
                        "video request rejected by upstream adapter",
                    )
                    .await;
                    return Err(err);
                }
            };
            ctx.upstream_request_path = Some(prepared.log_path.clone());
            ctx.upstream_response_mode = Some(prepared.response_mode.as_str());
            forward_prepared_openai(&state, &ctx.upstream, protocol, &headers, prepared).await
        } else {
            forward_openai_with_content_type(
                &state,
                &upstream,
                protocol,
                upstream_body.clone(),
                VIDEO_CREATE_PATH,
                meta.content_type.clone(),
                false,
            )
            .await
        };

        match response {
            Ok(upstream_response) => {
                let status = reqwest_status(upstream_response.status());
                if status.is_success() {
                    ctx.mark_final_with_permit(&mut request_permit);
                    return finish_video_create_success(
                        ctx,
                        upstream_response,
                        prepared_video_billing.map(|prepared| prepared.metadata),
                    )
                    .await;
                }

                let error_body = read_upstream_error_body(upstream_response).await;
                let failure = describe_upstream_http_failure(status, &error_body);
                if bound_asset_upstream.is_none()
                    && should_failover_upstream_failure(
                        &ctx,
                        &attempted_upstreams,
                        failure.failoverable(),
                        retryable_failovers,
                    )
                    .await
                {
                    retryable_failovers += 1;
                    record_upstream_http_failure(&ctx, status, &failure, "upstream video failover")
                        .await;
                    tracing::warn!(
                        provider = %ctx.upstream.provider,
                        channel_id = ctx.upstream.channel_id,
                        channel_name = %ctx.upstream.channel_name,
                        channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                        channel_key_id = ?ctx.upstream.channel_key_id,
                        credential_id = ?ctx.upstream.credential_id,
                        protocol = ctx.protocol.as_str(),
                        model = %ctx.model,
                        path = ctx.path,
                        upstream_status = status.as_u16(),
                        failover_attempt = retryable_failovers,
                        max_failovers = ctx.state.config.relay.max_upstream_failovers,
                        "failoverable upstream video http failure; trying another upstream"
                    );
                    continue;
                }

                ctx.mark_final_with_permit(&mut request_permit);
                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => {
                let retryable = err.retryable();
                if bound_asset_upstream.is_none()
                    && should_failover_upstream_failure(
                        &ctx,
                        &attempted_upstreams,
                        retryable,
                        retryable_failovers,
                    )
                    .await
                {
                    retryable_failovers += 1;
                    record_upstream_transport_failure_for_failover(&ctx, err.to_string()).await;
                    log_relay_transport_failover(&ctx, &err.to_string(), retryable_failovers);
                    continue;
                }
                ctx.mark_final_with_permit(&mut request_permit);
                return crate::relay::finish_relay(ctx, Err(err)).await;
            }
        }
    }
}

async fn finish_video_create_success(
    mut ctx: RelayContext,
    upstream_response: reqwest::Response,
    video_billing_metadata: Option<VideoBillingMetadata>,
) -> AppResult<Response> {
    let status = reqwest_status(upstream_response.status());
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = match upstream_response.bytes().await {
        Ok(body) => {
            ctx.release_request_permit();
            adapter_for_endpoint(
                &ctx.upstream.provider,
                &ctx.upstream.base_url,
                ctx.upstream.adapter_hint.as_deref(),
            )
            .normalize_response_body(RelayRoute::Videos, body)?
        }
        Err(err) => {
            ctx.release_request_permit();
            release_empty_hold(&ctx.state, ctx.hold, "openai video create body read error").await;
            return Err(err.into());
        }
    };
    tracing::info!(
        provider = %ctx.upstream.provider,
        channel_id = ctx.upstream.channel_id,
        channel_name = %ctx.upstream.channel_name,
        model = %ctx.model,
        external_model = %ctx.external_model,
        upstream_status = status.as_u16(),
        upstream_response = %String::from_utf8_lossy(&body),
        "upstream openai video create response"
    );
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            release_empty_hold(&ctx.state, ctx.hold, "openai video create parse error").await;
            return Err(err.into());
        }
    };
    let mut task_metadata = value.clone();
    attach_video_task_relay_metadata(&mut task_metadata, &ctx);
    if let Some(metadata) = &video_billing_metadata {
        video::attach_video_billing_metadata(&mut task_metadata, metadata);
    }
    let video_id = match video_response_id(&value) {
        Some(id) if !id.is_empty() => id,
        _ => {
            release_empty_hold(&ctx.state, ctx.hold, "openai video create missing id").await;
            return Err(AppError::BadRequest(
                "upstream video response is missing id".to_string(),
            ));
        }
    };
    let status_text = video_status_text(&value, "queued");
    let terminal = video_terminal(&status_text);
    if let Err(err) = upstream_task::insert_task(
        &ctx.state.db.pool,
        NewUpstreamTask {
            task_type: UpstreamTaskType::OpenAiVideo,
            upstream_task_id: video_id,
            auth: &ctx.auth,
            protocol: ctx.protocol,
            upstream: &ctx.upstream,
            model: Some(&ctx.external_model),
            upstream_model: Some(&ctx.upstream_model),
            status: &status_text,
            terminal,
            hold: &ctx.hold,
            upstream_metadata: task_metadata,
        },
        crate::task::POLL_INTERVAL,
        ctx.state.config.task.upstream_retention,
    )
    .await
    {
        tracing::warn!(video_id, "failed to insert openai video task");
        release_empty_hold(&ctx.state, ctx.hold, "openai video task insert error").await;
        return Err(err);
    }
    if terminal {
        let usage = crate::billing::parse_usage_from_bytes(&body, false);
        task_billing::finalize_for_auth(
            &ctx.state,
            &ctx.auth,
            video_id,
            UpstreamTaskType::OpenAiVideo,
            usage,
            true,
        )
        .await?;
    }
    response_from_bytes(status, content_type, body)
}

fn video_request_meta(headers: &HeaderMap, body: &[u8]) -> AppResult<VideoRequestMeta> {
    let (content_type, content_type_text) = content_type_header(headers)?;
    let lower = content_type_text.to_ascii_lowercase();
    if lower.starts_with("application/json") {
        return json_video_request_meta(body, content_type);
    }
    if lower.starts_with("multipart/form-data") {
        return multipart_video_request_meta(body, &content_type_text, content_type);
    }
    Err(AppError::BadRequest(
        "videos requests require application/json or multipart/form-data".to_string(),
    ))
}

fn json_video_request_meta(body: &[u8], content_type: HeaderValue) -> AppResult<VideoRequestMeta> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let model = required_json_string_field(&value, "model")?;
    validate_json_video_references(&value)?;
    Ok(VideoRequestMeta {
        model: model.clone(),
        request_params: RelayRequestParams::video(
            json_string_field(&value, "size"),
            positive_i64_field(&value, "seconds")
                .or_else(|| positive_i64_field(&value, "duration")),
        ),
        video_billing_input: video::json_video_billing_input(&value, Some(&model)),
        content_type,
        is_json: true,
    })
}

fn multipart_video_request_meta(
    body: &[u8],
    content_type_text: &str,
    content_type: HeaderValue,
) -> AppResult<VideoRequestMeta> {
    let boundary = multipart_boundary(content_type_text)?;
    validate_multipart_video_references(body, &boundary)?;
    let mut model = None;
    let mut prompt = None;
    let mut size = None;
    let mut seconds = None;
    for (name, value) in multipart_text_fields(body, &boundary)? {
        match name.as_str() {
            "model" if !value.is_empty() => model = Some(value),
            "prompt" if !value.trim().is_empty() => prompt = Some(value),
            "size" if !value.is_empty() => size = Some(safe_multipart_log_label(&value)),
            "seconds" | "duration" => seconds = positive_i64_text(&value),
            "input_reference" => {
                return Err(AppError::BadRequest(
                    "input_reference must be uploaded as an image file".into(),
                ));
            }
            _ => {}
        }
    }
    let model = model.ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    if prompt.is_none() {
        return Err(AppError::BadRequest("prompt is required".to_string()));
    }
    let video_billing_input = video::video_billing_input(size.as_deref(), seconds, false);
    Ok(VideoRequestMeta {
        model,
        request_params: RelayRequestParams::video(size, seconds),
        video_billing_input,
        content_type,
        is_json: false,
    })
}

/// Validate the reference shape at the public OpenAI-compatible boundary.
/// Provider-specific multimodal extensions remain in JSON `content[]` and
/// are validated by the selected adapter.
fn validate_json_video_references(value: &Value) -> AppResult<()> {
    let Some(reference) = value.get("input_reference") else {
        return validate_extended_video_content(value);
    };
    let image_url = match reference {
        Value::String(url) => url.clone(),
        Value::Object(object) => {
            if object.contains_key("file_id") {
                return Err(AppError::BadRequest(
                    "input_reference.file_id is not supported; use multipart input_reference or an asset://asset_* reference".into(),
                ));
            }
            object
                .get("image_url")
                .and_then(Value::as_str)
                .filter(|url| !url.trim().is_empty())
                .ok_or_else(|| {
                    AppError::BadRequest("input_reference.image_url is required".into())
                })?
                .to_string()
        }
        _ => {
            return Err(AppError::BadRequest(
                "input_reference must be an image URL or an object with image_url".into(),
            ));
        }
    };
    let lower = image_url.to_ascii_lowercase();
    if !(lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("data:image/")
        || lower.starts_with("asset://asset_"))
    {
        return Err(AppError::BadRequest(
            "input_reference.image_url must be an http(s) URL, image data URL, or asset://asset_* reference".into(),
        ));
    }
    validate_extended_video_content(value)
}

fn validate_extended_video_content(value: &Value) -> AppResult<()> {
    let Some(content) = value.get("content") else {
        return Ok(());
    };
    let content = content
        .as_array()
        .ok_or_else(|| AppError::BadRequest("content must be an array".into()))?;
    for item in content {
        let object = item
            .as_object()
            .ok_or_else(|| AppError::BadRequest("content items must be objects".into()))?;
        let item_type = object
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::BadRequest("content item type is required".into()))?;
        match item_type {
            "text" => {
                if object
                    .get("text")
                    .and_then(Value::as_str)
                    .is_none_or(|text| text.trim().is_empty())
                {
                    return Err(AppError::BadRequest(
                        "text content requires non-empty text".into(),
                    ));
                }
            }
            "image_url" | "video_url" | "audio_url" => {
                if object
                    .get(item_type)
                    .and_then(Value::as_object)
                    .and_then(|media| media.get("url"))
                    .and_then(Value::as_str)
                    .is_none_or(|url| url.trim().is_empty())
                {
                    return Err(AppError::BadRequest(format!(
                        "{item_type} content requires {item_type}.url"
                    )));
                }
            }
            _ => {
                return Err(AppError::BadRequest(format!(
                    "unsupported video content type: {item_type}"
                )));
            }
        }
    }
    Ok(())
}

fn validate_multipart_video_references(body: &[u8], boundary: &str) -> AppResult<()> {
    let files = multipart_files(body, boundary)?;
    if let Some(file) = files.iter().find(|file| file.name != "input_reference") {
        return Err(AppError::BadRequest(format!(
            "unsupported video multipart file field: {}",
            file.name
        )));
    }
    let references: Vec<_> = files
        .into_iter()
        .filter(|file| file.name == "input_reference")
        .collect();
    if references.len() > 1 {
        return Err(AppError::BadRequest(
            "input_reference accepts only one reference image in the OpenAI video protocol".into(),
        ));
    }
    if let Some(reference) = references.first() {
        if reference.data.is_empty() {
            return Err(AppError::BadRequest(
                "input_reference image file must not be empty".into(),
            ));
        }
        let content_type = reference
            .content_type
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !content_type.is_empty() && !content_type.starts_with("image/") {
            return Err(AppError::BadRequest(
                "input_reference must be an image file".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn video_terminal(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "completed" | "succeeded" | "success" | "failed" | "cancelled" | "canceled" | "expired"
    )
}

pub(crate) fn video_status_text(value: &Value, fallback: &str) -> String {
    value
        .get("status")
        .and_then(Value::as_str)
        .or_else(|| value.get("task_status").and_then(Value::as_str))
        .or_else(|| nested_string(value, &["output", "status"]))
        .or_else(|| nested_string(value, &["output", "task_status"]))
        .unwrap_or(fallback)
        .to_ascii_lowercase()
}

fn video_response_id(value: &Value) -> Option<&str> {
    value
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| value.get("task_id").and_then(Value::as_str))
        .or_else(|| nested_string(value, &["output", "id"]))
        .or_else(|| nested_string(value, &["output", "task_id"]))
}

fn attach_video_task_relay_metadata(value: &mut Value, ctx: &RelayContext) {
    if !value.is_object() {
        return;
    }
    if !value.get("neogate").is_some_and(Value::is_object) {
        value["neogate"] = Value::Object(Default::default());
    }
    value["neogate"]["relay_trace_id"] = Value::String(ctx.relay_trace_id.to_string());
    let elapsed = ChronoDuration::from_std(ctx.started.elapsed()).unwrap_or_default();
    value["neogate"]["relay_started_at"] = Value::String((Utc::now() - elapsed).to_rfc3339());
}

fn nested_string<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_video_meta() {
        let meta = video_request_meta(
            &HeaderMap::new(),
            br#"{"model":"sora-2","prompt":"draw","size":"1280x720","seconds":4}"#,
        )
        .unwrap();

        assert_eq!(meta.model, "sora-2");
        assert_eq!(meta.request_params.video_size.as_deref(), Some("1280x720"));
        assert_eq!(meta.request_params.video_seconds, Some(4));
    }

    #[test]
    fn accepts_json_image_reference_and_multimodal_extension() {
        let value = serde_json::json!({
            "model": "sd_2.0_discount",
            "input_reference": {"image_url": "https://example.com/cover.png"},
            "content": [
                {"type": "text", "text": "walk"},
                {"type": "video_url", "video_url": {"url": "https://example.com/ref.mp4"}}
            ]
        });

        validate_json_video_references(&value).unwrap();
    }

    #[test]
    fn rejects_file_id_and_invalid_extended_content() {
        let file_id = serde_json::json!({
            "model": "sora-2",
            "input_reference": {"file_id": "file-123"}
        });
        assert!(validate_json_video_references(&file_id)
            .unwrap_err()
            .to_string()
            .contains("file_id is not supported"));

        let invalid_content = serde_json::json!({
            "model": "sd_2.0_discount",
            "content": [{"type": "video_url", "video_url": {}}]
        });
        assert!(validate_json_video_references(&invalid_content)
            .unwrap_err()
            .to_string()
            .contains("video_url.url"));
    }

    #[test]
    fn rejects_multiple_or_non_image_multipart_references() {
        let content_type = "multipart/form-data; boundary=boundary";
        let multiple = b"--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nsora-2\r\n--boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"a.png\"\r\nContent-Type: image/png\r\n\r\na\r\n--boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"b.png\"\r\nContent-Type: image/png\r\n\r\nb\r\n--boundary--\r\n";
        assert!(multipart_video_request_meta(
            multiple,
            content_type,
            HeaderValue::from_static("multipart/form-data; boundary=boundary"),
        )
        .unwrap_err()
        .to_string()
        .contains("only one reference image"));

        let video = b"--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nsora-2\r\n--boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"ref.mp4\"\r\nContent-Type: video/mp4\r\n\r\nvideo\r\n--boundary--\r\n";
        assert!(multipart_video_request_meta(
            video,
            content_type,
            HeaderValue::from_static("multipart/form-data; boundary=boundary"),
        )
        .unwrap_err()
        .to_string()
        .contains("must be an image file"));

        let empty = b"--boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nsora-2\r\n--boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\ndraw\r\n--boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"ref.png\"\r\nContent-Type: image/png\r\n\r\n\r\n--boundary--\r\n";
        assert!(multipart_video_request_meta(
            empty,
            content_type,
            HeaderValue::from_static("multipart/form-data; boundary=boundary"),
        )
        .unwrap_err()
        .to_string()
        .contains("must not be empty"));
    }

    #[test]
    fn parses_nested_video_task_response_fields() {
        let value = serde_json::json!({
            "output": {
                "task_id": "task_123",
                "task_status": "SUCCEEDED"
            }
        });

        assert_eq!(video_response_id(&value), Some("task_123"));
        assert_eq!(video_status_text(&value, "queued"), "succeeded");
        assert!(video_terminal(&video_status_text(&value, "queued")));
    }

    #[test]
    fn parses_and_rewrites_multipart_video_model() {
        let body = b"------neogate-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ncompany-video\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\ndraw\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n------neogate-boundary--\r\n";
        let content_type = "multipart/form-data; boundary=----neogate-boundary";
        let meta = multipart_video_request_meta(
            body,
            content_type,
            HeaderValue::from_static("multipart/form-data; boundary=----neogate-boundary"),
        )
        .unwrap();
        assert_eq!(meta.model, "company-video");

        let rewritten = rewrite_multipart_model_field(body, content_type, "sora-2").unwrap();
        let rewritten_text = std::str::from_utf8(&rewritten).unwrap();
        assert!(rewritten_text.contains("\r\nsora-2\r\n"));
        assert!(rewritten_text.contains("PNG_BYTES"));
        assert!(!rewritten_text.contains("company-video"));
    }

    #[test]
    fn multipart_video_requires_boundary() {
        let err = multipart_video_request_meta(
            b"",
            "multipart/form-data",
            HeaderValue::from_static("multipart/form-data"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("boundary is required"));
    }
}
