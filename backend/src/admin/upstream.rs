use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

use crate::{
    auth::AdminAuth,
    cache::InvalidationEvent,
    config::DEFAULT_ANTHROPIC_VERSION,
    error::{AppError, AppResult, UpstreamRequestError},
    id::DbId,
    input::trimmed_non_empty,
    AppState,
};

use super::{
    credentials::runtime_secret_from_enabled_credential, ensure_builtin_manual_provider_by_code,
    invalidate_cache, provider_default_endpoints, record_provider_models, OPENAI_OAUTH_PROTOCOL,
};

#[derive(Debug, Deserialize)]
pub(super) struct FetchUpstreamModelsRequest {
    channel_id: Option<DbId>,
    provider: String,
    protocol: Option<String>,
    base_url: Option<String>,
    secret: Option<String>,
    #[serde(default)]
    use_credentials: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct FetchUpstreamModelsResponse {
    models: Vec<String>,
}

pub(super) async fn upstream_models(
    State(state): State<Arc<AppState>>,
    _admin: AdminAuth,
    Json(req): Json<FetchUpstreamModelsRequest>,
) -> AppResult<Json<FetchUpstreamModelsResponse>> {
    let provider_code = req.provider.trim();
    let secret = trimmed_non_empty(req.secret.as_deref());

    if provider_code.is_empty() {
        return Err(AppError::BadRequest("provider is required".to_string()));
    }
    ensure_builtin_manual_provider_by_code(&state, provider_code).await?;

    let defaults = provider_default_endpoints(&state, provider_code)
        .await?
        .ok_or_else(|| AppError::BadRequest(format!("invalid provider: {provider_code}")))?;
    let protocol = trimmed_non_empty(req.protocol.as_deref())
        .map(str::to_string)
        .ok_or_else(|| AppError::BadRequest("protocol is required".to_string()))?;
    if protocol != "openai" && protocol != "anthropic" && protocol != OPENAI_OAUTH_PROTOCOL {
        return Err(AppError::BadRequest(format!(
            "invalid protocol: {protocol}"
        )));
    }
    if !defaults
        .iter()
        .any(|endpoint| endpoint.protocol == protocol)
    {
        return Err(AppError::BadRequest(format!(
            "provider {provider_code} does not support protocol {protocol}"
        )));
    }
    let base_url = trimmed_non_empty(req.base_url.as_deref())
        .ok_or_else(|| AppError::BadRequest("base_url is required".to_string()))?;

    if req.use_credentials && provider_code == "openai" && protocol == OPENAI_OAUTH_PROTOCOL {
        let models = openai_oauth_catalog_models(&state).await?;
        if models.is_empty() {
            return Err(AppError::BadRequest("no models returned".to_string()));
        }
        record_provider_models(&state, provider_code, &models, "upstream", false).await?;
        if let Some(channel_id) = req.channel_id {
            sync_channel_models_from_upstream(
                &state,
                channel_id,
                provider_code,
                &protocol,
                base_url,
                &models,
            )
            .await?;
            invalidate_cache(&state, InvalidationEvent::Routing).await;
        }
        return Ok(Json(FetchUpstreamModelsResponse { models }));
    }

    let channel_secret;
    let secret = if let Some(secret) = secret {
        secret
    } else if req.use_credentials {
        channel_secret = runtime_secret_from_enabled_credential(&state, provider_code).await?;
        channel_secret.as_str()
    } else if let Some(channel_id) = req.channel_id {
        channel_secret = upstream_model_secret_from_channel(&state, channel_id).await?;
        channel_secret.as_str()
    } else {
        return Err(AppError::BadRequest(
            "upstream api key is required".to_string(),
        ));
    };

    let models = fetch_upstream_models(&state, &protocol, base_url, secret).await?;
    if models.is_empty() {
        return Err(AppError::BadRequest("no models returned".to_string()));
    }
    record_provider_models(&state, provider_code, &models, "upstream", false).await?;
    if let Some(channel_id) = req.channel_id {
        sync_channel_models_from_upstream(
            &state,
            channel_id,
            provider_code,
            &protocol,
            base_url,
            &models,
        )
        .await?;
        invalidate_cache(&state, InvalidationEvent::Routing).await;
    }
    Ok(Json(FetchUpstreamModelsResponse { models }))
}

async fn sync_channel_models_from_upstream(
    state: &AppState,
    channel_id: DbId,
    provider: &str,
    protocol: &str,
    base_url: &str,
    models: &[String],
) -> AppResult<()> {
    let exists = sqlx::query(
        "SELECT 1
         FROM channel_endpoint ce
         JOIN channel c ON c.id = ce.channel_id
         WHERE ce.channel_id = $1
           AND c.provider = $2
           AND ce.protocol = $3
           AND ce.base_url = $4",
    )
    .bind(channel_id)
    .bind(provider)
    .bind(protocol)
    .bind(base_url)
    .fetch_optional(&state.db.pool)
    .await?;
    if exists.is_none() {
        return Ok(());
    }

    let mut seen = std::collections::HashSet::new();
    let upstream_models: Vec<String> = models
        .iter()
        .map(|model| model.trim())
        .filter(|model| !model.is_empty())
        .filter(|model| seen.insert((*model).to_string()))
        .map(str::to_string)
        .collect();
    let upstream_model_set: std::collections::HashSet<&str> =
        upstream_models.iter().map(String::as_str).collect();

    let configured_rows = sqlx::query(
        "SELECT DISTINCT btrim(endpoint_model.model) AS model
         FROM channel_endpoint ce
         CROSS JOIN unnest(ce.models) AS endpoint_model(model)
         WHERE ce.channel_id = $1
           AND ce.protocol = $2
           AND ce.base_url = $3
           AND btrim(endpoint_model.model) <> ''
         ORDER BY model ASC",
    )
    .bind(channel_id)
    .bind(protocol)
    .bind(base_url)
    .fetch_all(&state.db.pool)
    .await?;
    let configured_models: Vec<String> = configured_rows
        .iter()
        .map(|row| row.try_get("model"))
        .collect::<Result<_, _>>()?;

    for model in configured_models
        .iter()
        .filter(|model| upstream_model_set.contains(model.as_str()))
    {
        sqlx::query(
            "INSERT INTO channel_model
             (channel_id, model, enabled, status, runtime_status, last_seen_at)
             VALUES ($1, $2, FALSE, 'available', 'normal', now())
             ON CONFLICT (channel_id, model)
             DO UPDATE SET
                 status = 'available',
                 runtime_status = 'normal',
                 cooldown_until = NULL,
                 last_error = NULL,
                 last_status_code = NULL,
                 missing_since = NULL,
                 last_seen_at = now(),
                 updated_at = now()",
        )
        .bind(channel_id)
        .bind(model)
        .execute(&state.db.pool)
        .await?;
    }

    sqlx::query(
        "UPDATE channel_model
         SET enabled = FALSE,
             status = 'missing',
             runtime_status = 'failed',
             cooldown_until = NULL,
             last_error = 'upstream model is missing',
             last_status_code = NULL,
             missing_since = COALESCE(missing_since, now()),
             updated_at = now()
         WHERE channel_id = $1
           AND model = ANY($2)
           AND status = 'available'
           AND NOT (model = ANY($3))",
    )
    .bind(channel_id)
    .bind(&configured_models)
    .bind(&upstream_models)
    .execute(&state.db.pool)
    .await?;

    sqlx::query(
        "DELETE FROM channel_model cm
         WHERE cm.channel_id = $1
           AND NOT EXISTS (
               SELECT 1
               FROM channel_endpoint ce
               CROSS JOIN unnest(ce.models) AS endpoint_model(model)
               WHERE ce.channel_id = cm.channel_id
                 AND btrim(endpoint_model.model) = cm.model
           )",
    )
    .bind(channel_id)
    .execute(&state.db.pool)
    .await?;

    Ok(())
}

async fn openai_oauth_catalog_models(state: &AppState) -> AppResult<Vec<String>> {
    let rows = sqlx::query(
        "SELECT pm.model
         FROM provider_model pm
         JOIN provider p ON p.code = pm.provider
         WHERE pm.provider = 'openai'
           AND pm.enabled = TRUE
           AND NOT pm.model = ANY(p.default_models)
         ORDER BY pm.model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter()
        .map(|row| row.try_get("model"))
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

async fn upstream_model_secret_from_channel(
    state: &AppState,
    channel_id: DbId,
) -> AppResult<String> {
    let channel_row = sqlx::query("SELECT provider, use_credentials FROM channel WHERE id = $1")
        .bind(channel_id)
        .fetch_optional(&state.db.pool)
        .await?
        .ok_or(AppError::NotFound)?;
    let provider: String = channel_row.try_get("provider")?;
    let use_credentials: bool = channel_row.try_get("use_credentials")?;
    if use_credentials {
        return runtime_secret_from_enabled_credential(state, &provider).await;
    }

    let row = sqlx::query(
        "SELECT id, secret_ciphertext
         FROM channel_key
         WHERE channel_id = $1 AND enabled = true
         ORDER BY healthy DESC, last_used_at DESC NULLS LAST, created_at ASC
         LIMIT 1",
    )
    .bind(channel_id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or_else(|| AppError::BadRequest("这个上游服务没有可用的上游 Key".to_string()))?;

    let key_id: DbId = row.try_get("id")?;
    let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
    Ok(state.secrets.plaintext(key_id, &secret_ciphertext)?)
}

pub(crate) async fn fetch_upstream_models(
    state: &AppState,
    protocol: &str,
    base_url: &str,
    secret: &str,
) -> AppResult<Vec<String>> {
    let url = crate::relay::upstream_url(base_url, "/v1/models");
    let mut request = state.http.get(url);

    if protocol == "anthropic" {
        request = request
            .header("x-api-key", secret)
            .header("anthropic-version", DEFAULT_ANTHROPIC_VERSION);
    } else {
        request = request.bearer_auth(secret);
    }

    let response = request
        .send()
        .await
        .map_err(|err| upstream_models_request_error(base_url, err))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::BadRequest(upstream_models_error_message(
            status.as_u16(),
        )));
    }

    let value = response
        .json::<Value>()
        .await
        .map_err(|_| AppError::BadRequest("上游模型列表响应格式无效".to_string()))?;
    let models = extract_model_ids(&value);
    if models.is_empty() {
        return Err(AppError::BadRequest("no models returned".to_string()));
    }

    Ok(models)
}

fn upstream_models_request_error(base_url: &str, err: reqwest::Error) -> AppError {
    AppError::UpstreamRequest(UpstreamRequestError::from_reqwest(
        upstream_models_error_provider(base_url),
        &err,
    ))
}

fn upstream_models_error_provider(base_url: &str) -> String {
    base_url
        .parse::<reqwest::Url>()
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "upstream".to_string())
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

fn upstream_models_error_message(status: u16) -> String {
    match status {
        401 | 403 => "API 密钥无效或无权限，请检查后重试".to_string(),
        404 => "Base URL 不正确，未找到模型列表接口".to_string(),
        429 => "上游请求过于频繁，请稍后重试".to_string(),
        500..=599 => "上游服务暂时不可用，请稍后重试".to_string(),
        _ => "获取模型列表失败，请检查 Base URL 和 API 密钥".to_string(),
    }
}
