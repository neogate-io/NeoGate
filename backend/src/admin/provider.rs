use chrono::{DateTime, Utc};
use serde::Serialize;
#[cfg(test)]
use serde_json::json;
use serde_json::Value;
use sqlx::Row;

use crate::{
    error::{AppError, AppResult},
    AppState,
};

pub const OPENAI_OAUTH_PROTOCOL: &str = "openai_oauth";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AudioTranscriptionAdapter {
    AsyncFile,
    MultimodalGeneration,
    QwenRealtime,
}

pub(crate) fn catalog_audio_transcription_adapter(
    provider: &str,
    capabilities: &Value,
) -> Option<AudioTranscriptionAdapter> {
    if !provider.trim().eq_ignore_ascii_case("qwen") {
        return None;
    }
    let transcription = capabilities.pointer("/catalog/capabilities/audio_transcription")?;
    let interfaces = capabilities.pointer("/catalog/interfaces")?.as_array()?;
    interfaces.iter().find_map(|interface| {
        if interface.get("operation")?.as_str()? != "audio_transcription" {
            return None;
        }
        let mode = interface.get("mode")?.as_str()?;
        let transport = interface.get("transport")?.as_str()?;
        let protocol = interface.get("upstream_protocol")?.as_str()?;
        let request_style = interface.get("request_style")?.as_str()?;
        match (mode, transport, protocol, request_style) {
            ("file", "https", "dashscope_http", "dashscope_async_file")
                if transcription.get("file").and_then(Value::as_bool) == Some(true) =>
            {
                Some(AudioTranscriptionAdapter::AsyncFile)
            }
            ("file", "https", "dashscope_http", "dashscope_multimodal_generation")
                if transcription.get("file").and_then(Value::as_bool) == Some(true) =>
            {
                Some(AudioTranscriptionAdapter::MultimodalGeneration)
            }
            ("realtime", "websocket", "dashscope_websocket", "dashscope_qwen_realtime")
                if transcription.get("realtime").and_then(Value::as_bool) == Some(true) =>
            {
                Some(AudioTranscriptionAdapter::QwenRealtime)
            }
            _ => None,
        }
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRecord {
    pub id: i64,
    pub code: String,
    pub display_name: String,
    pub name: String,
    pub default_endpoints: Vec<ProviderDefaultEndpointRecord>,
    pub enabled: bool,
    pub sort_order: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderDefaultEndpointRecord {
    pub protocol: String,
    pub base_url: String,
}

pub async fn list_providers(state: &AppState) -> AppResult<Vec<ProviderRecord>> {
    let rows = sqlx::query(
        "SELECT id, code, display_name, name,
                default_openai_base_url, default_openai_oauth_base_url, default_anthropic_base_url,
                enabled, sort_order, created_at, updated_at
         FROM provider
         WHERE enabled = TRUE
         ORDER BY sort_order ASC, display_name ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;

    rows.iter().map(provider_from_row).collect()
}

pub async fn provider_default_endpoint_base_url(
    state: &AppState,
    code: &str,
    protocol: &str,
) -> AppResult<Option<String>> {
    let row = sqlx::query(
        "SELECT default_openai_base_url, default_openai_oauth_base_url, default_anthropic_base_url
         FROM provider
         WHERE code = $1 AND enabled = TRUE",
    )
    .bind(code)
    .fetch_optional(&state.db.pool)
    .await?;

    row.map(|row| match protocol {
        "openai" => row.try_get("default_openai_base_url").map_err(Into::into),
        "anthropic" => row
            .try_get("default_anthropic_base_url")
            .map_err(Into::into),
        OPENAI_OAUTH_PROTOCOL => row
            .try_get("default_openai_oauth_base_url")
            .map_err(Into::into),
        other => Err(AppError::BadRequest(format!("invalid protocol: {other}"))),
    })
    .transpose()
}

pub async fn provider_default_endpoints(
    state: &AppState,
    code: &str,
) -> AppResult<Option<Vec<ProviderDefaultEndpointRecord>>> {
    let row = sqlx::query(
        "SELECT default_openai_base_url, default_openai_oauth_base_url, default_anthropic_base_url
         FROM provider
         WHERE code = $1 AND enabled = TRUE",
    )
    .bind(code)
    .fetch_optional(&state.db.pool)
    .await?;

    row.map(|row| provider_default_endpoints_from_row(&row))
        .transpose()
}

pub async fn record_provider_models(
    state: &AppState,
    provider: &str,
    models: &[String],
    source: &str,
    enabled: bool,
) -> AppResult<()> {
    let mut seen = std::collections::HashSet::new();
    for model in models {
        let model = model.trim();
        if model.is_empty() || !seen.insert(model.to_string()) {
            continue;
        }
        sqlx::query(
            "INSERT INTO provider_model
             (provider, model, display_name, source, billing_meter, capabilities, enabled)
             VALUES ($1, $2, $2, $3, 'token', '{}'::JSONB, $4)
             ON CONFLICT (provider, model)
             DO UPDATE SET
                 display_name = CASE
                     WHEN provider_model.display_name = '' THEN EXCLUDED.display_name
                     ELSE provider_model.display_name
                 END,
                 source = EXCLUDED.source,
                 enabled = provider_model.enabled OR EXCLUDED.enabled,
                 discovered_at = CASE
                     WHEN EXCLUDED.source = 'upstream' THEN now()
                     ELSE provider_model.discovered_at
                 END,
                 updated_at = now()",
        )
        .bind(provider)
        .bind(model)
        .bind(source)
        .bind(enabled)
        .execute(&state.db.pool)
        .await?;
    }

    Ok(())
}

fn provider_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProviderRecord> {
    Ok(ProviderRecord {
        id: row.try_get("id")?,
        code: row.try_get("code")?,
        display_name: row.try_get("display_name")?,
        name: row.try_get("name")?,
        default_endpoints: provider_default_endpoints_from_row(row)?,
        enabled: row.try_get("enabled")?,
        sort_order: row.try_get("sort_order")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn provider_default_endpoints_from_row(
    row: &sqlx::postgres::PgRow,
) -> AppResult<Vec<ProviderDefaultEndpointRecord>> {
    Ok(vec![
        ProviderDefaultEndpointRecord {
            protocol: "openai".to_string(),
            base_url: row.try_get("default_openai_base_url")?,
        },
        ProviderDefaultEndpointRecord {
            protocol: OPENAI_OAUTH_PROTOCOL.to_string(),
            base_url: row.try_get("default_openai_oauth_base_url")?,
        },
        ProviderDefaultEndpointRecord {
            protocol: "anthropic".to_string(),
            base_url: row.try_get("default_anthropic_base_url")?,
        },
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_supported_audio_adapters_from_catalog_interfaces() {
        for (request_style, mode, transport, protocol, expected) in [
            (
                "dashscope_async_file",
                "file",
                "https",
                "dashscope_http",
                AudioTranscriptionAdapter::AsyncFile,
            ),
            (
                "dashscope_multimodal_generation",
                "file",
                "https",
                "dashscope_http",
                AudioTranscriptionAdapter::MultimodalGeneration,
            ),
            (
                "dashscope_qwen_realtime",
                "realtime",
                "websocket",
                "dashscope_websocket",
                AudioTranscriptionAdapter::QwenRealtime,
            ),
        ] {
            let capabilities = json!({
                "catalog": {
                    "capabilities": {
                        "audio_transcription": { "file": mode == "file", "realtime": mode == "realtime" }
                    },
                    "interfaces": [{
                        "operation": "audio_transcription",
                        "mode": mode,
                        "transport": transport,
                        "upstream_protocol": protocol,
                        "request_style": request_style
                    }]
                }
            });
            assert_eq!(
                catalog_audio_transcription_adapter("qwen", &capabilities),
                Some(expected)
            );
        }
    }

    #[test]
    fn rejects_missing_inconsistent_or_unsupported_catalog_interfaces() {
        let unsupported = json!({
            "catalog": {
                "capabilities": {
                    "audio_transcription": { "file": false, "realtime": true }
                },
                "interfaces": [{
                    "operation": "audio_transcription",
                    "mode": "realtime",
                    "transport": "websocket",
                    "upstream_protocol": "dashscope_websocket",
                    "request_style": "dashscope_realtime"
                }]
            }
        });
        assert_eq!(
            catalog_audio_transcription_adapter("qwen", &unsupported),
            None
        );
        assert_eq!(
            catalog_audio_transcription_adapter("custom", &unsupported),
            None
        );
        assert_eq!(
            catalog_audio_transcription_adapter("qwen", &json!({})),
            None
        );
    }
}
