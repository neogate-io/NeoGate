use axum::{
    http::{header, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures_util::StreamExt;

use crate::{
    error::{AppError, AppResult, UpstreamRequestError},
    relay::{body_from_bytes, body_from_stream, finish_relay, RelayContext},
};

pub(super) async fn finish_bridge_json(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
    convert: fn(&[u8], &str) -> AppResult<Bytes>,
    trailing_error_message: &'static str,
) -> AppResult<Response> {
    let (body, trailing_error) = read_body_until_error(upstream_response).await;
    let converted = match convert(&body, &ctx.model) {
        Ok(converted) => {
            if let Some(err) = trailing_error {
                tracing::debug!(
                    provider = %ctx.upstream.provider,
                    channel_id = ctx.upstream.channel_id,
                    channel_name = %ctx.upstream.channel_name,
                    channel_endpoint_id = ctx.upstream.channel_endpoint_id,
                    channel_key_id = ?ctx.upstream.channel_key_id,
                    credential_id = ?ctx.upstream.credential_id,
                    model = %ctx.model,
                    path = ctx.path,
                    base_url = %ctx.upstream.base_url,
                    error = %err,
                    detail = trailing_error_message,
                    "ignored trailing upstream body read error after complete fallback response"
                );
            }
            converted
        }
        Err(parse_err) => {
            if let Some(err) = trailing_error {
                let app_err = AppError::UpstreamRequest(UpstreamRequestError::from_reqwest(
                    ctx.upstream.provider.clone(),
                    &err,
                ));
                return finish_relay(ctx, Err(app_err)).await;
            }
            return Err(parse_err);
        }
    };
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body_from_bytes(ctx, status, converted))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

async fn read_body_until_error(
    upstream_response: reqwest::Response,
) -> (Vec<u8>, Option<reqwest::Error>) {
    let mut body = Vec::new();
    let mut stream = upstream_response.bytes_stream();
    let mut trailing_error = None;
    while let Some(chunk) = stream.next().await {
        match chunk {
            Ok(chunk) => body.extend_from_slice(&chunk),
            Err(err) => {
                trailing_error = Some(err);
                break;
            }
        }
    }
    (body, trailing_error)
}

pub(super) trait BridgeSseConverter {
    fn push(&mut self, chunk: &[u8]) -> Bytes;
    fn stopped(&self) -> bool;
}

pub(super) fn finish_bridge_stream<C: BridgeSseConverter + Send + 'static>(
    ctx: RelayContext,
    status: StatusCode,
    upstream_response: reqwest::Response,
    new_converter: fn(String) -> C,
    trailing_error_message: &'static str,
) -> AppResult<Response> {
    let content_length = upstream_response.content_length();
    let usage_buffer_limit_bytes = ctx.state.config.relay.usage_buffer_limit_bytes;
    let converter = new_converter(ctx.model.clone());
    let upstream_stream = upstream_response.bytes_stream();
    let stream = futures_util::stream::unfold(
        (upstream_stream, converter),
        move |(mut upstream_stream, mut converter)| async move {
            match upstream_stream.next().await {
                Some(Ok(chunk)) => {
                    let bytes = converter.push(&chunk);
                    Some((Ok(bytes), (upstream_stream, converter)))
                }
                Some(Err(err)) if converter.stopped() => {
                    tracing::debug!(
                        error = %err,
                        detail = trailing_error_message,
                        "ignored trailing upstream body read error after completed fallback stream"
                    );
                    None
                }
                Some(Err(err)) => Some((Err(err.to_string()), (upstream_stream, converter))),
                None => None,
            }
        },
    )
    .boxed();

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(body_from_stream(
            ctx,
            status,
            content_length,
            usage_buffer_limit_bytes,
            stream,
        ))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}
