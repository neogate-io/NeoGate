use std::time::Instant;

use anyhow::{Context, Result};
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::StatusCode;
use serde_json::{json, Value};
use sqlx::Row;

use crate::app::AppContext;

type DbId = i64;

const DIAGNOSTIC_COOLDOWN_MINUTES: i64 = 5;
const PROBE_SAMPLE_RETENTION_DAYS: i64 = 7;
const OPENAI_OAUTH_PROTOCOL: &str = "openai_oauth";
const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

struct ChannelTarget {
    id: DbId,
    provider: String,
    enabled: bool,
    use_credentials: bool,
}

struct EndpointTarget {
    id: DbId,
    protocol: String,
    base_url: String,
    models: Vec<String>,
    enabled: bool,
}

struct KeyTarget {
    id: Option<DbId>,
    secret: String,
    enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProbeStatus {
    Ok,
    Failed,
    Skipped,
}

impl ProbeStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DiagnosticStatus {
    Ok,
    Failed,
}

struct DiagnosticStep {
    status: DiagnosticStatus,
    message: String,
    duration_ms: i64,
    status_code: Option<u16>,
}

struct ProbeOutcome {
    endpoint_id: Option<DbId>,
    key_id: Option<DbId>,
    protocol: String,
    model: String,
    status: ProbeStatus,
    latency_ms: Option<i64>,
    status_code: Option<u16>,
    error_summary: Option<String>,
}

pub(crate) async fn run(context: &AppContext) -> Result<()> {
    let channel_ids = due_channel_ids(context).await?;
    if channel_ids.is_empty() {
        cleanup_probe_samples(context).await?;
        return Ok(());
    }

    tracing::info!(count = channel_ids.len(), "probing due upstream channels");
    for channel_id in channel_ids {
        if let Err(err) = probe_channel_once(context, channel_id).await {
            tracing::warn!(channel_id, error = %err, "failed to probe upstream channel");
        }
    }

    cleanup_probe_samples(context).await?;
    Ok(())
}

async fn due_channel_ids(context: &AppContext) -> Result<Vec<DbId>> {
    let rows = sqlx::query(
        r#"
        SELECT c.id
        FROM channel c
        WHERE c.enabled = TRUE
          AND EXISTS (
              SELECT 1
              FROM channel_endpoint ce
              JOIN channel_model cm ON cm.channel_id = c.id
              WHERE ce.channel_id = c.id
                AND ce.enabled = TRUE
                AND cm.enabled = TRUE
                AND cm.status = 'available'
                AND ce.protocol <> 'openai_oauth'
          )
          AND (
              (
                  SELECT cps.created_at
                  FROM channel_probe cps
                  WHERE cps.channel_id = c.id
                  ORDER BY cps.created_at DESC, cps.id DESC
                  LIMIT 1
              ) IS NULL
              OR (
                  SELECT cps.created_at
                  FROM channel_probe cps
                  WHERE cps.channel_id = c.id
                  ORDER BY cps.created_at DESC, cps.id DESC
                  LIMIT 1
              ) <= now() - interval '10 minutes'
          )
        ORDER BY c.priority DESC, c.created_at ASC
        LIMIT 50
        "#,
    )
    .fetch_all(&context.db)
    .await?;

    rows.iter()
        .map(|row| row.try_get("id").context("missing channel id"))
        .collect()
}

async fn probe_channel_once(context: &AppContext, channel_id: DbId) -> Result<()> {
    let channel = load_channel(context, channel_id).await?;
    let endpoints = load_endpoints(context, channel_id).await?;
    let keys = load_keys(context, &channel).await?;
    let outcome = run_channel_probe(context, &channel, &endpoints, &keys).await;
    persist_probe_sample(context, channel.id, &outcome).await?;
    if let Some(endpoint_id) = outcome.endpoint_id {
        persist_endpoint_probe_health(context, endpoint_id, &outcome).await?;
    }
    if let Some(key_id) = outcome.key_id {
        persist_key_probe_health(context, key_id, &outcome).await?;
    }
    Ok(())
}

async fn load_channel(context: &AppContext, channel_id: DbId) -> Result<ChannelTarget> {
    let row = sqlx::query(
        "SELECT id, provider, enabled, use_credentials
         FROM channel
         WHERE id = $1",
    )
    .bind(channel_id)
    .fetch_optional(&context.db)
    .await?
    .context("channel not found")?;

    Ok(ChannelTarget {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        enabled: row.try_get("enabled")?,
        use_credentials: row.try_get("use_credentials")?,
    })
}

async fn load_endpoints(context: &AppContext, channel_id: DbId) -> Result<Vec<EndpointTarget>> {
    let rows = sqlx::query(
        "SELECT id, protocol, base_url, models, enabled
         FROM channel_endpoint
         WHERE channel_id = $1
         ORDER BY CASE protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END,
                  created_at ASC",
    )
    .bind(channel_id)
    .fetch_all(&context.db)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(EndpointTarget {
                id: row.try_get("id")?,
                protocol: row.try_get("protocol")?,
                base_url: row.try_get("base_url")?,
                models: row.try_get("models")?,
                enabled: row.try_get("enabled")?,
            })
        })
        .collect()
}

async fn load_keys(context: &AppContext, channel: &ChannelTarget) -> Result<Vec<KeyTarget>> {
    if channel.use_credentials {
        let secret = runtime_secret_from_enabled_credential(context, &channel.provider).await?;
        return Ok(vec![KeyTarget {
            id: None,
            secret,
            enabled: true,
        }]);
    }

    let rows = sqlx::query(
        "SELECT id, secret_ciphertext, enabled
         FROM channel_key
         WHERE channel_id = $1
         ORDER BY enabled DESC, healthy DESC, created_at ASC",
    )
    .bind(channel.id)
    .fetch_all(&context.db)
    .await?;

    rows.iter()
        .map(|row| {
            let ciphertext: String = row.try_get("secret_ciphertext")?;
            Ok(KeyTarget {
                id: Some(row.try_get("id")?),
                secret: context.secrets.plaintext(&ciphertext)?,
                enabled: row.try_get("enabled")?,
            })
        })
        .collect()
}

async fn runtime_secret_from_enabled_credential(
    context: &AppContext,
    provider: &str,
) -> Result<String> {
    if provider != "openai" {
        anyhow::bail!("credential files are only supported for OpenAI providers");
    }

    let row = sqlx::query(
        "SELECT content_ciphertext
         FROM credential
         WHERE provider = $1 AND enabled = true
         ORDER BY updated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(provider)
    .fetch_optional(&context.db)
    .await?
    .context("no enabled OpenAI credential file")?;

    let content_ciphertext: String = row.try_get("content_ciphertext")?;
    let value: Value = serde_json::from_str(&context.secrets.plaintext(&content_ciphertext)?)?;
    openai_runtime_secret(&value).context("credential file does not contain a usable OpenAI token")
}

fn openai_runtime_secret(value: &Value) -> Option<String> {
    value
        .get("OPENAI_API_KEY")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|secret| !secret.is_empty())
        .map(str::to_string)
        .or_else(|| {
            value
                .get("access_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(str::to_string)
        })
}

async fn run_channel_probe(
    context: &AppContext,
    channel: &ChannelTarget,
    endpoints: &[EndpointTarget],
    keys: &[KeyTarget],
) -> ProbeOutcome {
    let Some(endpoint) = endpoints.iter().find(|endpoint| {
        endpoint.enabled
            && endpoint.protocol != OPENAI_OAUTH_PROTOCOL
            && probe_model(endpoint).is_some()
    }) else {
        return ProbeOutcome {
            endpoint_id: None,
            key_id: None,
            protocol: String::new(),
            model: String::new(),
            status: ProbeStatus::Skipped,
            latency_ms: None,
            status_code: None,
            error_summary: Some("没有可探测的文本模型端点".to_string()),
        };
    };

    if !channel.enabled {
        return ProbeOutcome {
            endpoint_id: Some(endpoint.id),
            key_id: None,
            protocol: endpoint.protocol.clone(),
            model: probe_model(endpoint).unwrap_or_default(),
            status: ProbeStatus::Skipped,
            latency_ms: None,
            status_code: None,
            error_summary: Some("通道已停用".to_string()),
        };
    }

    let Some(key) = keys.iter().find(|key| key.enabled) else {
        return ProbeOutcome {
            endpoint_id: Some(endpoint.id),
            key_id: None,
            protocol: endpoint.protocol.clone(),
            model: probe_model(endpoint).unwrap_or_default(),
            status: ProbeStatus::Failed,
            latency_ms: None,
            status_code: None,
            error_summary: Some("没有启用的上游 Key 或凭证".to_string()),
        };
    };

    let model = probe_model(endpoint).unwrap_or_default();
    let step = run_probe_step(context, endpoint, key, &model).await;
    ProbeOutcome {
        endpoint_id: Some(endpoint.id),
        key_id: key.id,
        protocol: endpoint.protocol.clone(),
        model,
        status: if step.status == DiagnosticStatus::Ok {
            ProbeStatus::Ok
        } else {
            ProbeStatus::Failed
        },
        latency_ms: Some(step.duration_ms),
        status_code: step.status_code,
        error_summary: (step.status != DiagnosticStatus::Ok).then_some(step.message),
    }
}

async fn run_probe_step(
    context: &AppContext,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    model: &str,
) -> DiagnosticStep {
    let started = Instant::now();
    let (path, body) = probe_request(endpoint, model);
    tracing::info!(
        endpoint_id = endpoint.id,
        protocol = %endpoint.protocol,
        base_url = %endpoint.base_url,
        model = %model,
        path = %path,
        "scheduled channel probe request started"
    );
    let response = upstream_request(context, endpoint, key, "POST", path, Some(body))
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
                    model = %model,
                    status = status.as_u16(),
                    duration_ms,
                    "scheduled channel probe request succeeded"
                );
            } else {
                tracing::warn!(
                    endpoint_id = endpoint.id,
                    protocol = %endpoint.protocol,
                    model = %model,
                    status = status.as_u16(),
                    duration_ms,
                    "scheduled channel probe request failed"
                );
            }
            DiagnosticStep {
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
                model = %model,
                duration_ms,
                error = %message,
                "scheduled channel probe request errored"
            );
            DiagnosticStep {
                status: DiagnosticStatus::Failed,
                message,
                duration_ms,
                status_code: None,
            }
        }
    }
}

fn upstream_request(
    context: &AppContext,
    endpoint: &EndpointTarget,
    key: &KeyTarget,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> reqwest::RequestBuilder {
    let url = upstream_url(&endpoint.base_url, path);
    let mut request = match method {
        "POST" => context.http.post(url),
        _ => context.http.get(url),
    };

    request = if endpoint.protocol == "anthropic" {
        request
            .header("x-api-key", &key.secret)
            .header("anthropic-version", DEFAULT_ANTHROPIC_VERSION)
    } else {
        request.bearer_auth(&key.secret)
    };

    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .json(&body);
    }
    request
}

fn probe_request(endpoint: &EndpointTarget, model: &str) -> (&'static str, Value) {
    if endpoint.protocol == "anthropic" {
        return (
            "/v1/messages",
            json!({
                "model": model,
                "max_tokens": 1,
                "messages": [{ "role": "user", "content": "ping" }]
            }),
        );
    }

    (
        "/v1/chat/completions",
        json!({
            "model": model,
            "max_tokens": 1,
            "messages": [{ "role": "user", "content": "ping" }]
        }),
    )
}

fn upstream_url(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if is_versioned_openai_compatible_base(base) && path.starts_with("/v1/") {
        format!("{}{}", base, &path[3..])
    } else {
        format!("{base}{path}")
    }
}

fn is_versioned_openai_compatible_base(base_url: &str) -> bool {
    let last_segment = base_url.rsplit('/').next().unwrap_or_default();
    if last_segment == "openai" {
        return true;
    }
    matches!(
        last_segment,
        "v1" | "v2" | "v3" | "v4" | "v1beta" | "v1beta1"
    )
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

async fn persist_probe_sample(
    context: &AppContext,
    channel_id: DbId,
    outcome: &ProbeOutcome,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO channel_probe
            (channel_id, channel_key_id, protocol, model,
             status, latency_ms, status_code, error_summary)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#,
    )
    .bind(channel_id)
    .bind(outcome.key_id)
    .bind(&outcome.protocol)
    .bind(&outcome.model)
    .bind(outcome.status.as_str())
    .bind(outcome.latency_ms)
    .bind(outcome.status_code.map(i32::from))
    .bind(&outcome.error_summary)
    .execute(&context.db)
    .await?;
    Ok(())
}

async fn persist_endpoint_probe_health(
    context: &AppContext,
    endpoint_id: DbId,
    outcome: &ProbeOutcome,
) -> Result<()> {
    match outcome.status {
        ProbeStatus::Ok => persist_endpoint_health(context, endpoint_id, true, None).await,
        ProbeStatus::Failed => {
            persist_endpoint_health(
                context,
                endpoint_id,
                false,
                outcome.error_summary.as_deref(),
            )
            .await
        }
        ProbeStatus::Skipped => Ok(()),
    }
}

async fn persist_key_probe_health(
    context: &AppContext,
    key_id: DbId,
    outcome: &ProbeOutcome,
) -> Result<()> {
    match outcome.status {
        ProbeStatus::Ok => persist_key_health(context, key_id, true, None).await,
        ProbeStatus::Failed => {
            persist_key_health(context, key_id, false, outcome.error_summary.as_deref()).await
        }
        ProbeStatus::Skipped => Ok(()),
    }
}

async fn persist_endpoint_health(
    context: &AppContext,
    endpoint_id: DbId,
    healthy: bool,
    summary: Option<&str>,
) -> Result<()> {
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
    .bind((!healthy).then_some(summary.unwrap_or("定时探测失败")))
    .bind(cooldown_until)
    .execute(&context.db)
    .await?;
    Ok(())
}

async fn persist_key_health(
    context: &AppContext,
    key_id: DbId,
    healthy: bool,
    summary: Option<&str>,
) -> Result<()> {
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
    .bind((!healthy).then_some(summary.unwrap_or("定时探测失败")))
    .bind(cooldown_until)
    .execute(&context.db)
    .await?;
    Ok(())
}

async fn cleanup_probe_samples(context: &AppContext) -> Result<()> {
    sqlx::query("DELETE FROM channel_probe WHERE created_at < now() - $1::interval")
        .bind(format!("{PROBE_SAMPLE_RETENTION_DAYS} days"))
        .execute(&context.db)
        .await?;
    Ok(())
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
    if err.is_timeout() {
        return "连接上游超时，请检查网络或上游状态".to_string();
    }

    let details = format!("{err:?}").to_ascii_lowercase();
    if details.contains("tls") {
        "TLS 握手失败，请检查 Base URL 证书".to_string()
    } else if details.contains("dns") {
        "DNS 解析失败，请检查 Base URL 域名".to_string()
    } else if details.contains("connect") {
        "无法连接上游，请检查网络、防火墙或 Base URL".to_string()
    } else {
        "上游请求失败，请检查网络和配置".to_string()
    }
}
