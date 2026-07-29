use base64::{engine::general_purpose::STANDARD, Engine as _};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;
use reqwest::{multipart, RequestBuilder, Response, Url};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    config::UPSTREAM_TIMEOUT,
    error::{AppError, AppResult, UpstreamErrorKind, UpstreamRequestError},
    relay::selector::SelectedUpstream,
    AppState,
};

const POLICY_BASE_URL: &str =
    "https://dashscope.aliyuncs.com/api/v1/uploads?action=getPolicy&model=";
const WORKSPACE_HOST_SUFFIX: &str = ".cn-beijing.maas.aliyuncs.com";

#[derive(Debug, Clone)]
pub(crate) struct Submission {
    pub task_id: String,
    pub status: String,
    pub request_id: Option<String>,
    pub completed: Option<CompletedSubmission>,
}

#[derive(Debug, Clone)]
pub(crate) struct CompletedSubmission {
    pub text: String,
    pub duration_seconds: f64,
    pub duration_source: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) enum PollResult {
    Pending {
        status: String,
        request_id: Option<String>,
    },
    Completed {
        text: String,
        duration_seconds: f64,
        duration_source: &'static str,
        request_id: Option<String>,
    },
    Failed {
        status: String,
        message: String,
        request_id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct PolicyEnvelope {
    data: UploadPolicy,
}

#[derive(Debug, Deserialize)]
struct UploadPolicy {
    policy: String,
    signature: String,
    upload_dir: String,
    upload_host: String,
    #[serde(alias = "OSSAccessKeyId", alias = "accessid")]
    oss_access_key_id: String,
}

pub(crate) async fn upload_and_submit(
    state: &AppState,
    upstream: &SelectedUpstream,
    model: &str,
    audio: Bytes,
    extension: &str,
    language: Option<&str>,
) -> AppResult<Submission> {
    ensure_asr_upstream(upstream)?;
    let policy_response = send(
        upstream,
        state
            .http
            .get(format!("{POLICY_BASE_URL}{model}"))
            .bearer_auth(&upstream.secret),
    )
    .await?;
    let policy_body = successful_body(state, upstream, policy_response, "upload policy").await?;
    let policy: PolicyEnvelope = serde_json::from_slice(&policy_body).map_err(|_| {
        AppError::UpstreamUnavailable("Alibaba ASR upload policy response was invalid".to_string())
    })?;
    validate_aliyun_url(&policy.data.upload_host)?;

    let filename = format!("{}.{}", Uuid::new_v4(), extension);
    let object_key = format!(
        "{}/{}",
        policy.data.upload_dir.trim_end_matches('/'),
        filename
    );
    let part = multipart::Part::bytes(audio.to_vec()).file_name(filename);
    let form = multipart::Form::new()
        .text("OSSAccessKeyId", policy.data.oss_access_key_id)
        .text("Signature", policy.data.signature)
        .text("policy", policy.data.policy)
        .text("key", object_key.clone())
        .text("x-oss-object-acl", "private")
        .text("x-oss-forbid-overwrite", "true")
        .text("success_action_status", "200")
        .part("file", part);
    let upload_response = send(
        upstream,
        state.http.post(&policy.data.upload_host).multipart(form),
    )
    .await?;
    successful_body(state, upstream, upload_response, "temporary OSS upload").await?;

    let mut parameters = serde_json::Map::new();
    if let Some(language) = language {
        parameters.insert("language_hints".to_string(), json!([language]));
    }
    let submit_response = send(
        upstream,
        state
            .http
            .post(native_url(
                &upstream.base_url,
                "/services/audio/asr/transcription",
            )?)
            .bearer_auth(&upstream.secret)
            .header("X-DashScope-Async", "enable")
            .header("X-DashScope-OssResourceResolve", "enable")
            .json(&json!({
                "model": model,
                "input": { "file_urls": [format!("oss://{object_key}")] },
                "parameters": parameters,
            })),
    )
    .await?;
    let submit_body = successful_body(state, upstream, submit_response, "task submission").await?;
    let value: Value = serde_json::from_slice(&submit_body).map_err(|_| {
        AppError::UpstreamUnavailable("Alibaba ASR submission response was invalid".to_string())
    })?;
    let task_id = value
        .pointer("/output/task_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            AppError::UpstreamUnavailable(
                "Alibaba ASR submission response missing task_id".to_string(),
            )
        })?;
    Ok(Submission {
        task_id: task_id.to_string(),
        status: task_status(&value, "PENDING"),
        request_id: request_id(&value),
        completed: None,
    })
}

pub(crate) async fn transcribe_multimodal(
    state: &AppState,
    upstream: &SelectedUpstream,
    model: &str,
    audio: Bytes,
    extension: &str,
    local_duration_seconds: f64,
) -> AppResult<Submission> {
    ensure_asr_upstream(upstream)?;
    let media_type = match extension {
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "m4a" => "audio/mp4",
        "ogg" => "audio/ogg",
        _ => return Err(AppError::BadRequest("unsupported audio format".to_string())),
    };
    let data_uri = format!("data:{media_type};base64,{}", STANDARD.encode(audio));
    let response = send(
        upstream,
        state
            .http
            .post(native_url(
                &upstream.base_url,
                "/services/aigc/multimodal-generation/generation",
            )?)
            .bearer_auth(&upstream.secret)
            .header("X-DashScope-SSE", "disable")
            .json(&json!({
                "model": model,
                "input": {
                    "messages": [{
                        "role": "user",
                        "content": [{
                            "type": "input_audio",
                            "input_audio": { "data": data_uri },
                        }],
                    }],
                },
                "parameters": { "format": extension },
            })),
    )
    .await?;
    let body = successful_body(state, upstream, response, "multimodal transcription").await?;
    let value: Value = serde_json::from_slice(&body).map_err(|_| {
        AppError::UpstreamUnavailable("Alibaba ASR multimodal response was invalid".to_string())
    })?;
    let text = value
        .pointer("/output/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            AppError::UpstreamUnavailable(
                "Alibaba ASR multimodal response missing text".to_string(),
            )
        })?;
    let (duration_seconds, duration_source) = value
        .pointer("/usage/duration")
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map_or((local_duration_seconds, "local_fallback"), |duration| {
            (duration, "upstream")
        });
    let request_id = request_id(&value);
    Ok(Submission {
        task_id: request_id
            .clone()
            .unwrap_or_else(|| format!("flash-{}", Uuid::new_v4())),
        status: "SUCCEEDED".to_string(),
        request_id,
        completed: Some(CompletedSubmission {
            text: text.to_string(),
            duration_seconds,
            duration_source,
        }),
    })
}

pub(crate) async fn poll(
    state: &AppState,
    upstream: &SelectedUpstream,
    _model: &str,
    task_id: &str,
    local_duration_seconds: f64,
) -> AppResult<PollResult> {
    ensure_asr_upstream(upstream)?;
    let response = send(
        upstream,
        state
            .http
            .get(native_url(
                &upstream.base_url,
                &format!("/tasks/{task_id}"),
            )?)
            .bearer_auth(&upstream.secret),
    )
    .await?;
    let status_code = response.status();
    if !status_code.is_success() {
        if status_code.as_u16() == 429 || status_code.is_server_error() {
            return Err(retryable_http_error(upstream, "task poll", status_code));
        }
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: format!(
                "Alibaba ASR task poll returned HTTP {}",
                status_code.as_u16()
            ),
            request_id: None,
        });
    }
    let body = read_bounded(state, response).await?;
    let value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            return Ok(PollResult::Failed {
                status: "FAILED".to_string(),
                message: "Alibaba ASR task response was invalid".to_string(),
                request_id: None,
            })
        }
    };
    let status = task_status(&value, "PENDING");
    let request_id = request_id(&value);
    if matches!(status.as_str(), "PENDING" | "RUNNING") {
        return Ok(PollResult::Pending { status, request_id });
    }
    if !matches!(status.as_str(), "SUCCEEDED" | "SUCCESS") {
        return Ok(PollResult::Failed {
            status,
            message: task_message(&value),
            request_id,
        });
    }
    let Some(result) = value.pointer("/output/results/0") else {
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: "Alibaba ASR task result was missing".to_string(),
            request_id,
        });
    };
    let subtask_status = result
        .get("subtask_status")
        .and_then(Value::as_str)
        .unwrap_or("SUCCEEDED")
        .to_ascii_uppercase();
    if !matches!(subtask_status.as_str(), "SUCCEEDED" | "SUCCESS") {
        return Ok(PollResult::Failed {
            status: subtask_status,
            message: task_message(result),
            request_id,
        });
    }
    let Some(transcription_url) = result.get("transcription_url").and_then(Value::as_str) else {
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: "Alibaba ASR result URL was missing".to_string(),
            request_id,
        });
    };
    if validate_aliyun_url(transcription_url).is_err() {
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: "Alibaba ASR returned an untrusted result URL".to_string(),
            request_id,
        });
    }
    let result_response = send(upstream, state.http.get(transcription_url)).await?;
    let result_status = result_response.status();
    if !result_status.is_success() {
        if result_status.as_u16() == 429 || result_status.is_server_error() {
            return Err(retryable_http_error(
                upstream,
                "result download",
                result_status,
            ));
        }
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: format!(
                "Alibaba ASR result download returned HTTP {}",
                result_status.as_u16()
            ),
            request_id,
        });
    }
    let result_body = match read_bounded(state, result_response).await {
        Ok(body) => body,
        Err(AppError::PayloadTooLarge(_)) => {
            return Ok(PollResult::Failed {
                status: "FAILED".to_string(),
                message: "Alibaba ASR transcription result exceeded the relay body limit"
                    .to_string(),
                request_id,
            })
        }
        Err(err) => return Err(err),
    };
    let transcription: Value = match serde_json::from_slice(&result_body) {
        Ok(value) => value,
        Err(_) => {
            return Ok(PollResult::Failed {
                status: "FAILED".to_string(),
                message: "Alibaba ASR transcription result was invalid".to_string(),
                request_id,
            })
        }
    };
    let Some(text) = transcription_text(&transcription) else {
        return Ok(PollResult::Failed {
            status: "FAILED".to_string(),
            message: "Alibaba ASR transcription result missing text".to_string(),
            request_id,
        });
    };
    let upstream_duration = duration_seconds(&value).or_else(|| duration_seconds(&transcription));
    let (duration_seconds, duration_source) = upstream_duration
        .filter(|duration| duration.is_finite() && *duration > 0.0 && *duration <= 43_200.0)
        .map_or((local_duration_seconds, "local_fallback"), |duration| {
            (duration, "upstream")
        });
    Ok(PollResult::Completed {
        text,
        duration_seconds,
        duration_source,
        request_id,
    })
}

fn ensure_asr_upstream(upstream: &SelectedUpstream) -> AppResult<()> {
    if !upstream.provider.eq_ignore_ascii_case("qwen") {
        return Err(AppError::BadRequest(
            "Alibaba ASR requires a qwen channel".to_string(),
        ));
    }
    if upstream.channel_key_id.is_none() || upstream.credential_id.is_some() {
        return Err(AppError::BadRequest(
            "Alibaba ASR requires a key-backed qwen channel".to_string(),
        ));
    }
    let url = Url::parse(&upstream.base_url)
        .map_err(|_| AppError::BadRequest("invalid Alibaba ASR endpoint URL".to_string()))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let trusted_dashscope_host =
        host == "dashscope.aliyuncs.com" || host.ends_with(WORKSPACE_HOST_SUFFIX);
    if url.scheme() != "https" || !trusted_dashscope_host {
        return Err(AppError::BadRequest(
            "Alibaba ASR endpoint must use a trusted DashScope host".to_string(),
        ));
    }
    Ok(())
}

fn native_url(base_url: &str, path: &str) -> AppResult<String> {
    let base = base_url.trim_end_matches('/');
    let base = base
        .strip_suffix("/compatible-mode/v1")
        .or_else(|| base.strip_suffix("/compatible-mode"))
        .or_else(|| base.strip_suffix("/api/v1"))
        .unwrap_or(base);
    Ok(format!("{base}/api/v1/{}", path.trim_start_matches('/')))
}

fn validate_aliyun_url(value: &str) -> AppResult<()> {
    let url = Url::parse(value)
        .map_err(|_| AppError::UpstreamUnavailable("invalid Alibaba Cloud URL".to_string()))?;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    if url.scheme() != "https" || !(host == "aliyuncs.com" || host.ends_with(".aliyuncs.com")) {
        return Err(AppError::UpstreamUnavailable(
            "Alibaba Cloud returned an untrusted URL".to_string(),
        ));
    }
    Ok(())
}

async fn send(upstream: &SelectedUpstream, request: RequestBuilder) -> AppResult<Response> {
    match tokio::time::timeout(UPSTREAM_TIMEOUT, request.send()).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(err)) => Err(AppError::UpstreamRequest(
            UpstreamRequestError::from_reqwest(upstream.provider.clone(), &err),
        )),
        Err(_) => Err(AppError::UpstreamRequest(UpstreamRequestError::new(
            UpstreamErrorKind::Timeout,
            upstream.provider.clone(),
            "Alibaba ASR request timed out",
        ))),
    }
}

async fn successful_body(
    state: &AppState,
    upstream: &SelectedUpstream,
    response: Response,
    operation: &str,
) -> AppResult<Bytes> {
    let status = response.status();
    if !status.is_success() {
        if status.as_u16() == 429 || status.is_server_error() {
            return Err(retryable_http_error(upstream, operation, status));
        }
        let body = read_bounded(state, response).await.unwrap_or_default();
        let code = safe_upstream_error_code(&body)
            .map(|code| format!(" ({code})"))
            .unwrap_or_default();
        return Err(AppError::BadRequest(format!(
            "Alibaba ASR {operation} was rejected with HTTP {}{code}",
            status.as_u16(),
        )));
    }
    read_bounded(state, response).await
}

fn safe_upstream_error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    ["/code", "/error/code", "/output/code"]
        .into_iter()
        .find_map(|pointer| value.pointer(pointer).and_then(Value::as_str))
        .filter(|code| {
            !code.is_empty()
                && code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
        .map(str::to_string)
}

fn retryable_http_error(
    upstream: &SelectedUpstream,
    operation: &str,
    status: reqwest::StatusCode,
) -> AppError {
    AppError::UpstreamRequest(UpstreamRequestError::new(
        UpstreamErrorKind::Request,
        upstream.provider.clone(),
        format!("Alibaba ASR {operation} returned HTTP {}", status.as_u16()),
    ))
}

async fn read_bounded(state: &AppState, response: Response) -> AppResult<Bytes> {
    let limit = state.config.relay.body_limit_bytes;
    let mut output = BytesMut::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if output.len().saturating_add(chunk.len()) > limit {
            return Err(AppError::PayloadTooLarge(
                "Alibaba ASR response exceeded the relay body limit".to_string(),
            ));
        }
        output.extend_from_slice(&chunk);
    }
    Ok(output.freeze())
}

fn task_status(value: &Value, fallback: &str) -> String {
    value
        .pointer("/output/task_status")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_ascii_uppercase()
}

fn request_id(value: &Value) -> Option<String> {
    value
        .get("request_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn task_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("Alibaba ASR task failed")
        .to_string()
}

fn transcription_text(value: &Value) -> Option<String> {
    let transcripts = value
        .get("transcripts")
        .or_else(|| value.pointer("/output/transcripts"))?
        .as_array()?;
    let parts: Vec<&str> = transcripts
        .iter()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .filter(|text| !text.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn duration_seconds(value: &Value) -> Option<f64> {
    value
        .pointer("/usage/duration")
        .or_else(|| value.get("duration"))
        .and_then(Value::as_f64)
        .or_else(|| {
            value
                .pointer("/properties/original_duration_in_milliseconds")
                .and_then(Value::as_f64)
                .map(|duration| duration / 1000.0)
        })
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
            channel_name: "bailian".to_string(),
            base_url: base_url.to_string(),
            responses_chat_fallback: false,
            secret: "sk-test".to_string(),
            account_id: None,
            adapter_hint: None,
            affinity: None,
        }
    }

    #[test]
    fn extracts_transcript_text_and_duration() {
        let value = json!({
            "properties": { "original_duration_in_milliseconds": 1250 },
            "transcripts": [{"text": "one"}, {"text": "two"}]
        });
        assert_eq!(transcription_text(&value).as_deref(), Some("one\ntwo"));
        assert_eq!(duration_seconds(&value), Some(1.25));
    }

    #[test]
    fn rejects_non_aliyun_urls() {
        assert!(validate_aliyun_url("https://example.com/result.json").is_err());
        assert!(validate_aliyun_url("https://bucket.oss-cn-beijing.aliyuncs.com/a").is_ok());
    }

    #[test]
    fn extracts_only_safe_upstream_error_codes() {
        assert_eq!(
            safe_upstream_error_code(br#"{"code":"InvalidParameter"}"#).as_deref(),
            Some("InvalidParameter")
        );
        assert_eq!(
            safe_upstream_error_code(br#"{"error":{"code":"Bad_Request-1"}}"#).as_deref(),
            Some("Bad_Request-1")
        );
        assert_eq!(
            safe_upstream_error_code(br#"{"code":"unsafe value"}"#),
            None
        );
    }

    #[test]
    fn accepts_trusted_dashscope_endpoints() {
        let public = upstream("https://dashscope.aliyuncs.com/compatible-mode/v1");
        let workspace =
            upstream("https://workspace-id.cn-beijing.maas.aliyuncs.com/compatible-mode/v1");
        assert!(ensure_asr_upstream(&public).is_ok());
        assert!(ensure_asr_upstream(&workspace).is_ok());
    }

    #[test]
    fn native_urls_strip_compatible_mode_suffixes() {
        assert_eq!(
            native_url(
                "https://dashscope.aliyuncs.com/compatible-mode/v1/",
                "/tasks/task-1"
            )
            .unwrap(),
            "https://dashscope.aliyuncs.com/api/v1/tasks/task-1"
        );
        assert_eq!(
            native_url(
                "https://workspace-id.cn-beijing.maas.aliyuncs.com/api/v1",
                "/services/audio/asr/transcription"
            )
            .unwrap(),
            "https://workspace-id.cn-beijing.maas.aliyuncs.com/api/v1/services/audio/asr/transcription"
        );
    }
}
