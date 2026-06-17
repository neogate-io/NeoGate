use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::Row;

use crate::{
    error::{AppError, AppResult},
    AppState,
};

pub const CUSTOM_PROVIDER_CODE: &str = "custom";
pub const NEWAPI_PROVIDER_CODE: &str = "newapi";
const CUSTOM_PROVIDER_DISPLAY_NAME: &str = "自定义";
const CUSTOM_PROVIDER_NAME: &str = "Custom";
const NEWAPI_PROVIDER_DISPLAY_NAME: &str = "NewAPI";
const NEWAPI_PROVIDER_NAME: &str = "NewAPI";
pub const OPENAI_OAUTH_PROTOCOL: &str = "openai_oauth";

#[derive(Debug, Clone, Serialize)]
pub struct ProviderRecord {
    pub id: i64,
    pub code: String,
    pub display_name: String,
    pub name: String,
    pub default_models: Vec<String>,
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
    ensure_custom_provider(state).await?;
    ensure_newapi_provider(state).await?;

    let rows = sqlx::query(
        "SELECT id, code, display_name, name, default_models,
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

pub async fn provider_default_models(
    state: &AppState,
    code: &str,
) -> AppResult<Option<Vec<String>>> {
    let row = sqlx::query(
        "SELECT default_models
         FROM provider
         WHERE code = $1 AND enabled = TRUE",
    )
    .bind(code)
    .fetch_optional(&state.db.pool)
    .await?;

    row.map(|row| row.try_get("default_models"))
        .transpose()
        .map_err(Into::into)
}

pub async fn ensure_newapi_provider(state: &AppState) -> AppResult<()> {
    ensure_builtin_manual_provider(
        state,
        NEWAPI_PROVIDER_CODE,
        NEWAPI_PROVIDER_DISPLAY_NAME,
        NEWAPI_PROVIDER_NAME,
        1,
    )
    .await
}

pub async fn ensure_custom_provider(state: &AppState) -> AppResult<()> {
    ensure_builtin_manual_provider(
        state,
        CUSTOM_PROVIDER_CODE,
        CUSTOM_PROVIDER_DISPLAY_NAME,
        CUSTOM_PROVIDER_NAME,
        0,
    )
    .await
}

async fn ensure_builtin_manual_provider(
    state: &AppState,
    code: &str,
    display_name: &str,
    name: &str,
    sort_order: i32,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO provider
         (code, display_name, name, default_models, default_openai_base_url,
          default_openai_oauth_base_url, default_anthropic_base_url, enabled, sort_order)
         VALUES ($1, $2, $3, ARRAY[]::TEXT[], '', '', '', TRUE, $4)
         ON CONFLICT (code) DO UPDATE
         SET display_name = EXCLUDED.display_name,
             name = EXCLUDED.name,
             default_models = EXCLUDED.default_models,
             default_openai_base_url = EXCLUDED.default_openai_base_url,
             default_openai_oauth_base_url = EXCLUDED.default_openai_oauth_base_url,
             default_anthropic_base_url = EXCLUDED.default_anthropic_base_url,
             enabled = TRUE,
             sort_order = EXCLUDED.sort_order,
             updated_at = now()",
    )
    .bind(code)
    .bind(display_name)
    .bind(name)
    .bind(sort_order)
    .execute(&state.db.pool)
    .await?;

    Ok(())
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
             (provider, model, display_name, source, enabled)
             VALUES ($1, $2, $2, $3, $4)
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
        default_models: row.try_get("default_models")?,
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
