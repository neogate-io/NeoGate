use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

const MODELS_DEV_PRICING_URL: &str = "https://models.dev/api.json";
const PRICE_TEMPLATE_SOURCE_MODELS_DEV: &str = "models_dev";

#[derive(Debug, Serialize)]
pub struct ProviderPriceRecord {
    pub id: DbId,
    pub provider: String,
    pub model: String,
    pub input_price_usd_micros: i64,
    pub output_price_usd_micros: i64,
    pub cache_read_price_usd_micros: Option<i64>,
    pub cache_write_price_usd_micros: Option<i64>,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ProviderModelRecord {
    pub id: DbId,
    pub provider: String,
    pub model: String,
    pub display_name: String,
    pub source: String,
    pub enabled: bool,
    pub discovered_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PricingTemplateRecord {
    pub id: DbId,
    pub provider: String,
    pub model: String,
    pub input_price_usd_micros: i64,
    pub output_price_usd_micros: i64,
    pub cache_read_price_usd_micros: Option<i64>,
    pub cache_write_price_usd_micros: Option<i64>,
    pub source: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PricingPolicyRecord {
    pub id: DbId,
    pub name: String,
    pub user_group: Option<String>,
    pub multiplier_micros: i64,
    pub enabled: bool,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PricingTemplateSyncResult {
    pub source: String,
    pub fetched: usize,
    pub saved: u64,
    pub skipped: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpsertProviderPriceRequest {
    pub provider: String,
    pub model: String,
    pub input_price_usd_micros: i64,
    pub output_price_usd_micros: i64,
    pub cache_read_price_usd_micros: Option<i64>,
    pub cache_write_price_usd_micros: Option<i64>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpsertPricingPolicyRequest {
    pub id: Option<DbId>,
    pub name: String,
    pub user_group: Option<String>,
    pub multiplier_micros: i64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Deserialize)]
pub struct SyncPricingTemplatesRequest {
    #[serde(default = "default_pricing_template_sync_source")]
    pub source: String,
}

#[derive(Debug, Deserialize)]
struct ModelsDevProvider {
    #[serde(default)]
    models: HashMap<String, ModelsDevModel>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModel {
    cost: Option<ModelsDevCost>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevCost {
    input: Option<f64>,
    output: Option<f64>,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
}

fn default_enabled() -> bool {
    true
}

fn default_pricing_template_sync_source() -> String {
    PRICE_TEMPLATE_SOURCE_MODELS_DEV.to_string()
}

pub async fn list_provider_models(state: &AppState) -> AppResult<Vec<ProviderModelRecord>> {
    let rows = sqlx::query(
        "SELECT id, provider, model, display_name, source, enabled,
                discovered_at, created_at, updated_at
         FROM provider_model
         ORDER BY provider ASC, model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(provider_model_from_row).collect()
}

pub async fn list_provider_prices(state: &AppState) -> AppResult<Vec<ProviderPriceRecord>> {
    let rows = sqlx::query(
        "SELECT id, provider, model, input_price_usd_micros,
                output_price_usd_micros, cache_read_price_usd_micros,
                cache_write_price_usd_micros,
                enabled, created_at, updated_at
         FROM provider_price
         ORDER BY provider ASC, model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(provider_price_from_row).collect()
}

pub async fn list_pricing_templates(state: &AppState) -> AppResult<Vec<PricingTemplateRecord>> {
    let rows = sqlx::query(
        "SELECT id, provider, model, input_price_usd_micros,
                output_price_usd_micros, cache_read_price_usd_micros,
                cache_write_price_usd_micros, source, enabled,
                created_at, updated_at
         FROM pricing_template
         ORDER BY provider ASC, model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(pricing_template_from_row).collect()
}

pub async fn list_pricing_policies(state: &AppState) -> AppResult<Vec<PricingPolicyRecord>> {
    let rows = sqlx::query(
        "SELECT id, name, user_group, multiplier_micros,
                enabled, priority, created_at, updated_at
         FROM pricing_policy
         ORDER BY priority DESC, name ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(pricing_policy_from_row).collect()
}

pub async fn sync_pricing_templates(
    state: &AppState,
    req: SyncPricingTemplatesRequest,
) -> AppResult<PricingTemplateSyncResult> {
    match req.source.trim() {
        "" | PRICE_TEMPLATE_SOURCE_MODELS_DEV => sync_models_dev_pricing_templates(state).await,
        source => Err(AppError::BadRequest(format!(
            "unsupported pricing template source: {source}"
        ))),
    }
}

pub async fn upsert_provider_price(
    state: &AppState,
    req: UpsertProviderPriceRequest,
) -> AppResult<ProviderPriceRecord> {
    validate_price(&req)?;
    ensure_model_is_known(state, &req.provider, &req.model).await?;
    let row = sqlx::query(
        "INSERT INTO provider_price
         (provider, model, input_price_usd_micros,
          output_price_usd_micros, cache_read_price_usd_micros,
          cache_write_price_usd_micros, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (provider, model)
         DO UPDATE SET
             input_price_usd_micros = EXCLUDED.input_price_usd_micros,
             output_price_usd_micros = EXCLUDED.output_price_usd_micros,
             cache_read_price_usd_micros = EXCLUDED.cache_read_price_usd_micros,
             cache_write_price_usd_micros = EXCLUDED.cache_write_price_usd_micros,
             enabled = EXCLUDED.enabled,
             updated_at = now()
         RETURNING id, provider, model, input_price_usd_micros,
                   output_price_usd_micros, cache_read_price_usd_micros,
                   cache_write_price_usd_micros,
                   enabled, created_at, updated_at",
    )
    .bind(req.provider)
    .bind(req.model)
    .bind(req.input_price_usd_micros)
    .bind(req.output_price_usd_micros)
    .bind(req.cache_read_price_usd_micros)
    .bind(req.cache_write_price_usd_micros)
    .bind(req.enabled)
    .fetch_one(&state.db.pool)
    .await?;
    let price = provider_price_from_row(&row)?;
    sync_channel_model_enabled_for_price(state, &price).await?;
    Ok(price)
}

async fn sync_channel_model_enabled_for_price(
    state: &AppState,
    price: &ProviderPriceRecord,
) -> AppResult<()> {
    if price.enabled {
        sqlx::query(
            "UPDATE channel_model cm
             SET enabled = TRUE,
                 updated_at = now()
             FROM channel_endpoint ce
             WHERE cm.channel_id = ce.channel_id
               AND cm.provider = $1
               AND cm.model = $2
               AND cm.model = ANY(ce.models)
               AND cm.status = 'available'",
        )
        .bind(&price.provider)
        .bind(&price.model)
        .execute(&state.db.pool)
        .await?;
    } else {
        sqlx::query(
            "UPDATE channel_model
             SET enabled = FALSE,
                 updated_at = now()
             WHERE provider = $1
               AND model = $2",
        )
        .bind(&price.provider)
        .bind(&price.model)
        .execute(&state.db.pool)
        .await?;
    }
    Ok(())
}

async fn sync_models_dev_pricing_templates(
    state: &AppState,
) -> AppResult<PricingTemplateSyncResult> {
    let upstream = state
        .http
        .get(MODELS_DEV_PRICING_URL)
        .send()
        .await
        .map_err(models_dev_pricing_unavailable)?
        .error_for_status()
        .map_err(models_dev_pricing_unavailable)?
        .json::<HashMap<String, ModelsDevProvider>>()
        .await
        .map_err(models_dev_pricing_unavailable)?;

    let provider_codes = enabled_provider_codes(state).await?;
    let mut fetched = 0usize;
    let mut skipped = 0usize;
    let mut saved = 0u64;

    for (upstream_provider, provider_data) in upstream {
        fetched += provider_data.models.len();
        let Some(provider) = normalize_template_provider(&upstream_provider) else {
            skipped += provider_data.models.len();
            continue;
        };
        if !provider_codes.contains(provider) {
            skipped += provider_data.models.len();
            continue;
        }

        for (model, model_data) in provider_data.models {
            let model = model.trim();
            if model.is_empty() {
                skipped += 1;
                continue;
            }
            let Some(cost) = model_data.cost else {
                skipped += 1;
                continue;
            };
            let Some(input_price_usd_micros) = usd_per_million_to_micros(cost.input) else {
                skipped += 1;
                continue;
            };
            let Some(output_price_usd_micros) = usd_per_million_to_micros(cost.output) else {
                skipped += 1;
                continue;
            };
            let cache_read_price_usd_micros = usd_per_million_to_micros(cost.cache_read);
            let cache_write_price_usd_micros = usd_per_million_to_micros(cost.cache_write);
            saved += upsert_synced_pricing_template(
                state,
                PricingTemplateUpsert {
                    provider,
                    model,
                    input_price_usd_micros,
                    output_price_usd_micros,
                    cache_read_price_usd_micros,
                    cache_write_price_usd_micros,
                    source: PRICE_TEMPLATE_SOURCE_MODELS_DEV,
                },
            )
            .await?;
        }
    }

    Ok(PricingTemplateSyncResult {
        source: PRICE_TEMPLATE_SOURCE_MODELS_DEV.to_string(),
        fetched,
        saved,
        skipped,
    })
}

fn models_dev_pricing_unavailable(err: reqwest::Error) -> AppError {
    tracing::warn!(
        error = %err,
        error_debug = ?err,
        "failed to sync pricing templates from models.dev"
    );
    AppError::UpstreamUnavailable("pricing reference source is temporarily unavailable".to_string())
}

async fn enabled_provider_codes(state: &AppState) -> AppResult<HashSet<String>> {
    let rows = sqlx::query("SELECT code FROM provider WHERE enabled = TRUE")
        .fetch_all(&state.db.pool)
        .await?;
    rows.iter()
        .map(|row| row.try_get("code"))
        .collect::<Result<HashSet<String>, sqlx::Error>>()
        .map_err(Into::into)
}

struct PricingTemplateUpsert<'a> {
    provider: &'a str,
    model: &'a str,
    input_price_usd_micros: i64,
    output_price_usd_micros: i64,
    cache_read_price_usd_micros: Option<i64>,
    cache_write_price_usd_micros: Option<i64>,
    source: &'a str,
}

async fn upsert_synced_pricing_template(
    state: &AppState,
    template: PricingTemplateUpsert<'_>,
) -> AppResult<u64> {
    sqlx::query(
        "INSERT INTO provider_model
         (provider, model, display_name, source, enabled)
         VALUES ($1, $2, $2, 'upstream', FALSE)
         ON CONFLICT (provider, model) DO NOTHING",
    )
    .bind(template.provider)
    .bind(template.model.trim())
    .execute(&state.db.pool)
    .await?;

    let result = sqlx::query(
        "INSERT INTO pricing_template
         (provider, model, input_price_usd_micros,
          output_price_usd_micros, cache_read_price_usd_micros,
          cache_write_price_usd_micros, source, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, TRUE)
         ON CONFLICT (provider, model)
         DO UPDATE SET
             input_price_usd_micros = EXCLUDED.input_price_usd_micros,
             output_price_usd_micros = EXCLUDED.output_price_usd_micros,
             cache_read_price_usd_micros = EXCLUDED.cache_read_price_usd_micros,
             cache_write_price_usd_micros = EXCLUDED.cache_write_price_usd_micros,
             source = EXCLUDED.source,
             enabled = TRUE,
             updated_at = now()
         ",
    )
    .bind(template.provider)
    .bind(template.model.trim())
    .bind(template.input_price_usd_micros)
    .bind(template.output_price_usd_micros)
    .bind(template.cache_read_price_usd_micros)
    .bind(template.cache_write_price_usd_micros)
    .bind(template.source)
    .execute(&state.db.pool)
    .await?;
    Ok(result.rows_affected())
}

fn normalize_template_provider(provider: &str) -> Option<&'static str> {
    match provider.trim().to_ascii_lowercase().as_str() {
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "google" | "google-ai-studio" | "gemini" => Some("google"),
        "deepseek" => Some("deepseek"),
        "qwen" | "dashscope" | "alibaba" => Some("qwen"),
        "moonshot" | "kimi" => Some("moonshot"),
        "zhipu" | "bigmodel" => Some("zhipu"),
        "doubao" | "volcengine" | "volcengine-ark" => Some("doubao"),
        "baidu" | "qianfan" => Some("baidu"),
        "tencent" | "hunyuan" => Some("tencent"),
        "minimax" => Some("minimax"),
        "stepfun" => Some("stepfun"),
        "baichuan" => Some("baichuan"),
        "iflytek" | "spark" => Some("iflytek"),
        "sensenova" => Some("sensenova"),
        "siliconflow" => Some("siliconflow"),
        _ => None,
    }
}

fn usd_per_million_to_micros(value: Option<f64>) -> Option<i64> {
    let value = value?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let micros = (value * 1_000_000.0).round();
    if micros > i64::MAX as f64 {
        return None;
    }
    Some(micros as i64)
}

pub async fn upsert_pricing_policy(
    state: &AppState,
    req: UpsertPricingPolicyRequest,
) -> AppResult<PricingPolicyRecord> {
    validate_pricing_policy(&req)?;
    let user_group = req
        .user_group
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_string();
    let row = if let Some(id) = req.id {
        sqlx::query(
            "UPDATE pricing_policy
             SET name = $2,
                 user_group = $3,
                 multiplier_micros = $4,
                 enabled = $5,
                 priority = $6,
                 updated_at = now()
             WHERE id = $1
             RETURNING id, name, user_group, multiplier_micros,
                       enabled, priority, created_at, updated_at",
        )
        .bind(id)
        .bind(req.name)
        .bind(&user_group)
        .bind(req.multiplier_micros)
        .bind(req.enabled)
        .bind(req.priority)
        .fetch_optional(&state.db.pool)
        .await?
        .ok_or(AppError::NotFound)?
    } else {
        sqlx::query(
            "INSERT INTO pricing_policy
             (name, user_group, multiplier_micros, enabled, priority)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING id, name, user_group, multiplier_micros,
                       enabled, priority, created_at, updated_at",
        )
        .bind(req.name)
        .bind(&user_group)
        .bind(req.multiplier_micros)
        .bind(req.enabled)
        .bind(req.priority)
        .fetch_one(&state.db.pool)
        .await?
    };
    pricing_policy_from_row(&row)
}

fn validate_price(req: &UpsertProviderPriceRequest) -> AppResult<()> {
    if req.provider.trim().is_empty() || req.model.trim().is_empty() {
        return Err(AppError::BadRequest(
            "provider and model are required".to_string(),
        ));
    }
    if req.input_price_usd_micros < 0
        || req.output_price_usd_micros < 0
        || req
            .cache_read_price_usd_micros
            .is_some_and(|price| price < 0)
        || req
            .cache_write_price_usd_micros
            .is_some_and(|price| price < 0)
    {
        return Err(AppError::BadRequest(
            "price must be non-negative".to_string(),
        ));
    }
    Ok(())
}

fn validate_pricing_policy(req: &UpsertPricingPolicyRequest) -> AppResult<()> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("policy name is required".to_string()));
    }
    if req.multiplier_micros < 0 {
        return Err(AppError::BadRequest(
            "pricing policy multiplier must be non-negative".to_string(),
        ));
    }
    if req
        .user_group
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .is_empty()
    {
        return Err(AppError::BadRequest(
            "user_group is required for user group pricing policies".to_string(),
        ));
    }
    Ok(())
}

async fn ensure_model_is_known(state: &AppState, provider: &str, model: &str) -> AppResult<()> {
    let row = sqlx::query(
        r#"
        SELECT 1
        FROM provider_model
        WHERE provider = $1
          AND model = $2
        LIMIT 1
        "#,
    )
    .bind(provider)
    .bind(model)
    .fetch_optional(&state.db.pool)
    .await?;

    if row.is_none() {
        return Err(AppError::BadRequest(
            "model is not known for this provider".to_string(),
        ));
    }

    Ok(())
}

fn provider_model_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProviderModelRecord> {
    Ok(ProviderModelRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        display_name: row.try_get("display_name")?,
        source: row.try_get("source")?,
        enabled: row.try_get("enabled")?,
        discovered_at: row.try_get("discovered_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn provider_price_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProviderPriceRecord> {
    Ok(ProviderPriceRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        input_price_usd_micros: row.try_get("input_price_usd_micros")?,
        output_price_usd_micros: row.try_get("output_price_usd_micros")?,
        cache_read_price_usd_micros: row.try_get("cache_read_price_usd_micros")?,
        cache_write_price_usd_micros: row.try_get("cache_write_price_usd_micros")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn pricing_template_from_row(row: &sqlx::postgres::PgRow) -> AppResult<PricingTemplateRecord> {
    Ok(PricingTemplateRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        input_price_usd_micros: row.try_get("input_price_usd_micros")?,
        output_price_usd_micros: row.try_get("output_price_usd_micros")?,
        cache_read_price_usd_micros: row.try_get("cache_read_price_usd_micros")?,
        cache_write_price_usd_micros: row.try_get("cache_write_price_usd_micros")?,
        source: row.try_get("source")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn pricing_policy_from_row(row: &sqlx::postgres::PgRow) -> AppResult<PricingPolicyRecord> {
    Ok(PricingPolicyRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        user_group: row.try_get("user_group")?,
        multiplier_micros: row.try_get("multiplier_micros")?,
        enabled: row.try_get("enabled")?,
        priority: row.try_get("priority")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_models_dev_usd_per_million_to_micros() {
        assert_eq!(usd_per_million_to_micros(Some(0.1)), Some(100_000));
        assert_eq!(usd_per_million_to_micros(Some(1.25)), Some(1_250_000));
        assert_eq!(usd_per_million_to_micros(Some(-1.0)), None);
        assert_eq!(usd_per_million_to_micros(None), None);
    }

    #[test]
    fn normalizes_known_pricing_template_providers() {
        assert_eq!(
            normalize_template_provider("google-ai-studio"),
            Some("google")
        );
        assert_eq!(normalize_template_provider("dashscope"), Some("qwen"));
        assert_eq!(normalize_template_provider("unknown-provider"), None);
    }

    #[test]
    fn parses_models_dev_cache_prices() {
        let model: ModelsDevModel = serde_json::from_str(
            r#"{"cost":{"input":5,"output":30,"cache_read":0.5,"cache_write":6.25}}"#,
        )
        .unwrap();
        let cost = model.cost.unwrap();

        assert_eq!(usd_per_million_to_micros(cost.input), Some(5_000_000));
        assert_eq!(usd_per_million_to_micros(cost.output), Some(30_000_000));
        assert_eq!(usd_per_million_to_micros(cost.cache_read), Some(500_000));
        assert_eq!(usd_per_million_to_micros(cost.cache_write), Some(6_250_000));
    }
}
