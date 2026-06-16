use std::collections::HashMap;

use anyhow::{Context, Result};
use serde_json::Value;
use sqlx::Row;

use crate::app::AppContext;

type DbId = i64;

struct EndpointTarget {
    channel_id: DbId,
    provider: String,
    protocol: String,
    base_url: String,
    use_credentials: bool,
}

pub(crate) async fn run(context: &AppContext) -> Result<()> {
    let endpoints = syncable_endpoints(context).await?;
    if endpoints.is_empty() {
        tracing::info!("no upstream endpoints are eligible for model sync");
        return Ok(());
    }

    tracing::info!(
        count = endpoints.len(),
        "syncing upstream model catalogs from enabled channels"
    );
    let mut channel_models = HashMap::<DbId, ChannelModelSync>::new();
    for endpoint in endpoints {
        match fetch_endpoint_models(context, &endpoint).await {
            Ok(models) => {
                if models.is_empty() {
                    tracing::warn!(
                        channel_id = endpoint.channel_id,
                        provider = %endpoint.provider,
                        protocol = %endpoint.protocol,
                        "upstream returned no models"
                    );
                    continue;
                }
                record_provider_models(context, &endpoint.provider, &models).await?;
                channel_models
                    .entry(endpoint.channel_id)
                    .or_insert_with(|| ChannelModelSync {
                        provider: endpoint.provider.clone(),
                        models: Vec::new(),
                    })
                    .models
                    .extend(models);
                tracing::info!(
                    channel_id = endpoint.channel_id,
                    provider = %endpoint.provider,
                    protocol = %endpoint.protocol,
                    "fetched upstream models"
                );
            }
            Err(err) => {
                tracing::warn!(
                    channel_id = endpoint.channel_id,
                    provider = %endpoint.provider,
                    protocol = %endpoint.protocol,
                    base_url = %endpoint.base_url,
                    error = %err,
                    "failed to fetch upstream models"
                );
            }
        }
    }

    for (channel_id, sync) in channel_models {
        let models = normalized_models(&sync.models);
        sync_channel_models(context, channel_id, &sync.provider, &models).await?;
        tracing::info!(
            channel_id,
            provider = %sync.provider,
            count = models.len(),
            "synced upstream models"
        );
    }

    Ok(())
}

struct ChannelModelSync {
    provider: String,
    models: Vec<String>,
}

async fn syncable_endpoints(context: &AppContext) -> Result<Vec<EndpointTarget>> {
    let rows = sqlx::query(
        "SELECT c.id AS channel_id, c.provider, c.use_credentials,
                ce.protocol, ce.base_url
         FROM channel c
         JOIN channel_endpoint ce ON ce.channel_id = c.id
         WHERE c.enabled = TRUE
           AND ce.enabled = TRUE
           AND ce.protocol = 'openai'
         ORDER BY c.priority DESC, c.created_at ASC,
                  ce.created_at ASC",
    )
    .fetch_all(&context.db)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(EndpointTarget {
                channel_id: row.try_get("channel_id")?,
                provider: row.try_get("provider")?,
                use_credentials: row.try_get("use_credentials")?,
                protocol: row.try_get("protocol")?,
                base_url: row.try_get("base_url")?,
            })
        })
        .collect()
}

async fn fetch_endpoint_models(
    context: &AppContext,
    endpoint: &EndpointTarget,
) -> Result<Vec<String>> {
    let secret = endpoint_secret(context, endpoint).await?;
    fetch_upstream_models(context, endpoint, &secret).await
}

async fn endpoint_secret(context: &AppContext, endpoint: &EndpointTarget) -> Result<String> {
    if endpoint.use_credentials {
        return runtime_secret_from_enabled_credential(context, &endpoint.provider).await;
    }

    let row = sqlx::query(
        "SELECT secret_ciphertext
         FROM channel_key
         WHERE channel_id = $1 AND enabled = true
         ORDER BY healthy DESC, last_used_at DESC NULLS LAST, created_at ASC
         LIMIT 1",
    )
    .bind(endpoint.channel_id)
    .fetch_optional(&context.db)
    .await?
    .context("upstream channel has no enabled key")?;

    let ciphertext: String = row.try_get("secret_ciphertext")?;
    context.secrets.plaintext(&ciphertext)
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

async fn fetch_upstream_models(
    context: &AppContext,
    endpoint: &EndpointTarget,
    secret: &str,
) -> Result<Vec<String>> {
    let url = upstream_url(&endpoint.base_url, "/v1/models");
    let request = context.http.get(url).bearer_auth(secret);

    let response = request.send().await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("upstream model list returned HTTP {}", status.as_u16());
    }

    let value = response.json::<Value>().await?;
    Ok(extract_model_ids(&value))
}

async fn record_provider_models(
    context: &AppContext,
    provider: &str,
    models: &[String],
) -> Result<()> {
    for model in normalized_models(models) {
        sqlx::query(
            "INSERT INTO provider_model
             (provider, model, display_name, source, enabled)
             VALUES ($1, $2, $2, 'upstream', FALSE)
             ON CONFLICT (provider, model)
             DO UPDATE SET
                 display_name = CASE
                     WHEN provider_model.display_name = '' THEN EXCLUDED.display_name
                     ELSE provider_model.display_name
                 END,
                 source = 'upstream',
                 discovered_at = now(),
                 updated_at = now()",
        )
        .bind(provider)
        .bind(model)
        .execute(&context.db)
        .await?;
    }
    Ok(())
}

async fn sync_channel_models(
    context: &AppContext,
    channel_id: DbId,
    provider: &str,
    models: &[String],
) -> Result<()> {
    let models = normalized_models(models);
    for model in &models {
        sqlx::query(
            "INSERT INTO channel_model
             (channel_id, provider, model, enabled, status, runtime_status, last_seen_at)
             VALUES ($1, $2, $3, FALSE, 'available', 'normal', now())
             ON CONFLICT (channel_id, model)
             DO UPDATE SET
                 status = 'available',
                 missing_since = NULL,
                 last_seen_at = now(),
                 updated_at = now()",
        )
        .bind(channel_id)
        .bind(provider)
        .bind(model)
        .execute(&context.db)
        .await?;
    }

    sqlx::query(
        "UPDATE channel_model
         SET status = 'missing',
             missing_since = COALESCE(missing_since, now()),
             updated_at = now()
         WHERE channel_id = $1
           AND status = 'available'
           AND NOT (model = ANY($2))",
    )
    .bind(channel_id)
    .bind(&models)
    .execute(&context.db)
    .await?;

    Ok(())
}

fn normalized_models(models: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    models
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .filter(|model| seen.insert((*model).to_string()))
        .map(str::to_string)
        .collect()
}

fn extract_model_ids(value: &Value) -> Vec<String> {
    let Some(data) = value.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut models = Vec::new();
    for item in data {
        if let Some(id) = item
            .get("id")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            models.push(id.to_string());
        }
    }
    models.sort();
    models.dedup();
    models
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
