use std::{collections::HashMap, io::Cursor, sync::Arc, time::Instant};

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use bytes::Bytes;
use chrono::Utc;
use serde_json::{json, Value};
use symphonia::core::{
    formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::BillingMeter,
    error::{AppError, AppResult},
    provider::adapters::bailian_asr,
    relay::{release_empty_hold, reserve_billable_credit, selector::AttemptedUpstream, RelayBody},
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseFormat {
    Json,
    Text,
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
    audio: Bytes,
    extension: &'static str,
    duration_seconds: f64,
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
    let _request_permit = state.user_request_limiter.try_acquire(auth.user_id).await?;
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
        if transcription_api == AudioTranscriptionApi::MultimodalGeneration
            && request.language.is_some()
        {
            return Err(unsupported_option("language"));
        }
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

    let inline_result = submission.completed.as_ref().map(|completed| {
        json!({
            "text": completed.text,
            "duration_seconds": completed.duration_seconds,
            "duration_source": completed.duration_source,
            "request_id": submission.request_id,
        })
    });
    let task_metadata = json!({
        "neogate": {
            "local_duration_seconds": request.duration_seconds,
            "response_format": match request.response_format {
                ResponseFormat::Json => "json",
                ResponseFormat::Text => "text",
            },
            "relay_started_at": Utc::now().to_rfc3339(),
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
            auth: &auth,
            protocol,
            upstream: &upstream,
            model: Some(&resolved.external_model),
            upstream_model: Some(&resolved.target_model),
            status: &submission.status,
            terminal: false,
            hold: &hold,
            upstream_metadata: task_metadata,
        },
        crate::task::POLL_INTERVAL,
        crate::task::AUDIO_TASK_RETENTION,
    )
    .await
    {
        tracing::error!(
            upstream_task_id = %submission.task_id,
            channel_id = upstream.channel_id,
            "Alibaba ASR task was accepted upstream but could not be persisted"
        );
        release_empty_hold(&state, hold, "Alibaba ASR task persistence failure").await;
        return Err(err);
    }
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
            );
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
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
    for (name, value) in multipart_text_fields(body, &boundary)? {
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
                | "timestamp_granularities[]"
                | "diarization"
                | "speaker_labels"
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
    let response_format = match fields
        .remove("response_format")
        .unwrap_or_else(|| "json".to_string())
        .as_str()
    {
        "json" => ResponseFormat::Json,
        "text" => ResponseFormat::Text,
        _ => {
            return Err(AppError::BadRequestWithCode {
                code: "unsupported_response_format",
                message: "response_format must be json or text",
            })
        }
    };
    if fields.get("prompt").is_some_and(|value| !value.is_empty()) {
        return Err(unsupported_option("prompt"));
    }
    if fields.get("stream").is_some_and(|value| value != "false") {
        return Err(unsupported_option("stream"));
    }
    if let Some(value) = fields.get("temperature") {
        let temperature = value
            .parse::<f64>()
            .map_err(|_| AppError::BadRequest("temperature must be a number".to_string()))?;
        if !temperature.is_finite() || temperature != 0.0 {
            return Err(unsupported_option("temperature"));
        }
    }
    for field in ["timestamp_granularities[]", "diarization", "speaker_labels"] {
        if fields.contains_key(field) {
            return Err(unsupported_option(field));
        }
    }
    let language = fields
        .remove("language")
        .filter(|value| !value.is_empty())
        .map(normalize_language)
        .transpose()?;
    let mut files = multipart_files(body, &boundary)?;
    if files.len() != 1 || files[0].name != "file" {
        return Err(AppError::BadRequest(
            "exactly one file field is required".to_string(),
        ));
    }
    let audio = files.remove(0).data;
    if audio.is_empty() {
        return Err(AppError::BadRequest("file must not be empty".to_string()));
    }
    let extension = detected_extension(&audio)?;
    let duration_seconds = audio_duration(&audio, extension)?;
    Ok(AudioRequest {
        model,
        response_format,
        language,
        audio,
        extension,
        duration_seconds,
    })
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

async fn ensure_audio_transcription_capability(
    pool: &sqlx::PgPool,
    provider: &str,
    model: &str,
) -> AppResult<AudioTranscriptionApi> {
    let api = sqlx::query_scalar::<_, String>(
        "SELECT capabilities->>'audio_transcription_api'
         FROM provider_model
         WHERE lower(provider) = lower($1)
           AND lower(model) = lower($2)
           AND capabilities @> '{\"audio_transcription\": true}'::JSONB
         LIMIT 1",
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(pool)
    .await?;
    match api.as_deref() {
        Some("async_file") => Ok(AudioTranscriptionApi::AsyncFile),
        Some("multimodal_generation") => Ok(AudioTranscriptionApi::MultimodalGeneration),
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
            return Err(unsupported_audio_format());
        }
        return Ok("ogg");
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

fn audio_duration(data: &[u8], extension: &str) -> AppResult<f64> {
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
    Ok(duration)
}

fn unsupported_audio_format() -> AppError {
    AppError::BadRequestWithCode {
        code: "unsupported_audio_format",
        message: "supported audio formats are MP3, WAV, FLAC, M4A/MP4 AAC, and OGG/Vorbis",
    }
}

fn ceil_duration(duration: f64) -> AppResult<i64> {
    if !duration.is_finite() || duration <= 0.0 || duration > MAX_AUDIO_DURATION_SECONDS {
        return Err(AppError::BadRequest("invalid audio duration".to_string()));
    }
    Ok(duration.ceil() as i64)
}

fn task_response(metadata: &Value, format: ResponseFormat, status: &str) -> AppResult<Response> {
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
    match format {
        ResponseFormat::Json => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )
            .body(Body::from(serde_json::to_vec(&json!({ "text": text }))?))
            .map_err(|err| AppError::BadRequest(err.to_string())),
        ResponseFormat::Text => Response::builder()
            .status(StatusCode::OK)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )
            .body(Body::from(text.to_string()))
            .map_err(|err| AppError::BadRequest(err.to_string())),
    }
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

    fn multipart_body(fields: &[(&str, &str)], files: &[&[u8]]) -> (HeaderMap, Vec<u8>) {
        let mut body = Vec::new();
        for (name, value) in fields {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                    .as_bytes(),
            );
        }
        for file in files {
            body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
            body.extend_from_slice(
                b"Content-Disposition: form-data; name=\"file\"; filename=\"ignored.wav\"\r\n",
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

    #[test]
    fn detects_supported_container_magic() {
        assert_eq!(detected_extension(b"RIFF1234WAVEdata").unwrap(), "wav");
        assert_eq!(detected_extension(b"fLaCdata").unwrap(), "flac");
        assert_eq!(detected_extension(b"ID3data").unwrap(), "mp3");
        assert_eq!(detected_extension(b"1234ftypM4A ").unwrap(), "m4a");
        assert_eq!(detected_extension(b"OggS-vorbis").unwrap(), "ogg");
        assert!(detected_extension(b"OggS-OpusHead").is_err());
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
    fn rejects_unsupported_openai_options() {
        for (name, value) in [
            ("response_format", "verbose_json"),
            ("temperature", "0.1"),
            ("prompt", "context"),
            ("stream", "true"),
            ("timestamp_granularities[]", "word"),
        ] {
            let (headers, body) =
                multipart_body(&[("model", "fun-asr-flash-2026-06-15"), (name, value)], &[]);
            assert!(parse_request(&headers, &body).is_err(), "accepted {name}");
        }
    }
}
