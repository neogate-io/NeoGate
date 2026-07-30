use std::{sync::Arc, time::Instant};

use axum::{
    extract::{
        ws::{Message as ClientMessage, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::{header::AUTHORIZATION, HeaderValue},
    response::Response,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{client::IntoClientRequest, Message as UpstreamMessage},
    MaybeTlsStream, WebSocketStream,
};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{BillableUsage, BillingAccounts, BillingMeter, DebitHold, Price, SettleRequest},
    error::{AppError, AppResult, UpstreamErrorKind, UpstreamRequestError},
    relay::{
        release_empty_hold, reserve_billable_credit,
        selector::{SelectedUpstream, UpstreamProtocol},
    },
    usage::UsageInsert,
    AppState,
};

use super::select_upstream_excluding;

const REALTIME_PATH: &str = "/v1/realtime";
const INITIAL_AUDIO_RESERVATION_SECONDS: i64 = 60;
const MAX_AUDIO_CHUNK_BYTES: usize = 15 * 1024 * 1024;
const MAX_CLIENT_MESSAGE_BYTES: usize = 21 * 1024 * 1024;
const DEFAULT_SAMPLE_RATE: u32 = 16_000;
const PCM_BYTES_PER_SAMPLE: u64 = 2;

type UpstreamSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, Deserialize)]
pub(crate) struct RealtimeQuery {
    model: String,
}

struct RealtimeContext {
    state: Arc<AppState>,
    auth: UserAuth,
    upstream: SelectedUpstream,
    protocol: UpstreamProtocol,
    external_model: String,
    upstream_model: String,
    routing: Option<crate::project::models::UsageRoutingSnapshot>,
    price: Price,
    hold: DebitHold,
    relay_trace_id: Uuid,
    started: Instant,
}

#[derive(Debug)]
struct ProxyOutcome {
    session_finished: bool,
    client_sent_finish: bool,
    audio_seconds: i64,
    meter_source: &'static str,
    first_response_ms: Option<i64>,
    error_summary: Option<String>,
}

#[derive(Debug)]
struct AudioMeter {
    format: String,
    sample_rate: u32,
    decoded_audio_bytes: u64,
    first_audio_at: Option<Instant>,
    last_audio_at: Option<Instant>,
    server_audio_end_ms: u64,
    client_sent_finish: bool,
    session_finished: bool,
    first_response_ms: Option<i64>,
    upstream_error: Option<String>,
}

impl Default for AudioMeter {
    fn default() -> Self {
        Self {
            format: "pcm".to_string(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            decoded_audio_bytes: 0,
            first_audio_at: None,
            last_audio_at: None,
            server_audio_end_ms: 0,
            client_sent_finish: false,
            session_finished: false,
            first_response_ms: None,
            upstream_error: None,
        }
    }
}

impl AudioMeter {
    fn observe_client_text(&mut self, text: &str) -> AppResult<()> {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return Ok(());
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session.update") => {
                if let Some(format) = value
                    .pointer("/session/input_audio_format")
                    .and_then(Value::as_str)
                {
                    self.format = format.to_ascii_lowercase();
                }
                if let Some(sample_rate) = value
                    .pointer("/session/sample_rate")
                    .and_then(Value::as_u64)
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| matches!(value, 8_000 | 16_000))
                {
                    self.sample_rate = sample_rate;
                }
            }
            Some("input_audio_buffer.append") => {
                let Some(audio) = value.get("audio").and_then(Value::as_str) else {
                    return Ok(());
                };
                let decoded_len = base64_decoded_len(audio)?;
                if decoded_len > MAX_AUDIO_CHUNK_BYTES {
                    return Err(AppError::PayloadTooLarge(format!(
                        "input_audio_buffer.append audio exceeds {MAX_AUDIO_CHUNK_BYTES} bytes"
                    )));
                }
                self.decoded_audio_bytes =
                    self.decoded_audio_bytes.saturating_add(decoded_len as u64);
                let now = Instant::now();
                self.first_audio_at.get_or_insert(now);
                self.last_audio_at = Some(now);
            }
            Some("session.finish") => self.client_sent_finish = true,
            _ => {}
        }
        Ok(())
    }

    fn observe_server_text(&mut self, text: &str, started: Instant) {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return;
        };
        self.first_response_ms
            .get_or_insert_with(|| started.elapsed().as_millis().min(i64::MAX as u128) as i64);
        match value.get("type").and_then(Value::as_str) {
            Some("input_audio_buffer.speech_stopped") => {
                if let Some(end_ms) = value.get("audio_end_ms").and_then(Value::as_u64) {
                    self.server_audio_end_ms = self.server_audio_end_ms.max(end_ms);
                }
            }
            Some("session.finished") => self.session_finished = true,
            Some("error") | Some("conversation.item.input_audio_transcription.failed") => {
                self.upstream_error = Some(
                    value
                        .pointer("/error/message")
                        .and_then(Value::as_str)
                        .unwrap_or("Alibaba ASR realtime error")
                        .chars()
                        .take(500)
                        .collect(),
                );
            }
            _ => {}
        }
    }

    fn finish(self) -> ProxyOutcome {
        let (audio_seconds, meter_source) = if self.decoded_audio_bytes == 0 {
            (0, "none")
        } else if matches!(self.format.as_str(), "pcm" | "pcm16") {
            let bytes_per_second = u64::from(self.sample_rate) * PCM_BYTES_PER_SAMPLE;
            (
                self.decoded_audio_bytes.div_ceil(bytes_per_second) as i64,
                "pcm_bytes",
            )
        } else if self.server_audio_end_ms > 0 {
            (
                self.server_audio_end_ms.div_ceil(1_000) as i64,
                "server_vad",
            )
        } else {
            let elapsed = self
                .first_audio_at
                .zip(self.last_audio_at)
                .map(|(first, last)| last.saturating_duration_since(first))
                .unwrap_or_default();
            (elapsed.as_secs().max(1) as i64, "stream_elapsed")
        };
        ProxyOutcome {
            session_finished: self.session_finished,
            client_sent_finish: self.client_sent_finish,
            audio_seconds,
            meter_source,
            first_response_ms: self.first_response_ms,
            error_summary: self.upstream_error,
        }
    }
}

pub(crate) async fn openai_realtime(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Query(query): Query<RealtimeQuery>,
    ws: WebSocketUpgrade,
) -> AppResult<Response> {
    let requested_model = query.model.trim();
    if requested_model.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    let resolved = crate::project::models::resolve_project_model(
        &state.db.pool,
        auth.project_id,
        requested_model,
    )
    .await?;
    let (protocol, upstream) = select_upstream_excluding(
        &state,
        REALTIME_PATH,
        &resolved.target_model,
        resolved.target_channel_id,
        None,
        &[],
    )
    .await?;
    let upstream_url = qwen_realtime_url(&upstream, &resolved.target_model)?;
    let price = state
        .billing
        .price_for(
            &state.db.pool,
            upstream.channel_id,
            &resolved.target_model,
            &auth.user_group,
        )
        .await?;
    let unit_price = realtime_audio_unit_price(&price)?;
    let model_credit_account = auth.model_credit_account(&resolved.external_model).cloned();
    let hold = reserve_billable_credit(
        &state,
        &auth,
        model_credit_account.as_ref(),
        INITIAL_AUDIO_RESERVATION_SECONDS.saturating_mul(unit_price),
    )
    .await?;
    let permit = match state.user_request_limiter.try_acquire(auth.user_id).await {
        Ok(permit) => permit,
        Err(err) => {
            release_empty_hold(&state, hold, "realtime ASR concurrency rejection").await;
            return Err(err);
        }
    };
    let upstream_socket = match connect_upstream(&upstream, &upstream_url).await {
        Ok(socket) => socket,
        Err(err) => {
            release_empty_hold(&state, hold, "realtime ASR connection failure").await;
            return Err(err);
        }
    };
    let ctx = RealtimeContext {
        state,
        auth,
        upstream,
        protocol,
        external_model: resolved.external_model,
        upstream_model: resolved.target_model,
        routing: resolved.routing,
        price,
        hold,
        relay_trace_id: Uuid::new_v4(),
        started: Instant::now(),
    };

    Ok(ws
        .max_message_size(MAX_CLIENT_MESSAGE_BYTES)
        .on_upgrade(move |client| async move {
            let _permit = permit;
            let outcome = proxy_session(client, upstream_socket, ctx.started).await;
            finish_session(ctx, outcome).await;
        }))
}

fn realtime_audio_unit_price(price: &Price) -> AppResult<i64> {
    if price.billing_meter != BillingMeter::Audio {
        return Err(AppError::BadRequestWithCode {
            code: "audio_price_required",
            message: "Alibaba realtime ASR requires audio per-second pricing",
        });
    }
    price
        .unit_price_micros
        .filter(|value| *value > 0)
        .ok_or(AppError::BadRequestWithCode {
            code: "audio_price_required",
            message: "Alibaba realtime ASR requires a positive per-second price",
        })
}

fn qwen_realtime_url(upstream: &SelectedUpstream, model: &str) -> AppResult<Url> {
    if !upstream.provider.eq_ignore_ascii_case("qwen") {
        return Err(AppError::BadRequest(
            "Alibaba realtime ASR requires a qwen channel".to_string(),
        ));
    }
    if upstream.channel_key_id.is_none() || upstream.credential_id.is_some() {
        return Err(AppError::BadRequest(
            "Alibaba realtime ASR requires a key-backed qwen channel".to_string(),
        ));
    }
    let mut url = Url::parse(&upstream.base_url)
        .map_err(|_| AppError::BadRequest("invalid Alibaba ASR endpoint URL".to_string()))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let trusted = matches!(
        host.as_str(),
        "dashscope.aliyuncs.com" | "dashscope-intl.aliyuncs.com"
    ) || host.ends_with(".cn-beijing.maas.aliyuncs.com")
        || host.ends_with(".ap-southeast-1.maas.aliyuncs.com");
    if url.scheme() != "https" || !trusted {
        return Err(AppError::BadRequest(
            "Alibaba ASR endpoint must use a trusted DashScope host".to_string(),
        ));
    }
    url.set_scheme("wss")
        .map_err(|_| AppError::BadRequest("invalid Alibaba ASR endpoint URL".to_string()))?;
    url.set_path("/api-ws/v1/realtime");
    url.set_query(None);
    url.query_pairs_mut().append_pair("model", model);
    Ok(url)
}

async fn connect_upstream(upstream: &SelectedUpstream, url: &Url) -> AppResult<UpstreamSocket> {
    let mut request = url.as_str().into_client_request().map_err(|err| {
        AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Request,
            upstream.provider.clone(),
            format!("invalid realtime WebSocket request: {err}"),
        ))
    })?;
    let authorization = HeaderValue::from_str(&format!("Bearer {}", upstream.secret))
        .map_err(|_| AppError::BadRequest("invalid Alibaba ASR channel key".to_string()))?;
    request.headers_mut().insert(AUTHORIZATION, authorization);
    let result =
        tokio::time::timeout(std::time::Duration::from_secs(30), connect_async(request)).await;
    match result {
        Ok(Ok((socket, _))) => Ok(socket),
        Ok(Err(err)) => Err(AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Connect,
            upstream.provider.clone(),
            format!("realtime WebSocket handshake failed: {err}"),
        ))),
        Err(_) => Err(AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Timeout,
            upstream.provider.clone(),
            "realtime WebSocket handshake timed out",
        ))),
    }
}

async fn proxy_session(
    client: WebSocket,
    upstream: UpstreamSocket,
    started: Instant,
) -> ProxyOutcome {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let mut meter = AudioMeter::default();
    let mut transport_error = None;

    loop {
        tokio::select! {
            client_message = client_rx.next() => {
                match client_message {
                    Some(Ok(message)) => {
                        if let ClientMessage::Text(text) = &message {
                            if let Err(err) = meter.observe_client_text(text.as_str()) {
                                let error = client_error_event(&err.to_string());
                                let _ = client_tx.send(ClientMessage::Text(error.into())).await;
                                transport_error = Some(err.to_string());
                                break;
                            }
                        }
                        let closing = matches!(message, ClientMessage::Close(_));
                        if let Err(err) = upstream_tx.send(to_upstream_message(message)).await {
                            transport_error = Some(format!("upstream WebSocket send failed: {err}"));
                            break;
                        }
                        if closing {
                            break;
                        }
                    }
                    Some(Err(err)) => {
                        transport_error = Some(format!("client WebSocket receive failed: {err}"));
                        break;
                    }
                    None => break,
                }
            }
            upstream_message = upstream_rx.next() => {
                match upstream_message {
                    Some(Ok(message)) => {
                        if let UpstreamMessage::Text(text) = &message {
                            meter.observe_server_text(text.as_str(), started);
                        }
                        let session_finished = meter.session_finished;
                        let closing = matches!(message, UpstreamMessage::Close(_));
                        if let Some(message) = to_client_message(message) {
                            if let Err(err) = client_tx.send(message).await {
                                transport_error = Some(format!("client WebSocket send failed: {err}"));
                                break;
                            }
                        }
                        if closing || session_finished {
                            break;
                        }
                    }
                    Some(Err(err)) => {
                        transport_error = Some(format!("upstream WebSocket receive failed: {err}"));
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    let _ = upstream_tx.send(UpstreamMessage::Close(None)).await;
    let _ = client_tx.send(ClientMessage::Close(None)).await;
    let mut outcome = meter.finish();
    if outcome.error_summary.is_none() {
        outcome.error_summary = transport_error;
    }
    outcome
}

fn to_upstream_message(message: ClientMessage) -> UpstreamMessage {
    match message {
        ClientMessage::Text(text) => UpstreamMessage::Text(text.to_string().into()),
        ClientMessage::Binary(bytes) => UpstreamMessage::Binary(bytes),
        ClientMessage::Ping(bytes) => UpstreamMessage::Ping(bytes),
        ClientMessage::Pong(bytes) => UpstreamMessage::Pong(bytes),
        ClientMessage::Close(frame) => UpstreamMessage::Close(frame.map(|frame| {
            tokio_tungstenite::tungstenite::protocol::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }
        })),
    }
}

fn to_client_message(message: UpstreamMessage) -> Option<ClientMessage> {
    match message {
        UpstreamMessage::Text(text) => Some(ClientMessage::Text(text.to_string().into())),
        UpstreamMessage::Binary(bytes) => Some(ClientMessage::Binary(bytes)),
        UpstreamMessage::Ping(bytes) => Some(ClientMessage::Ping(bytes)),
        UpstreamMessage::Pong(bytes) => Some(ClientMessage::Pong(bytes)),
        UpstreamMessage::Close(frame) => Some(ClientMessage::Close(frame.map(|frame| {
            axum::extract::ws::CloseFrame {
                code: frame.code.into(),
                reason: frame.reason.to_string().into(),
            }
        }))),
        UpstreamMessage::Frame(_) => None,
    }
}

async fn finish_session(ctx: RealtimeContext, outcome: ProxyOutcome) {
    let success = outcome.session_finished && outcome.error_summary.is_none();
    let latency_ms = ctx.started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let model_credit_account = ctx.auth.model_credit_account(&ctx.external_model).cloned();

    let billing = if success && outcome.audio_seconds > 0 {
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
                        user_key_model_credit_account: model_credit_account.as_ref(),
                        user_key_credit_account: &ctx.auth.user_key_credit_account,
                        project_credit_account: &ctx.auth.project_credit_account,
                    },
                    hold: ctx.hold.clone(),
                    usage: Some(BillableUsage::audio_seconds(outcome.audio_seconds)),
                    price: &ctx.price,
                },
            )
            .await
        {
            Ok(charge) => Some(charge),
            Err(err) => {
                tracing::error!(
                    relay_trace_id = %ctx.relay_trace_id,
                    "failed to settle realtime ASR billing: {err}"
                );
                release_empty_hold(
                    &ctx.state,
                    ctx.hold.clone(),
                    "realtime ASR settlement failure",
                )
                .await;
                None
            }
        }
    } else {
        release_empty_hold(
            &ctx.state,
            ctx.hold.clone(),
            "incomplete realtime ASR session",
        )
        .await;
        None
    };

    if outcome.meter_source == "stream_elapsed" {
        tracing::warn!(
            relay_trace_id = %ctx.relay_trace_id,
            model = %ctx.upstream_model,
            audio_seconds = outcome.audio_seconds,
            "Opus realtime ASR duration used stream timing fallback"
        );
    }
    if !outcome.client_sent_finish {
        tracing::warn!(
            relay_trace_id = %ctx.relay_trace_id,
            model = %ctx.upstream_model,
            "realtime ASR client disconnected without session.finish"
        );
    }

    tracing::info!(
        relay_trace_id = %ctx.relay_trace_id,
        protocol = ctx.protocol.as_str(),
        channel_id = ctx.upstream.channel_id,
        model = %ctx.upstream_model,
        success,
        audio_seconds = outcome.audio_seconds,
        meter_source = outcome.meter_source,
        latency_ms,
        "realtime ASR session finished"
    );

    let usage = UsageInsert {
        user_id: ctx.auth.user_id,
        project_id: ctx.auth.project_id,
        user_key_id: ctx.auth.user_key_id,
        channel_id: ctx.upstream.channel_id,
        channel_key_id: ctx.upstream.channel_key_id,
        credential_id: ctx.upstream.credential_id,
        relay_trace_id: Some(ctx.relay_trace_id),
        relay_attempt: 1,
        relay_final: true,
        model: Some(ctx.external_model),
        upstream_model: Some(ctx.upstream_model),
        routing_phase: "relay".to_string(),
        routing: ctx.routing,
        status_code: Some(if success { 200 } else { 502 }),
        streamed: true,
        latency_ms,
        first_response_ms: outcome.first_response_ms,
        output_tokens_per_second: None,
        error_summary: outcome.error_summary.or_else(|| {
            (!outcome.session_finished).then(|| "realtime ASR session did not finish".to_string())
        }),
        token_usage: None,
        billing_meter: BillingMeter::Audio,
        billable_units: if success { outcome.audio_seconds } else { 0 },
        billing,
    };
    if usage.billing.is_some() {
        ctx.state.billing_outbox.enqueue_or_retry(usage);
    } else if let Err(err) = ctx.state.usage.enqueue(usage, None).await {
        tracing::warn!("failed to enqueue realtime ASR usage: {err}");
    }
}

fn base64_decoded_len(value: &str) -> AppResult<usize> {
    STANDARD
        .decode(value)
        .map(|bytes| bytes.len())
        .map_err(|_| AppError::BadRequest("audio must be valid base64".to_string()))
}

fn client_error_event(message: &str) -> String {
    json!({
        "type": "error",
        "event_id": format!("event_{}", Uuid::new_v4()),
        "error": {
            "type": "invalid_request_error",
            "code": "invalid_audio",
            "message": message,
            "param": "audio"
        }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream(base_url: &str) -> SelectedUpstream {
        SelectedUpstream {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
            provider: "qwen".to_string(),
            channel_name: "Qwen".to_string(),
            base_url: base_url.to_string(),
            adapter_hint: None,
            responses_chat_fallback: false,
            secret: "sk-test".to_string(),
            account_id: None,
            affinity: None,
        }
    }

    #[test]
    fn builds_realtime_url_from_compatible_base_url() {
        let url = qwen_realtime_url(
            &upstream("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            "qwen3-asr-flash-realtime",
        )
        .unwrap();
        assert_eq!(
            url.as_str(),
            "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime"
        );
    }

    #[test]
    fn accepts_workspace_and_singapore_hosts() {
        for base_url in [
            "https://ws-123.cn-beijing.maas.aliyuncs.com/compatible-mode/v1",
            "https://ws-123.ap-southeast-1.maas.aliyuncs.com/compatible-mode/v1",
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        ] {
            assert!(qwen_realtime_url(&upstream(base_url), "model").is_ok());
        }
    }

    #[test]
    fn rejects_untrusted_hosts() {
        for base_url in [
            "http://dashscope.aliyuncs.com/compatible-mode/v1",
            "https://dashscope.aliyuncs.com.example.com/compatible-mode/v1",
        ] {
            assert!(qwen_realtime_url(&upstream(base_url), "model").is_err());
        }
    }

    #[test]
    fn meters_pcm_audio_from_decoded_bytes() {
        let mut meter = AudioMeter::default();
        let audio = STANDARD.encode(vec![0_u8; 32_001]);
        meter
            .observe_client_text(
                &json!({
                    "type": "input_audio_buffer.append",
                    "event_id": "event-1",
                    "audio": audio,
                })
                .to_string(),
            )
            .unwrap();
        let outcome = meter.finish();
        assert_eq!(outcome.audio_seconds, 2);
        assert_eq!(outcome.meter_source, "pcm_bytes");
    }

    #[test]
    fn applies_session_audio_settings() {
        let mut meter = AudioMeter::default();
        meter
            .observe_client_text(
                &json!({
                    "type": "session.update",
                    "session": { "input_audio_format": "opus", "sample_rate": 8000 }
                })
                .to_string(),
            )
            .unwrap();
        assert_eq!(meter.format, "opus");
        assert_eq!(meter.sample_rate, 8_000);
    }

    #[test]
    fn observes_server_terminal_and_error_events() {
        let started = Instant::now();
        let mut meter = AudioMeter::default();
        meter.observe_server_text(
            r#"{"type":"input_audio_buffer.speech_stopped","audio_end_ms":2101}"#,
            started,
        );
        meter.observe_server_text(r#"{"type":"session.finished"}"#, started);
        meter.observe_server_text(
            r#"{"type":"error","error":{"message":"bad audio"}}"#,
            started,
        );
        assert_eq!(meter.server_audio_end_ms, 2101);
        assert!(meter.session_finished);
        assert_eq!(meter.upstream_error.as_deref(), Some("bad audio"));
    }
}
