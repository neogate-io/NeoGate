use std::time::Instant;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    billing::BILLABLE_PRICE_CONDITION_CP,
    cache::InvalidationEvent,
    config::{DEFAULT_ANTHROPIC_VERSION, UPSTREAM_TIMEOUT},
    error::{AppError, AppResult, UpstreamErrorKind},
    id::DbId,
    input::trimmed_non_empty,
    provider::adapters::{adapter_for_endpoint, RelayRoute},
    relay::{
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
    AppState,
};

use super::provider::OPENAI_OAUTH_PROTOCOL;
use super::{channel::mask_channel_key, credentials::runtime_secret_from_enabled_credential};

const DIAGNOSTIC_COOLDOWN_MINUTES: i64 = 5;

async fn send_with_upstream_timeout(
    request: reqwest::RequestBuilder,
) -> Result<reqwest::Response, AppError> {
    match tokio::time::timeout(UPSTREAM_TIMEOUT, request.send()).await {
        Ok(result) => result.map_err(AppError::from),
        Err(_) => Err(AppError::BadRequest(format!(
            "diagnostic probe timed out after {} seconds",
            UPSTREAM_TIMEOUT.as_secs()
        ))),
    }
}

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
    pub masked_key: Option<String>,
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

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticScope {
    #[default]
    All,
    Models,
    Text,
    Image,
    Video,
}

impl DiagnosticScope {
    fn includes_models(self) -> bool {
        matches!(self, Self::All | Self::Models)
    }

    fn includes_text(self) -> bool {
        matches!(self, Self::All | Self::Text)
    }

    fn includes_image(self) -> bool {
        matches!(self, Self::All | Self::Image)
    }

    fn includes_video(self) -> bool {
        matches!(self, Self::All | Self::Video)
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DiagnoseChannelRequest {
    #[serde(default)]
    pub scope: DiagnosticScope,
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
    adapter_hint: Option<String>,
    models: Vec<String>,
    image_models: Vec<String>,
    video_models: Vec<String>,
    enabled: bool,
}

struct KeyTarget {
    id: Option<DbId>,
    name: String,
    masked_key: Option<String>,
    key_prefix: Option<String>,
    secret: String,
    enabled: bool,
}

pub async fn diagnose_channel_with_scope(
    state: &AppState,
    channel_id: DbId,
    scope: DiagnosticScope,
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
        let report =
            diagnose_endpoint(state, &channel, endpoint, &keys, scope, progress.as_ref()).await;
        if report.status != DiagnosticStatus::Skipped {
            persist_endpoint_health(state, endpoint.id, &report).await?;
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
    scope: DiagnosticScope,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
) -> EndpointDiagnosticReport {
    if !endpoint.enabled {
        return empty_endpoint_report(
            endpoint,
            DiagnosticStatus::Skipped,
            "端点已停用，未执行诊断",
            Vec::new(),
        );
    }

    if !channel.enabled {
        return empty_endpoint_report(
            endpoint,
            DiagnosticStatus::Skipped,
            "通道已停用，未执行诊断",
            Vec::new(),
        );
    }

    let enabled_keys: Vec<_> = keys.iter().filter(|key| key.enabled).collect();
    if enabled_keys.is_empty() {
        return empty_endpoint_report(
            endpoint,
            DiagnosticStatus::Failed,
            "没有启用的上游 Key 或凭证",
            endpoint.models.clone(),
        );
    }

    let mut discovered_models = Vec::new();
    let mut key_reports = Vec::new();
    for key in enabled_keys {
        let report = diagnose_key(state, channel, endpoint, key, scope, progress).await;
        for model in report.discovered_models.clone() {
            if !discovered_models.iter().any(|item| item == &model) {
                discovered_models.push(model);
            }
        }
        if let Some(key_id) = key.id {
            let _ = persist_key_health(state, key_id, &report).await;
        }
        key_reports.push(report);
    }
    persist_model_probe_results_best_effort(state, endpoint, &key_reports).await;

    discovered_models.sort();
    let missing_configured_models = if scope.includes_models() {
        missing_models(&endpoint.models, &discovered_models)
    } else {
        Vec::new()
    };
    let mut status = aggregate_status(key_reports.iter().map(|item| item.status));
    if status == DiagnosticStatus::Ok && !missing_configured_models.is_empty() {
        status = DiagnosticStatus::Warning;
    }

    endpoint_report(
        endpoint,
        status,
        endpoint_summary(status, &missing_configured_models),
        discovered_models,
        missing_configured_models,
        key_reports,
    )
}

fn empty_endpoint_report(
    endpoint: &EndpointTarget,
    status: DiagnosticStatus,
    summary: &str,
    missing_configured_models: Vec<String>,
) -> EndpointDiagnosticReport {
    endpoint_report(
        endpoint,
        status,
        summary.to_string(),
        Vec::new(),
        missing_configured_models,
        Vec::new(),
    )
}

fn endpoint_report(
    endpoint: &EndpointTarget,
    status: DiagnosticStatus,
    summary: String,
    discovered_models: Vec<String>,
    missing_configured_models: Vec<String>,
    keys: Vec<KeyDiagnosticReport>,
) -> EndpointDiagnosticReport {
    EndpointDiagnosticReport {
        endpoint_id: endpoint.id,
        protocol: endpoint.protocol.clone(),
        base_url: endpoint.base_url.clone(),
        status,
        summary,
        discovered_models,
        configured_models: endpoint.models.clone(),
        missing_configured_models,
        keys,
    }
}

async fn diagnose_key(
    state: &AppState,
    _channel: &ChannelDiagnosticTarget,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    scope: DiagnosticScope,
    progress: Option<&tokio::sync::mpsc::UnboundedSender<ChannelDiagnosticEvent>>,
) -> KeyDiagnosticReport {
    if endpoint.protocol == OPENAI_OAUTH_PROTOCOL {
        return KeyDiagnosticReport {
            key_id: key.id,
            key_name: mask_possible_secret_label(&key.name),
            masked_key: key.masked_key.clone(),
            key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
            status: DiagnosticStatus::Warning,
            summary: "OpenAI OAuth 通道需要账号上下文，已跳过主动调用诊断".to_string(),
            discovered_models: Vec::new(),
            steps: vec![skipped_step(
                "probe",
                "OpenAI OAuth 主动诊断暂未启用；请以实际调用结果为准",
            )],
        };
    }

    let mut steps = Vec::new();
    let discovered_models = if scope.includes_models() {
        let models_step = run_models_step(state, endpoint, key).await;
        let discovered_models = models_step.models.clone();
        steps.push(models_step.step);
        discovered_models
    } else {
        Vec::new()
    };

    if scope.includes_text() && !endpoint.models.is_empty() {
        for model in &endpoint.models {
            send_model_started_event(progress, endpoint, key, model);
            let step = run_probe_step(state, endpoint, key, model).await;
            send_model_result_event(progress, endpoint, key, model, &step);
            steps.push(step);
        }
    }
    if scope.includes_video() && !endpoint.video_models.is_empty() {
        for model in &endpoint.video_models {
            send_model_started_event(progress, endpoint, key, model);
            let step = run_video_probe_step(state, endpoint, key, model).await;
            send_model_result_event(progress, endpoint, key, model, &step);
            steps.push(step);
        }
    }
    if scope.includes_image() && !endpoint.image_models.is_empty() {
        for model in &endpoint.image_models {
            send_model_started_event(progress, endpoint, key, model);
            let step = run_image_probe_step(state, endpoint, key, model).await;
            send_model_result_event(progress, endpoint, key, model, &step);
            steps.push(step);
        }
    }

    let status = if steps.is_empty() {
        DiagnosticStatus::Skipped
    } else {
        aggregate_status(steps.iter().map(|item| item.status))
    };
    KeyDiagnosticReport {
        key_id: key.id,
        key_name: mask_possible_secret_label(&key.name),
        masked_key: key.masked_key.clone(),
        key_prefix: key.key_prefix.as_deref().map(mask_key_prefix),
        status,
        summary: key_summary(status),
        discovered_models,
        steps,
    }
}

fn diagnostic_step(
    name: impl Into<String>,
    status: DiagnosticStatus,
    message: impl Into<String>,
    duration_ms: i64,
    status_code: Option<u16>,
) -> DiagnosticStep {
    DiagnosticStep {
        name: name.into(),
        status,
        message: message.into(),
        duration_ms,
        status_code,
    }
}

fn skipped_step(name: &str, message: &str) -> DiagnosticStep {
    diagnostic_step(name, DiagnosticStatus::Skipped, message, 0, None)
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
    let response = send_with_upstream_timeout(upstream_request(
        state,
        endpoint,
        key,
        "GET",
        "/v1/models",
        None,
    ))
    .await;
    match response {
        Ok(response) => {
            let status = response.status();
            if !status.is_success() {
                let message = upstream_failure_message(status, response).await;
                return ModelsStepResult {
                    step: diagnostic_step(
                        "models",
                        DiagnosticStatus::Failed,
                        message,
                        started.elapsed().as_millis() as i64,
                        Some(status.as_u16()),
                    ),
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
                        step: diagnostic_step(
                            "models",
                            status,
                            if models.is_empty() {
                                "模型列表接口可访问，但没有返回模型".to_string()
                            } else {
                                format!("模型列表可访问，发现 {} 个模型", models.len())
                            },
                            started.elapsed().as_millis() as i64,
                            Some(StatusCode::OK.as_u16()),
                        ),
                        models,
                    }
                }
                Err(_) => ModelsStepResult {
                    step: diagnostic_step(
                        "models",
                        DiagnosticStatus::Failed,
                        "模型列表响应不是有效 JSON",
                        started.elapsed().as_millis() as i64,
                        Some(status.as_u16()),
                    ),
                    models: Vec::new(),
                },
            }
        }
        Err(err) => ModelsStepResult {
            step: diagnostic_step(
                "models",
                DiagnosticStatus::Failed,
                transport_error_message(&err),
                started.elapsed().as_millis() as i64,
                None,
            ),
            models: Vec::new(),
        },
    }
}

/// 公共 probe 执行骨架：发送请求、记录日志、构造 DiagnosticStep。
/// `step_prefix` 用于生成 step 名称（如 "probe"、"video_probe"），
/// `kind` 作为结构化日志字段以区分探测类型，
/// `success_message` 由调用方提供成功时的提示文本。
#[allow(clippy::too_many_arguments)]
async fn execute_url_probe(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
    step_prefix: &str,
    kind: &str,
    request: DiagnosticProbeRequest,
    success_message: impl FnOnce(&str) -> String,
) -> DiagnosticStep {
    let started = Instant::now();
    let key_label = diagnostic_key_log_label(key);
    tracing::info!(
        endpoint_id = endpoint.id,
        protocol = %endpoint.protocol,
        base_url = %endpoint.base_url,
        key = %key_label,
        kind,
        model,
        path = %request.log_path,
        url = %request.url,
        "diagnostic probe started"
    );
    let response = send_with_upstream_timeout(upstream_request_url(
        state,
        endpoint,
        key,
        "POST",
        &request.url,
        request.extra_headers,
        Some(request.body),
    ))
    .await;
    let step_name = format!("{step_prefix}:{model}");
    match response {
        Ok(response) => {
            let status = response.status();
            let duration_ms = started.elapsed().as_millis() as i64;
            if status.is_success() {
                tracing::info!(
                    endpoint_id = endpoint.id,
                    protocol = %endpoint.protocol,
                    key = %key_label,
                    kind,
                    model,
                    status = status.as_u16(),
                    duration_ms,
                    "diagnostic probe succeeded"
                );
            } else {
                tracing::warn!(
                    endpoint_id = endpoint.id,
                    protocol = %endpoint.protocol,
                    key = %key_label,
                    kind,
                    model,
                    status = status.as_u16(),
                    duration_ms,
                    "diagnostic probe failed"
                );
            }
            let message = if status.is_success() {
                success_message(model)
            } else {
                upstream_failure_message(status, response).await
            };
            diagnostic_step(
                step_name,
                if status.is_success() {
                    DiagnosticStatus::Ok
                } else {
                    DiagnosticStatus::Failed
                },
                message,
                duration_ms,
                Some(status.as_u16()),
            )
        }
        Err(err) => {
            let duration_ms = started.elapsed().as_millis() as i64;
            let message = transport_error_message(&err);
            tracing::warn!(
                endpoint_id = endpoint.id,
                protocol = %endpoint.protocol,
                key = %key_label,
                kind,
                model,
                duration_ms,
                error = %message,
                "diagnostic probe errored"
            );
            diagnostic_step(
                step_name,
                DiagnosticStatus::Failed,
                message,
                duration_ms,
                None,
            )
        }
    }
}

async fn run_probe_step(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> DiagnosticStep {
    execute_url_probe(
        state,
        endpoint,
        key,
        model,
        "probe",
        "text",
        probe_request(endpoint, model),
        |m| format!("模型 {m} 轻量调用成功"),
    )
    .await
}

async fn run_video_probe_step(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> DiagnosticStep {
    let started = Instant::now();
    let request = match video_probe_request(endpoint, key, model) {
        Ok(r) => r,
        Err(err) => {
            return diagnostic_step(
                format!("video_probe:{model}"),
                DiagnosticStatus::Failed,
                err.to_string(),
                started.elapsed().as_millis() as i64,
                None,
            );
        }
    };
    execute_url_probe(
        state,
        endpoint,
        key,
        model,
        "video_probe",
        "video",
        request,
        |m| format!("视频模型 {m} 任务创建成功"),
    )
    .await
}

async fn run_image_probe_step(
    state: &AppState,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> DiagnosticStep {
    execute_url_probe(
        state,
        endpoint,
        key,
        model,
        "image_probe",
        "image",
        image_probe_request(endpoint, model),
        |m| format!("图片模型 {m} 生成请求成功"),
    )
    .await
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

    let adapter = adapter_for_endpoint(
        &endpoint.provider,
        &endpoint.base_url,
        endpoint.adapter_hint.as_deref(),
    );
    let route = RelayRoute::ChatCompletions;
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

fn video_probe_request(
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> AppResult<DiagnosticProbeRequest> {
    let adapter = adapter_for_endpoint(
        &endpoint.provider,
        &endpoint.base_url,
        endpoint.adapter_hint.as_deref(),
    );
    let route = RelayRoute::Videos;
    let body = serde_json::to_vec(&json!({
        "model": model,
        "prompt": "ping",
        "resolution": "720P",
        "seconds": 3
    }))?;
    let prepared = adapter.prepare_openai_request(
        &SelectedUpstream {
            channel_id: 0,
            channel_endpoint_id: endpoint.id,
            channel_key_id: key.id,
            credential_id: None,
            provider: endpoint.provider.clone(),
            channel_name: "diagnostic".to_string(),
            base_url: endpoint.base_url.clone(),
            adapter_hint: endpoint.adapter_hint.clone(),
            responses_chat_fallback: false,
            secret: key.secret.clone(),
            account_id: None,
            affinity: None,
        },
        upstream_protocol_from_str(&endpoint.protocol)?,
        route,
        body.into(),
        &reqwest::header::HeaderMap::new(),
        false,
    )?;
    let body: Value = serde_json::from_slice(&prepared.body)?;
    Ok(DiagnosticProbeRequest {
        log_path: prepared.log_path,
        url: prepared.url,
        extra_headers: prepared.extra_headers,
        body,
    })
}

fn image_probe_request(endpoint: &EndpointTarget, model: &str) -> DiagnosticProbeRequest {
    let adapter = adapter_for_endpoint(
        &endpoint.provider,
        &endpoint.base_url,
        endpoint.adapter_hint.as_deref(),
    );
    let route = RelayRoute::ImageGenerations;
    DiagnosticProbeRequest {
        log_path: route.path().to_string(),
        url: adapter.resolve_url(&endpoint.base_url, route),
        extra_headers: reqwest::header::HeaderMap::new(),
        body: json!({
            "model": model,
            "prompt": "ping",
            "size": "1024x1024",
            "n": 1
        }),
    }
}

fn upstream_protocol_from_str(protocol: &str) -> AppResult<UpstreamProtocol> {
    match protocol {
        "openai" => Ok(UpstreamProtocol::Openai),
        "openai_oauth" => Ok(UpstreamProtocol::OpenAiOauth),
        "anthropic" => Ok(UpstreamProtocol::Anthropic),
        other => Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
    }
}

fn probe_model(endpoint: &EndpointTarget) -> Option<String> {
    endpoint.models.first().cloned()
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
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT ce.id, c.provider, ce.protocol, ce.base_url, ce.adapter_hint,
                COALESCE(cm.models, ARRAY[]::TEXT[]) AS models,
                COALESCE(cm.image_models, ARRAY[]::TEXT[]) AS image_models,
                COALESCE(cm.video_models, ARRAY[]::TEXT[]) AS video_models,
                ce.enabled
         FROM channel_endpoint ce
         JOIN channel c ON c.id = ce.channel_id
         LEFT JOIN LATERAL (
             SELECT array_agg(cm.model ORDER BY cm.model) FILTER (
                        WHERE cp.billing_meter = 'token'
                          AND cp.enabled = TRUE
                          AND {BILLABLE_PRICE_CONDITION_CP}
                    ) AS models,
                    array_agg(cm.model ORDER BY cm.model) FILTER (
                        WHERE cp.billing_meter = 'image'
                          AND cp.enabled = TRUE
                          AND {BILLABLE_PRICE_CONDITION_CP}
                    ) AS image_models,
                    array_agg(cm.model ORDER BY cm.model) FILTER (
                        WHERE cp.billing_meter = 'video'
                          AND cp.enabled = TRUE
                          AND {BILLABLE_PRICE_CONDITION_CP}
                    ) AS video_models
             FROM channel_model cm
             JOIN channel_price cp
               ON cp.channel_id = cm.channel_id
              AND cp.model = cm.model
             WHERE cm.channel_id = ce.channel_id
         ) cm ON TRUE
         WHERE ce.channel_id = $1
         ORDER BY CASE protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END,
                  ce.created_at ASC"
    )))
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
            adapter_hint: row.try_get("adapter_hint").unwrap_or_default(),
            models: row.try_get("models").unwrap_or_default(),
            image_models: row.try_get("image_models").unwrap_or_default(),
            video_models: row.try_get("video_models").unwrap_or_default(),
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
            masked_key: None,
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
            let secret = state.secrets.plaintext(id, &ciphertext)?;
            Ok(KeyTarget {
                id: Some(id),
                name: row.try_get("name")?,
                masked_key: Some(mask_channel_key(&secret)),
                key_prefix: row.try_get("key_prefix")?,
                secret,
                enabled: row.try_get("enabled")?,
            })
        })
        .collect()
}

async fn persist_key_health(
    state: &AppState,
    key_id: DbId,
    report: &KeyDiagnosticReport,
) -> AppResult<()> {
    let recovered =
        report.status == DiagnosticStatus::Ok || report.status == DiagnosticStatus::Warning;
    if !recovered
        && (!key_report_has_hard_cooldown_failure(report)
            || !can_cooldown_key(state, key_id).await?)
    {
        tracing::info!(
            key_id,
            "skipping diagnostic key cooldown because it would remove the last routable path or the error is not key-scoped"
        );
        persist_key_soft_failure(state, key_id, Some(&report.summary)).await?;
        return Ok(());
    }
    let cooldown_until =
        (!recovered).then(|| Utc::now() + ChronoDuration::minutes(DIAGNOSTIC_COOLDOWN_MINUTES));
    sqlx::query(
        "UPDATE channel_key
         SET healthy = TRUE,
             last_error = $2,
             cooldown_until = $3,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(key_id)
    .bind((!recovered).then_some(report.summary.as_str()))
    .bind(cooldown_until)
    .execute(&state.db.pool)
    .await?;
    invalidate_routing(state).await;
    Ok(())
}

async fn persist_endpoint_health(
    state: &AppState,
    endpoint_id: DbId,
    report: &EndpointDiagnosticReport,
) -> AppResult<()> {
    let recovered =
        report.status == DiagnosticStatus::Ok || report.status == DiagnosticStatus::Warning;
    if !recovered
        && (!endpoint_report_has_hard_cooldown_failure(report)
            || !can_cooldown_endpoint(state, endpoint_id).await?)
    {
        tracing::info!(
            endpoint_id,
            "skipping diagnostic endpoint cooldown because it would remove the last routable path or the error is soft"
        );
        persist_endpoint_soft_failure(state, endpoint_id, Some(&report.summary)).await?;
        return Ok(());
    }
    let cooldown_until =
        (!recovered).then(|| Utc::now() + ChronoDuration::minutes(DIAGNOSTIC_COOLDOWN_MINUTES));
    sqlx::query(
        "UPDATE channel_endpoint
         SET healthy = TRUE,
             last_error = $2,
             cooldown_until = $3,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(endpoint_id)
    .bind((!recovered).then_some(report.summary.as_str()))
    .bind(cooldown_until)
    .execute(&state.db.pool)
    .await?;
    invalidate_routing(state).await;
    Ok(())
}

async fn persist_endpoint_soft_failure(
    state: &AppState,
    endpoint_id: DbId,
    summary: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE channel_endpoint
         SET last_error = $2,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(endpoint_id)
    .bind(summary)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn persist_key_soft_failure(
    state: &AppState,
    key_id: DbId,
    summary: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE channel_key
         SET last_error = $2,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(key_id)
    .bind(summary)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn invalidate_routing(state: &AppState) {
    state
        .cache_invalidator
        .invalidate(state, InvalidationEvent::Routing)
        .await;
}

async fn persist_model_probe_results_best_effort(
    state: &AppState,
    endpoint: &EndpointTarget,
    key_reports: &[KeyDiagnosticReport],
) {
    if let Err(err) = persist_model_probe_results(state, endpoint, key_reports).await {
        tracing::warn!(
            endpoint_id = endpoint.id,
            provider = %endpoint.provider,
            error = %err,
            "failed to persist diagnostic model probe results"
        );
    }
}

#[derive(Clone)]
struct ModelProbeSummary {
    status: DiagnosticStatus,
    message: String,
    status_code: Option<u16>,
}

async fn persist_model_probe_results(
    state: &AppState,
    endpoint: &EndpointTarget,
    key_reports: &[KeyDiagnosticReport],
) -> AppResult<()> {
    let mut summaries = std::collections::HashMap::<String, ModelProbeSummary>::new();
    for step in key_reports
        .iter()
        .flat_map(|report| report.steps.iter())
        .filter(|step| step.name.starts_with("probe:"))
    {
        let model = step.name.trim_start_matches("probe:").to_string();
        summaries
            .entry(model)
            .and_modify(|summary| merge_model_probe_summary(summary, step))
            .or_insert_with(|| ModelProbeSummary {
                status: step.status,
                message: step.message.clone(),
                status_code: step.status_code,
            });
    }

    for (model, summary) in summaries {
        match summary.status {
            DiagnosticStatus::Ok | DiagnosticStatus::Warning => {
                persist_model_probe_success(state, endpoint, &model, summary.status_code).await?;
            }
            DiagnosticStatus::Failed => {
                persist_model_probe_failure(state, endpoint, &model, &summary).await?;
            }
            DiagnosticStatus::Skipped => {}
        }
    }
    Ok(())
}

fn merge_model_probe_summary(summary: &mut ModelProbeSummary, step: &DiagnosticStep) {
    if summary.status == DiagnosticStatus::Ok || step.status == DiagnosticStatus::Skipped {
        return;
    }
    if step.status == DiagnosticStatus::Ok {
        summary.status = DiagnosticStatus::Ok;
        summary.message = step.message.clone();
        summary.status_code = step.status_code;
        return;
    }
    if summary.status != DiagnosticStatus::Failed && step.status == DiagnosticStatus::Failed {
        summary.status = DiagnosticStatus::Failed;
        summary.message = step.message.clone();
        summary.status_code = step.status_code;
    }
}

async fn persist_model_probe_success(
    state: &AppState,
    endpoint: &EndpointTarget,
    model: &str,
    status_code: Option<u16>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE channel_model cm
         SET runtime_status = 'normal',
             cooldown_until = NULL,
             last_error = NULL,
             last_status_code = $3,
             last_probe_at = now(),
             success_count = success_count + 1,
             updated_at = now()
         FROM channel_endpoint ce
         WHERE ce.id = $1
           AND cm.channel_id = ce.channel_id
           AND cm.model = $2",
    )
    .bind(endpoint.id)
    .bind(model)
    .bind(status_code.map(i32::from))
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn persist_model_probe_failure(
    state: &AppState,
    endpoint: &EndpointTarget,
    model: &str,
    summary: &ModelProbeSummary,
) -> AppResult<()> {
    // 鉴权/配置类硬错误才禁用模型；上游临时不可用(5xx)、限流(429)等软失败
    // 仅记录错误并累加失败计数，不禁用——避免探针瞬时失败误伤正常模型。
    let should_disable = should_disable_model_on_failure(summary.status_code)
        && can_disable_channel_model_for_endpoint(state, endpoint.id, model).await?;
    if should_disable_model_on_failure(summary.status_code) && !should_disable {
        tracing::info!(
            endpoint_id = endpoint.id,
            provider = %endpoint.provider,
            protocol = %endpoint.protocol,
            model,
            "skipping diagnostic model disable because it would remove the last routable path"
        );
    }
    sqlx::query(
        "UPDATE channel_model cm
         SET enabled = CASE WHEN $5 THEN FALSE ELSE cm.enabled END,
             runtime_status = CASE WHEN $5 THEN 'failed' ELSE cm.runtime_status END,
             cooldown_until = CASE WHEN $5 THEN NULL ELSE cm.cooldown_until END,
             last_error = $3,
             last_status_code = $4,
             last_probe_at = now(),
             failure_count = failure_count + 1,
             updated_at = now()
         FROM channel_endpoint ce
         WHERE ce.id = $1
           AND cm.channel_id = ce.channel_id
           AND cm.model = $2",
    )
    .bind(endpoint.id)
    .bind(model)
    .bind(summary.message.chars().take(500).collect::<String>())
    .bind(summary.status_code.map(i32::from))
    .bind(should_disable)
    .execute(&state.db.pool)
    .await?;
    if should_disable {
        invalidate_routing(state).await;
    }
    Ok(())
}

/// 判定模型探测失败是否应当禁用该模型。
///
/// 仅鉴权/配置类硬错误才禁用；上游临时不可用(5xx)、限流(429)等不禁用，
/// 与定时探测的冷却门槛保持一致，避免瞬态错误误伤正常模型。
fn should_disable_model_on_failure(status_code: Option<u16>) -> bool {
    matches!(status_code, Some(401) | Some(403) | Some(404))
}

fn should_cooldown_endpoint_on_failure(status_code: Option<u16>) -> bool {
    matches!(status_code, Some(401) | Some(403) | Some(404))
}

fn should_cooldown_key_on_failure(status_code: Option<u16>) -> bool {
    matches!(status_code, Some(401) | Some(403))
}

fn endpoint_report_has_hard_cooldown_failure(report: &EndpointDiagnosticReport) -> bool {
    report
        .keys
        .iter()
        .flat_map(|key| key.steps.iter())
        .filter(|step| step.status == DiagnosticStatus::Failed)
        .any(|step| should_cooldown_endpoint_on_failure(step.status_code))
}

fn key_report_has_hard_cooldown_failure(report: &KeyDiagnosticReport) -> bool {
    report
        .steps
        .iter()
        .filter(|step| step.status == DiagnosticStatus::Failed)
        .any(|step| should_cooldown_key_on_failure(step.status_code))
}

async fn can_cooldown_endpoint(state: &AppState, endpoint_id: DbId) -> AppResult<bool> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        WITH target AS (
            SELECT ce.id, ce.channel_id, ce.protocol, ce.models
            FROM channel_endpoint ce
            WHERE ce.id = $1
              AND ce.enabled = TRUE
              AND ce.healthy = TRUE
        ),
        affected AS (
            SELECT DISTINCT target.protocol, cm.model
            FROM target
            JOIN channel c ON c.id = target.channel_id
            JOIN channel_model cm ON cm.channel_id = target.channel_id
            JOIN channel_price cp ON cp.channel_id = c.id
                                  AND cp.model = cm.model
                                  AND cp.enabled = TRUE
                                  AND {BILLABLE_PRICE_CONDITION_CP}
            WHERE cm.enabled = TRUE
              AND cm.status = 'available'
              AND (
                  cm.runtime_status = 'normal'
                  OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
              )
              AND (
                  EXISTS (
                      SELECT 1
                      FROM unnest(target.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) = cm.model
                  )
                  OR NOT EXISTS (
                      SELECT 1
                      FROM unnest(target.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) <> ''
                  )
              )
        )
        SELECT
            (SELECT count(*) FROM affected) AS affected_count,
            (
                SELECT count(*)
                FROM affected a
                WHERE EXISTS (
                    SELECT 1
                    FROM channel c
                    JOIN provider p ON p.code = c.provider
                    JOIN channel_endpoint ce ON ce.channel_id = c.id
                    JOIN channel_model cm ON cm.channel_id = c.id
                    WHERE p.enabled = TRUE
                      AND c.enabled = TRUE
                      AND ce.id <> $1
                      AND ce.protocol = a.protocol
                      AND ce.enabled = TRUE
                      AND ce.healthy = TRUE
                      AND (ce.cooldown_until IS NULL OR ce.cooldown_until <= now())
                      AND cm.model = a.model
                      AND cm.enabled = TRUE
                      AND cm.status = 'available'
                      AND EXISTS (
                          SELECT 1
                          FROM channel_price cp
                          WHERE cp.channel_id = c.id
                            AND cp.model = cm.model
                            AND cp.enabled = TRUE
                            AND {BILLABLE_PRICE_CONDITION_CP}
                      )
                      AND (
                          cm.runtime_status = 'normal'
                          OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
                      )
                      AND (
                          EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) = cm.model
                          )
                          OR NOT EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) <> ''
                          )
                      )
                      AND (
                          (
                              c.use_credentials = FALSE
                              AND EXISTS (
                                  SELECT 1 FROM channel_key ck
                                  WHERE ck.channel_id = c.id
                                    AND ck.enabled = TRUE
                                    AND ck.healthy = TRUE
                                    AND (ck.cooldown_until IS NULL OR ck.cooldown_until <= now())
                              )
                          )
                          OR (
                              c.use_credentials = TRUE
                              AND EXISTS (
                                  SELECT 1 FROM credential cr
                                  WHERE cr.provider = c.provider
                                    AND cr.enabled = TRUE
                              )
                          )
                      )
                )
            ) AS covered_count
        "#,
    )))
    .bind(endpoint_id)
    .fetch_one(&state.db.pool)
    .await?;

    let affected_count: i64 = row.try_get("affected_count")?;
    let covered_count: i64 = row.try_get("covered_count")?;
    Ok(affected_count > 0 && affected_count == covered_count)
}

async fn can_cooldown_key(state: &AppState, key_id: DbId) -> AppResult<bool> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        WITH target_key AS (
            SELECT ck.id, ck.channel_id
            FROM channel_key ck
            WHERE ck.id = $1
              AND ck.enabled = TRUE
              AND ck.healthy = TRUE
              AND (ck.cooldown_until IS NULL OR ck.cooldown_until <= now())
        ),
        affected AS (
            SELECT DISTINCT ce.protocol, cm.model
            FROM target_key tk
            JOIN channel c ON c.id = tk.channel_id
            JOIN provider p ON p.code = c.provider
            JOIN channel_endpoint ce ON ce.channel_id = c.id
            JOIN channel_model cm ON cm.channel_id = c.id
            JOIN channel_price cp ON cp.channel_id = c.id
                                  AND cp.model = cm.model
                                  AND cp.enabled = TRUE
                                  AND {BILLABLE_PRICE_CONDITION_CP}
            WHERE p.enabled = TRUE
              AND c.enabled = TRUE
              AND ce.enabled = TRUE
              AND ce.healthy = TRUE
              AND (ce.cooldown_until IS NULL OR ce.cooldown_until <= now())
              AND cm.enabled = TRUE
              AND cm.status = 'available'
              AND (
                  cm.runtime_status = 'normal'
                  OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
              )
              AND (
                  EXISTS (
                      SELECT 1
                      FROM unnest(ce.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) = cm.model
                  )
                  OR NOT EXISTS (
                      SELECT 1
                      FROM unnest(ce.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) <> ''
                  )
              )
        )
        SELECT
            (SELECT count(*) FROM affected) AS affected_count,
            (
                SELECT count(*)
                FROM affected a
                WHERE EXISTS (
                    SELECT 1
                    FROM channel c
                    JOIN provider p ON p.code = c.provider
                    JOIN channel_endpoint ce ON ce.channel_id = c.id
                    JOIN channel_model cm ON cm.channel_id = c.id
                    WHERE p.enabled = TRUE
                      AND c.enabled = TRUE
                      AND ce.protocol = a.protocol
                      AND ce.enabled = TRUE
                      AND ce.healthy = TRUE
                      AND (ce.cooldown_until IS NULL OR ce.cooldown_until <= now())
                      AND cm.model = a.model
                      AND cm.enabled = TRUE
                      AND cm.status = 'available'
                      AND EXISTS (
                          SELECT 1
                          FROM channel_price cp
                          WHERE cp.channel_id = c.id
                            AND cp.model = cm.model
                            AND cp.enabled = TRUE
                            AND {BILLABLE_PRICE_CONDITION_CP}
                      )
                      AND (
                          cm.runtime_status = 'normal'
                          OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
                      )
                      AND (
                          EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) = cm.model
                          )
                          OR NOT EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) <> ''
                          )
                      )
                      AND (
                          (
                              c.use_credentials = FALSE
                              AND EXISTS (
                                  SELECT 1 FROM channel_key ck
                                  WHERE ck.channel_id = c.id
                                    AND ck.id <> $1
                                    AND ck.enabled = TRUE
                                    AND ck.healthy = TRUE
                                    AND (ck.cooldown_until IS NULL OR ck.cooldown_until <= now())
                              )
                          )
                          OR (
                              c.use_credentials = TRUE
                              AND EXISTS (
                                  SELECT 1 FROM credential cr
                                  WHERE cr.provider = c.provider
                                    AND cr.enabled = TRUE
                              )
                          )
                      )
                )
            ) AS covered_count
        "#,
    )))
    .bind(key_id)
    .fetch_one(&state.db.pool)
    .await?;

    let affected_count: i64 = row.try_get("affected_count")?;
    let covered_count: i64 = row.try_get("covered_count")?;
    Ok(affected_count > 0 && affected_count == covered_count)
}

async fn can_disable_channel_model_for_endpoint(
    state: &AppState,
    endpoint_id: DbId,
    model: &str,
) -> AppResult<bool> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        WITH target AS (
            SELECT ce.channel_id
            FROM channel_endpoint ce
            WHERE ce.id = $1
        ),
        affected AS (
            SELECT DISTINCT ce.protocol, cm.model
            FROM target
            JOIN channel c ON c.id = target.channel_id
            JOIN channel_endpoint ce ON ce.channel_id = c.id
            JOIN channel_model cm ON cm.channel_id = c.id
            JOIN channel_price cp ON cp.channel_id = c.id
                                  AND cp.model = cm.model
                                  AND cp.enabled = TRUE
                                  AND {BILLABLE_PRICE_CONDITION_CP}
            WHERE ce.enabled = TRUE
              AND ce.healthy = TRUE
              AND (ce.cooldown_until IS NULL OR ce.cooldown_until <= now())
              AND cm.model = $2
              AND cm.enabled = TRUE
              AND cm.status = 'available'
              AND (
                  cm.runtime_status = 'normal'
                  OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
              )
              AND (
                  EXISTS (
                      SELECT 1
                      FROM unnest(ce.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) = cm.model
                  )
                  OR NOT EXISTS (
                      SELECT 1
                      FROM unnest(ce.models) AS endpoint_model(model)
                      WHERE btrim(endpoint_model.model) <> ''
                  )
              )
        )
        SELECT
            (SELECT count(*) FROM affected) AS affected_count,
            (
                SELECT count(*)
                FROM affected a
                WHERE EXISTS (
                    SELECT 1
                    FROM target
                    JOIN channel c ON c.id <> target.channel_id
                    JOIN provider p ON p.code = c.provider
                    JOIN channel_endpoint ce ON ce.channel_id = c.id
                    JOIN channel_model cm ON cm.channel_id = c.id
                    WHERE p.enabled = TRUE
                      AND c.enabled = TRUE
                      AND ce.protocol = a.protocol
                      AND ce.enabled = TRUE
                      AND ce.healthy = TRUE
                      AND (ce.cooldown_until IS NULL OR ce.cooldown_until <= now())
                      AND cm.model = a.model
                      AND cm.enabled = TRUE
                      AND cm.status = 'available'
                      AND EXISTS (
                          SELECT 1
                          FROM channel_price cp
                          WHERE cp.channel_id = c.id
                            AND cp.model = cm.model
                            AND cp.enabled = TRUE
                            AND {BILLABLE_PRICE_CONDITION_CP}
                      )
                      AND (
                          cm.runtime_status = 'normal'
                          OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
                      )
                      AND (
                          EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) = cm.model
                          )
                          OR NOT EXISTS (
                              SELECT 1
                              FROM unnest(ce.models) AS endpoint_model(model)
                              WHERE btrim(endpoint_model.model) <> ''
                          )
                      )
                      AND (
                          (
                              c.use_credentials = FALSE
                              AND EXISTS (
                                  SELECT 1 FROM channel_key ck
                                  WHERE ck.channel_id = c.id
                                    AND ck.enabled = TRUE
                                    AND ck.healthy = TRUE
                                    AND (ck.cooldown_until IS NULL OR ck.cooldown_until <= now())
                              )
                          )
                          OR (
                              c.use_credentials = TRUE
                              AND EXISTS (
                                  SELECT 1 FROM credential cr
                                  WHERE cr.provider = c.provider
                                    AND cr.enabled = TRUE
                              )
                          )
                      )
                )
            ) AS covered_count
        "#,
    )))
    .bind(endpoint_id)
    .bind(model)
    .fetch_one(&state.db.pool)
    .await?;

    let affected_count: i64 = row.try_get("affected_count")?;
    let covered_count: i64 = row.try_get("covered_count")?;
    Ok(affected_count > 0 && affected_count == covered_count)
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

fn upstream_status_message(status: StatusCode, error_summary: &str) -> String {
    match status.as_u16() {
        400 => "上游拒绝测试请求，请检查模型名或请求协议".to_string(),
        401 | 403 => "认证失败，请检查上游 Key 或凭证权限".to_string(),
        404 => "接口不存在，请检查 Base URL 和协议".to_string(),
        429 if upstream_quota_exhausted(error_summary) => {
            "上游配额已耗尽，请充值、扩容套餐或更换 Key".to_string()
        }
        429 => "上游限流，请稍后重试或切换 Key".to_string(),
        500..=599 => "上游服务暂时不可用".to_string(),
        _ => format!("上游返回 HTTP {}", status.as_u16()),
    }
}

async fn upstream_failure_message(status: StatusCode, response: reqwest::Response) -> String {
    match response.text().await {
        Ok(body) => {
            let summary = upstream_error_body_summary(&body);
            let base = upstream_status_message(status, &summary);
            if summary.is_empty() {
                base
            } else {
                format!("{base}: {summary}")
            }
        }
        Err(_) => upstream_status_message(status, ""),
    }
}

fn upstream_quota_exhausted(error_summary: &str) -> bool {
    let normalized = error_summary.to_ascii_lowercase();
    normalized.contains("allocationquota")
        || normalized.contains("quota has been exhausted")
        || normalized.contains("quota exhausted")
        || normalized.contains("insufficient_quota")
}

fn upstream_error_body_summary(body: &str) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        if let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message").or_else(|| error.get("detail")))
            .and_then(Value::as_str)
            .or_else(|| value.get("message").and_then(Value::as_str))
            .or_else(|| value.get("detail").and_then(Value::as_str))
        {
            return truncate_error_summary(message);
        }
    }
    truncate_error_summary(trimmed)
}

fn truncate_error_summary(value: &str) -> String {
    const MAX_CHARS: usize = 300;
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.chars().count() <= MAX_CHARS {
        return value;
    }
    let mut output: String = value.chars().take(MAX_CHARS).collect();
    output.push_str("...");
    output
}

fn transport_error_message(err: &AppError) -> String {
    if let AppError::Reqwest(err) = err {
        return match UpstreamErrorKind::from_reqwest(err) {
            UpstreamErrorKind::Timeout => "连接上游超时，请检查网络或上游状态".to_string(),
            UpstreamErrorKind::Tls => "TLS 握手失败，请检查 Base URL 证书".to_string(),
            UpstreamErrorKind::Dns => "DNS 解析失败，请检查 Base URL 域名".to_string(),
            UpstreamErrorKind::Connect => "无法连接上游，请检查网络、防火墙或 Base URL".to_string(),
            UpstreamErrorKind::Request => "上游请求失败，请检查网络和配置".to_string(),
        };
    }
    err.to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn failed_step(status_code: Option<u16>) -> DiagnosticStep {
        DiagnosticStep {
            name: "probe:gpt-test".to_string(),
            status: DiagnosticStatus::Failed,
            message: "failed".to_string(),
            duration_ms: 1,
            status_code,
        }
    }

    #[test]
    fn endpoint_and_key_hard_cooldown_classification_differs_for_404() {
        assert!(should_cooldown_endpoint_on_failure(Some(404)));
        assert!(!should_cooldown_key_on_failure(Some(404)));

        for code in [Some(401), Some(403)] {
            assert!(should_cooldown_endpoint_on_failure(code));
            assert!(should_cooldown_key_on_failure(code));
        }

        for code in [None, Some(400), Some(429), Some(500)] {
            assert!(!should_cooldown_endpoint_on_failure(code));
            assert!(!should_cooldown_key_on_failure(code));
        }
    }

    #[test]
    fn reports_detect_hard_cooldown_failures_from_steps() {
        let key_report_404 = KeyDiagnosticReport {
            key_id: Some(1),
            key_name: "key".to_string(),
            masked_key: None,
            key_prefix: None,
            status: DiagnosticStatus::Failed,
            summary: "failed".to_string(),
            discovered_models: Vec::new(),
            steps: vec![failed_step(Some(404))],
        };
        let endpoint_report_404 = EndpointDiagnosticReport {
            endpoint_id: 1,
            protocol: "openai".to_string(),
            base_url: "https://example.com".to_string(),
            status: DiagnosticStatus::Failed,
            summary: "failed".to_string(),
            discovered_models: Vec::new(),
            configured_models: Vec::new(),
            missing_configured_models: Vec::new(),
            keys: vec![key_report_404.clone()],
        };

        assert!(endpoint_report_has_hard_cooldown_failure(
            &endpoint_report_404
        ));
        assert!(!key_report_has_hard_cooldown_failure(&key_report_404));

        let key_report_401 = KeyDiagnosticReport {
            steps: vec![failed_step(Some(401))],
            ..key_report_404
        };
        assert!(key_report_has_hard_cooldown_failure(&key_report_401));
    }

    #[test]
    fn upstream_error_summary_reads_nested_error_message() {
        let body = r#"{"error":{"message":"model permission denied","type":"forbidden"}}"#;

        assert_eq!(upstream_error_body_summary(body), "model permission denied");
    }

    #[test]
    fn upstream_error_summary_truncates_plain_text() {
        let body = "x".repeat(400);

        assert!(upstream_error_body_summary(&body).ends_with("..."));
    }

    #[test]
    fn allocation_quota_is_reported_as_exhausted_quota() {
        assert_eq!(
            upstream_status_message(
                StatusCode::TOO_MANY_REQUESTS,
                "Your token-plan quota has been exhausted.; code=Throttling.AllocationQuota",
            ),
            "上游配额已耗尽，请充值、扩容套餐或更换 Key"
        );
    }

    #[test]
    fn ordinary_429_is_reported_as_rate_limited() {
        assert_eq!(
            upstream_status_message(StatusCode::TOO_MANY_REQUESTS, "request limit exceeded"),
            "上游限流，请稍后重试或切换 Key"
        );
    }
}
