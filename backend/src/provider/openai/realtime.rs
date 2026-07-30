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
const SESSION_FINISH_TIMEOUT_SECS: u64 = 5;

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
    // OpenAI realtime clients never send session.finish; the gateway requests
    // it on their behalf when the client disconnects after sending audio.
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
    ensure_realtime_audio_transcription_capability(
        &state.db.pool,
        &upstream.provider,
        &resolved.target_model,
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
            let upstream_model = ctx.upstream_model.clone();
            let outcome = proxy_session(client, upstream_socket, ctx.started, upstream_model).await;
            finish_session(ctx, outcome).await;
        }))
}

async fn ensure_realtime_audio_transcription_capability(
    pool: &sqlx::PgPool,
    provider: &str,
    model: &str,
) -> AppResult<()> {
    let capabilities = sqlx::query_scalar::<_, Value>(
        "SELECT capabilities
         FROM provider_model
         WHERE lower(provider) = lower($1)
           AND lower(model) = lower($2)
         LIMIT 1",
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(pool)
    .await?;
    if capabilities.as_ref().and_then(|capabilities| {
        crate::admin::provider::catalog_audio_transcription_adapter(provider, capabilities)
    }) == Some(crate::admin::provider::AudioTranscriptionAdapter::QwenRealtime)
    {
        return Ok(());
    }
    Err(AppError::BadRequestWithCode {
        code: "unsupported_realtime_audio_model",
        message: "the selected model is not configured for realtime audio transcription",
    })
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
    url.query_pairs_mut()
        .append_pair("model", model)
        .append_pair("heartbeat", "true");
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
    upstream_model: String,
) -> ProxyOutcome {
    let (mut client_tx, mut client_rx) = client.split();
    let (mut upstream_tx, mut upstream_rx) = upstream.split();
    let mut meter = AudioMeter::default();
    let mut resampler = PcmResampler::default();
    let mut translator = UpstreamTranslator::new(upstream_model);
    let mut client_sent_audio = false;
    let mut transport_error = None;

    'session: loop {
        tokio::select! {
            client_message = client_rx.next() => {
                match client_message {
                    Some(Ok(ClientMessage::Text(text))) => {
                        let translated = translate_client_event(text.as_str(), &mut resampler);
                        client_sent_audio |= translated.audio_appended;
                        if let Some(to_client) = translated.to_client {
                            if client_tx.send(ClientMessage::Text(to_client.into())).await.is_err() {
                                transport_error = Some("client WebSocket send failed".to_string());
                                break 'session;
                            }
                        }
                        if let Some(to_upstream) = translated.to_upstream {
                            // The meter observes the translated 16 kHz stream bound for
                            // upstream: it is the same audio the client sent at 24 kHz,
                            // so the pcm_bytes -> seconds conversion stays exact.
                            if let Err(err) = meter.observe_client_text(&to_upstream) {
                                let error = client_error_event(&err.to_string());
                                let _ = client_tx.send(ClientMessage::Text(error.into())).await;
                                transport_error = Some(err.to_string());
                                break 'session;
                            }
                            if let Err(err) = upstream_tx.send(UpstreamMessage::Text(to_upstream.into())).await {
                                transport_error = Some(format!("upstream WebSocket send failed: {err}"));
                                break 'session;
                            }
                        }
                    }
                    // Do not forward client closes yet: the session.finish
                    // handshake below must run on the upstream socket first.
                    // tungstenite already queued its close reply when it read
                    // the client's close frame; flush delivers it. An explicit
                    // send(Close) here would fail with SendAfterClosing without
                    // flushing that queued reply.
                    Some(Ok(ClientMessage::Close(_))) => {
                        let _ = client_tx.flush().await;
                        break 'session;
                    }
                    Some(Ok(message)) => {
                        if let Err(err) = upstream_tx.send(to_upstream_message(message)).await {
                            transport_error = Some(format!("upstream WebSocket send failed: {err}"));
                            break 'session;
                        }
                    }
                    Some(Err(err)) => {
                        transport_error = Some(format!("client WebSocket receive failed: {err}"));
                        break 'session;
                    }
                    None => break 'session,
                }
            }
            upstream_message = upstream_rx.next() => {
                match upstream_message {
                    Some(Ok(UpstreamMessage::Text(text))) => {
                        meter.observe_server_text(text.as_str(), started);
                        let translated = translator.translate(text.as_str());
                        if let Some(to_client) = translated.to_client {
                            if client_tx.send(ClientMessage::Text(to_client.into())).await.is_err() {
                                transport_error = Some("client WebSocket send failed".to_string());
                                break 'session;
                            }
                        }
                        if let Some(pending) = translated.pending {
                            if client_tx.send(ClientMessage::Text(pending.into())).await.is_err() {
                                transport_error = Some("client WebSocket send failed".to_string());
                                break 'session;
                            }
                        }
                        if translated.session_finished {
                            break 'session;
                        }
                    }
                    Some(Ok(message)) => {
                        let closing = matches!(message, UpstreamMessage::Close(_));
                        if let Some(message) = to_client_message(message) {
                            if let Err(err) = client_tx.send(message).await {
                                transport_error = Some(format!("client WebSocket send failed: {err}"));
                                break 'session;
                            }
                        }
                        if closing {
                            break 'session;
                        }
                    }
                    Some(Err(err)) => {
                        transport_error = Some(format!("upstream WebSocket receive failed: {err}"));
                        break 'session;
                    }
                    None => break 'session,
                }
            }
        }
    }

    // OpenAI clients close the socket without any terminal event, but Alibaba
    // drops the last utterance unless session.finish is requested first. When
    // audio was sent and the session did not finish, request the final
    // transcript and keep translating upstream events (bounded by a timeout)
    // until session.finished arrives.
    if client_sent_audio && !meter.session_finished {
        let finish = json!({
            "type": "session.finish",
            "event_id": format!("event_{}", Uuid::new_v4()),
        })
        .to_string();
        if upstream_tx
            .send(UpstreamMessage::Text(finish.into()))
            .await
            .is_ok()
        {
            meter.client_sent_finish = true;
            let timeout =
                tokio::time::sleep(std::time::Duration::from_secs(SESSION_FINISH_TIMEOUT_SECS));
            tokio::pin!(timeout);
            loop {
                tokio::select! {
                    _ = &mut timeout => break,
                    upstream_message = upstream_rx.next() => {
                        match upstream_message {
                            Some(Ok(UpstreamMessage::Text(text))) => {
                                meter.observe_server_text(text.as_str(), started);
                                let translated = translator.translate(text.as_str());
                                if let Some(to_client) = translated.to_client {
                                    let _ = client_tx.send(ClientMessage::Text(to_client.into())).await;
                                }
                                if let Some(pending) = translated.pending {
                                    let _ = client_tx.send(ClientMessage::Text(pending.into())).await;
                                }
                                if translated.session_finished {
                                    break;
                                }
                            }
                            Some(Ok(_)) => {}
                            Some(Err(_)) | None => break,
                        }
                    }
                }
            }
        }
    }

    let _ = upstream_tx.send(UpstreamMessage::Close(None)).await;
    // send(Close) no-ops with an error when the client already closed; the
    // flush then delivers tungstenite's queued close reply either way.
    let _ = client_tx.send(ClientMessage::Close(None)).await;
    let _ = client_tx.flush().await;
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

// ---------------------------------------------------------------------------
// OpenAI realtime transcription (classic beta dialect) <-> Alibaba Qwen ASR
// realtime translation layer.
// ---------------------------------------------------------------------------

/// Streaming 24 kHz -> 16 kHz PCM16 mono resampler using 3:2 linear
/// interpolation. Output sample k sits at input position 3k/2, tracked in
/// half-sample units so the phase stays exact across arbitrarily split
/// `input_audio_buffer.append` payloads (including odd byte boundaries).
#[derive(Debug, Default)]
struct PcmResampler {
    pending_byte: Option<u8>,
    samples: std::collections::VecDeque<i16>,
    next_output_half: u64,
    consumed: u64,
}

impl PcmResampler {
    fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut rest = bytes;
        if let Some(low) = self.pending_byte.take() {
            match rest.split_first() {
                Some((high, tail)) => {
                    self.samples.push_back(i16::from_le_bytes([low, *high]));
                    rest = tail;
                }
                None => {
                    self.pending_byte = Some(low);
                    return Vec::new();
                }
            }
        }
        let even_len = rest.len() - rest.len() % 2;
        if even_len < rest.len() {
            self.pending_byte = Some(rest[even_len]);
        }
        for pair in rest[..even_len].chunks_exact(2) {
            self.samples
                .push_back(i16::from_le_bytes([pair[0], pair[1]]));
        }

        let mut output = Vec::new();
        loop {
            let index = self.next_output_half / 2;
            let half = self.next_output_half % 2;
            let available = self.consumed + self.samples.len() as u64;
            if index + half >= available {
                break;
            }
            let first = i32::from(self.samples[(index - self.consumed) as usize]);
            let sample = if half == 0 {
                first
            } else {
                let second = i32::from(self.samples[(index + 1 - self.consumed) as usize]);
                (first + second) / 2
            };
            output.extend_from_slice(&(sample as i16).to_le_bytes());
            self.next_output_half += 3;

            let keep_from = (self.next_output_half / 2).saturating_sub(self.consumed) as usize;
            if keep_from > 1_024 {
                self.samples.drain(..keep_from);
                self.consumed += keep_from as u64;
            }
        }
        output
    }
}

#[derive(Debug, Default)]
struct ClientTranslate {
    to_upstream: Option<String>,
    to_client: Option<String>,
    audio_appended: bool,
}

fn translate_client_event(text: &str, resampler: &mut PcmResampler) -> ClientTranslate {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return ClientTranslate {
            to_client: Some(openai_error_event(
                "invalid_request_error",
                "invalid_json",
                "client events must be valid JSON",
                None,
                None,
            )),
            ..ClientTranslate::default()
        };
    };
    let event_id = value
        .get("event_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    match value.get("type").and_then(Value::as_str) {
        Some("transcription_session.update") | Some("session.update") => {
            translate_session_update(&value, event_id)
        }
        Some("input_audio_buffer.append") => translate_audio_append(&value, event_id, resampler),
        Some("input_audio_buffer.commit") => ClientTranslate {
            to_upstream: Some(text.to_string()),
            ..ClientTranslate::default()
        },
        Some("input_audio_buffer.clear") => ClientTranslate {
            to_client: Some(
                json!({
                    "type": "input_audio_buffer.cleared",
                    "event_id": format!("event_{}", Uuid::new_v4()),
                })
                .to_string(),
            ),
            ..ClientTranslate::default()
        },
        Some(other) => ClientTranslate {
            to_client: Some(openai_error_event(
                "invalid_request_error",
                "invalid_event_type",
                &format!("unknown client event type: {other}"),
                Some("type"),
                event_id.as_deref(),
            )),
            ..ClientTranslate::default()
        },
        None => ClientTranslate {
            to_client: Some(openai_error_event(
                "invalid_request_error",
                "invalid_event_type",
                "client events must include a type",
                Some("type"),
                event_id.as_deref(),
            )),
            ..ClientTranslate::default()
        },
    }
}

fn translate_session_update(value: &Value, event_id: Option<String>) -> ClientTranslate {
    let reject = |message: String, param: &str| ClientTranslate {
        to_client: Some(openai_error_event(
            "invalid_request_error",
            "invalid_value",
            &message,
            Some(param),
            event_id.as_deref(),
        )),
        ..ClientTranslate::default()
    };
    let empty = json!({});
    let session = value.get("session").unwrap_or(&empty);
    match session
        .get("input_audio_format")
        .and_then(Value::as_str)
        .unwrap_or("pcm16")
    {
        "pcm16" => {}
        other => {
            return reject(
                format!("unsupported input_audio_format '{other}'; only 'pcm16' is supported"),
                "session.input_audio_format",
            );
        }
    }
    let mut turn_detection = Value::Null;
    if let Some(client_turn_detection) = session.get("turn_detection") {
        if !client_turn_detection.is_null() {
            match client_turn_detection
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("server_vad")
            {
                "server_vad" => {}
                other => {
                    return reject(
                        format!(
                            "unsupported turn_detection type '{other}'; only 'server_vad' is supported"
                        ),
                        "session.turn_detection.type",
                    );
                }
            }
            let mut qwen_turn_detection = json!({ "type": "server_vad" });
            if let Some(threshold) = client_turn_detection
                .get("threshold")
                .and_then(Value::as_f64)
            {
                // OpenAI thresholds span 0..1 while Qwen accepts -1..1.
                qwen_turn_detection["threshold"] = json!(threshold.clamp(-1.0, 1.0));
            }
            if let Some(silence_duration_ms) = client_turn_detection
                .get("silence_duration_ms")
                .and_then(Value::as_u64)
            {
                qwen_turn_detection["silence_duration_ms"] = json!(silence_duration_ms);
            }
            turn_detection = qwen_turn_detection;
        }
    }
    let mut qwen_session = json!({
        "modalities": ["text"],
        "input_audio_format": "pcm",
        "sample_rate": DEFAULT_SAMPLE_RATE,
    });
    if session.get("turn_detection").is_some() {
        qwen_session["turn_detection"] = turn_detection;
    }
    if let Some(language) = session
        .pointer("/input_audio_transcription/language")
        .and_then(Value::as_str)
    {
        qwen_session["input_audio_transcription"] = json!({ "language": language });
    }
    let mut upstream = json!({ "type": "session.update", "session": qwen_session });
    if let Some(event_id) = event_id {
        upstream["event_id"] = json!(event_id);
    }
    ClientTranslate {
        to_upstream: Some(upstream.to_string()),
        ..ClientTranslate::default()
    }
}

fn translate_audio_append(
    value: &Value,
    event_id: Option<String>,
    resampler: &mut PcmResampler,
) -> ClientTranslate {
    let invalid = |message: String| ClientTranslate {
        to_client: Some(openai_error_event(
            "invalid_request_error",
            "invalid_audio",
            &message,
            Some("audio"),
            event_id.as_deref(),
        )),
        audio_appended: true,
        ..ClientTranslate::default()
    };
    let Some(audio) = value.get("audio").and_then(Value::as_str) else {
        return invalid("input_audio_buffer.append requires base64 audio".to_string());
    };
    let decoded = match STANDARD.decode(audio) {
        Ok(decoded) => decoded,
        Err(_) => return invalid("audio must be valid base64".to_string()),
    };
    if decoded.len() > MAX_AUDIO_CHUNK_BYTES {
        return invalid(format!(
            "input_audio_buffer.append audio exceeds {MAX_AUDIO_CHUNK_BYTES} bytes"
        ));
    }
    let resampled = resampler.push(&decoded);
    let mut translate = ClientTranslate {
        audio_appended: true,
        ..ClientTranslate::default()
    };
    if resampled.is_empty() {
        // Too little audio to emit a 16 kHz sample yet; the bytes stay in the
        // resampler and flush with the next append.
        return translate;
    }
    let mut upstream = json!({
        "type": "input_audio_buffer.append",
        "audio": STANDARD.encode(resampled),
    });
    if let Some(event_id) = event_id {
        upstream["event_id"] = json!(event_id);
    }
    translate.to_upstream = Some(upstream.to_string());
    translate
}

fn openai_error_event(
    error_type: &str,
    code: &str,
    message: &str,
    param: Option<&str>,
    client_event_id: Option<&str>,
) -> String {
    json!({
        "type": "error",
        "event_id": format!("event_{}", Uuid::new_v4()),
        "error": {
            "type": error_type,
            "code": code,
            "message": message,
            "param": param,
            "event_id": client_event_id,
        }
    })
    .to_string()
}

fn openai_session_view(
    model: &str,
    language: Option<&str>,
    turn_detection: Option<(Option<f64>, Option<u64>)>,
    manual: bool,
) -> Value {
    let mut transcription = json!({ "model": model, "prompt": "" });
    if let Some(language) = language {
        transcription["language"] = json!(language);
    }
    let turn_detection = if manual {
        Value::Null
    } else {
        let (threshold, silence_duration_ms) = turn_detection.unwrap_or((None, None));
        json!({
            "type": "server_vad",
            "threshold": threshold.unwrap_or(0.5),
            "prefix_padding_ms": 300,
            "silence_duration_ms": silence_duration_ms.unwrap_or(500),
        })
    };
    json!({
        "modalities": ["text"],
        "input_audio_format": "pcm16",
        "input_audio_transcription": transcription,
        "turn_detection": turn_detection,
    })
}

#[derive(Debug)]
struct UpstreamTranslator {
    upstream_model: String,
    // Last full transcript (text + stash) emitted per item, used to diff
    // Qwen's full-snapshot `text` events into OpenAI `delta` events.
    last_full: std::collections::HashMap<String, String>,
}

#[derive(Debug, Default)]
struct UpstreamTranslate {
    to_client: Option<String>,
    /// Second client frame that must be sent right after `to_client`
    /// (used to emit a catch-up delta before `completed`).
    pending: Option<String>,
    session_finished: bool,
}

impl UpstreamTranslator {
    fn new(upstream_model: String) -> Self {
        Self {
            upstream_model,
            last_full: std::collections::HashMap::new(),
        }
    }

    fn translate(&mut self, text: &str) -> UpstreamTranslate {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            return UpstreamTranslate {
                to_client: Some(text.to_string()),
                ..UpstreamTranslate::default()
            };
        };
        let forward = || UpstreamTranslate {
            to_client: Some(text.to_string()),
            ..UpstreamTranslate::default()
        };
        let copy_event_id = |event: &mut Value| {
            if let Some(event_id) = value.get("event_id") {
                event["event_id"] = event_id.clone();
            }
        };
        match value.get("type").and_then(Value::as_str) {
            Some("session.created") => {
                // The first event an OpenAI client sees is the effective
                // session; Qwen's session.created itself is not forwarded.
                let session = openai_session_view(&self.upstream_model, None, None, false);
                UpstreamTranslate {
                    to_client: Some(
                        json!({
                            "type": "transcription_session.updated",
                            "event_id": format!("event_{}", Uuid::new_v4()),
                            "session": session,
                        })
                        .to_string(),
                    ),
                    ..UpstreamTranslate::default()
                }
            }
            Some("session.updated") => {
                let empty = json!({});
                let session = value.get("session").unwrap_or(&empty);
                let threshold = session
                    .pointer("/turn_detection/threshold")
                    .and_then(Value::as_f64);
                let silence_duration_ms = session
                    .pointer("/turn_detection/silence_duration_ms")
                    .and_then(Value::as_u64);
                let language = session
                    .pointer("/input_audio_transcription/language")
                    .and_then(Value::as_str);
                let manual = matches!(session.get("turn_detection"), Some(td) if td.is_null());
                let session = openai_session_view(
                    &self.upstream_model,
                    language,
                    Some((threshold, silence_duration_ms)),
                    manual,
                );
                let mut event = json!({
                    "type": "transcription_session.updated",
                    "event_id": format!("event_{}", Uuid::new_v4()),
                    "session": session,
                });
                copy_event_id(&mut event);
                UpstreamTranslate {
                    to_client: Some(event.to_string()),
                    ..UpstreamTranslate::default()
                }
            }
            Some("input_audio_buffer.speech_started")
            | Some("input_audio_buffer.speech_stopped")
            | Some("input_audio_buffer.committed") => forward(),
            Some("conversation.item.created") => {
                let item_id = value
                    .pointer("/item/id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let mut event = json!({
                    "type": "conversation.item.created",
                    "item": {
                        "id": item_id,
                        "object": "realtime.item",
                        "type": "message",
                        "status": "completed",
                        "role": "user",
                        "content": [{ "type": "input_audio", "transcript": null }],
                    }
                });
                copy_event_id(&mut event);
                UpstreamTranslate {
                    to_client: Some(event.to_string()),
                    ..UpstreamTranslate::default()
                }
            }
            Some("conversation.item.input_audio_transcription.text") => {
                let item_id = value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                // OpenAI deltas are append-only, while Qwen's `stash` is a
                // revisable draft suffix. Emit only growth of the confirmed
                // `text` prefix; the stash becomes visible once Qwen promotes
                // it into `text`.
                let confirmed = value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let last = self.last_full.get(&item_id).cloned().unwrap_or_default();
                let delta = match confirmed.strip_prefix(&last) {
                    Some(suffix) => suffix.to_string(),
                    None => {
                        // Confirmed text itself was revised (rare): rebase
                        // silently. The completed event stays authoritative.
                        if !confirmed.is_empty() {
                            self.last_full.insert(item_id.clone(), confirmed);
                        }
                        return UpstreamTranslate::default();
                    }
                };
                self.last_full.insert(item_id.clone(), confirmed);
                if delta.is_empty() {
                    return UpstreamTranslate::default();
                }
                let mut event = json!({
                    "type": "conversation.item.input_audio_transcription.delta",
                    "item_id": item_id,
                    "content_index": 0,
                    "delta": delta,
                });
                copy_event_id(&mut event);
                UpstreamTranslate {
                    to_client: Some(event.to_string()),
                    ..UpstreamTranslate::default()
                }
            }
            Some("conversation.item.input_audio_transcription.completed") => {
                let item_id = value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let transcript = value
                    .get("transcript")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let last = self.last_full.get(&item_id).cloned().unwrap_or_default();
                // Emit any confirmed-but-unsent suffix (e.g. final punctuation)
                // as a catch-up delta, so the concatenation of all deltas
                // equals this transcript.
                let catch_up = transcript
                    .strip_prefix(&last)
                    .filter(|suffix| !suffix.is_empty())
                    .map(|suffix| {
                        let mut event = json!({
                            "type": "conversation.item.input_audio_transcription.delta",
                            "item_id": item_id,
                            "content_index": 0,
                            "delta": suffix,
                        });
                        copy_event_id(&mut event);
                        event.to_string()
                    });
                self.last_full
                    .insert(item_id.clone(), transcript.to_string());
                let mut event = json!({
                    "type": "conversation.item.input_audio_transcription.completed",
                    "item_id": item_id,
                    "content_index": 0,
                    "transcript": transcript,
                });
                copy_event_id(&mut event);
                match catch_up {
                    Some(delta_frame) => UpstreamTranslate {
                        to_client: Some(delta_frame),
                        pending: Some(event.to_string()),
                        ..UpstreamTranslate::default()
                    },
                    None => UpstreamTranslate {
                        to_client: Some(event.to_string()),
                        ..UpstreamTranslate::default()
                    },
                }
            }
            Some("conversation.item.input_audio_transcription.failed") => {
                let item_id = value
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let error = value.get("error").cloned().unwrap_or_else(|| json!({}));
                let mut event = json!({
                    "type": "conversation.item.input_audio_transcription.failed",
                    "item_id": item_id,
                    "content_index": 0,
                    "error": {
                        "type": "server_error",
                        "code": error.get("code").cloned().unwrap_or(Value::Null),
                        "message": error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("transcription failed"),
                        "param": null,
                    }
                });
                copy_event_id(&mut event);
                UpstreamTranslate {
                    to_client: Some(event.to_string()),
                    ..UpstreamTranslate::default()
                }
            }
            Some("session.finished") => UpstreamTranslate {
                session_finished: true,
                ..UpstreamTranslate::default()
            },
            Some("error") => {
                let error = value.get("error").cloned().unwrap_or_else(|| json!({}));
                let event = json!({
                    "type": "error",
                    "event_id": format!("event_{}", Uuid::new_v4()),
                    "error": {
                        "type": error
                            .get("type")
                            .and_then(Value::as_str)
                            .unwrap_or("server_error"),
                        "code": error.get("code").cloned().unwrap_or(Value::Null),
                        "message": error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("upstream realtime error"),
                        "param": error.get("param").cloned().unwrap_or(Value::Null),
                        "event_id": value.get("event_id").cloned().unwrap_or(Value::Null),
                    }
                });
                UpstreamTranslate {
                    to_client: Some(event.to_string()),
                    ..UpstreamTranslate::default()
                }
            }
            _ => forward(),
        }
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
    if outcome.audio_seconds > 0 && !outcome.client_sent_finish {
        tracing::warn!(
            relay_trace_id = %ctx.relay_trace_id,
            model = %ctx.upstream_model,
            "realtime ASR session ended before the gateway could request session.finish"
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
            "wss://dashscope.aliyuncs.com/api-ws/v1/realtime?model=qwen3-asr-flash-realtime&heartbeat=true"
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

    fn translator() -> UpstreamTranslator {
        UpstreamTranslator::new("qwen3-asr-flash-realtime".to_string())
    }

    #[test]
    fn translates_transcription_session_update_to_qwen() {
        let mut resampler = PcmResampler::default();
        let translated = translate_client_event(
            &json!({
                "type": "transcription_session.update",
                "event_id": "event_client",
                "session": {
                    "input_audio_format": "pcm16",
                    "input_audio_transcription": {
                        "model": "gpt-4o-transcribe",
                        "language": "zh",
                        "prompt": "jargon"
                    },
                    "turn_detection": {
                        "type": "server_vad",
                        "threshold": 0.6,
                        "prefix_padding_ms": 300,
                        "silence_duration_ms": 400
                    },
                    "modalities": ["text"],
                    "input_audio_noise_reduction": { "type": "near_field" },
                    "include": ["item.input_audio_transcription.logprobs"]
                }
            })
            .to_string(),
            &mut resampler,
        );
        assert!(translated.to_client.is_none());
        let upstream: Value = serde_json::from_str(&translated.to_upstream.unwrap()).unwrap();
        assert_eq!(upstream["type"], "session.update");
        assert_eq!(upstream["event_id"], "event_client");
        assert_eq!(upstream["session"]["modalities"], json!(["text"]));
        assert_eq!(upstream["session"]["input_audio_format"], "pcm");
        assert_eq!(upstream["session"]["sample_rate"], 16_000);
        assert_eq!(
            upstream["session"]["input_audio_transcription"],
            json!({ "language": "zh" })
        );
        assert_eq!(upstream["session"]["turn_detection"]["type"], "server_vad");
        assert_eq!(upstream["session"]["turn_detection"]["threshold"], 0.6);
        assert_eq!(
            upstream["session"]["turn_detection"]["silence_duration_ms"],
            400
        );
        assert!(upstream["session"]["turn_detection"]
            .get("prefix_padding_ms")
            .is_none());
    }

    #[test]
    fn accepts_session_update_alias_and_clamps_threshold() {
        let mut resampler = PcmResampler::default();
        let translated = translate_client_event(
            &json!({
                "type": "session.update",
                "session": {
                    "input_audio_format": "pcm16",
                    "turn_detection": { "type": "server_vad", "threshold": 5.0 }
                }
            })
            .to_string(),
            &mut resampler,
        );
        let upstream: Value = serde_json::from_str(&translated.to_upstream.unwrap()).unwrap();
        assert_eq!(upstream["session"]["turn_detection"]["threshold"], 1.0);
    }

    #[test]
    fn passes_null_turn_detection_for_manual_mode() {
        let mut resampler = PcmResampler::default();
        let translated = translate_client_event(
            &json!({
                "type": "transcription_session.update",
                "session": { "input_audio_format": "pcm16", "turn_detection": null }
            })
            .to_string(),
            &mut resampler,
        );
        let upstream: Value = serde_json::from_str(&translated.to_upstream.unwrap()).unwrap();
        assert!(upstream["session"]["turn_detection"].is_null());
    }

    #[test]
    fn rejects_g711_and_semantic_vad_with_openai_error_shape() {
        let mut resampler = PcmResampler::default();
        for format in ["g711_ulaw", "g711_alaw"] {
            let translated = translate_client_event(
                &json!({
                    "type": "transcription_session.update",
                    "session": { "input_audio_format": format }
                })
                .to_string(),
                &mut resampler,
            );
            assert!(translated.to_upstream.is_none());
            let error: Value = serde_json::from_str(&translated.to_client.unwrap()).unwrap();
            assert_eq!(error["type"], "error");
            assert_eq!(error["error"]["type"], "invalid_request_error");
            assert_eq!(error["error"]["code"], "invalid_value");
            assert_eq!(error["error"]["param"], "session.input_audio_format");
        }

        let translated = translate_client_event(
            &json!({
                "type": "transcription_session.update",
                "session": {
                    "input_audio_format": "pcm16",
                    "turn_detection": { "type": "semantic_vad" }
                }
            })
            .to_string(),
            &mut resampler,
        );
        assert!(translated.to_upstream.is_none());
        let error: Value = serde_json::from_str(&translated.to_client.unwrap()).unwrap();
        assert_eq!(error["error"]["code"], "invalid_value");
        assert_eq!(error["error"]["param"], "session.turn_detection.type");
    }

    #[test]
    fn rejects_unknown_client_events() {
        let mut resampler = PcmResampler::default();
        let translated = translate_client_event(r#"{"type":"response.create"}"#, &mut resampler);
        assert!(translated.to_upstream.is_none());
        let error: Value = serde_json::from_str(&translated.to_client.unwrap()).unwrap();
        assert_eq!(error["type"], "error");
        assert_eq!(error["error"]["type"], "invalid_request_error");
    }

    #[test]
    fn forwards_commit_and_answers_clear_locally() {
        let mut resampler = PcmResampler::default();
        let commit =
            translate_client_event(r#"{"type":"input_audio_buffer.commit"}"#, &mut resampler);
        assert_eq!(
            commit.to_upstream.as_deref(),
            Some(r#"{"type":"input_audio_buffer.commit"}"#)
        );
        assert!(commit.to_client.is_none());

        let clear =
            translate_client_event(r#"{"type":"input_audio_buffer.clear"}"#, &mut resampler);
        assert!(clear.to_upstream.is_none());
        let cleared: Value = serde_json::from_str(&clear.to_client.unwrap()).unwrap();
        assert_eq!(cleared["type"], "input_audio_buffer.cleared");
    }

    #[test]
    fn resamples_append_audio_from_24khz_to_16khz() {
        let mut resampler = PcmResampler::default();
        let pcm = vec![0_u8; 48_000]; // one second of 24 kHz PCM16 mono
        let translated = translate_client_event(
            &json!({
                "type": "input_audio_buffer.append",
                "audio": STANDARD.encode(&pcm)
            })
            .to_string(),
            &mut resampler,
        );
        assert!(translated.audio_appended);
        assert!(translated.to_client.is_none());
        let upstream: Value = serde_json::from_str(&translated.to_upstream.unwrap()).unwrap();
        assert_eq!(upstream["type"], "input_audio_buffer.append");
        let audio = STANDARD
            .decode(upstream["audio"].as_str().unwrap())
            .unwrap();
        assert_eq!(audio.len(), 32_000); // one second of 16 kHz PCM16 mono
    }

    #[test]
    fn resampler_keeps_ratio_and_phase_across_chunks() {
        let samples: Vec<i16> = (0..24_000_i32)
            .map(|i| ((i * 37) % 20_000 - 10_000) as i16)
            .collect();
        let mut bytes = Vec::new();
        for sample in &samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }

        let mut one_shot = PcmResampler::default();
        let expected = one_shot.push(&bytes);
        assert_eq!(expected.len(), 32_000);

        // Odd chunk size crosses 16-bit sample boundaries.
        let mut chunked = PcmResampler::default();
        let mut actual = Vec::new();
        for chunk in bytes.chunks(1_001) {
            actual.extend_from_slice(&chunked.push(chunk));
        }
        assert_eq!(actual, expected);

        let output: Vec<i16> = expected
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        assert_eq!(output[0], samples[0]);
        assert_eq!(
            output[1],
            ((i32::from(samples[1]) + i32::from(samples[2])) / 2) as i16
        );
        assert_eq!(output[2], samples[3]);
    }

    #[test]
    fn session_created_becomes_transcription_session_updated() {
        let mut translator = translator();
        let out = translator.translate(r#"{"type":"session.created","session":{}}"#);
        assert!(!out.session_finished);
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(event["type"], "transcription_session.updated");
        assert_eq!(event["session"]["modalities"], json!(["text"]));
        assert_eq!(event["session"]["input_audio_format"], "pcm16");
        assert_eq!(
            event["session"]["input_audio_transcription"]["model"],
            "qwen3-asr-flash-realtime"
        );
        assert_eq!(event["session"]["turn_detection"]["type"], "server_vad");
        assert_eq!(event["session"]["turn_detection"]["threshold"], 0.5);
        assert_eq!(event["session"]["turn_detection"]["prefix_padding_ms"], 300);
        assert_eq!(
            event["session"]["turn_detection"]["silence_duration_ms"],
            500
        );
    }

    #[test]
    fn session_updated_reflects_effective_config() {
        let mut translator = translator();
        let out = translator.translate(
            &json!({
                "type": "session.updated",
                "session": {
                    "input_audio_format": "pcm",
                    "sample_rate": 16000,
                    "input_audio_transcription": { "language": "zh" },
                    "turn_detection": {
                        "type": "server_vad",
                        "threshold": 0.2,
                        "silence_duration_ms": 800
                    }
                }
            })
            .to_string(),
        );
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(event["type"], "transcription_session.updated");
        assert_eq!(event["session"]["input_audio_format"], "pcm16");
        assert_eq!(
            event["session"]["input_audio_transcription"]["language"],
            "zh"
        );
        assert_eq!(event["session"]["turn_detection"]["threshold"], 0.2);
        assert_eq!(event["session"]["turn_detection"]["prefix_padding_ms"], 300);
        assert_eq!(
            event["session"]["turn_detection"]["silence_duration_ms"],
            800
        );
    }

    #[test]
    fn forwards_speech_events_unchanged() {
        let mut translator = translator();
        let raw = r#"{"type":"input_audio_buffer.speech_stopped","audio_end_ms":2101}"#;
        let out = translator.translate(raw);
        assert_eq!(out.to_client.as_deref(), Some(raw));
    }

    #[test]
    fn rewrites_conversation_item_created() {
        let mut translator = translator();
        let out = translator.translate(
            &json!({
                "type": "conversation.item.created",
                "event_id": "event_1",
                "item": {
                    "id": "item_1",
                    "type": "message",
                    "role": "assistant",
                    "content": [{ "type": "audio", "transcript": "hello" }]
                }
            })
            .to_string(),
        );
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(event["event_id"], "event_1");
        assert_eq!(event["item"]["id"], "item_1");
        assert_eq!(event["item"]["object"], "realtime.item");
        assert_eq!(event["item"]["type"], "message");
        assert_eq!(event["item"]["status"], "completed");
        assert_eq!(event["item"]["role"], "user");
        assert_eq!(
            event["item"]["content"],
            json!([{ "type": "input_audio", "transcript": null }])
        );
    }

    #[test]
    fn converts_text_snapshots_to_deltas() {
        let mut translator = translator();
        let event: Value = serde_json::from_str(
            &translator
                .translate(
                    r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好","stash":""}"#,
                )
                .to_client
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            event["type"],
            "conversation.item.input_audio_transcription.delta"
        );
        assert_eq!(event["item_id"], "item_1");
        assert_eq!(event["content_index"], 0);
        assert_eq!(event["delta"], "你好");

        // Confirmed growth only: the revisable stash is not emitted.
        let event: Value = serde_json::from_str(
            &translator
                .translate(
                    r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世界","stash":"呀"}"#,
                )
                .to_client
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["delta"], "，世界");

        // Stash-only change: no confirmed growth, no event.
        let out = translator.translate(
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世界","stash":"啊"}"#,
        );
        assert!(out.to_client.is_none());

        // Confirmed text revised (shorter than before): silent rebase, then
        // later growth diffs against the revised base.
        let out = translator.translate(
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世","stash":""}"#,
        );
        assert!(out.to_client.is_none());
        let event: Value = serde_json::from_str(
            &translator
                .translate(
                    r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世界！","stash":""}"#,
                )
                .to_client
                .unwrap(),
        )
        .unwrap();
        assert_eq!(event["delta"], "界！");
    }

    #[test]
    fn completed_emits_catch_up_delta_before_completed_event() {
        let mut translator = translator();
        let out = translator.translate(
            &json!({
                "type": "conversation.item.input_audio_transcription.completed",
                "event_id": "event_2",
                "item_id": "item_1",
                "content_index": 0,
                "transcript": "你好，世界",
                "language": "zh",
                "emotion": "happy"
            })
            .to_string(),
        );
        // Nothing was streamed yet, so the whole transcript is caught up via a
        // delta frame, followed by the completed event in `pending`.
        let delta: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(
            delta["type"],
            "conversation.item.input_audio_transcription.delta"
        );
        assert_eq!(delta["delta"], "你好，世界");
        let event: Value = serde_json::from_str(&out.pending.unwrap()).unwrap();
        assert_eq!(
            event["type"],
            "conversation.item.input_audio_transcription.completed"
        );
        assert_eq!(event["event_id"], "event_2");
        assert_eq!(event["item_id"], "item_1");
        assert_eq!(event["content_index"], 0);
        assert_eq!(event["transcript"], "你好，世界");
        assert!(event.get("language").is_none());
        assert!(event.get("emotion").is_none());

        // last_full now equals the transcript, so a matching snapshot yields
        // no delta.
        let out = translator.translate(
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世界","stash":""}"#,
        );
        assert!(out.to_client.is_none());
    }

    #[test]
    fn completed_without_catch_up_sends_single_frame() {
        let mut translator = translator();
        translator.translate(
            r#"{"type":"conversation.item.input_audio_transcription.text","item_id":"item_1","text":"你好，世界","stash":"!"}"#,
        );
        let out = translator.translate(
            r#"{"type":"conversation.item.input_audio_transcription.completed","item_id":"item_1","content_index":0,"transcript":"你好，世界"}"#,
        );
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(
            event["type"],
            "conversation.item.input_audio_transcription.completed"
        );
        assert!(out.pending.is_none());
    }

    #[test]
    fn maps_failed_and_error_events_to_openai_shape() {
        let mut translator = translator();
        let out = translator.translate(
            &json!({
                "type": "conversation.item.input_audio_transcription.failed",
                "item_id": "item_1",
                "error": { "code": "audio_too_short", "message": "audio too short" }
            })
            .to_string(),
        );
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(
            event["type"],
            "conversation.item.input_audio_transcription.failed"
        );
        assert_eq!(event["item_id"], "item_1");
        assert_eq!(event["content_index"], 0);
        assert_eq!(event["error"]["type"], "server_error");
        assert_eq!(event["error"]["code"], "audio_too_short");
        assert_eq!(event["error"]["message"], "audio too short");

        let out = translator.translate(
            &json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "code": "invalid_audio",
                    "message": "bad audio",
                    "param": "audio"
                }
            })
            .to_string(),
        );
        let event: Value = serde_json::from_str(&out.to_client.unwrap()).unwrap();
        assert_eq!(event["type"], "error");
        assert_eq!(event["error"]["type"], "invalid_request_error");
        assert_eq!(event["error"]["code"], "invalid_audio");
        assert_eq!(event["error"]["message"], "bad audio");
        assert_eq!(event["error"]["param"], "audio");
    }

    #[test]
    fn drops_session_finished_and_flags_terminal() {
        let mut translator = translator();
        let out = translator.translate(r#"{"type":"session.finished"}"#);
        assert!(out.to_client.is_none());
        assert!(out.session_finished);
    }
}
