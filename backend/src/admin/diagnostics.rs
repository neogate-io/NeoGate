use std::time::Instant;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    config::DEFAULT_ANTHROPIC_VERSION,
    error::{AppError, AppResult, UpstreamErrorKind},
    id::DbId,
    input::trimmed_non_empty,
    provider::adapters::{adapter_for_provider, RelayRoute},
    relay::upstream_url,
    AppState,
};

use super::credentials::runtime_secret_from_enabled_credential;
use super::provider::OPENAI_OAUTH_PROTOCOL;

const DIAGNOSTIC_COOLDOWN_MINUTES: i64 = 5;

#[derive(Debug, Clone, Serialize)]
pub struct ChannelDiagnosticReport {
    pub channel_id: DbId,
    pub channel_name: String,
    pub provider: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub endpoints: Vec<EndpointDiagnosticReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelDiagnosticEvent {
    Started {
        channel_id: DbId,
        channel_name: String,
        provider: String,
    },
    ModelStarted {
        endpoint_id: DbId,
        protocol: String,
        base_url: String,
        key_id: Option<DbId>,
        key_name: String,
        key_prefix: Option<String>,
        model: String,
    },
    ModelResult {
        endpoint_id: DbId,
        protocol: String,
        base_url: String,
        key_id: Option<DbId>,
        key_name: String,
        key_prefix: Option<String>,
        model: String,
        step: DiagnosticStep,
    },
    Finished {
        report: ChannelDiagnosticReport,
    },
    Error {
        message: String,
    },
}

impl ChannelDiagnosticEvent {
    pub fn event_name(&self) -> &'static str {
        match self {
            Self::Started { .. } => "started",
            Self::ModelStarted { .. } => "model_started",
            Self::ModelResult { .. } => "model_result",
            Self::Finished { .. } => "finished",
            Self::Error { .. } => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelProbeSampleRecord {
    pub status: ProbeStatus,
    pub latency_ms: Option<i64>,
    pub status_code: Option<i32>,
    pub model: String,
    pub error_summary: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProbeStatus {
    Ok,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStatus {
    Ok,
    Warning,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct EndpointDiagnosticReport {
    pub endpoint_id: DbId,
    pub protocol: String,
    pub base_url: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub discovered_models: Vec<String>,
    pub configured_models: Vec<String>,
    pub missing_configured_models: Vec<String>,
    pub keys: Vec<KeyDiagnosticReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyDiagnosticReport {
    pub key_id: Option<DbId>,
    pub key_name: String,
    pub key_prefix: Option<String>,
    pub status: DiagnosticStatus,
    pub summary: String,
    pub discovered_models: Vec<String>,
    pub steps: Vec<DiagnosticStep>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticStep {
    pub name: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub duration_ms: i64,
    pub status_code: Option<u16>,
}

struct ChannelDiagnosticTarget {
    id: DbId,
    provider: String,
    name: String,
    enabled: bool,
    use_credentials: bool,
}

struct EndpointTarget {
    id: DbId,
    provider: String,
    protocol: String,
    base_url: String,
    models: Vec<String>,
    enabled: bool,
}

struct KeyTarget {
    id: Option<DbId>,
    name: String,
    key_prefix: Option<String>,
    secret: String,
    enabled: bool,
}

pub async fn diagnose_channel(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<ChannelDiagnosticReport> {
    diagnose_channel_with_progress(state, channel_id, None).await
}

pub async fn diagnose_channel_with_progress(
    state: &AppState,
    channel_id: DbId,
    progress: Option<tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
) -> AppResult<ChannelDiagnosticReport> {
    let started = Instant::now();
    let started_at = Utc::now();
    let channel = load_channel(state, channel_id).await?;
    let endpoints = load_endpoints(state, channel_id).await?;
    let keys = load_keys(state, &channel).await?;
    let mut endpoint_reports = Vec::new();

    send_diagnostic_event(
        &progress,
        ChannelDiagnosticEvent::Started {
            channel_id,
            channel_name: channel.name.clone(),
            provider: channel.provider.clone(),
        },
    );

    if let Some(endpoint) = select_diagnostic_endpoint(&endpoints) {
        let report = diagnose_endpoint(state, &channel, endpoint, &keys, progress.as_ref()).await;
        if report.status != DiagnosticStatus::Skipped {
            persist_endpoint_health(state, endpoint.id, report.status, &report.summary).await?;
        }
        endpoint_reports.push(report);
    }

    let finished_at = Utc::now();
    let status = aggregate_status(endpoint_reports.iter().map(|item| item.status));
    let summary = channel_summary(&channel, &endpoint_reports, status);
    let report = ChannelDiagnosticReport {
        channel_id,
        channel_name: channel.name,
        provider: channel.provider,
        status,
        summary,
        started_at,
        finished_at,
        duration_ms: started.elapsed().as_millis() as i64,
        endpoints: endpoint_reports,
    };
    send_diagnostic_event(
        &progress,
        ChannelDiagnosticEvent::Finished {
            report: report.clone(),
        },
    );
    Ok(report)
}

fn send_diagnostic_event(
    progress: &Option<tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
    event: ChannelDiagnosticEvent,
) {
    if let Some(progress) = progress {
        let _ = progress.send(event);
    }
}

fn select_diagnostic_endpoint(endpoints: &[EndpointTarget]) -> Option<&EndpointTarget> {
    endpoints
        .iter()
        .find(|endpoint| {
            endpoint.enabled
                && endpoint.protocol != OPENAI_OAUTH_PROTOCOL
                && probe_model(endpoint).is_some()
        })
        .or_else(|| endpoints.iter().find(|endpoint| endpoint.enabled))
        .or_else(|| endpoints.first())
}

pub async fn recent_probe_samples_by_channel(
    state: &AppState,
    channel_ids: &[DbId],
    limit_per_channel: i64,
) -> AppResult<std::collections::HashMap<DbId, Vec<ChannelProbeSampleRecord>>> {
    if channel_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT channel_id, status, latency_ms, status_code, model, error_summary, created_at
        FROM (
            SELECT cps.*,
                   row_number() OVER (
                       PARTITION BY cps.channel_id
                       ORDER BY cps.created_at DESC, cps.id DESC
                   ) AS rn
            FROM channel_probe cps
            WHERE cps.channel_id = ANY($1)
        ) ranked
        WHERE rn <= $2
        ORDER BY channel_id ASC, created_at ASC
        "#,
    )
    .bind(channel_ids)
    .bind(limit_per_channel)
    .fetch_all(&state.db.pool)
    .await?;

    let mut samples = std::collections::HashMap::<DbId, Vec<ChannelProbeSampleRecord>>::new();
    for row in rows {
        let channel_id: DbId = row.try_get("channel_id")?;
        samples
            .entry(channel_id)
            .or_default()
            .push(ChannelProbeSampleRecord {
                status: probe_status_from_str(row.try_get::<String, _>("status")?.as_str()),
                latency_ms: row.try_get("latency_ms")?,
                status_code: row.try_get("status_code")?,
                model: row.try_get("model")?,
                error_summary: row.try_get("error_summary")?,
                created_at: row.try_get("created_at")?,
            });
    }
    Ok(samples)
}

async fn diagnose_endpoint(
    state: &AppState,
    channel: &ChannelDiagnosticTarget,
    endpoint: &EndpointTarget,
    keys: &[KeyTarget],
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
) -> EndpointDiagnosticReport {
    if !endpoint.enabled {
        return EndpointDiagnosticReport {
            endpoint_id: endpoint.id,
            protocol: endpoint.protocol.clone(),
            base_url: endpoint.base_url.clone(),
            status: DiagnosticStatus::Skipped,
            summary: "端点已停用，未执行诊断".to_string(),
            discovered_models: Vec::new(),
            configured_models: endpoint.models.clone(),
            missing_configured_models: Vec::new(),
            keys: Vec::new(),
        };
    }

    if !channel.enabled {
        return EndpointDiagnosticReport {
            endpoint_id: endpoint.id,
            protocol: endpoint.protocol.clone(),
            base_url: endpoint.base_url.clone(),
            status: DiagnosticStatus::Skipped,
            summary: "通道已停用，未执行诊断".to_string(),
            discovered_models: Vec::new(),
            configured_models: endpoint.models.clone(),
            missing_configured_models: Vec::new(),
            keys: Vec::new(),
        };
    }

    let enabled_keys: Vec<_> = keys.iter().filter(|key| key.enabled).collect();
    if enabled_keys.is_empty() {
        return EndpointDiagnosticReport {
            endpoint_id: endpoint.id,
            protocol: endpoint.protocol.clone(),
            base_url: endpoint.base_url.clone(),
            status: DiagnosticStatus::Failed,
            summary: "没有启用的上游 Key 或凭证".to_string(),
            discovered_models: Vec::new(),
            configured_models: endpoint.models.clone(),
            missing_configured_models: endpoint.models.clone(),
            keys: Vec::new(),
        };
    }

    let mut discovered_models = Vec::new();
    let mut key_reports = Vec::new();
    for key in enabled_keys {
        let report = diagnose_key(state, channel, endpoint, key, progress).await;
        for model in report.discovered_models.clone() {
            if !discovered_models.iter().any(|item| item == &model) {
                discovered_models.push(model);
            }
        }
        if let Some(key_id) = key.id {
            let _ = persist_key_health(state, key_id, report.status, &report.summary).await;
        }
        key_reports.push(report);
    }

    discovered_models.sort();
    let missing_configured_models = missing_models(&endpoint.models, &discovered_models);
    let mut status = aggregate_status(key_reports.iter().map(|item| item.status));
    if status == DiagnosticStatus::Ok && !missing_configured_models.is_empty() {
        status = DiagnosticStatus::Warning;
    }

    EndpointDiagnosticReport {
        endpoint_id: endpoint.id,
        protocol: endpoint.protocol.clone(),
        base_url: endpoint.base_url.clone(),
        status,
        summary: endpoint_summary(status, &missing_configured_models),
        discovered_models,
        configured_models: endpoint.models.clone(),
        missing_configured_models,
        keys: key_reports,
    }
}

async fn diagnose_key(
    state: &AppState,
    _channel: &ChannelDiagnosticTarget,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
) -> KeyDiagnosticReport {
    if endpoint.protocol == OPENAI_OAUTH_PROTOCOL {
        return KeyDiagnosticReport {
            key_id: key.id,
            key_name: mask_possible_secret_label(&key.name),
            key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
            status: DiagnosticStatus::Warning,
            summary: "OpenAI OAuth 通道需要账号上下文，已跳过主动调用诊断".to_string(),
            discovered_models: Vec::new(),
            steps: vec![DiagnosticStep {
                name: "probe".to_string(),
                status: DiagnosticStatus::Skipped,
                message: "OpenAI OAuth 主动诊断暂未启用；请以实际调用结果为准".to_string(),
                duration_ms: 0,
                status_code: None,
            }],
        };
    }

    let mut steps = Vec::new();
    let models_step = run_models_step(state, endpoint, key).await;
    let discovered_models = models_step.models.clone();
    steps.push(models_step.step);

    let probe_models: Vec<_> = endpoint
        .models
        .iter()
        .filter(|&model| is_text_probe_model(model))
        .filter(|&model| discovered_models.is_empty() || discovered_models.contains(model))
        .cloned()
        .collect();
    if !probe_models.is_empty() {
        for model in probe_models {
            send_model_started_event(progress, endpoint, key, &model);
            let step = run_probe_step(state, endpoint, key, &model).await;
            send_model_result_event(progress, endpoint, key, &model, &step);
            steps.push(step);
        }
    } else if let Some(model) = discovered_models
        .iter()
        .find(|model| is_text_probe_model(model))
    {
        send_model_started_event(progress, endpoint, key, model);
        let step = run_probe_step(state, endpoint, key, model).await;
        send_model_result_event(progress, endpoint, key, model, &step);
        steps.push(step);
    } else {
        steps.push(DiagnosticStep {
            name: "probe".to_string(),
            status: DiagnosticStatus::Skipped,
            message: "没有可用于轻量调用测试的文本模型".to_string(),
            duration_ms: 0,
            status_code: None,
        });
    }

    let status = aggregate_status(steps.iter().map(|item| item.status));
    KeyDiagnosticReport {
        key_id: key.id,
        key_name: mask_possible_secret_label(&key.name),
        key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
        status,
        summary: key_summary(status),
        discovered_models,
        steps,
    }
}

fn send_model_started_event(
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) {
    if let Some(progress) = progress {
        let _ = progress.send(ChannelDiagnosticEvent::ModelStarted {
            endpoint_id: endpoint.id,
            protocol: endpoint.protocol.clone(),
            base_url: endpoint.base_url.clone(),
            key_id: key.id,
            key_name: mask_possible_secret_label(&key.name),
            key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
            model: model.to_string(),
        });
    }
}

fn send_model_result_event(
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
    step: &DiagnosticStep,
) {
    if let Some(progress) = progress {
        let _ = progress.send(ChannelDiagnosticEvent::ModelResult {
            endpoint_id: endpoint.id,
            protocol: endpoint.protocol.clone(),
            base_url: endpoint.base_url.clone(),
            key_id: key.id,
            key_name: mask_possible_secret_label(&key.name),
            key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
            model: model.to_string(),
            step: step.clone(),
        });
    }
}

struct ModelsStepResult {
    step: DiagnosticStep,
    models: Vec<String>,
}

async fn run_models_step(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
) -> ModelsStepResult {
    let started = Instant::now();
    let response = upstream_request(state, endpoint, key, "GET", "/v1/models", None)
        .send()
        .await;
    match response {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                return ModelsStepResult {
                    step: DiagnosticStep {
                        name: "models".to_string(),
                        status: DiagnosticStatus::Failed,
                        message: upstream_status_message(status),
                        duration_ms: started.elapsed().as_millis() as i64,
                        status_code: Some(status.as_u16()),
                    },
                    models: Vec::new(),
                };
            }
            match response.json::<Value>().await {
                Ok(value) => {
                    let models = extract_model_ids(&value);
                    let status = if models.is_empty() {
                        DiagnosticStatus::Warning
                    } else {
                        DiagnosticStatus::Ok
                    };
                    ModelsStepResult {
                        step: DiagnosticStep {
                            name: "models".to_string(),
                            status,
                            message: if models.is_empty() {
                                "模型列表接口可访问，但没有返回模型".to_string()
                            } else {
                                format!("模型列表可访问，发现 {} 个模型", models.len())
                            },
                            duration_ms: started.elapsed().as_millis() as i64,
                            status_code: Some(StatusCode::OK.as_u16()),
                        },
                        models,
                    }
                }
                Err(_) => ModelsStepResult {
                    step: DiagnosticStep {
                        name: "models".to_string(),
                        status: DiagnosticStatus::Failed,
                        message: "模型列表响应不是有效 JSON".to_string(),
                        duration_ms: started.elapsed().as_millis() as i64,
                        status_code: Some(status.as_u16()),
                    },
                    models: Vec::new(),
                },
            }
        }
        Err(err) => ModelsStepResult {
            step: DiagnosticStep {
                name: "models".to_string(),
                status: DiagnosticStatus::Failed,
                message: transport_error_message(&err),
                duration_ms: started.elapsed().as_millis() as i64,
                status_code: None,
            },
            models: Vec::new(),
        },
    }
}

async fn run_probe_step(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> DiagnosticStep {
    let started = Instant::now();
    let request = probe_request(endpoint, model);
    let key_label = diagnostic_key_log_label(key);
    tracing::info!(
        endpoint_id = endpoint.id,
        protocol = %endpoint.protocol,
        base_url = %endpoint.base_url,
        key = %key_label,
        model = %model,
        path = %request.log_path,
        url = %request.url,
        "diagnostic probe request started"
    );
    let response = upstream_request_url(
        state,
        endpoint,
        key,
        "POST",
        &request.url,
        request.extra_headers,
        Some(request.body),
    )
    .send()
    .await;
    match response {
        Ok(response) => {
            let status = response.status();
            let duration_ms = started.elapsed().as_millis() as i64;
            if status.is_success() {
                tracing::info!(
                    endpoint_id = endpoint.id,
                    protocol = %endpoint.protocol,
                    key = %key_label,
                    model = %model,
                    status = status.as_u16(),
                    duration_ms,
                    "diagnostic probe request succeeded"
                );
            } else {
                tracing::warn!(
                    endpoint_id = endpoint.id,
                    protocol = %endpoint.protocol,
                    key = %key_label,
                    model = %model,
                    status = status.as_u16(),
                    duration_ms,
                    "diagnostic probe request failed"
                );
            }
            DiagnosticStep {
                name: format!("probe:{model}"),
                status: if status.is_success() {
                    DiagnosticStatus::Ok
                } else {
                    DiagnosticStatus::Failed
                },
                message: if status.is_success() {
                    format!("模型 {model} 轻量调用成功")
                } else {
                    upstream_status_message(status)
                },
                duration_ms,
                status_code: Some(status.as_u16()),
            }
        }
        Err(err) => {
            let duration_ms = started.elapsed().as_millis() as i64;
            let message = transport_error_message(&err);
            tracing::warn!(
                endpoint_id = endpoint.id,
                protocol = %endpoint.protocol,
                key = %key_label,
                model = %model,
                duration_ms,
                error = %message,
                "diagnostic probe request errored"
            );
            DiagnosticStep {
                name: format!("probe:{model}"),
                status: DiagnosticStatus::Failed,
                message,
                duration_ms,
                status_code: None,
            }
        }
    }
}

fn upstream_request(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> reqwest::RequestBuilder {
    let url = upstream_url(&endpoint.base_url, path);
    let mut request = match method {
        "POST" => state.http.post(url),
        _ => state.http.get(url),
    };

    request = if endpoint.protocol == "anthropic" {
        request
            .header("x-api-key", &key.secret)
            .header("anthropic-version", DEFAULT_ANTHROPIC_VERSION)
    } else {
        request.bearer_auth(&key.secret)
    };

    if endpoint.protocol == OPENAI_OAUTH_PROTOCOL {
        request = request
            .header("accept", "text/event-stream")
            .header("connection", "Keep-Alive")
            .header("originator", "codex_cli_rs")
            .header("user-agent", "codex_cli_rs/0.118.0 (NeoGate; diagnostics)");
    }

    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .json(&body);
    }
    request
}

fn upstream_request_url(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    method: &str,
    url: &str,
    extra_headers: reqwest::header::HeaderMap,
    body: Option<Value>,
) -> reqwest::RequestBuilder {
    let mut request = match method {
        "POST" => state.http.post(url),
        _ => state.http.get(url),
    };

    request = if endpoint.protocol == "anthropic" {
        request
            .header("x-api-key", &key.secret)
            .header("anthropic-version", DEFAULT_ANTHROPIC_VERSION)
    } else {
        request.bearer_auth(&key.secret)
    };

    for (name, value) in &extra_headers {
        request = request.header(name, value.clone());
    }

    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .json(&body);
    }
    request
}

struct DiagnosticProbeRequest {
    log_path: String,
    url: String,
    extra_headers: reqwest::header::HeaderMap,
    body: Value,
}

fn probe_request(endpoint: &EndpointTarget, model: &str) -> DiagnosticProbeRequest {
    if endpoint.protocol == "anthropic" {
        let path = "/v1/messages";
        return DiagnosticProbeRequest {
            log_path: path.to_string(),
            url: upstream_url(&endpoint.base_url, path),
            extra_headers: reqwest::header::HeaderMap::new(),
            body: json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }]
            }),
        };
    }

    if endpoint.protocol == OPENAI_OAUTH_PROTOCOL {
        let path = "/responses";
        return DiagnosticProbeRequest {
            log_path: path.to_string(),
            url: upstream_url(&endpoint.base_url, path),
            extra_headers: reqwest::header::HeaderMap::new(),
            body: json!({
                "model": model,
                "input": "ping",
                "max_output_tokens": 1,
                "store": false,
                "stream": false
            }),
        };
    }

    let adapter = adapter_for_provider(&endpoint.provider);
    let route = RelayRoute::OpenAiChatCompletions;
    let mut extra_headers = reqwest::header::HeaderMap::new();
    if endpoint.provider.eq_ignore_ascii_case("qwen") {
        extra_headers.insert("x-dashscope-sse", "enable".parse().expect("valid header"));
    }
    DiagnosticProbeRequest {
        log_path: route.path().to_string(),
        url: adapter.resolve_url(&endpoint.base_url, route),
        extra_headers,
        body: json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }]
        }),
    }
}

fn probe_model(endpoint: &EndpointTarget) -> Option<String> {
    endpoint
        .models
        .iter()
        .find(|model| is_text_probe_model(model))
        .cloned()
}

fn is_text_probe_model(model: &str) -> bool {
    let lowered = model.to_ascii_lowercase();
    ![
        "embedding",
        "moderation",
        "image",
        "dall-e",
        "whisper",
        "tts",
        "audio",
        "rerank",
        "clip",
        "vision",
    ]
    .iter()
    .any(|marker| lowered.contains(marker))
}

async fn load_channel(state: &AppState, channel_id: DbId) -> AppResult<ChannelDiagnosticTarget> {
    let row = sqlx::query(
        "SELECT id, provider, name, enabled, use_credentials
         FROM channel
         WHERE id = $1",
    )
    .bind(channel_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;

    Ok(ChannelDiagnosticTarget {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        name: row.try_get("name")?,
        enabled: row.try_get("enabled")?,
        use_credentials: row.try_get("use_credentials")?,
    })
}

async fn load_endpoints(state: &AppState, channel_id: DbId) -> AppResult<Vec<EndpointTarget>> {
    let rows = sqlx::query(
        "SELECT ce.id, c.provider, ce.protocol, ce.base_url, ce.models, ce.enabled
         FROM channel_endpoint ce
         JOIN channel c ON c.id = ce.channel_id
         WHERE ce.channel_id = $1
         ORDER BY CASE protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END,
                  ce.created_at ASC",
    )
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await?;

    Ok(rows
        .iter()
        .map(|row| EndpointTarget {
            id: row.try_get("id").unwrap_or_default(),
            provider: row.try_get("provider").unwrap_or_default(),
            protocol: row.try_get("protocol").unwrap_or_default(),
            base_url: row.try_get("base_url").unwrap_or_default(),
            models: row.try_get("models").unwrap_or_default(),
            enabled: row.try_get("enabled").unwrap_or_default(),
        })
        .collect())
}

async fn load_keys(
    state: &AppState,
    channel: &ChannelDiagnosticTarget,
) -> AppResult<Vec<KeyTarget>> {
    if channel.use_credentials {
        let secret = runtime_secret_from_enabled_credential(state, &channel.provider).await?;
        return Ok(vec![KeyTarget {
            id: None,
            name: "启用的凭证文件".to_string(),
            key_prefix: None,
            secret,
            enabled: true,
        }]);
    }

    let rows = sqlx::query(
        "SELECT id, name, key_prefix, secret_ciphertext, enabled
         FROM channel_key
         WHERE channel_id = $1
         ORDER BY enabled DESC, healthy DESC, created_at ASC",
    )
    .bind(channel.id)
    .fetch_all(&state.db.pool)
    .await?;

    rows.iter()
        .map(|row| {
            let id: DbId = row.try_get("id")?;
            let ciphertext: String = row.try_get("secret_ciphertext")?;
            Ok(KeyTarget {
                id: Some(id),
                name: row.try_get("name")?,
                key_prefix: row.try_get("key_prefix")?,
                secret: state.secrets.plaintext(id, &ciphertext)?,
                enabled: row.try_get("enabled")?,
            })
        })
        .collect()
}

async fn persist_key_health(
    state: &AppState,
    key_id: DbId,
    status: DiagnosticStatus,
    summary: &str,
) -> AppResult<()> {
    let healthy = status == DiagnosticStatus::Ok || status == DiagnosticStatus::Warning;
    let cooldown_until =
        (!healthy).then(|| Utc::now() + ChronoDuration::minutes(DIAGNOSTIC_COOLDOWN_MINUTES));
    sqlx::query(
        "UPDATE channel_key
         SET healthy = $2,
             last_error = $3,
             cooldown_until = $4,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(key_id)
    .bind(healthy)
    .bind((!healthy).then_some(summary))
    .bind(cooldown_until)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn persist_endpoint_health(
    state: &AppState,
    endpoint_id: DbId,
    status: DiagnosticStatus,
    summary: &str,
) -> AppResult<()> {
    let healthy = status == DiagnosticStatus::Ok || status == DiagnosticStatus::Warning;
    let cooldown_until =
        (!healthy).then(|| Utc::now() + ChronoDuration::minutes(DIAGNOSTIC_COOLDOWN_MINUTES));
    sqlx::query(
        "UPDATE channel_endpoint
         SET healthy = $2,
             last_error = $3,
             cooldown_until = $4,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(endpoint_id)
    .bind(healthy)
    .bind((!healthy).then_some(summary))
    .bind(cooldown_until)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

fn aggregate_status(statuses: impl Iterator<Item = DiagnosticStatus>) -> DiagnosticStatus {
    let mut saw = false;
    let mut has_failed = false;
    let mut has_warning = false;
    for status in statuses {
        saw = true;
        match status {
            DiagnosticStatus::Failed => has_failed = true,
            DiagnosticStatus::Warning | DiagnosticStatus::Skipped => has_warning = true,
            DiagnosticStatus::Ok => {}
        }
    }
    if !saw || has_failed {
        DiagnosticStatus::Failed
    } else if has_warning {
        DiagnosticStatus::Warning
    } else {
        DiagnosticStatus::Ok
    }
}

fn channel_summary(
    channel: &ChannelDiagnosticTarget,
    endpoints: &[EndpointDiagnosticReport],
    status: DiagnosticStatus,
) -> String {
    if endpoints.is_empty() {
        return "通道没有配置端点".to_string();
    }
    if !channel.enabled {
        return "通道已停用，诊断已跳过".to_string();
    }
    match status {
        DiagnosticStatus::Ok => "已选端点和 Key 均通过诊断".to_string(),
        DiagnosticStatus::Warning => "通道可用，但存在需要关注的配置项".to_string(),
        DiagnosticStatus::Failed => "通道诊断失败，请检查失败步骤".to_string(),
        DiagnosticStatus::Skipped => "诊断已跳过".to_string(),
    }
}

fn endpoint_summary(status: DiagnosticStatus, missing_models: &[String]) -> String {
    if !missing_models.is_empty() {
        return format!(
            "有 {} 个配置模型未出现在上游模型列表中",
            missing_models.len()
        );
    }
    match status {
        DiagnosticStatus::Ok => "端点诊断通过".to_string(),
        DiagnosticStatus::Warning => "端点可访问，但存在警告".to_string(),
        DiagnosticStatus::Failed => "端点诊断失败".to_string(),
        DiagnosticStatus::Skipped => "端点诊断已跳过".to_string(),
    }
}

fn mask_key_prefix(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= 6 {
        return "******".to_string();
    }
    let head: String = trimmed.chars().take(4).collect();
    let tail: String = trimmed
        .chars()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}****{tail}")
}

fn mask_possible_secret_label(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with("sk-") || trimmed.starts_with("sess-") || trimmed.len() >= 24 {
        return mask_key_prefix(trimmed);
    }
    trimmed.to_string()
}

fn diagnostic_key_log_label(key: &KeyTarget) -> String {
    key.key_prefix
        .as_deref()
        .map(mask_key_prefix)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| mask_possible_secret_label(&key.name))
}

fn key_summary(status: DiagnosticStatus) -> String {
    match status {
        DiagnosticStatus::Ok => "Key 可用".to_string(),
        DiagnosticStatus::Warning => "Key 基本可用，但存在警告".to_string(),
        DiagnosticStatus::Failed => "Key 不可用".to_string(),
        DiagnosticStatus::Skipped => "Key 诊断已跳过".to_string(),
    }
}

fn upstream_status_message(status: StatusCode) -> String {
    match status.as_u16() {
        400 => "上游拒绝测试请求，请检查模型名或请求协议".to_string(),
        401 | 403 => "认证失败，请检查上游 Key 或凭证权限".to_string(),
        404 => "接口不存在，请检查 Base URL 和协议".to_string(),
        429 => "上游限流，请稍后重试或切换 Key".to_string(),
        500..=599 => "上游服务暂时不可用".to_string(),
        _ => format!("上游返回 HTTP {}", status.as_u16()),
    }
}

fn transport_error_message(err: &reqwest::Error) -> String {
    match UpstreamErrorKind::from_reqwest(err) {
        UpstreamErrorKind::Timeout => "连接上游超时，请检查网络或上游状态".to_string(),
        UpstreamErrorKind::Tls => "TLS 握手失败，请检查 Base URL 证书".to_string(),
        UpstreamErrorKind::Dns => "DNS 解析失败，请检查 Base URL 域名".to_string(),
        UpstreamErrorKind::Connect => "无法连接上游，请检查网络、防火墙或 Base URL".to_string(),
        UpstreamErrorKind::Request => "上游请求失败，请检查网络和配置".to_string(),
    }
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array());
    let Some(items) = data else {
        return Vec::new();
    };

    let mut models = Vec::new();
    for item in items {
        let id = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str);
        let Some(id) = trimmed_non_empty(id) else {
            continue;
        };
        if !models.iter().any(|model| model == id) {
            models.push(id.to_string());
        }
    }
    models
}

fn missing_models(configured: &[String], discovered: &[String]) -> Vec<String> {
    if configured.is_empty() || discovered.is_empty() {
        return Vec::new();
    }
    configured
        .iter()
        .filter(|model| !discovered.iter().any(|item| item == *model))
        .cloned()
        .collect()
}

fn probe_status_from_str(status: &str) -> ProbeStatus {
    match status {
        "ok" => ProbeStatus::Ok,
        "skipped" => ProbeStatus::Skipped,
        _ => ProbeStatus::Failed,
    }
}
