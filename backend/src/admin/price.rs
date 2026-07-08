use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    billing::{BillingMeter, PricingBasis},
    config::BillingCurrency,
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

const MODELS_DEV_PRICING_URL: &str = "https://models.dev/api.json";
const PRICE_TEMPLATE_SOURCE_MODELS_DEV: &str = "models_dev";
const PRICE_TEMPLATE_SOURCE_LOCAL_CNY: &str = "local_cny";
const PRICE_TEMPLATE_SOURCE_LOCAL_CNY_FX: &str = "local_cny_fx";

#[derive(Debug, Serialize)]
pub struct ProviderPriceRecord {
    pub id: DbId,
    pub provider: String,
    pub model: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: BillingMeter,
    pub unit_price_micros: Option<i64>,
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
    pub billing_meter: BillingMeter,
    pub capabilities: Value,
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
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: BillingMeter,
    pub unit_price_micros: Option<i64>,
    pub pricing_basis: PricingBasis,
    pub source: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ModelReferenceCatalogRecord {
    pub id: DbId,
    pub provider: String,
    pub model: String,
    pub display_name: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: BillingMeter,
    pub unit_price_micros: Option<i64>,
    pub pricing_basis: PricingBasis,
    pub source: String,
    pub enabled: bool,
    pub capabilities: Value,
    pub model_source: String,
    pub model_updated_at: DateTime<Utc>,
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
    pub removed: u64,
}

#[derive(Debug, Deserialize)]
pub struct UpsertProviderPriceRequest {
    pub provider: String,
    pub model: String,
    pub input_price_micros: i64,
    pub output_price_micros: i64,
    pub cache_read_price_micros: Option<i64>,
    pub cache_write_price_micros: Option<i64>,
    pub billing_meter: BillingMeter,
    pub unit_price_micros: Option<i64>,
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
    #[serde(default)]
    metadata: Option<ModelsDevProviderMetadata>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevProviderMetadata {
    /// 汇率换算脚本(fetch_modelsdev_pricing.py)写入的 USD->CNY 汇率。
    /// 存在即表示该 provider 的价格为汇率换算价,非原生 CNY 官方价。
    #[allow(dead_code)]
    fx_rate: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct ModelsDevModel {
    id: Option<String>,
    name: Option<String>,
    family: Option<String>,
    attachment: Option<bool>,
    reasoning: Option<bool>,
    tool_call: Option<bool>,
    structured_output: Option<bool>,
    temperature: Option<bool>,
    knowledge: Option<String>,
    release_date: Option<String>,
    last_updated: Option<String>,
    modalities: Option<ModelsDevModalities>,
    open_weights: Option<bool>,
    limit: Option<ModelsDevLimit>,
    cost: Option<ModelsDevCost>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelsDevModalities {
    #[serde(default)]
    input: Vec<String>,
    #[serde(default)]
    output: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ModelsDevLimit {
    context: Option<i64>,
    input: Option<i64>,
    output: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct ModelsDevCost {
    input: Option<f64>,
    output: Option<f64>,
    cache_read: Option<f64>,
    cache_write: Option<f64>,
    // 非 token 口径字段(由抓取脚本写入)
    per_image: Option<f64>,
    per_call: Option<f64>,
    per_hour: Option<f64>,
    per_second: Option<f64>,
    #[allow(dead_code)]
    per_unit: Option<f64>,
    #[allow(dead_code)]
    per_thousand_calls: Option<f64>,
    per_10k_token_input: Option<f64>,
    per_10k_token_output: Option<f64>,
    /// 抓取脚本推断的口径字符串(见 PricingBasis::as_str)。
    basis: Option<String>,
    /// 多档视频价的完整档位结构(由抓取脚本 parse_multi_tier_video 写入),
    /// 仅 `basis=multi_tier_video` 时存在。代表档仍写入 input/output。
    video_tiers: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Copy)]
struct PricingMicros {
    input_price_micros: i64,
    output_price_micros: i64,
    cache_read_price_micros: Option<i64>,
    cache_write_price_micros: Option<i64>,
    billing_meter: BillingMeter,
    unit_price_micros: Option<i64>,
    pricing_basis: PricingBasis,
}

fn default_enabled() -> bool {
    true
}

fn default_pricing_template_sync_source() -> String {
    PRICE_TEMPLATE_SOURCE_MODELS_DEV.to_string()
}

pub async fn list_provider_models(state: &AppState) -> AppResult<Vec<ProviderModelRecord>> {
    let rows = sqlx::query(
        "SELECT id, provider, model, display_name, source, billing_meter,
                capabilities, enabled,
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
        "SELECT id, provider, model, input_price_micros,
                output_price_micros, cache_read_price_micros,
                cache_write_price_micros, billing_meter,
                unit_price_micros,
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
        "SELECT id, provider, model, input_price_micros,
                output_price_micros, cache_read_price_micros,
                cache_write_price_micros, billing_meter,
                unit_price_micros, pricing_basis, source, enabled,
                created_at, updated_at
         FROM pricing_template
         ORDER BY provider ASC, model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(pricing_template_from_row).collect()
}

pub async fn list_model_reference_catalog(
    state: &AppState,
) -> AppResult<Vec<ModelReferenceCatalogRecord>> {
    let rows = sqlx::query(
        "SELECT pt.id, pt.provider, pt.model, pm.display_name,
                pt.input_price_micros, pt.output_price_micros,
                pt.cache_read_price_micros, pt.cache_write_price_micros,
                pt.billing_meter, pt.unit_price_micros, pt.pricing_basis,
                pt.source, pt.enabled, pm.capabilities, pm.source AS model_source,
                pm.updated_at AS model_updated_at, pt.created_at, pt.updated_at
         FROM pricing_template pt
         JOIN provider_model pm
           ON pm.provider = pt.provider AND pm.model = pt.model
         ORDER BY pt.provider ASC, pt.model ASC",
    )
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter().map(model_reference_catalog_from_row).collect()
}

pub async fn live_model_reference_catalog(
    state: &AppState,
) -> AppResult<Vec<ModelReferenceCatalogRecord>> {
    let upstream = match state.config.billing_currency {
        BillingCurrency::Usd => fetch_models_dev_pricing_json(state).await?,
        BillingCurrency::Cny => fetch_local_cny_pricing_json(state).await?,
    };
    let provider_codes = enabled_provider_codes(state).await?;
    let now = Utc::now();
    let mut records = Vec::new();

    for (upstream_provider, provider_data) in upstream {
        let Some(provider) = normalize_template_provider(&upstream_provider) else {
            continue;
        };
        if !provider_codes.contains(provider) {
            continue;
        }

        let source = match state.config.billing_currency {
            BillingCurrency::Usd => PRICE_TEMPLATE_SOURCE_MODELS_DEV,
            BillingCurrency::Cny => provider_source_for_local_cny(&provider_data),
        };
        for (model, model_data) in provider_data.models {
            let template = match state.config.billing_currency {
                BillingCurrency::Usd => {
                    pricing_template_from_models_dev_model(provider, &model, &model_data)
                }
                BillingCurrency::Cny => {
                    pricing_template_from_local_cny_model(provider, &model, &model_data)
                }
            };
            let Some(template) = template else { continue };
            let Some(prices) = template.prices else {
                continue;
            };
            records.push(ModelReferenceCatalogRecord {
                id: 0,
                provider: provider.to_string(),
                model: model.trim().to_string(),
                display_name: model_data
                    .name
                    .clone()
                    .filter(|name| !name.trim().is_empty())
                    .unwrap_or_else(|| model.trim().to_string()),
                input_price_micros: prices.input_price_micros,
                output_price_micros: prices.output_price_micros,
                cache_read_price_micros: prices.cache_read_price_micros,
                cache_write_price_micros: prices.cache_write_price_micros,
                billing_meter: template.billing_meter,
                unit_price_micros: prices.unit_price_micros,
                pricing_basis: prices.pricing_basis,
                source: source.to_string(),
                enabled: true,
                capabilities: template.capabilities,
                model_source: "upstream".to_string(),
                model_updated_at: now,
                created_at: now,
                updated_at: now,
            });
        }
    }

    records.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(records)
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

/// 判断 local_cny 目录中的 provider 价格来源:含 `metadata.fx_rate` 的为汇率换算价,
/// 否则为原生 CNY 官方价。返回对应的 `pricing_template.source` 值。
fn provider_source_for_local_cny(provider_data: &ModelsDevProvider) -> &'static str {
    if provider_data
        .metadata
        .as_ref()
        .is_some_and(|meta| meta.fx_rate.is_some())
    {
        PRICE_TEMPLATE_SOURCE_LOCAL_CNY_FX
    } else {
        PRICE_TEMPLATE_SOURCE_LOCAL_CNY
    }
}

pub async fn sync_pricing_templates(
    state: &AppState,
    req: SyncPricingTemplatesRequest,
) -> AppResult<PricingTemplateSyncResult> {
    match req.source.trim() {
        "" => match state.config.billing_currency {
            BillingCurrency::Usd => sync_models_dev_pricing_templates(state).await,
            BillingCurrency::Cny => sync_local_cny_pricing_templates(state).await,
        },
        PRICE_TEMPLATE_SOURCE_MODELS_DEV => sync_models_dev_pricing_templates(state).await,
        PRICE_TEMPLATE_SOURCE_LOCAL_CNY => sync_local_cny_pricing_templates(state).await,
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
         (provider, model, input_price_micros,
          output_price_micros, cache_read_price_micros,
          cache_write_price_micros, billing_meter,
          unit_price_micros, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         ON CONFLICT (provider, model)
         DO UPDATE SET
             input_price_micros = EXCLUDED.input_price_micros,
             output_price_micros = EXCLUDED.output_price_micros,
             cache_read_price_micros = EXCLUDED.cache_read_price_micros,
             cache_write_price_micros = EXCLUDED.cache_write_price_micros,
             billing_meter = EXCLUDED.billing_meter,
             unit_price_micros = EXCLUDED.unit_price_micros,
             enabled = EXCLUDED.enabled,
             updated_at = now()
         RETURNING id, provider, model, input_price_micros,
                   output_price_micros, cache_read_price_micros,
                   cache_write_price_micros, billing_meter,
                   unit_price_micros,
                   enabled, created_at, updated_at",
    )
    .bind(req.provider)
    .bind(req.model)
    .bind(req.input_price_micros)
    .bind(req.output_price_micros)
    .bind(req.cache_read_price_micros)
    .bind(req.cache_write_price_micros)
    .bind(req.billing_meter.as_str())
    .bind(req.unit_price_micros)
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
             JOIN channel c ON c.id = ce.channel_id
             WHERE cm.channel_id = ce.channel_id
               AND c.provider = $1
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
            "UPDATE channel_model cm
             SET enabled = FALSE,
                 updated_at = now()
             FROM channel c
             WHERE c.id = cm.channel_id
               AND c.provider = $1
               AND cm.model = $2",
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
            let Some(template) =
                pricing_template_from_models_dev_model(provider, &model, &model_data)
            else {
                skipped += 1;
                continue;
            };
            saved +=
                upsert_synced_pricing_template(state, template, PRICE_TEMPLATE_SOURCE_MODELS_DEV)
                    .await?;
        }
    }

    let removed = prune_stale_pricing_templates(state, PRICE_TEMPLATE_SOURCE_LOCAL_CNY).await?;

    Ok(PricingTemplateSyncResult {
        source: PRICE_TEMPLATE_SOURCE_MODELS_DEV.to_string(),
        fetched,
        saved,
        skipped,
        removed,
    })
}

const LOCAL_CNY_PRICING_JSON_PATH: &str = "/model-pricing.json";

async fn sync_local_cny_pricing_templates(
    state: &AppState,
) -> AppResult<PricingTemplateSyncResult> {
    let upstream = fetch_local_cny_pricing_json(state).await?;
    apply_local_cny_pricing_templates(state, upstream).await
}

async fn fetch_local_cny_pricing_json(
    state: &AppState,
) -> AppResult<HashMap<String, ModelsDevProvider>> {
    let Some(base_url) = state.config.public_base_url.as_deref() else {
        return Err(AppError::BadRequest(
            "PUBLIC_BASE_URL is not configured, cannot fetch local CNY pricing JSON".to_string(),
        ));
    };
    let url = format!("{base_url}{LOCAL_CNY_PRICING_JSON_PATH}");
    state
        .http
        .get(&url)
        .send()
        .await
        .map_err(local_cny_pricing_unavailable)?
        .error_for_status()
        .map_err(local_cny_pricing_unavailable)?
        .json::<HashMap<String, ModelsDevProvider>>()
        .await
        .map_err(local_cny_pricing_unavailable)
}

async fn fetch_models_dev_pricing_json(
    state: &AppState,
) -> AppResult<HashMap<String, ModelsDevProvider>> {
    state
        .http
        .get(MODELS_DEV_PRICING_URL)
        .send()
        .await
        .map_err(models_dev_pricing_unavailable)?
        .error_for_status()
        .map_err(models_dev_pricing_unavailable)?
        .json::<HashMap<String, ModelsDevProvider>>()
        .await
        .map_err(models_dev_pricing_unavailable)
}

async fn apply_local_cny_pricing_templates(
    state: &AppState,
    upstream: HashMap<String, ModelsDevProvider>,
) -> AppResult<PricingTemplateSyncResult> {
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

        let source = provider_source_for_local_cny(&provider_data);
        for (model, model_data) in provider_data.models {
            let Some(template) =
                pricing_template_from_local_cny_model(provider, &model, &model_data)
            else {
                skipped += 1;
                continue;
            };
            saved += upsert_synced_pricing_template(state, template, source).await?;
        }
    }

    let removed = prune_stale_pricing_templates(state, PRICE_TEMPLATE_SOURCE_MODELS_DEV).await?;

    Ok(PricingTemplateSyncResult {
        source: PRICE_TEMPLATE_SOURCE_LOCAL_CNY.to_string(),
        fetched,
        saved,
        skipped,
        removed,
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
fn local_cny_pricing_unavailable(err: reqwest::Error) -> AppError {
    tracing::warn!(
        error = %err,
        error_debug = ?err,
        "failed to sync local CNY pricing templates from public base url"
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
    billing_meter: BillingMeter,
    capabilities: Value,
    prices: Option<PricingMicros>,
}

fn pricing_template_from_models_dev_model<'a>(
    provider: &'a str,
    model: &'a str,
    model_data: &ModelsDevModel,
) -> Option<PricingTemplateUpsert<'a>> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    let billing_meter = billing_meter_from_models_dev_model(model_data);
    let prices = pricing_micros_from_models_dev_model(model_data);
    Some(PricingTemplateUpsert {
        provider,
        model,
        billing_meter,
        capabilities: models_dev_capabilities(model_data),
        prices,
    })
}

fn pricing_template_from_local_cny_model<'a>(
    provider: &'a str,
    model: &'a str,
    model_data: &ModelsDevModel,
) -> Option<PricingTemplateUpsert<'a>> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }
    let billing_meter = billing_meter_from_models_dev_model(model_data);
    let prices = pricing_micros_from_local_pricing_model(model_data);
    Some(PricingTemplateUpsert {
        provider,
        model,
        billing_meter,
        capabilities: local_pricing_capabilities(model_data),
        prices,
    })
}

fn pricing_micros_from_models_dev_model(model: &ModelsDevModel) -> Option<PricingMicros> {
    pricing_micros_from_cost(model.cost.as_ref()?)
}

fn pricing_micros_from_local_pricing_model(model: &ModelsDevModel) -> Option<PricingMicros> {
    pricing_micros_from_cost(model.cost.as_ref()?)
}

/// 按 `cost.basis` 口径分流构造参考价微单位。
/// 所有口径共用 `×1_000_000` 微单位换算,前端按 `pricing_basis` 选择展示标签。
/// `billing_meter` 仅 token/image,不引入新值,实际计费链路不受影响。
fn pricing_micros_from_cost(cost: &ModelsDevCost) -> Option<PricingMicros> {
    let basis = parse_pricing_basis(cost.basis.as_deref());
    use PricingBasis::*;
    match basis {
        Image => {
            let unit_price = cny_to_micros(cost.per_image)?;
            Some(PricingMicros {
                input_price_micros: 0,
                output_price_micros: 0,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Image,
                unit_price_micros: Some(unit_price),
                pricing_basis: Image,
            })
        }
        Call => {
            let unit_price = cny_to_micros(cost.per_call)?;
            Some(PricingMicros {
                input_price_micros: 0,
                output_price_micros: 0,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: Some(unit_price),
                pricing_basis: Call,
            })
        }
        Hour => {
            let unit_price = cny_to_micros(cost.per_hour)?;
            Some(PricingMicros {
                input_price_micros: 0,
                output_price_micros: 0,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: Some(unit_price),
                pricing_basis: Hour,
            })
        }
        Second => {
            let unit_price = cny_to_micros(cost.per_second)?;
            Some(PricingMicros {
                input_price_micros: 0,
                output_price_micros: 0,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: Some(unit_price),
                pricing_basis: Second,
            })
        }
        Per10kToken => {
            let input = per_10k_to_micros(cost.per_10k_token_input)?;
            let output = per_10k_to_micros(cost.per_10k_token_output)
                .or_else(|| per_10k_to_micros(cost.per_10k_token_input))?;
            Some(PricingMicros {
                input_price_micros: input,
                output_price_micros: output,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: None,
                pricing_basis: Per10kToken,
            })
        }
        MultiTierVideo => {
            // input/output 已由抓取脚本取代表档(最低档)写入,按 token 微单位处理。
            let input = per_million_to_micros(cost.input)?;
            let output =
                per_million_to_micros(cost.output).or_else(|| per_million_to_micros(cost.input))?;
            Some(PricingMicros {
                input_price_micros: input,
                output_price_micros: output,
                cache_read_price_micros: None,
                cache_write_price_micros: None,
                billing_meter: BillingMeter::Token,
                unit_price_micros: None,
                pricing_basis: MultiTierVideo,
            })
        }
        Token => {
            let input = per_million_to_micros(cost.input)?;
            let output = per_million_to_micros(cost.output)?;
            Some(PricingMicros {
                input_price_micros: input,
                output_price_micros: output,
                cache_read_price_micros: per_million_to_micros(cost.cache_read),
                cache_write_price_micros: per_million_to_micros(cost.cache_write),
                billing_meter: BillingMeter::Token,
                unit_price_micros: None,
                pricing_basis: Token,
            })
        }
    }
}

fn parse_pricing_basis(value: Option<&str>) -> PricingBasis {
    match value {
        Some("image") => PricingBasis::Image,
        Some("call") => PricingBasis::Call,
        Some("hour") => PricingBasis::Hour,
        Some("second") => PricingBasis::Second,
        Some("per_10k_token") => PricingBasis::Per10kToken,
        Some("multi_tier_video") => PricingBasis::MultiTierVideo,
        _ => PricingBasis::Token,
    }
}

fn billing_meter_from_models_dev_model(model: &ModelsDevModel) -> BillingMeter {
    if model
        .modalities
        .as_ref()
        .is_some_and(|modalities| modality_contains(&modalities.output, "image"))
    {
        BillingMeter::Image
    } else {
        BillingMeter::Token
    }
}

fn modality_contains(values: &[String], target: &str) -> bool {
    values
        .iter()
        .any(|value| value.trim().eq_ignore_ascii_case(target))
}

fn models_dev_capabilities(model: &ModelsDevModel) -> Value {
    json!({
        "id": model.id,
        "name": model.name,
        "family": model.family,
        "attachment": model.attachment,
        "reasoning": model.reasoning,
        "tool_call": model.tool_call,
        "structured_output": model.structured_output,
        "temperature": model.temperature,
        "knowledge": model.knowledge,
        "release_date": model.release_date,
        "last_updated": model.last_updated,
        "modalities": model.modalities,
        "open_weights": model.open_weights,
        "limit": model.limit,
    })
}

fn local_pricing_capabilities(model: &ModelsDevModel) -> Value {
    let mut payload = json!({
        "id": model.id,
        "name": model.name,
        "modalities": model.modalities,
    });
    if let Some(cost) = model.cost.as_ref() {
        if let Some(video_tiers) = cost.video_tiers.as_ref() {
            payload["video_tiers"] = json!(video_tiers);
        }
    }
    payload
}

async fn upsert_synced_pricing_template(
    state: &AppState,
    template: PricingTemplateUpsert<'_>,
    source: &str,
) -> AppResult<u64> {
    let model = template.model.trim();
    sqlx::query(
        "INSERT INTO provider_model
         (provider, model, display_name, source, billing_meter, capabilities, enabled)
         VALUES ($1, $2, $2, 'upstream', $3, $4, FALSE)
         ON CONFLICT (provider, model)
         DO UPDATE SET
             billing_meter = EXCLUDED.billing_meter,
             capabilities = EXCLUDED.capabilities,
             updated_at = now()",
    )
    .bind(template.provider)
    .bind(model)
    .bind(template.billing_meter.as_str())
    .bind(&template.capabilities)
    .execute(&state.db.pool)
    .await?;

    let Some(prices) = template.prices else {
        return Ok(0);
    };
    let result = sqlx::query(
        "INSERT INTO pricing_template
         (provider, model, input_price_micros,
          output_price_micros, cache_read_price_micros,
          cache_write_price_micros, billing_meter,
          unit_price_micros, pricing_basis, source, enabled)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, TRUE)
         ON CONFLICT (provider, model)
         DO UPDATE SET
             input_price_micros = EXCLUDED.input_price_micros,
             output_price_micros = EXCLUDED.output_price_micros,
             cache_read_price_micros = EXCLUDED.cache_read_price_micros,
             cache_write_price_micros = EXCLUDED.cache_write_price_micros,
             billing_meter = EXCLUDED.billing_meter,
             unit_price_micros = EXCLUDED.unit_price_micros,
             pricing_basis = EXCLUDED.pricing_basis,
             source = EXCLUDED.source,
             enabled = TRUE,
             updated_at = now()
         ",
    )
    .bind(template.provider)
    .bind(model)
    .bind(prices.input_price_micros)
    .bind(prices.output_price_micros)
    .bind(prices.cache_read_price_micros)
    .bind(prices.cache_write_price_micros)
    .bind(prices.billing_meter.as_str())
    .bind(prices.unit_price_micros)
    .bind(prices.pricing_basis.as_str())
    .bind(source)
    .execute(&state.db.pool)
    .await?;
    Ok(result.rows_affected())
}

/// 同步切换计费币种/数据源后,清理上一个自动同步源残留的参考价记录,
/// 避免 CNY 计费下残留的 models.dev USD 价格被当作 CNY 展示(反之亦然)。
/// 仅清理指定的旧自动源;手动确认价(confirmed_price)等其它来源不受影响。
async fn prune_stale_pricing_templates(state: &AppState, stale_source: &str) -> AppResult<u64> {
    let result = sqlx::query(
        "DELETE FROM pricing_template
         WHERE source = $1",
    )
    .bind(stale_source)
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
        "zhipu" | "zhipuai" | "bigmodel" => Some("zhipu"),
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

fn per_million_to_micros(value: Option<f64>) -> Option<i64> {
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

/// 参考价展示用:按张/按次/按小时的 CNY 单价转 micro-CNY。
/// 与 `per_million_to_micros` 同为 ×1_000_000,前端按 `pricing_basis` 标签区分单位语义。
fn cny_to_micros(value: Option<f64>) -> Option<i64> {
    per_million_to_micros(value)
}

/// 参考价展示用:按万 token 的 CNY 单价转 micro-CNY。前端按"万Token"口径展示。
fn per_10k_to_micros(value: Option<f64>) -> Option<i64> {
    per_million_to_micros(value)
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
    let prices = PricingMicros {
        input_price_micros: req.input_price_micros,
        output_price_micros: req.output_price_micros,
        cache_read_price_micros: req.cache_read_price_micros,
        cache_write_price_micros: req.cache_write_price_micros,
        billing_meter: req.billing_meter,
        unit_price_micros: req.unit_price_micros,
        // 手动录入价格默认按 token 口径展示;image 计费时展示口径同步为 image。
        pricing_basis: match req.billing_meter {
            BillingMeter::Image => PricingBasis::Image,
            BillingMeter::Token => PricingBasis::Token,
        },
    };
    if !prices_are_non_negative(prices) {
        return Err(AppError::BadRequest(
            "price must be non-negative".to_string(),
        ));
    }
    if prices.billing_meter == BillingMeter::Image {
        match prices.unit_price_micros {
            Some(price) if price > 0 => {}
            _ => {
                return Err(AppError::BadRequest(
                    "unit price is required for image billing".to_string(),
                ));
            }
        }
    }
    Ok(())
}

fn prices_are_non_negative(prices: PricingMicros) -> bool {
    prices.input_price_micros >= 0
        && prices.output_price_micros >= 0
        && prices
            .cache_read_price_micros
            .is_none_or(|price| price >= 0)
        && prices
            .cache_write_price_micros
            .is_none_or(|price| price >= 0)
        && prices.unit_price_micros.is_none_or(|price| price >= 0)
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
        billing_meter: billing_meter_from_row(row)?,
        capabilities: row.try_get("capabilities")?,
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
        input_price_micros: row.try_get("input_price_micros")?,
        output_price_micros: row.try_get("output_price_micros")?,
        cache_read_price_micros: row.try_get("cache_read_price_micros")?,
        cache_write_price_micros: row.try_get("cache_write_price_micros")?,
        billing_meter: billing_meter_from_row(row)?,
        unit_price_micros: row.try_get("unit_price_micros")?,
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
        input_price_micros: row.try_get("input_price_micros")?,
        output_price_micros: row.try_get("output_price_micros")?,
        cache_read_price_micros: row.try_get("cache_read_price_micros")?,
        cache_write_price_micros: row.try_get("cache_write_price_micros")?,
        billing_meter: billing_meter_from_row(row)?,
        unit_price_micros: row.try_get("unit_price_micros")?,
        pricing_basis: pricing_basis_from_row(row)?,
        source: row.try_get("source")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn model_reference_catalog_from_row(
    row: &sqlx::postgres::PgRow,
) -> AppResult<ModelReferenceCatalogRecord> {
    Ok(ModelReferenceCatalogRecord {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        display_name: row.try_get("display_name")?,
        input_price_micros: row.try_get("input_price_micros")?,
        output_price_micros: row.try_get("output_price_micros")?,
        cache_read_price_micros: row.try_get("cache_read_price_micros")?,
        cache_write_price_micros: row.try_get("cache_write_price_micros")?,
        billing_meter: billing_meter_from_row(row)?,
        unit_price_micros: row.try_get("unit_price_micros")?,
        pricing_basis: pricing_basis_from_row(row)?,
        source: row.try_get("source")?,
        enabled: row.try_get("enabled")?,
        capabilities: row.try_get("capabilities")?,
        model_source: row.try_get("model_source")?,
        model_updated_at: row.try_get("model_updated_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn billing_meter_from_row(row: &sqlx::postgres::PgRow) -> Result<BillingMeter, sqlx::Error> {
    let value: String = row.try_get("billing_meter")?;
    BillingMeter::from_strict_str(&value).map_err(|err| sqlx::Error::Decode(err.into()))
}

fn pricing_basis_from_row(row: &sqlx::postgres::PgRow) -> Result<PricingBasis, sqlx::Error> {
    let value: String = row.try_get("pricing_basis")?;
    Ok(PricingBasis::from_str_lenient(&value))
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
    fn converts_models_dev_per_million_to_micros() {
        assert_eq!(per_million_to_micros(Some(0.1)), Some(100_000));
        assert_eq!(per_million_to_micros(Some(1.25)), Some(1_250_000));
        assert_eq!(per_million_to_micros(Some(-1.0)), None);
        assert_eq!(per_million_to_micros(None), None);
    }

    #[test]
    fn normalizes_known_pricing_template_providers() {
        assert_eq!(
            normalize_template_provider("google-ai-studio"),
            Some("google")
        );
        assert_eq!(normalize_template_provider("dashscope"), Some("qwen"));
        assert_eq!(normalize_template_provider("zhipuai"), Some("zhipu"));
        assert_eq!(normalize_template_provider("zhipuai-coding-plan"), None);
        assert_eq!(normalize_template_provider("unknown-provider"), None);
    }

    #[test]
    fn parses_models_dev_cache_prices() {
        let model: ModelsDevModel = serde_json::from_str(
            r#"{"cost":{"input":5,"output":30,"cache_read":0.5,"cache_write":6.25}}"#,
        )
        .unwrap();
        let cost = model.cost.unwrap();

        assert_eq!(per_million_to_micros(cost.input), Some(5_000_000));
        assert_eq!(per_million_to_micros(cost.output), Some(30_000_000));
        assert_eq!(per_million_to_micros(cost.cache_read), Some(500_000));
        assert_eq!(per_million_to_micros(cost.cache_write), Some(6_250_000));
    }

    #[test]
    fn keeps_models_dev_image_output_cost_as_token_pricing() {
        let model: ModelsDevModel = serde_json::from_str(
            r#"{
                "modalities":{"input":["text"],"output":["image"]},
                "cost":{"input":8,"output":30}
            }"#,
        )
        .unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(
            billing_meter_from_models_dev_model(&model),
            BillingMeter::Image
        );
        assert_eq!(prices.billing_meter, BillingMeter::Token);
        assert_eq!(prices.pricing_basis, PricingBasis::Token);
        assert_eq!(prices.input_price_micros, 8_000_000);
        assert_eq!(prices.output_price_micros, 30_000_000);
        assert_eq!(prices.unit_price_micros, None);
    }

    #[test]
    fn pricing_basis_image_uses_unit_price() {
        let model: ModelsDevModel =
            serde_json::from_str(r#"{"cost":{"per_image":0.22,"basis":"image"}}"#).unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(prices.pricing_basis, PricingBasis::Image);
        assert_eq!(prices.billing_meter, BillingMeter::Image);
        assert_eq!(prices.unit_price_micros, Some(220_000));
        assert_eq!(prices.input_price_micros, 0);
        assert_eq!(prices.output_price_micros, 0);
    }

    #[test]
    fn pricing_basis_call_uses_unit_price() {
        let model: ModelsDevModel =
            serde_json::from_str(r#"{"cost":{"per_call":2.4,"basis":"call"}}"#).unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(prices.pricing_basis, PricingBasis::Call);
        assert_eq!(prices.billing_meter, BillingMeter::Token);
        assert_eq!(prices.unit_price_micros, Some(2_400_000));
    }

    #[test]
    fn pricing_basis_per_10k_token_uses_input_output() {
        let model: ModelsDevModel = serde_json::from_str(
            r#"{"cost":{"per_10k_token_input":0.36,"per_10k_token_output":3.6,"basis":"per_10k_token"}}"#,
        )
        .unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(prices.pricing_basis, PricingBasis::Per10kToken);
        assert_eq!(prices.input_price_micros, 360_000);
        assert_eq!(prices.output_price_micros, 3_600_000);
    }

    #[test]
    fn pricing_basis_multi_tier_video_uses_representative_tier() {
        let model: ModelsDevModel =
            serde_json::from_str(r#"{"cost":{"input":16,"output":16,"basis":"multi_tier_video"}}"#)
                .unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(prices.pricing_basis, PricingBasis::MultiTierVideo);
        assert_eq!(prices.input_price_micros, 16_000_000);
        assert_eq!(prices.output_price_micros, 16_000_000);
    }

    #[test]
    fn pricing_basis_second_uses_unit_price() {
        let model: ModelsDevModel =
            serde_json::from_str(r#"{"cost":{"per_second":0.9,"basis":"second"}}"#).unwrap();
        let prices = pricing_micros_from_models_dev_model(&model).unwrap();

        assert_eq!(prices.pricing_basis, PricingBasis::Second);
        assert_eq!(prices.billing_meter, BillingMeter::Token);
        assert_eq!(prices.unit_price_micros, Some(900_000));
    }
}
