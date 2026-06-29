use std::{sync::Arc, time::Instant};

use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{BillableUsage, BillingAccounts, BillingMeter, SettleRequest},
    error::{AppError, AppResult},
    provider::newapi,
    relay::{
        describe_upstream_http_failure, finish_relay, forward_openai_with_content_type,
        handle_upstream_http_error, log_upstream_http_failure, read_upstream_error_body,
        record_upstream_http_failure, record_upstream_transport_failure_for_failover,
        release_empty_hold, reserve_billable_credit, reserve_credit, respond_upstream_http_failure,
        response_from_bytes, rewrite_relay_body_model, selector::AttemptedUpstream,
        should_failover_retryable_upstream_failure, BodyKind, RelayContext, RelayRequestParams,
    },
    AppState,
};

use super::{log_relay_transport_failover, select_upstream_excluding};

#[derive(Debug, Clone)]
struct ImageRequestMeta {
    model: String,
    stream: bool,
    image_count: i64,
    request_params: RelayRequestParams,
    content_type: HeaderValue,
}

pub(super) async fn relay_openai_image(
    state: Arc<AppState>,
    auth: UserAuth,
    headers: HeaderMap,
    body: Bytes,
    path: &'static str,
) -> AppResult<Response> {
    let meta = image_request_meta(path, &headers, &body)?;
    let resolved =
        crate::project::models::resolve_project_model(&state.db.pool, auth.project_id, &meta.model)
            .await?;
    let content_type = meta.content_type.to_str().unwrap_or("");
    let upstream_body = if resolved.target_model == meta.model {
        body.clone()
    } else if content_type
        .to_ascii_lowercase()
        .starts_with("application/json")
    {
        rewrite_relay_body_model(body.clone(), BodyKind::OpenaiJson, &resolved.target_model)?
    } else {
        rewrite_multipart_model_field(&body, content_type, &resolved.target_model)?
    };
    let user_key_model_credit_account =
        auth.model_credit_account(&resolved.external_model).cloned();
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let estimated_image_units = meta.image_count.max(1);
    let relay_trace_id = Uuid::new_v4();
    let mut retryable_failovers = 0;
    let mut attempted_upstreams = Vec::new();

    loop {
        let started = Instant::now();
        let (protocol, upstream) = select_upstream_excluding(
            &state,
            path,
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
                &upstream.provider,
                &resolved.target_model,
                &auth.user_group,
            )
            .await?;
        let hold = if price.billing_meter == BillingMeter::Image {
            reserve_billable_credit(
                &state,
                &auth,
                user_key_model_credit_account.as_ref(),
                estimated_image_units.saturating_mul(
                    price
                        .unit_price_usd_micros
                        .ok_or_else(|| {
                            AppError::BadRequest(
                                "unit price is required for image billing".to_string(),
                            )
                        })?
                        .max(0),
                ),
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
            path,
            model: resolved.target_model.clone(),
            external_model: resolved.external_model.clone(),
            upstream_model: resolved.target_model.clone(),
            streamed: meta.stream,
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
        };
        let response = forward_openai_with_content_type(
            &state,
            &upstream,
            protocol,
            upstream_body.clone(),
            path,
            meta.content_type.clone(),
            meta.stream,
        )
        .await;

        match response {
            Ok(upstream_response) => {
                let status = StatusCode::from_u16(upstream_response.status().as_u16())
                    .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
                if status.is_success() {
                    ctx.mark_final_with_permit(&mut request_permit);
                    if newapi::should_wrap_image_stream(&upstream.provider, meta.stream, path) {
                        return finish_newapi_image_stream(
                            ctx,
                            Ok(upstream_response),
                            estimated_image_units,
                        )
                        .await;
                    }
                    if ctx.price.billing_meter == BillingMeter::Image {
                        return finish_image_relay(
                            ctx,
                            Ok(upstream_response),
                            estimated_image_units,
                        )
                        .await;
                    }
                    return finish_relay(ctx, Ok(upstream_response)).await;
                }

                let error_body = read_upstream_error_body(upstream_response).await;
                let mut failure = describe_upstream_http_failure(status, &error_body);
                if newapi::should_retry_image_variation(&upstream.provider, path)
                    && newapi::should_retry_variation_as_edit(ctx.path, status, &error_body)
                {
                    failure.retryable = true;
                }
                if should_failover_retryable_upstream_failure(
                    &ctx,
                    &attempted_upstreams,
                    failure.retryable,
                    retryable_failovers,
                )
                .await
                {
                    retryable_failovers += 1;
                    log_upstream_http_failure(&ctx, status, &failure, None);
                    record_upstream_http_failure(&ctx, status, &failure, "upstream image failover")
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
                        "retryable upstream image http failure; retrying another upstream"
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
                    let summary = err.to_string();
                    log_relay_transport_failover(&ctx, &summary, retryable_failovers);
                    record_upstream_transport_failure_for_failover(&ctx, summary).await;
                    continue;
                }
                ctx.mark_final_with_permit(&mut request_permit);
                return finish_relay(ctx, Err(err)).await;
            }
        }
    }
}

async fn finish_image_relay(
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
    requested_image_count: i64,
) -> AppResult<Response> {
    if ctx.streamed {
        return finish_streamed_image_relay(ctx, response, requested_image_count).await;
    }
    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return handle_upstream_http_error(ctx, status, upstream_response).await;
    }
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let body = upstream_response.bytes().await?;
    let image_count = image_count_from_response_body(&body).ok_or_else(|| {
        AppError::BadRequest("image response missing non-empty data array".to_string())
    })?;
    let billing = settle_image_hold(&ctx, image_count, "image relay").await;
    let usage = crate::relay::usage_from_context(
        &ctx,
        Some(status.as_u16() as i32),
        None,
        None,
        None,
        billing,
    );
    crate::relay::enqueue_relay_usage(&ctx.state, usage, None).await;
    response_from_bytes(status, content_type, body)
}

async fn finish_streamed_image_relay(
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
    requested_image_count: i64,
) -> AppResult<Response> {
    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return handle_upstream_http_error(ctx, status, upstream_response).await;
    }
    let content_type = upstream_response
        .headers()
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("text/event-stream"));
    finish_streamed_image_upstream(
        ctx,
        status,
        content_type,
        upstream_response,
        requested_image_count,
    )
    .await
}

async fn finish_streamed_image_upstream(
    ctx: RelayContext,
    status: StatusCode,
    content_type: HeaderValue,
    upstream_response: reqwest::Response,
    requested_image_count: i64,
) -> AppResult<Response> {
    let relay = ImageStreamRelay {
        ctx: Some(ctx),
        status,
        stream: upstream_response.bytes_stream().boxed(),
        image_count: requested_image_count,
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from_stream(futures_util::stream::unfold(
            Some(relay),
            |relay| async move {
                let mut relay = relay?;
                match relay.stream.next().await {
                    Some(Ok(chunk)) => Some((Ok::<Bytes, std::io::Error>(chunk), Some(relay))),
                    Some(Err(err)) => {
                        relay.finish_error(err.to_string()).await;
                        Some((Err(std::io::Error::other(err)), None))
                    }
                    None => {
                        relay.finish_success().await;
                        None
                    }
                }
            },
        )))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

struct ImageStreamRelay {
    ctx: Option<RelayContext>,
    status: StatusCode,
    stream: futures_util::stream::BoxStream<'static, Result<Bytes, reqwest::Error>>,
    image_count: i64,
}

impl ImageStreamRelay {
    async fn finish_success(mut self) {
        let Some(ctx) = self.ctx.take() else {
            return;
        };
        let billing = settle_image_hold(&ctx, self.image_count, "streamed image relay").await;
        let usage = crate::relay::usage_from_context(
            &ctx,
            Some(self.status.as_u16() as i32),
            None,
            None,
            None,
            billing,
        );
        crate::relay::enqueue_relay_usage(&ctx.state, usage, None).await;
    }

    async fn finish_error(mut self, error: String) {
        let Some(ctx) = self.ctx.take() else {
            return;
        };
        release_empty_hold(&ctx.state, ctx.hold.clone(), "streamed image relay error").await;
        let failure = crate::relay::key_failure_from_context(&ctx, error.clone()).await;
        let usage = crate::relay::usage_from_context(
            &ctx,
            Some(self.status.as_u16() as i32),
            Some(error),
            None,
            None,
            None,
        );
        crate::relay::enqueue_relay_usage(&ctx.state, usage, failure).await;
    }
}

impl Drop for ImageStreamRelay {
    fn drop(&mut self) {
        if self.ctx.is_some() {
            tracing::warn!("image stream ended before completion; skipping image billing settle");
        }
    }
}

async fn settle_image_hold(
    ctx: &RelayContext,
    image_count: i64,
    context: &str,
) -> Option<crate::billing::BillingCharge> {
    match ctx
        .state
        .billing
        .settle(
            &ctx.state.db.pool,
            SettleRequest {
                accounts: BillingAccounts {
                    user_id: ctx.auth.user_id,
                    project_id: ctx.auth.project_id,
                    user_key_id: ctx.auth.user_key_id,
                    user_key_model_credit_account: ctx.user_key_model_credit_account.as_ref(),
                    user_key_credit_account: &ctx.auth.user_key_credit_account,
                    project_credit_account: &ctx.auth.project_credit_account,
                },
                hold: ctx.hold.clone(),
                usage: Some(BillableUsage::image(image_count)),
                price: &ctx.price,
            },
        )
        .await
    {
        Ok(billing) => Some(billing),
        Err(err) => {
            tracing::warn!("failed to settle {context} hold: {err}");
            None
        }
    }
}

async fn finish_newapi_image_stream(
    ctx: RelayContext,
    response: AppResult<reqwest::Response>,
    requested_image_count: i64,
) -> AppResult<Response> {
    let upstream_response = match response {
        Ok(response) => response,
        Err(err) => return finish_relay(ctx, Err(err)).await,
    };
    let status = StatusCode::from_u16(upstream_response.status().as_u16())
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    if !status.is_success() {
        return handle_upstream_http_error(ctx, status, upstream_response).await;
    }
    let content_type = upstream_response
        .headers()
        .get("content-type")
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    if newapi::is_event_stream(&content_type) {
        if ctx.price.billing_meter == BillingMeter::Image {
            return finish_streamed_image_upstream(
                ctx,
                status,
                content_type,
                upstream_response,
                requested_image_count,
            )
            .await;
        }
        return finish_relay(ctx, Ok(upstream_response)).await;
    }

    tracing::warn!(
        provider = %ctx.upstream.provider,
        channel_id = ctx.upstream.channel_id,
        channel_name = %ctx.upstream.channel_name,
        channel_endpoint_id = ctx.upstream.channel_endpoint_id,
        channel_key_id = ?ctx.upstream.channel_key_id,
        credential_id = ?ctx.upstream.credential_id,
        model = %ctx.model,
        path = ctx.path,
        "NewAPI image stream request returned non-SSE response; relaying upstream body without JSON-to-SSE buffering"
    );
    if ctx.price.billing_meter == BillingMeter::Image {
        finish_image_relay(ctx, Ok(upstream_response), requested_image_count).await
    } else {
        finish_relay(ctx, Ok(upstream_response)).await
    }
}

fn image_request_meta(
    path: &'static str,
    headers: &HeaderMap,
    body: &[u8],
) -> AppResult<ImageRequestMeta> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("application/json"));
    let content_type_text = content_type
        .to_str()
        .map_err(|_| AppError::BadRequest("invalid content-type header".to_string()))?;
    let content_type_text = content_type_text.to_string();
    let lower_content_type = content_type_text.to_ascii_lowercase();

    if lower_content_type.starts_with("application/json") {
        if path == "/v1/images/variations" {
            return Err(AppError::BadRequest(
                "image variations require multipart/form-data".to_string(),
            ));
        }
        return json_image_request_meta(body, content_type);
    }

    if lower_content_type.starts_with("multipart/form-data") {
        if path == "/v1/images/generations" {
            return Err(AppError::BadRequest(
                "image generations require application/json".to_string(),
            ));
        }
        return multipart_image_request_meta(body, &content_type_text, content_type);
    }

    Err(AppError::BadRequest(
        "images requests require application/json or multipart/form-data".to_string(),
    ))
}

fn json_image_request_meta(body: &[u8], content_type: HeaderValue) -> AppResult<ImageRequestMeta> {
    let value: Value = serde_json::from_slice(body)
        .map_err(|err| AppError::BadRequest(format!("invalid json: {err}")))?;
    let model = value
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty())
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?
        .to_string();
    let stream = value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let image_count = image_count_from_value(&value).unwrap_or(1);
    Ok(ImageRequestMeta {
        model,
        stream,
        image_count,
        request_params: RelayRequestParams::image(
            image_count,
            string_field(&value, "size"),
            string_field(&value, "quality"),
            string_field(&value, "style"),
        ),
        content_type,
    })
}

fn multipart_image_request_meta(
    body: &[u8],
    content_type_text: &str,
    content_type: HeaderValue,
) -> AppResult<ImageRequestMeta> {
    let boundary = multipart_boundary(content_type_text)?;
    let mut model = None;
    let mut stream = false;
    let mut image_count = 1_i64;
    let mut size = None;
    let mut quality = None;
    let mut style = None;
    for (name, value) in multipart_text_fields(body, &boundary)? {
        match name.as_str() {
            "model" if !value.is_empty() => model = Some(value),
            "stream" => stream = value == "true",
            "n" => image_count = parse_positive_image_count(&value).unwrap_or(1),
            "size" if !value.is_empty() => size = Some(value),
            "quality" if !value.is_empty() => quality = Some(value),
            "style" if !value.is_empty() => style = Some(value),
            _ => {}
        }
    }
    let model = model.ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    Ok(ImageRequestMeta {
        model,
        stream,
        image_count,
        request_params: RelayRequestParams::image(image_count, size, quality, style),
        content_type,
    })
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn image_count_from_value(value: &Value) -> Option<i64> {
    value
        .get("n")
        .and_then(Value::as_i64)
        .filter(|count| *count > 0)
}

fn parse_positive_image_count(value: &str) -> Option<i64> {
    value.trim().parse::<i64>().ok().filter(|count| *count > 0)
}

fn image_count_from_response_body(body: &[u8]) -> Option<i64> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let count = value
        .get("data")
        .and_then(Value::as_array)
        .map(|items| items.len() as i64)?;
    (count > 0).then_some(count)
}

fn multipart_boundary(content_type: &str) -> AppResult<String> {
    for part in content_type.split(';').skip(1) {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("boundary") {
            let boundary = value.trim().trim_matches('"');
            if boundary.is_empty() {
                break;
            }
            return Ok(boundary.to_string());
        }
    }
    Err(AppError::BadRequest(
        "multipart/form-data boundary is required".to_string(),
    ))
}

fn multipart_text_fields(body: &[u8], boundary: &str) -> AppResult<Vec<(String, String)>> {
    multipart_text_fields_with_ranges(body, boundary).map(|fields| {
        fields
            .into_iter()
            .map(|field| (field.name, field.value))
            .collect()
    })
}

struct MultipartTextField {
    name: String,
    value: String,
    value_start: usize,
    value_end: usize,
}

fn multipart_text_fields_with_ranges(
    body: &[u8],
    boundary: &str,
) -> AppResult<Vec<MultipartTextField>> {
    let marker = format!("--{boundary}").into_bytes();
    let mut fields = Vec::new();
    let Some(mut cursor) = find_bytes(body, &marker) else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };

    loop {
        cursor += marker.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        cursor = skip_line_break(body, cursor)?;
        let Some(next_marker_offset) = find_bytes(&body[cursor..], &marker) else {
            return Err(AppError::BadRequest("invalid multipart body".to_string()));
        };
        let mut part = &body[cursor..cursor + next_marker_offset];
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        if let Some(field) = multipart_text_field(part, cursor)? {
            fields.push(field);
        }
        cursor += next_marker_offset;
    }

    Ok(fields)
}

fn rewrite_multipart_model_field(
    body: &[u8],
    content_type: &str,
    target_model: &str,
) -> AppResult<Bytes> {
    let boundary = multipart_boundary(content_type)?;
    let fields = multipart_text_fields_with_ranges(body, &boundary)?;
    let Some(field) = fields.into_iter().find(|field| field.name == "model") else {
        return Err(AppError::BadRequest("model is required".to_string()));
    };
    let mut output =
        Vec::with_capacity(body.len().saturating_sub(field.value.len()) + target_model.len());
    output.extend_from_slice(&body[..field.value_start]);
    output.extend_from_slice(target_model.as_bytes());
    output.extend_from_slice(&body[field.value_end..]);
    Ok(Bytes::from(output))
}

fn skip_line_break(body: &[u8], cursor: usize) -> AppResult<usize> {
    if body.get(cursor..cursor + 2) == Some(b"\r\n") {
        return Ok(cursor + 2);
    }
    if body.get(cursor..cursor + 1) == Some(b"\n") {
        return Ok(cursor + 1);
    }
    Err(AppError::BadRequest("invalid multipart body".to_string()))
}

fn multipart_text_field(part: &[u8], part_start: usize) -> AppResult<Option<MultipartTextField>> {
    let (headers, value, value_start_offset) = if let Some(offset) = find_bytes(part, b"\r\n\r\n") {
        (&part[..offset], &part[offset + 4..], offset + 4)
    } else if let Some(offset) = find_bytes(part, b"\n\n") {
        (&part[..offset], &part[offset + 2..], offset + 2)
    } else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };
    let headers = std::str::from_utf8(headers)
        .map_err(|_| AppError::BadRequest("invalid multipart headers".to_string()))?;
    let Some(disposition) = headers.lines().find(|line| {
        line.to_ascii_lowercase()
            .starts_with("content-disposition:")
    }) else {
        return Ok(None);
    };
    if disposition.contains("filename=") {
        return Ok(None);
    }
    let Some(name) = multipart_disposition_name(disposition) else {
        return Ok(None);
    };
    let value_text = std::str::from_utf8(value)
        .map_err(|_| AppError::BadRequest("invalid multipart text field".to_string()))?
        .trim()
        .to_string();
    let leading_ws = value.len() - value.trim_ascii_start().len();
    let trailing_ws = value.trim_ascii_end().len();
    Ok(Some(MultipartTextField {
        name,
        value: value_text,
        value_start: part_start + value_start_offset + leading_ws,
        value_end: part_start + value_start_offset + trailing_ws,
    }))
}

fn multipart_disposition_name(disposition: &str) -> Option<String> {
    let (_, params) = disposition.split_once(':')?;
    for param in params.split(';').skip(1) {
        let (key, value) = param.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case("name") {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_json_image_generation_meta() {
        let body = br#"{"model":"gpt-image-1","prompt":"draw","stream":true}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let meta = image_request_meta("/v1/images/generations", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(meta.stream);
        assert_eq!(meta.image_count, 1);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("application/json")
        );
    }

    #[test]
    fn parses_json_image_count() {
        let body = br#"{"model":"gpt-image-1","prompt":"draw","n":3}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let meta = image_request_meta("/v1/images/generations", &headers, body).unwrap();

        assert_eq!(meta.image_count, 3);
    }

    #[test]
    fn parses_json_image_edit_meta() {
        let body = br#"{"model":"gpt-image-2","prompt":"edit","images":[{"image_url":"data:image/png;base64,AAAA"}],"stream":true,"partial_images":2}"#;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-2");
        assert!(meta.stream);
        assert_eq!(meta.image_count, 1);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("application/json")
        );
    }

    #[test]
    fn parses_multipart_image_edit_model_without_rewriting_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=----neogate-boundary"),
        );
        let body = b"------neogate-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n------neogate-boundary--\r\n";

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(!meta.stream);
        assert_eq!(meta.image_count, 1);
        assert_eq!(
            meta.content_type,
            HeaderValue::from_static("multipart/form-data; boundary=----neogate-boundary")
        );
    }

    #[test]
    fn rewrites_multipart_image_model_field() {
        let body = b"------neogate-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ncompany-image\r\n------neogate-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n------neogate-boundary--\r\n";

        let rewritten = rewrite_multipart_model_field(
            body,
            "multipart/form-data; boundary=----neogate-boundary",
            "gpt-image-1",
        )
        .unwrap();
        let text = std::str::from_utf8(&rewritten).unwrap();

        assert!(text.contains("\r\n\r\ngpt-image-1\r\n"));
        assert!(text.contains("PNG_BYTES"));
        assert!(!text.contains("company-image"));
    }

    #[test]
    fn parses_multipart_image_edit_stream_flag() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=\"quoted-boundary\""),
        );
        let body = b"--quoted-boundary\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n--quoted-boundary\r\nContent-Disposition: form-data; name=\"stream\"\r\n\r\ntrue\r\n--quoted-boundary\r\nContent-Disposition: form-data; name=\"n\"\r\n\r\n2\r\n--quoted-boundary\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n--quoted-boundary--\r\n";

        let meta = image_request_meta("/v1/images/edits", &headers, body).unwrap();

        assert_eq!(meta.model, "gpt-image-1");
        assert!(meta.stream);
        assert_eq!(meta.image_count, 2);
    }

    #[test]
    fn counts_images_from_json_response_data() {
        assert_eq!(
            image_count_from_response_body(br#"{"data":[{"url":"a"},{"url":"b"}]}"#),
            Some(2)
        );
        assert_eq!(image_count_from_response_body(br#"{"data":[]}"#), None);
    }

    #[test]
    fn multipart_image_variation_requires_model() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );
        let body = b"--x\r\nContent-Disposition: form-data; name=\"image\"; filename=\"input.png\"\r\nContent-Type: image/png\r\n\r\nPNG_BYTES\r\n--x--\r\n";

        let err = image_request_meta("/v1/images/variations", &headers, body).unwrap_err();

        assert!(err.to_string().contains("model is required"));
    }

    #[test]
    fn image_generation_rejects_multipart_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );

        let err = image_request_meta(
        "/v1/images/generations",
        &headers,
        b"--x\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\ngpt-image-1\r\n--x--\r\n",
    )
    .unwrap_err();

        assert!(err
            .to_string()
            .contains("image generations require application/json"));
    }

    #[test]
    fn image_variation_rejects_json_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );

        let err = image_request_meta(
            "/v1/images/variations",
            &headers,
            br#"{"model":"dall-e-2","image":"data:image/png;base64,AAAA"}"#,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("image variations require multipart/form-data"));
    }

    #[test]
    fn multipart_image_request_requires_boundary() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data"),
        );

        let err = image_request_meta("/v1/images/edits", &headers, b"").unwrap_err();

        assert!(err
            .to_string()
            .contains("multipart/form-data boundary is required"));
    }

    #[test]
    fn multipart_image_request_rejects_invalid_body() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("multipart/form-data; boundary=x"),
        );

        let err = image_request_meta("/v1/images/edits", &headers, b"not multipart").unwrap_err();

        assert!(err.to_string().contains("invalid multipart body"));
    }

    #[test]
    fn image_request_rejects_unsupported_content_type() {
        let mut headers = HeaderMap::new();
        headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"));

        let err = image_request_meta("/v1/images/generations", &headers, b"model=gpt-image-1")
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("application/json or multipart/form-data"));
    }
}
