use std::{sync::Arc, time::Instant};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method},
    response::Response,
};
use bytes::Bytes;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::video::{self, VideoBillingInput, VideoBillingMetadata},
    error::{reqwest_status, AppError, AppResult},
    provider::adapters::{adapter_for_provider, RelayRoute},
    relay::{
        describe_upstream_http_failure, finish_task_json_response, forward_openai_bound,
        forward_openai_with_content_type, forward_prepared_openai, raw_upstream_response,
        read_upstream_error_body, record_upstream_http_failure,
        record_upstream_transport_failure_for_failover, release_empty_hold,
        reserve_billable_credit, reserve_credit, respond_upstream_http_failure,
        response_from_bytes, rewrite_relay_body_model, selector::AttemptedUpstream,
        should_failover_retryable_upstream_failure, BodyKind, RelayBody, RelayContext,
        RelayRequestParams,
    },
    task::{
        billing as task_billing,
        upstream::{self as upstream_task, NewUpstreamTask, UpstreamTaskType},
    },
    AppState,
};

use super::{
    content_type_header, json_string_field, log_relay_transport_failover,
    multipart::{
        multipart_boundary, multipart_text_fields, rewrite_multipart_model_field,
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
    let response = forward_openai_bound(&state, &upstream, Method::GET, &path, None).await?;
    finish_task_json_response(state, auth, task, response).await
}

pub(crate) async fn openai_video_content(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(video_id): Path<String>,
) -> AppResult<Response> {
    let (_task, upstream) =
        upstream_task::fetch_task_for_auth(&state, &auth, UpstreamTaskType::OpenAiVideo, &video_id)
            .await?;
    let path = format!("/v1/videos/{video_id}/content");
    let response = forward_openai_bound(&state, &upstream, Method::GET, &path, None).await?;
    raw_upstream_response(response).await
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
    let upstream_body = if resolved.target_model == meta.model {
        body.clone()
    } else if content_type_text
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        rewrite_relay_body_model(body.clone(), BodyKind::OpenaiJson, &resolved.target_model)?
    } else {
        rewrite_multipart_model_field(&body, content_type_text, &resolved.target_model)?
    };
    let user_key_model_credit_account =
        auth.model_credit_account(&resolved.external_model).cloned();
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let relay_trace_id = Uuid::new_v4();
    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();

    loop {
        let started = Instant::now();
        let (protocol, upstream) = select_upstream_excluding(
            &state,
            VIDEO_CREATE_PATH,
            &resolved.target_model,
            resolved.target_channel_id,
            None,
            &attempted_upstreams,
        )
        .await?;
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
            request_params: meta.request_params.clone(),
            request_permit: None,
            upstream_request_path: Some(RelayRoute::Videos.path().to_string()),
            upstream_response_mode: None,
        };
        let adapter = adapter_for_provider(&ctx.upstream.provider);
        let response = if meta.is_json || adapter.name() == "doubao" {
            let prepared = adapter.prepare_openai_request(
                &ctx.upstream,
                protocol,
                RelayRoute::Videos,
                upstream_body.clone(),
                &headers,
                false,
            )?;
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
                if should_failover_retryable_upstream_failure(
                    &ctx,
                    &attempted_upstreams,
                    failure.retryable,
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
                        "retryable upstream video http failure; retrying another upstream"
                    );
                    continue;
                }

                ctx.mark_final_with_permit(&mut request_permit);
                return respond_upstream_http_failure(ctx, status, failure).await;
            }
            Err(err) => {
                let retryable = err.retryable();
                if should_failover_retryable_upstream_failure(
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
            body
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
        ctx.state.config.task.upstream_poll_interval,
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
    Ok(VideoRequestMeta {
        model,
        request_params: RelayRequestParams::video(
            json_string_field(&value, "size"),
            positive_i64_field(&value, "seconds")
                .or_else(|| positive_i64_field(&value, "duration")),
        ),
        video_billing_input: video::json_video_billing_input(&value),
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
    let mut model = None;
    let mut size = None;
    let mut seconds = None;
    for (name, value) in multipart_text_fields(body, &boundary)? {
        match name.as_str() {
            "model" if !value.is_empty() => model = Some(value),
            "size" if !value.is_empty() => size = Some(safe_multipart_log_label(&value)),
            "seconds" | "duration" => seconds = positive_i64_text(&value),
            _ => {}
        }
    }
    let model = model.ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    let video_billing_input = video::video_billing_input(size.as_deref(), seconds, false);
    Ok(VideoRequestMeta {
        model,
        request_params: RelayRequestParams::video(size, seconds),
        video_billing_input,
        content_type,
        is_json: false,
    })
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
        let body = b"------neogate-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ncompany-video\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"input_reference\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n------neogate-boundary--\r\n";
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
