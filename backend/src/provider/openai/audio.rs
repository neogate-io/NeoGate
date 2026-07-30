use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
    time::Instant,
};

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use bytes::Bytes;
use chrono::Utc;
use futures_util::{stream::BoxStream, StreamExt};
use serde_json::{json, Value};
use symphonia::core::{
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{BillingMeter, DebitHold},
    error::{AppError, AppResult},
    provider::adapters::bailian_asr,
    relay::{
        release_empty_hold, reserve_billable_credit, selector::AttemptedUpstream, RelayBody,
        UserRequestPermit,
    },
    task::upstream::{self as upstream_task, NewUpstreamTask, UpstreamTaskType},
    AppState,
};

use super::{
    content_type_header,
    multipart::{multipart_boundary, multipart_files, multipart_text_fields},
    select_upstream_excluding,
};

const TRANSCRIPTION_PATH: &str = "/v1/audio/transcriptions";
const MAX_AUDIO_DURATION_SECONDS: f64 = 12.0 * 60.0 * 60.0;
const FLASH_MAX_AUDIO_DURATION_SECONDS: f64 = 5.0 * 60.0;
const OPENAI_MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const FLASH_MAX_BASE64_AUDIO_BYTES: usize = 10 * 1024 * 1024;
const FLASH_CONTEXT_MAX_CHARS: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseFormat {
    Json,
    Text,
    Srt,
    VerboseJson,
    Vtt,
    DiarizedJson,
}

impl ResponseFormat {
    fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Text => "text",
            Self::Srt => "srt",
            Self::VerboseJson => "verbose_json",
            Self::Vtt => "vtt",
            Self::DiarizedJson => "diarized_json",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AudioTranscriptionApi {
    AsyncFile,
    MultimodalGeneration,
}

#[derive(Debug)]
struct AudioRequest {
    model: String,
    response_format: ResponseFormat,
    language: Option<String>,
    languages: Vec<String>,
    prompt: Option<String>,
    keywords: Vec<String>,
    timestamp_granularities: Vec<String>,
    include: Vec<String>,
    stream: bool,
    temperature: Option<f64>,
    chunking_strategy: Option<String>,
    known_speaker_names: Vec<String>,
    known_speaker_references: Vec<String>,
    audio: Bytes,
    extension: &'static str,
    duration_seconds: f64,
    sample_rate: Option<u32>,
}

pub(crate) async fn openai_audio_transcriptions(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    headers: HeaderMap,
    RelayBody(body): RelayBody,
) -> AppResult<Response> {
    let request = parse_request(&headers, &body)?;
    let resolved = crate::project::models::resolve_project_model(
        &state.db.pool,
        auth.project_id,
        &request.model,
    )
    .await?;
    let user_key_model_credit_account =
        auth.model_credit_account(&resolved.external_model).cloned();
    let mut request_permit = Some(state.user_request_limiter.try_acquire(auth.user_id).await?);
    let relay_trace_id = Uuid::new_v4();
    let started = Instant::now();
    let mut attempted_upstreams = Vec::new();
    let mut failovers = 0;

    let (submission, upstream, protocol, hold) = loop {
        let (protocol, upstream) = select_upstream_excluding(
            &state,
            TRANSCRIPTION_PATH,
            &resolved.target_model,
            resolved.target_channel_id,
            None,
            &attempted_upstreams,
        )
        .await?;
        attempted_upstreams.push(AttemptedUpstream::from(&upstream));
        let transcription_api = ensure_audio_transcription_capability(
            &state.db.pool,
            &upstream.provider,
            &resolved.target_model,
        )
        .await?;
        validate_request_for_api(&request, transcription_api)?;
        let price = state
            .billing
            .price_for(
                &state.db.pool,
                upstream.channel_id,
                &resolved.target_model,
                &auth.user_group,
            )
            .await?;
        if price.billing_meter != BillingMeter::Audio {
            return Err(AppError::BadRequestWithCode {
                code: "audio_price_required",
                message: "Alibaba ASR requires audio per-second pricing",
            });
        }
        let unit_price = price.unit_price_micros.filter(|price| *price > 0).ok_or(
            AppError::BadRequestWithCode {
                code: "audio_price_required",
                message: "Alibaba ASR requires a positive per-second price",
            },
        )?;
        let estimated_seconds = ceil_duration(request.duration_seconds)?;
        let hold = reserve_billable_credit(
            &state,
            &auth,
            user_key_model_credit_account.as_ref(),
            estimated_seconds.saturating_mul(unit_price),
        )
        .await?;
        if transcription_api == AudioTranscriptionApi::MultimodalGeneration && request.stream {
            let response = bailian_asr::transcribe_multimodal_stream(
                &state,
                &upstream,
                &resolved.target_model,
                request.audio.clone(),
                request.extension,
                bailian_asr::MultimodalOptions {
                    context: transcription_context(&request)?.as_deref(),
                    sample_rate: request.sample_rate,
                },
            )
            .await;
            match response {
                Ok(response) => {
                    return audio_stream_response(
                        AudioStreamContext {
                            state: state.clone(),
                            auth,
                            upstream,
                            protocol,
                            external_model: resolved.external_model,
                            upstream_model: resolved.target_model,
                            hold: Some(hold),
                            request_permit: request_permit.take(),
                            relay_trace_id,
                            started_at: Utc::now(),
                            local_duration_seconds: request.duration_seconds,
                            language: response_language(&request),
                        },
                        response,
                    );
                }
                Err(err) => {
                    release_empty_hold(&state, hold, "Alibaba ASR streaming submission failure")
                        .await;
                    if err.retryable() && failovers < state.config.relay.max_upstream_failovers {
                        failovers += 1;
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        let context = transcription_context(&request)?;
        let submission = match transcription_api {
            AudioTranscriptionApi::AsyncFile => {
                bailian_asr::upload_and_submit(
                    &state,
                    &upstream,
                    &resolved.target_model,
                    request.audio.clone(),
                    request.extension,
                    request.language.as_deref(),
                )
                .await
            }
            AudioTranscriptionApi::MultimodalGeneration => {
                bailian_asr::transcribe_multimodal(
                    &state,
                    &upstream,
                    &resolved.target_model,
                    request.audio.clone(),
                    request.extension,
                    request.duration_seconds,
                    bailian_asr::MultimodalOptions {
                        context: context.as_deref(),
                        sample_rate: request.sample_rate,
                    },
                )
                .await
            }
        };
        match submission {
            Ok(submission) => break (submission, upstream, protocol, hold),
            Err(err) => {
                release_empty_hold(&state, hold, "Alibaba ASR pre-submission failure").await;
                if err.retryable() && failovers < state.config.relay.max_upstream_failovers {
                    failovers += 1;
                    tracing::warn!(
                        model = %resolved.target_model,
                        channel_id = upstream.channel_id,
                        failover_attempt = failovers,
                        "retryable Alibaba ASR submission failure; selecting another upstream"
                    );
                    continue;
                }
                return Err(err);
            }
        }
    };
    persist_submission(
        &state,
        &auth,
        &upstream,
        protocol,
        &resolved.external_model,
        &resolved.target_model,
        &submission,
        &hold,
        request.duration_seconds,
        request.response_format,
        response_language(&request),
        relay_trace_id,
        Utc::now(),
    )
    .await?;
    tracing::info!(
        model = %resolved.target_model,
        audio_bytes = request.audio.len(),
        local_duration_seconds = request.duration_seconds,
        channel_id = upstream.channel_id,
        upstream_task_id = %submission.task_id,
        upstream_request_id = submission.request_id.as_deref().unwrap_or(""),
        upstream_status = %submission.status,
        "Alibaba ASR transcription task submitted"
    );
    state.task_wakeup.notify_one();

    loop {
        let task = upstream_task::fetch_task(
            &state.db.pool,
            auth.user_key_id,
            UpstreamTaskType::AudioTranscription,
            &submission.task_id,
        )
        .await?;
        if task.terminal {
            let elapsed_ms = started.elapsed().as_millis() as i64;
            tracing::info!(
                upstream_task_id = %submission.task_id,
                channel_id = upstream.channel_id,
                status = %task.status,
                elapsed_ms,
                "Alibaba ASR transcription request completed"
            );
            return task_response(
                &task.upstream_metadata,
                request.response_format,
                &task.status,
                response_language(&request).as_deref(),
                &request.timestamp_granularities,
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

#[allow(clippy::too_many_arguments)]
async fn persist_submission(
    state: &Arc<AppState>,
    auth: &UserAuth,
    upstream: &crate::relay::selector::SelectedUpstream,
    protocol: crate::relay::selector::UpstreamProtocol,
    external_model: &str,
    upstream_model: &str,
    submission: &bailian_asr::Submission,
    hold: &DebitHold,
    local_duration_seconds: f64,
    response_format: ResponseFormat,
    language: Option<String>,
    relay_trace_id: Uuid,
    started_at: chrono::DateTime<Utc>,
) -> AppResult<()> {
    let inline_result = submission.completed.as_ref().map(|completed| {
        json!({
            "text": completed.text,
            "duration_seconds": completed.duration_seconds,
            "duration_source": completed.duration_source,
            "details": completed.details,
            "request_id": submission.request_id,
        })
    });
    let task_metadata = json!({
        "neogate": {
            "local_duration_seconds": local_duration_seconds,
            "response_format": response_format.as_str(),
            "language": language,
            "relay_started_at": started_at.to_rfc3339(),
            "relay_trace_id": relay_trace_id,
            "request_id": submission.request_id,
            "inline_result": inline_result,
        }
    });
    if let Err(err) = upstream_task::insert_task(
        &state.db.pool,
        NewUpstreamTask {
            task_type: UpstreamTaskType::AudioTranscription,
            upstream_task_id: &submission.task_id,
            auth,
            protocol,
            upstream,
            model: Some(external_model),
            upstream_model: Some(upstream_model),
            status: &submission.status,
            terminal: false,
            hold,
            upstream_metadata: task_metadata,
        },
        crate::task::POLL_INTERVAL,
        crate::task::AUDIO_TASK_RETENTION,
    )
    .await
    {
        release_empty_hold(state, hold.clone(), "Alibaba ASR task persistence failure").await;
        return Err(err);
    }
    state.task_wakeup.notify_one();
    Ok(())
}

fn parse_request(headers: &HeaderMap, body: &[u8]) -> AppResult<AudioRequest> {
    let (_, content_type) = content_type_header(headers)?;
    if !content_type
        .to_ascii_lowercase()
        .starts_with("multipart/form-data")
    {
        return Err(AppError::BadRequest(
            "audio transcriptions require multipart/form-data".to_string(),
        ));
    }
    let boundary = multipart_boundary(&content_type)?;
    let mut fields: HashMap<String, String> = HashMap::new();
    let mut array_fields: HashMap<String, Vec<String>> = HashMap::new();
    for (name, value) in multipart_text_fields(body, &boundary)? {
        let canonical = name.strip_suffix("[]").unwrap_or(&name);
        if matches!(
            canonical,
            "timestamp_granularities"
                | "include"
                | "keywords"
                | "languages"
                | "known_speaker_names"
                | "known_speaker_references"
        ) {
            array_fields
                .entry(canonical.to_string())
                .or_default()
                .push(value);
            continue;
        }
        if fields.insert(name.clone(), value).is_some() {
            return Err(AppError::BadRequest(format!(
                "multipart field {name} must not be repeated"
            )));
        }
    }
    for name in fields.keys() {
        if !matches!(
            name.as_str(),
            "model"
                | "response_format"
                | "language"
                | "temperature"
                | "prompt"
                | "stream"
                | "chunking_strategy"
        ) {
            return Err(AppError::BadRequest(format!(
                "unsupported audio transcription field: {name}"
            )));
        }
    }
    let model = fields
        .remove("model")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::BadRequest("model is required".to_string()))?;
    let response_format =
        match fields
            .remove("response_format")
            .unwrap_or_else(|| "json".to_string())
            .as_str()
        {
            "json" => ResponseFormat::Json,
            "text" => ResponseFormat::Text,
            "srt" => ResponseFormat::Srt,
            "verbose_json" => ResponseFormat::VerboseJson,
            "vtt" => ResponseFormat::Vtt,
            "diarized_json" => ResponseFormat::DiarizedJson,
            _ => return Err(AppError::BadRequestWithCode {
                code: "unsupported_response_format",
                message:
                    "response_format must be json, text, srt, verbose_json, vtt, or diarized_json",
            }),
        };
    let stream = fields
        .remove("stream")
        .map(|value| parse_bool("stream", &value))
        .transpose()?
        .unwrap_or(false);
    let temperature = if let Some(value) = fields.remove("temperature") {
        let temperature = value
            .parse::<f64>()
            .map_err(|_| AppError::BadRequest("temperature must be a number".to_string()))?;
        if !temperature.is_finite() || !(0.0..=1.0).contains(&temperature) {
            return Err(AppError::BadRequest(
                "temperature must be between 0 and 1".to_string(),
            ));
        }
        Some(temperature)
    } else {
        None
    };
    let language = fields
        .remove("language")
        .filter(|value| !value.is_empty())
        .map(normalize_language)
        .transpose()?;
    let languages = array_fields
        .remove("languages")
        .unwrap_or_default()
        .into_iter()
        .map(normalize_language)
        .collect::<AppResult<Vec<_>>>()?;
    if language.is_some() && !languages.is_empty() {
        return Err(AppError::BadRequest(
            "language and languages must not be used together".to_string(),
        ));
    }
    let keywords = array_fields.remove("keywords").unwrap_or_default();
    for keyword in &keywords {
        if keyword.contains(['<', '>', '\r', '\n']) {
            return Err(AppError::BadRequest(
                "keywords must not contain angle brackets or line breaks".to_string(),
            ));
        }
    }
    let timestamp_granularities = array_fields
        .remove("timestamp_granularities")
        .unwrap_or_default();
    if timestamp_granularities
        .iter()
        .any(|value| !matches!(value.as_str(), "word" | "segment"))
    {
        return Err(AppError::BadRequest(
            "timestamp_granularities values must be word or segment".to_string(),
        ));
    }
    let include = array_fields.remove("include").unwrap_or_default();
    let known_speaker_names = array_fields
        .remove("known_speaker_names")
        .unwrap_or_default();
    let mut known_speaker_references = array_fields
        .remove("known_speaker_references")
        .unwrap_or_default();
    let mut files = multipart_files(body, &boundary)?;
    let mut audio_files = Vec::new();
    for file in files.drain(..) {
        match file.name.strip_suffix("[]").unwrap_or(&file.name) {
            "file" => audio_files.push(file),
            "known_speaker_references" => {
                known_speaker_references.push("multipart-file".to_string());
            }
            name => {
                return Err(AppError::BadRequest(format!(
                    "unsupported audio transcription file field: {name}"
                )))
            }
        }
    }
    if audio_files.len() != 1 {
        return Err(AppError::BadRequest(
            "exactly one file field is required".to_string(),
        ));
    }
    let audio = audio_files.remove(0).data;
    if audio.is_empty() {
        return Err(AppError::BadRequest("file must not be empty".to_string()));
    }
    if audio.len() > OPENAI_MAX_AUDIO_BYTES {
        return Err(AppError::PayloadTooLarge(
            "audio files are limited to 25 MB".to_string(),
        ));
    }
    let extension = detected_extension(&audio)?;
    let (duration_seconds, sample_rate) = audio_info(&audio, extension)?;
    Ok(AudioRequest {
        model,
        response_format,
        language,
        languages,
        prompt: fields.remove("prompt").filter(|value| !value.is_empty()),
        keywords,
        timestamp_granularities,
        include,
        stream,
        temperature,
        chunking_strategy: fields.remove("chunking_strategy"),
        known_speaker_names,
        known_speaker_references,
        audio,
        extension,
        duration_seconds,
        sample_rate,
    })
}

fn parse_bool(name: &str, value: &str) -> AppResult<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(AppError::BadRequest(format!(
            "{name} must be true or false"
        ))),
    }
}

fn normalize_language(value: String) -> AppResult<String> {
    let language = value.trim().to_ascii_lowercase();
    if !(2..=16).contains(&language.len())
        || !language
            .bytes()
            .all(|byte| byte.is_ascii_alphabetic() || byte == b'-')
    {
        return Err(AppError::BadRequest(
            "language must be an ISO language code".to_string(),
        ));
    }
    Ok(language)
}

fn validate_request_for_api(request: &AudioRequest, api: AudioTranscriptionApi) -> AppResult<()> {
    if request.response_format == ResponseFormat::DiarizedJson
        || !request.known_speaker_names.is_empty()
        || !request.known_speaker_references.is_empty()
        || request.chunking_strategy.is_some()
    {
        return Err(unsupported_option("speaker diarization"));
    }
    if !request.include.is_empty() {
        return Err(unsupported_option("include"));
    }
    if request.stream && request.response_format != ResponseFormat::Json {
        return Err(AppError::BadRequest(
            "stream=true requires response_format=json for this model".to_string(),
        ));
    }
    if !request.timestamp_granularities.is_empty()
        && request.response_format != ResponseFormat::VerboseJson
    {
        return Err(AppError::BadRequest(
            "timestamp_granularities requires response_format=verbose_json".to_string(),
        ));
    }
    match api {
        AudioTranscriptionApi::AsyncFile => {
            if request.stream {
                return Err(unsupported_option("stream"));
            }
            if request.prompt.is_some()
                || !request.keywords.is_empty()
                || !request.languages.is_empty()
            {
                return Err(unsupported_option("transcription context"));
            }
        }
        AudioTranscriptionApi::MultimodalGeneration => {
            let encoded_len = request.audio.len().div_ceil(3).saturating_mul(4);
            if encoded_len > FLASH_MAX_BASE64_AUDIO_BYTES {
                return Err(AppError::PayloadTooLarge(
                    "fun-asr-flash audio must not exceed 10 MB after Base64 encoding".to_string(),
                ));
            }
            if request.duration_seconds > FLASH_MAX_AUDIO_DURATION_SECONDS {
                return Err(AppError::BadRequestWithCode {
                    code: "invalid_audio_duration",
                    message: "fun-asr-flash audio duration must not exceed 5 minutes",
                });
            }
            if request
                .temperature
                .is_some_and(|temperature| temperature != 0.0)
            {
                return Err(unsupported_option("temperature"));
            }
        }
    }
    Ok(())
}

fn transcription_context(request: &AudioRequest) -> AppResult<Option<String>> {
    let mut parts = Vec::new();
    if let Some(prompt) = request.prompt.as_deref() {
        parts.push(prompt.trim().to_string());
    }
    if !request.keywords.is_empty() {
        parts.push(request.keywords.join(", "));
    }
    let languages = request
        .language
        .iter()
        .chain(request.languages.iter())
        .cloned()
        .collect::<Vec<_>>();
    if !languages.is_empty() {
        parts.push(format!("Expected languages: {}", languages.join(", ")));
    }
    let context = parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if context.chars().count() > FLASH_CONTEXT_MAX_CHARS {
        return Err(AppError::BadRequestWithCode {
            code: "invalid_prompt",
            message:
                "combined prompt, keywords, and language context must not exceed 400 characters",
        });
    }
    Ok((!context.is_empty()).then_some(context))
}

fn response_language(request: &AudioRequest) -> Option<String> {
    request
        .language
        .clone()
        .or_else(|| request.languages.first().cloned())
}

async fn ensure_audio_transcription_capability(
    pool: &sqlx::PgPool,
    provider: &str,
    model: &str,
) -> AppResult<AudioTranscriptionApi> {
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
    match capabilities.as_ref().and_then(|capabilities| {
        crate::admin::provider::catalog_audio_transcription_adapter(provider, capabilities)
    }) {
        Some(crate::admin::provider::AudioTranscriptionAdapter::AsyncFile) => {
            Ok(AudioTranscriptionApi::AsyncFile)
        }
        Some(crate::admin::provider::AudioTranscriptionAdapter::MultimodalGeneration) => {
            Ok(AudioTranscriptionApi::MultimodalGeneration)
        }
        _ => Err(AppError::BadRequestWithCode {
            code: "unsupported_audio_model",
            message: "the selected model is not configured for audio transcription",
        }),
    }
}

fn unsupported_option(option: &str) -> AppError {
    AppError::BadRequest(format!(
        "audio transcription option {option} is not supported"
    ))
}

fn detected_extension(data: &[u8]) -> AppResult<&'static str> {
    if data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WAVE") {
        return Ok("wav");
    }
    if data.starts_with(b"fLaC") {
        return Ok("flac");
    }
    if data.starts_with(b"OggS") {
        if data
            .windows(8)
            .take(256)
            .any(|window| window == b"OpusHead")
        {
            return Ok("opus");
        }
        return Ok("ogg");
    }
    if data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Ok("webm");
    }
    if data.get(4..8) == Some(b"ftyp") {
        return Ok("m4a");
    }
    if data.starts_with(b"ID3")
        || data
            .windows(2)
            .take(4096)
            .any(|window| window[0] == 0xff && window[1] & 0xe0 == 0xe0)
    {
        return Ok("mp3");
    }
    Err(unsupported_audio_format())
}

fn audio_info(data: &[u8], extension: &str) -> AppResult<(f64, Option<u32>)> {
    let mut hint = Hint::new();
    hint.with_extension(extension);
    let source = MediaSourceStream::new(Box::new(Cursor::new(data.to_vec())), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            source,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| unsupported_audio_format())?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(unsupported_audio_format)?;
    let track_id = track.id;
    let time_base = track.codec_params.time_base;
    let n_frames = track.codec_params.n_frames;
    let sample_rate = track.codec_params.sample_rate;
    let duration = match (time_base, n_frames) {
        (Some(time_base), Some(frames)) if frames > 0 => {
            let time = time_base.calc_time(frames);
            time.seconds as f64 + time.frac
        }
        (Some(time_base), _) => {
            let mut end = 0_u64;
            while let Ok(packet) = format.next_packet() {
                if packet.track_id() == track_id {
                    end = end.max(packet.ts().saturating_add(packet.dur()));
                }
            }
            let time = time_base.calc_time(end);
            time.seconds as f64 + time.frac
        }
        _ => return Err(unsupported_audio_format()),
    };
    if !duration.is_finite() || duration <= 0.0 || duration > MAX_AUDIO_DURATION_SECONDS {
        return Err(AppError::BadRequestWithCode {
            code: "invalid_audio_duration",
            message: "audio duration must be between 0 and 12 hours",
        });
    }
    Ok((duration, sample_rate))
}

fn unsupported_audio_format() -> AppError {
    AppError::BadRequestWithCode {
        code: "unsupported_audio_format",
        message:
            "supported audio formats are MP3/MPEG, WAV, FLAC, M4A/MP4, OGG/Vorbis, Opus, and WebM",
    }
}

fn ceil_duration(duration: f64) -> AppResult<i64> {
    if !duration.is_finite() || duration <= 0.0 || duration > MAX_AUDIO_DURATION_SECONDS {
        return Err(AppError::BadRequest("invalid audio duration".to_string()));
    }
    Ok(duration.ceil() as i64)
}

fn task_response(
    metadata: &Value,
    format: ResponseFormat,
    status: &str,
    language: Option<&str>,
    timestamp_granularities: &[String],
) -> AppResult<Response> {
    if status != "completed" {
        let message = metadata
            .pointer("/result/error")
            .and_then(Value::as_str)
            .unwrap_or("Alibaba ASR transcription failed");
        return Err(AppError::UpstreamUnavailable(message.to_string()));
    }
    let text = metadata
        .pointer("/result/text")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::UpstreamUnavailable("transcription text is missing".to_string())
        })?;
    let duration = metadata
        .pointer("/result/duration_seconds")
        .and_then(Value::as_f64)
        .unwrap_or_default();
    let sentences = result_sentences(metadata);
    let body = match format {
        ResponseFormat::Json => serde_json::to_vec(&json!({
            "text": text,
            "languages": language.map(|code| vec![json!({ "code": code })]).unwrap_or_default(),
            "usage": { "type": "duration", "seconds": duration.ceil() as i64 },
        }))?,
        ResponseFormat::Text => text.as_bytes().to_vec(),
        ResponseFormat::Srt => subtitle_response(text, duration, &sentences, false).into_bytes(),
        ResponseFormat::Vtt => subtitle_response(text, duration, &sentences, true).into_bytes(),
        ResponseFormat::VerboseJson => serde_json::to_vec(&verbose_response(
            text,
            duration,
            language,
            &sentences,
            timestamp_granularities,
        ))?,
        ResponseFormat::DiarizedJson => return Err(unsupported_option("speaker diarization")),
    };
    let content_type = match format {
        ResponseFormat::Json | ResponseFormat::VerboseJson => "application/json",
        ResponseFormat::Text => "text/plain; charset=utf-8",
        ResponseFormat::Srt => "application/x-subrip; charset=utf-8",
        ResponseFormat::Vtt => "text/vtt; charset=utf-8",
        ResponseFormat::DiarizedJson => unreachable!(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, HeaderValue::from_static(content_type))
        .body(Body::from(body))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

fn result_sentences(metadata: &Value) -> Vec<Value> {
    let details = metadata.pointer("/result/details");
    if let Some(sentences) = details
        .and_then(|value| value.get("sentences"))
        .and_then(Value::as_array)
    {
        return sentences.clone();
    }
    if let Some(transcripts) = details
        .and_then(|value| value.get("transcripts"))
        .and_then(Value::as_array)
    {
        return transcripts
            .iter()
            .filter_map(|transcript| transcript.get("sentences").and_then(Value::as_array))
            .flatten()
            .cloned()
            .collect();
    }
    details
        .and_then(|value| value.get("sentence"))
        .filter(|sentence| sentence.is_object())
        .cloned()
        .into_iter()
        .collect()
}

fn verbose_response(
    text: &str,
    duration: f64,
    language: Option<&str>,
    sentences: &[Value],
    timestamp_granularities: &[String],
) -> Value {
    let segments = sentences
        .iter()
        .enumerate()
        .map(|(index, sentence)| {
            json!({
                "id": sentence.get("sentence_id").and_then(Value::as_i64).unwrap_or(index as i64 + 1) - 1,
                "seek": 0,
                "start": milliseconds(sentence, "begin_time"),
                "end": milliseconds(sentence, "end_time"),
                "text": sentence.get("text").and_then(Value::as_str).unwrap_or_default(),
                "tokens": [],
                "temperature": 0.0,
                "avg_logprob": 0.0,
                "compression_ratio": 0.0,
                "no_speech_prob": 0.0,
            })
        })
        .collect::<Vec<_>>();
    let words = sentences
        .iter()
        .flat_map(|sentence| {
            sentence
                .get("words")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .map(|word| {
            let mut text = word
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            text.push_str(
                word.get("punctuation")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            json!({
                "word": text,
                "start": milliseconds(word, "begin_time"),
                "end": milliseconds(word, "end_time"),
            })
        })
        .collect::<Vec<_>>();
    let mut response = json!({
        "task": "transcribe",
        "language": language.unwrap_or("unknown"),
        "duration": duration,
        "text": text,
        "segments": segments,
        "words": words,
        "usage": { "type": "duration", "seconds": duration.ceil() as i64 },
    });
    let granularities = timestamp_granularities
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if granularities.is_empty() || !granularities.contains("word") {
        response.as_object_mut().expect("object").remove("words");
    }
    if !granularities.is_empty() && !granularities.contains("segment") {
        response.as_object_mut().expect("object").remove("segments");
    }
    response
}

fn milliseconds(value: &Value, field: &str) -> f64 {
    value.get(field).and_then(Value::as_f64).unwrap_or_default() / 1000.0
}

fn subtitle_response(text: &str, duration: f64, sentences: &[Value], vtt: bool) -> String {
    let fallback;
    let entries = if sentences.is_empty() {
        fallback = vec![json!({
            "begin_time": 0,
            "end_time": (duration * 1000.0).round() as i64,
            "text": text,
        })];
        &fallback
    } else {
        sentences
    };
    let mut output = if vtt {
        "WEBVTT\n\n".to_string()
    } else {
        String::new()
    };
    for (index, sentence) in entries.iter().enumerate() {
        if !vtt {
            output.push_str(&(index + 1).to_string());
            output.push('\n');
        }
        let start = sentence
            .get("begin_time")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        let end = sentence
            .get("end_time")
            .and_then(Value::as_i64)
            .unwrap_or_else(|| (duration * 1000.0).round() as i64);
        output.push_str(&format!(
            "{} --> {}\n{}\n\n",
            subtitle_timestamp(start, vtt),
            subtitle_timestamp(end, vtt),
            sentence
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
        ));
    }
    output
}

fn subtitle_timestamp(milliseconds: i64, vtt: bool) -> String {
    let milliseconds = milliseconds.max(0);
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds / 60_000 % 60;
    let seconds = milliseconds / 1_000 % 60;
    let fraction = milliseconds % 1_000;
    format!(
        "{hours:02}:{minutes:02}:{seconds:02}{}{fraction:03}",
        if vtt { '.' } else { ',' }
    )
}

struct AudioStreamContext {
    state: Arc<AppState>,
    auth: UserAuth,
    upstream: crate::relay::selector::SelectedUpstream,
    protocol: crate::relay::selector::UpstreamProtocol,
    external_model: String,
    upstream_model: String,
    hold: Option<DebitHold>,
    request_permit: Option<UserRequestPermit>,
    relay_trace_id: Uuid,
    started_at: chrono::DateTime<Utc>,
    local_duration_seconds: f64,
    language: Option<String>,
}

fn audio_stream_response(
    context: AudioStreamContext,
    response: reqwest::Response,
) -> AppResult<Response> {
    let relay = AudioStreamRelay {
        stream: response.bytes_stream().boxed(),
        context: Some(context),
        converter: AlibabaToOpenAiTranscriptionSse::default(),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(futures_util::stream::unfold(
            Some(relay),
            |relay| async move {
                let mut relay = relay?;
                loop {
                    match relay.stream.next().await {
                        Some(Ok(chunk)) => match relay.converter.push(&chunk) {
                            Ok(output) if output.is_empty() => continue,
                            Ok(output) => {
                                return Some((Ok::<Bytes, std::io::Error>(output), Some(relay)))
                            }
                            Err(err) => {
                                let frame = relay.finish_error(err.to_string()).await;
                                return Some((Ok(frame), None));
                            }
                        },
                        Some(Err(err)) => {
                            let frame = relay.finish_error(err.to_string()).await;
                            return Some((Ok(frame), None));
                        }
                        None => match relay.finish_success().await {
                            Ok(frame) => return Some((Ok(frame), None)),
                            Err(err) => {
                                let frame = openai_stream_error(&err.to_string());
                                return Some((Ok(frame), None));
                            }
                        },
                    }
                }
            },
        )))
        .map_err(|err| AppError::BadRequest(err.to_string()))
}

struct AudioStreamRelay {
    stream: BoxStream<'static, Result<Bytes, reqwest::Error>>,
    context: Option<AudioStreamContext>,
    converter: AlibabaToOpenAiTranscriptionSse,
}

impl AudioStreamRelay {
    async fn finish_success(&mut self) -> AppResult<Bytes> {
        let local_duration_seconds = self
            .context
            .as_ref()
            .map(|context| context.local_duration_seconds)
            .unwrap_or_default();
        let (frame, completed, request_id) = self.converter.finish(local_duration_seconds)?;
        let mut context = self.context.take().ok_or_else(|| {
            AppError::UpstreamUnavailable("audio stream context missing".to_string())
        })?;
        context.request_permit.take();
        let hold = context.hold.take().ok_or_else(|| {
            AppError::UpstreamUnavailable("audio billing hold missing".to_string())
        })?;
        let submission = bailian_asr::Submission {
            task_id: request_id
                .clone()
                .unwrap_or_else(|| format!("flash-{}", Uuid::new_v4())),
            status: "SUCCEEDED".to_string(),
            request_id,
            completed: Some(completed),
        };
        persist_submission(
            &context.state,
            &context.auth,
            &context.upstream,
            context.protocol,
            &context.external_model,
            &context.upstream_model,
            &submission,
            &hold,
            context.local_duration_seconds,
            ResponseFormat::Json,
            context.language,
            context.relay_trace_id,
            context.started_at,
        )
        .await?;
        Ok(frame)
    }

    async fn finish_error(&mut self, error: String) -> Bytes {
        if let Some(mut context) = self.context.take() {
            context.request_permit.take();
            if let Some(hold) = context.hold.take() {
                release_empty_hold(&context.state, hold, "Alibaba ASR stream failure").await;
            }
        }
        openai_stream_error(&error)
    }
}

impl Drop for AudioStreamRelay {
    fn drop(&mut self) {
        let Some(mut context) = self.context.take() else {
            return;
        };
        context.request_permit.take();
        if let Some(hold) = context.hold.take() {
            let state = context.state.clone();
            tokio::spawn(async move {
                release_empty_hold(&state, hold, "abandoned Alibaba ASR stream").await;
            });
        }
    }
}

#[derive(Default)]
struct AlibabaToOpenAiTranscriptionSse {
    buffer: Vec<u8>,
    emitted_text: String,
    final_text: String,
    sentences: Vec<Value>,
    sentence_ids: HashSet<i64>,
    duration_seconds: Option<f64>,
    request_id: Option<String>,
}

impl AlibabaToOpenAiTranscriptionSse {
    fn push(&mut self, chunk: &[u8]) -> AppResult<Bytes> {
        self.buffer.extend_from_slice(chunk);
        let mut output = Vec::new();
        while let Some((end, delimiter_len)) = sse_frame_boundary(&self.buffer) {
            let frame = self.buffer.drain(..end).collect::<Vec<_>>();
            self.buffer.drain(..delimiter_len);
            self.process_frame(&frame, &mut output)?;
        }
        Ok(Bytes::from(output))
    }

    fn finish(
        &mut self,
        local_duration_seconds: f64,
    ) -> AppResult<(Bytes, bailian_asr::CompletedSubmission, Option<String>)> {
        let mut output = Vec::new();
        if !self.buffer.is_empty() {
            let frame = std::mem::take(&mut self.buffer);
            self.process_frame(&frame, &mut output)?;
        }
        if self.final_text.is_empty() {
            self.final_text = self
                .sentences
                .iter()
                .filter_map(|sentence| sentence.get("text").and_then(Value::as_str))
                .collect::<String>();
        }
        if self.final_text.trim().is_empty() {
            return Err(AppError::UpstreamUnavailable(
                "Alibaba ASR stream returned no transcription text".to_string(),
            ));
        }
        if let Some(delta) = self.final_text.strip_prefix(&self.emitted_text) {
            if !delta.is_empty() {
                append_sse(
                    &mut output,
                    &json!({ "type": "transcript.text.delta", "delta": delta, "logprobs": [] }),
                );
            }
        }
        let duration = self
            .duration_seconds
            .unwrap_or(local_duration_seconds)
            .max(0.0);
        append_sse(
            &mut output,
            &json!({
                "type": "transcript.text.done",
                "text": self.final_text,
                "logprobs": [],
                "usage": { "type": "duration", "seconds": duration.ceil() as i64 },
            }),
        );
        let completed = bailian_asr::CompletedSubmission {
            text: self.final_text.clone(),
            duration_seconds: duration,
            duration_source: if self.duration_seconds.is_some() {
                "upstream"
            } else {
                "local_fallback"
            },
            details: Some(json!({
                "text": self.final_text,
                "sentences": self.sentences,
            })),
        };
        Ok((Bytes::from(output), completed, self.request_id.clone()))
    }

    fn process_frame(&mut self, frame: &[u8], output: &mut Vec<u8>) -> AppResult<()> {
        let text = String::from_utf8_lossy(frame);
        let data = text
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&data).map_err(|_| {
            AppError::UpstreamUnavailable("Alibaba ASR returned invalid SSE data".to_string())
        })?;
        if let Some(message) = value
            .get("message")
            .or_else(|| value.get("code"))
            .and_then(Value::as_str)
        {
            return Err(AppError::UpstreamUnavailable(message.to_string()));
        }
        if let Some(text) = value.pointer("/output/text").and_then(Value::as_str) {
            self.final_text = text.to_string();
        }
        if let Some(duration) = value.pointer("/usage/duration").and_then(Value::as_f64) {
            if duration.is_finite() && duration > 0.0 {
                self.duration_seconds = Some(duration);
            }
        }
        if let Some(request_id) = value.get("request_id").and_then(Value::as_str) {
            self.request_id = Some(request_id.to_string());
        }
        if let Some(delta) = self.final_text.strip_prefix(&self.emitted_text) {
            if !delta.is_empty() {
                append_sse(
                    output,
                    &json!({ "type": "transcript.text.delta", "delta": delta, "logprobs": [] }),
                );
                self.emitted_text = self.final_text.clone();
            }
        }
        let Some(sentence) = value.pointer("/output/sentence") else {
            return Ok(());
        };
        if !sentence
            .get("sentence_end")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Ok(());
        }
        let sentence_id = sentence
            .get("sentence_id")
            .and_then(Value::as_i64)
            .unwrap_or(self.sentences.len() as i64 + 1);
        if self.sentence_ids.insert(sentence_id) {
            self.sentences.push(sentence.clone());
        }
        Ok(())
    }
}

fn sse_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|window| window == b"\n\n");
    let crlf = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

fn append_sse(output: &mut Vec<u8>, value: &Value) {
    output.extend_from_slice(b"data: ");
    output.extend_from_slice(&serde_json::to_vec(value).expect("SSE value is serializable"));
    output.extend_from_slice(b"\n\n");
}

fn openai_stream_error(message: &str) -> Bytes {
    let mut output = Vec::new();
    append_sse(
        &mut output,
        &json!({
            "type": "error",
            "error": { "type": "server_error", "code": "upstream_error", "message": message },
        }),
    );
    Bytes::from(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDARY: &str = "neogate-audio-test-boundary";

    fn wav_one_second() -> Vec<u8> {
        let data_len = 16_000_u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8_000_u32.to_le_bytes());
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.resize(44 + data_len as usize, 0);
        wav
    }

    fn multipart_body_with_names(
        fields: &[(&str, &str)],
        files: &[(&str, &[u8])],
    ) -> (HeaderMap, Vec<u8>) {
        let mut body = Vec::new();
        for (name, value) in fields {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                    .as_bytes(),
            );
        }
        for (name, file) in files {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{name}\"; filename=\"ignored.wav\"\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
            body.extend_from_slice(file);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(&format!("multipart/form-data; boundary={BOUNDARY}")).unwrap(),
        );
        (headers, body)
    }

    fn multipart_body(fields: &[(&str, &str)], files: &[&[u8]]) -> (HeaderMap, Vec<u8>) {
        let files = files.iter().map(|file| ("file", *file)).collect::<Vec<_>>();
        multipart_body_with_names(fields, &files)
    }

    #[test]
    fn detects_supported_container_magic() {
        assert_eq!(detected_extension(b"RIFF1234WAVEdata").unwrap(), "wav");
        assert_eq!(detected_extension(b"fLaCdata").unwrap(), "flac");
        assert_eq!(detected_extension(b"ID3data").unwrap(), "mp3");
        assert_eq!(detected_extension(b"1234ftypM4A ").unwrap(), "m4a");
        assert_eq!(detected_extension(b"OggS-vorbis").unwrap(), "ogg");
        assert_eq!(detected_extension(b"OggS-OpusHead").unwrap(), "opus");
    }

    #[test]
    fn validates_language_codes() {
        assert_eq!(normalize_language("ZH-cn".to_string()).unwrap(), "zh-cn");
        assert!(normalize_language("zh_cn".to_string()).is_err());
    }

    #[test]
    fn parses_audio_defaults_and_one_second_wav() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body(&[("model", "fun-asr-flash-2026-06-15")], &[&wav]);
        let request = parse_request(&headers, &body).unwrap();
        assert_eq!(request.model, "fun-asr-flash-2026-06-15");
        assert_eq!(request.response_format, ResponseFormat::Json);
        assert_eq!(request.extension, "wav");
        assert!((request.duration_seconds - 1.0).abs() < 0.001);
    }

    #[test]
    fn parses_text_format_and_language_for_paraformer() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body(
            &[
                ("model", "paraformer-v2"),
                ("response_format", "text"),
                ("language", "ZH-cn"),
                ("temperature", "0"),
                ("prompt", ""),
                ("stream", "false"),
            ],
            &[&wav],
        );
        let request = parse_request(&headers, &body).unwrap();
        assert_eq!(request.response_format, ResponseFormat::Text);
        assert_eq!(request.language.as_deref(), Some("zh-cn"));
    }

    #[test]
    fn rejects_duplicate_fields_files_and_empty_file() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body(
            &[
                ("model", "fun-asr-flash-2026-06-15"),
                ("model", "paraformer-v2"),
            ],
            &[&wav],
        );
        assert!(parse_request(&headers, &body).is_err());

        let (headers, body) =
            multipart_body(&[("model", "fun-asr-flash-2026-06-15")], &[&wav, &wav]);
        assert!(parse_request(&headers, &body).is_err());

        let (headers, body) = multipart_body(&[("model", "fun-asr-flash-2026-06-15")], &[b""]);
        assert!(parse_request(&headers, &body).is_err());
    }

    #[test]
    fn parses_repeated_array_fields_and_context() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body(
            &[
                ("model", "fun-asr-flash-2026-06-15"),
                ("response_format", "verbose_json"),
                ("prompt", "Project NeoGate"),
                ("keywords[]", "DashScope"),
                ("keywords[]", "Fun-ASR"),
                ("languages[]", "zh-CN"),
                ("languages[]", "en"),
                ("timestamp_granularities[]", "word"),
                ("timestamp_granularities[]", "segment"),
            ],
            &[&wav],
        );
        let request = parse_request(&headers, &body).unwrap();
        assert_eq!(request.keywords, ["DashScope", "Fun-ASR"]);
        assert_eq!(request.languages, ["zh-cn", "en"]);
        assert_eq!(request.timestamp_granularities, ["word", "segment"]);
        assert_eq!(
            transcription_context(&request).unwrap().as_deref(),
            Some("Project NeoGate\nDashScope, Fun-ASR\nExpected languages: zh-cn, en")
        );
        validate_request_for_api(&request, AudioTranscriptionApi::MultimodalGeneration).unwrap();
    }

    #[test]
    fn recognizes_speaker_reference_file_before_rejecting_diarization() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body_with_names(
            &[
                ("model", "fun-asr-flash-2026-06-15"),
                ("response_format", "diarized_json"),
                ("known_speaker_names[]", "Alice"),
            ],
            &[("file", &wav), ("known_speaker_references[]", &wav)],
        );
        let request = parse_request(&headers, &body).unwrap();
        assert_eq!(request.known_speaker_references.len(), 1);
        assert!(
            validate_request_for_api(&request, AudioTranscriptionApi::MultimodalGeneration)
                .is_err()
        );
    }

    #[test]
    fn enforces_flash_duration_base64_size_and_context_limits() {
        let wav = wav_one_second();
        let (headers, body) = multipart_body(&[("model", "fun-asr-flash-2026-06-15")], &[&wav]);
        let mut request = parse_request(&headers, &body).unwrap();
        request.duration_seconds = FLASH_MAX_AUDIO_DURATION_SECONDS + 0.001;
        assert!(
            validate_request_for_api(&request, AudioTranscriptionApi::MultimodalGeneration)
                .is_err()
        );

        request.duration_seconds = 1.0;
        request.audio = Bytes::from(vec![0; FLASH_MAX_BASE64_AUDIO_BYTES / 4 * 3 + 1]);
        assert!(matches!(
            validate_request_for_api(&request, AudioTranscriptionApi::MultimodalGeneration),
            Err(AppError::PayloadTooLarge(_))
        ));

        request.audio = Bytes::from_static(b"audio");
        request.prompt = Some("x".repeat(FLASH_CONTEXT_MAX_CHARS + 1));
        assert!(transcription_context(&request).is_err());
    }

    #[test]
    fn converts_fragmented_sse_with_sentence_and_word_timestamps() {
        let mut converter = AlibabaToOpenAiTranscriptionSse::default();
        let event = json!({
            "request_id": "req-1",
            "output": {
                "text": "hello",
                "sentence": {
                    "sentence_id": 1,
                    "sentence_end": true,
                    "begin_time": 0,
                    "end_time": 500,
                    "text": "hello",
                    "words": [{"text": "hello", "begin_time": 0, "end_time": 500}]
                }
            },
            "usage": {"duration": 1.25}
        });
        let mut frame = Vec::new();
        append_sse(&mut frame, &event);
        let split = frame.len() / 2;
        assert!(converter.push(&frame[..split]).unwrap().is_empty());
        let delta = String::from_utf8(converter.push(&frame[split..]).unwrap().to_vec()).unwrap();
        assert!(delta.contains("transcript.text.delta"));
        assert!(delta.contains("hello"));

        let (done, completed, request_id) = converter.finish(1.0).unwrap();
        assert!(String::from_utf8(done.to_vec())
            .unwrap()
            .contains("transcript.text.done"));
        assert_eq!(completed.text, "hello");
        assert_eq!(completed.duration_seconds, 1.25);
        assert_eq!(
            completed
                .details
                .as_ref()
                .and_then(|value| value.pointer("/sentences/0/words/0/text"))
                .and_then(Value::as_str),
            Some("hello")
        );
        assert_eq!(request_id.as_deref(), Some("req-1"));
    }

    #[tokio::test]
    async fn renders_openai_response_formats_and_timestamp_granularities() {
        let metadata = json!({
            "result": {
                "text": "hello world",
                "duration_seconds": 1.25,
                "details": {
                    "sentences": [{
                        "sentence_id": 1,
                        "begin_time": 0,
                        "end_time": 1250,
                        "text": "hello world",
                        "words": [{"text": "hello", "punctuation": " ", "begin_time": 0, "end_time": 500}]
                    }]
                }
            }
        });
        for format in [
            ResponseFormat::Json,
            ResponseFormat::Text,
            ResponseFormat::Srt,
            ResponseFormat::Vtt,
        ] {
            let response = task_response(&metadata, format, "completed", Some("en"), &[]).unwrap();
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert!(!body.is_empty(), "empty {} response", format.as_str());
        }

        let response = task_response(
            &metadata,
            ResponseFormat::VerboseJson,
            "completed",
            Some("en"),
            &["word".to_string()],
        )
        .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert!(value.get("words").is_some());
        assert!(value.get("segments").is_none());
        assert!(task_response(
            &metadata,
            ResponseFormat::DiarizedJson,
            "completed",
            Some("en"),
            &[],
        )
        .is_err());
    }
}
