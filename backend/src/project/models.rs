use std::{collections::HashSet, sync::Arc, time::Instant};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::{
    admin::setting::resolve_default_text_model,
    billing::BILLABLE_PROVIDER_PRICE_CONDITION_PP,
    config::DEFAULT_ANTHROPIC_VERSION,
    error::{AppError, AppResult},
    id::DbId,
    relay::{
        read_upstream_error_body,
        selector::{SelectedUpstream, UpstreamProtocol},
        upstream_url,
    },
    AppState,
};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectModelRecord {
    pub id: DbId,
    pub project_id: DbId,
    pub model: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    pub target_channel_name: Option<String>,
    pub route_mode: String,
    pub routing_config: ProjectModelRoutingConfig,
    pub candidates: Vec<ProjectModelCandidateRecord>,
    pub enabled: bool,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ResolvedProjectModel {
    pub external_model: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectModelCandidateRecord {
    pub id: DbId,
    pub project_model_id: DbId,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    pub target_channel_name: Option<String>,
    pub tier: String,
    pub priority: i32,
    pub weight: i32,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectModelRoutingConfig {
    #[serde(default = "default_smart_model_name")]
    pub smart_model_name: String,
    #[serde(default = "default_routing_tier")]
    pub default_tier: String,
    #[serde(default = "default_low_confidence_threshold")]
    pub low_confidence_threshold: f64,
    #[serde(default)]
    pub classifier_enabled: bool,
    #[serde(default)]
    pub classifier_model: Option<String>,
}

impl Default for ProjectModelRoutingConfig {
    fn default() -> Self {
        Self {
            smart_model_name: default_smart_model_name(),
            default_tier: default_routing_tier(),
            low_confidence_threshold: default_low_confidence_threshold(),
            classifier_enabled: false,
            classifier_model: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpsertProjectModelCandidateRequest {
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    pub tier: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_candidate_weight")]
    pub weight: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpsertProjectModelRequest {
    pub model: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    #[serde(default = "default_route_mode")]
    pub route_mode: String,
    #[serde(default)]
    pub routing_config: ProjectModelRoutingConfig,
    #[serde(default)]
    pub candidates: Vec<UpsertProjectModelCandidateRequest>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectModelRequest {
    pub model: Option<String>,
    pub target_model: Option<String>,
    pub target_channel_id: Option<Option<DbId>>,
    pub routing_config: Option<ProjectModelRoutingConfig>,
    pub candidates: Option<Vec<UpsertProjectModelCandidateRequest>>,
    pub enabled: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AutoConfigureProjectModelRequest {
    #[serde(default = "default_auto_configure_mode")]
    pub mode: String,
    pub classifier_model: Option<String>,
    #[serde(default = "default_max_candidates_per_tier")]
    pub max_candidates_per_tier: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AutoConfigureResponse {
    pub suggestions: Vec<AutoSuggestion>,
    pub warnings: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSuggestion {
    pub tier: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    pub target_channel_name: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
struct AutoConfigureAvailableModel {
    model: String,
    provider: String,
    channel_id: DbId,
    channel_name: String,
    protocol: String,
    input_price_usd_micros: Option<i64>,
    output_price_usd_micros: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct LlmAutoConfigureResponse {
    simple: Option<LlmAutoConfigureItem>,
    standard: Option<LlmAutoConfigureItem>,
    advanced: Option<LlmAutoConfigureItem>,
}

#[derive(Debug, Deserialize)]
struct LlmAutoConfigureItem {
    model: String,
    channel_id: Option<DbId>,
    reason: Option<String>,
}

fn default_auto_configure_mode() -> String {
    "fill_missing".to_string()
}

fn default_max_candidates_per_tier() -> usize {
    1
}

fn default_enabled() -> bool {
    true
}

fn default_route_mode() -> String {
    "direct".to_string()
}

fn default_smart_model_name() -> String {
    "auto".to_string()
}

fn default_routing_tier() -> String {
    "standard".to_string()
}

fn default_low_confidence_threshold() -> f64 {
    0.7
}

fn default_candidate_weight() -> i32 {
    1
}

pub async fn list_project_models(
    pool: &PgPool,
    project_id: DbId,
) -> AppResult<Vec<ProjectModelRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT pm.id, pm.project_id, pm.model, pm.target_model, pm.target_channel_id,
               c.name AS target_channel_name,
               pm.route_mode, pm.routing_config,
               pm.enabled, pm.description, pm.created_at, pm.updated_at
        FROM project_model pm
        LEFT JOIN channel c ON c.id = pm.target_channel_id
        WHERE pm.project_id = $1
        ORDER BY pm.model ASC
        "#,
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;

    let mut records = Vec::with_capacity(rows.len());
    for row in rows {
        records.push(project_model_from_row(pool, &row).await?);
    }
    Ok(records)
}

pub async fn project_has_models(pool: &PgPool, project_id: DbId) -> AppResult<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM project_model WHERE project_id = $1)",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    Ok(exists)
}

pub async fn auto_configure_project_model(
    state: &Arc<AppState>,
    project_id: DbId,
    req: AutoConfigureProjectModelRequest,
) -> AppResult<AutoConfigureResponse> {
    ensure_project_exists(&state.db.pool, project_id).await?;
    let mode = normalize_auto_configure_mode(&req.mode)?;
    let max_candidates_per_tier = req.max_candidates_per_tier.clamp(1, 3);
    let available = list_auto_configure_available_models(&state.db.pool).await?;
    if available.is_empty() {
        return Err(AppError::BadRequest(
            "没有可用于自动配置的上游模型".to_string(),
        ));
    }
    let existing_tiers = existing_smart_candidate_tiers(&state.db.pool, project_id).await?;
    let mut warnings = Vec::new();
    let configured_model = req
        .classifier_model
        .as_deref()
        .map(normalize_model)
        .transpose()?;
    state.selector.invalidate().await;
    let configured_classifier = if let Some(model) = configured_model {
        resolve_admin_text_model(state, &model, None).await
    } else if let Some((model, channel_id, _)) = resolve_default_text_model(state).await? {
        resolve_admin_text_model(state, &model, Some(channel_id)).await
    } else {
        Ok(None)
    }?;
    let Some((classifier_model, classifier_protocol, upstream)) = configured_classifier else {
        return Err(AppError::BadRequest(
            "请先在其他设置中配置一个可调用的默认文本大模型，再使用自动配置。".to_string(),
        ));
    };
    let mut suggestions = match llm_auto_configure_suggestions(
        state,
        classifier_protocol,
        &classifier_model,
        &upstream,
        &available,
        &existing_tiers,
        &mode,
        max_candidates_per_tier,
    )
    .await
    {
        Ok(items) if !items.is_empty() => items,
        Ok(_) => {
            return Err(AppError::BadRequest(
                "默认文本大模型没有返回可用建议，请调整默认文本大模型后重试。".to_string(),
            ))
        }
        Err(err) => {
            tracing::warn!(error = %err, "smart model auto-configure LLM failed");
            return Err(AppError::UpstreamUnavailable(
                "默认文本大模型暂时不可用，请检查其他设置中的默认文本大模型。".to_string(),
            ));
        }
    };
    suggestions = filter_auto_configure_mode(suggestions, &existing_tiers, &mode);
    if suggestions.is_empty() && mode == "fill_missing" && !existing_tiers.is_empty() {
        warnings.push("当前智能模型已包含简单、标准、高级档位，无需补全。".to_string());
    }
    Ok(AutoConfigureResponse {
        suggestions,
        warnings,
        source: "llm".to_string(),
    })
}

pub async fn resolve_project_model(
    pool: &PgPool,
    project_id: DbId,
    requested_model: &str,
) -> AppResult<ResolvedProjectModel> {
    resolve_project_model_with_context(pool, project_id, requested_model, None).await
}

#[derive(Debug, Clone, Default)]
pub struct ProjectModelRequestContext {
    pub text: String,
    pub message_count: usize,
    pub has_tools: bool,
    pub has_response_format: bool,
    pub has_images: bool,
    pub reasoning_effort: Option<String>,
}

impl ProjectModelRequestContext {
    pub fn from_value(value: &Value) -> Self {
        let mut context = Self::default();
        context.has_tools = value
            .get("tools")
            .and_then(Value::as_array)
            .map(|items| !items.is_empty())
            .unwrap_or(false);
        context.has_response_format = value.get("response_format").is_some()
            || value
                .get("text")
                .and_then(|text| text.get("format"))
                .is_some();
        context.reasoning_effort = value
            .get("reasoning_effort")
            .or_else(|| {
                value
                    .get("reasoning")
                    .and_then(|reasoning| reasoning.get("effort"))
            })
            .and_then(Value::as_str)
            .map(str::to_string);
        collect_request_text(value, &mut context);
        context
    }
}

fn collect_request_text(value: &Value, context: &mut ProjectModelRequestContext) {
    match value {
        Value::String(text) => {
            context.text.push_str(text);
            context.text.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_request_text(item, context);
            }
        }
        Value::Object(object) => {
            if object.contains_key("role")
                && (object.contains_key("content") || object.contains_key("parts"))
            {
                context.message_count += 1;
            }
            if object
                .get("type")
                .and_then(Value::as_str)
                .map(|item_type| item_type.contains("image"))
                .unwrap_or(false)
            {
                context.has_images = true;
            }
            for (key, item) in object {
                if matches!(
                    key.as_str(),
                    "messages" | "input" | "content" | "parts" | "prompt" | "system"
                ) {
                    collect_request_text(item, context);
                }
            }
        }
        _ => {}
    }
}

pub async fn resolve_project_model_with_context(
    pool: &PgPool,
    project_id: DbId,
    requested_model: &str,
    context: Option<ProjectModelRequestContext>,
) -> AppResult<ResolvedProjectModel> {
    let requested_model = normalize_model(requested_model)?;
    let row = sqlx::query(
        r#"
        SELECT id, model, target_model, target_channel_id, route_mode, routing_config, enabled,
               EXISTS (SELECT 1 FROM project_model WHERE project_id = $1) AS project_has_models
        FROM project_model
        WHERE project_id = $1 AND model = $2
        "#,
    )
    .bind(project_id)
    .bind(&requested_model)
    .fetch_optional(pool)
    .await?;

    if let Some(row) = row {
        let enabled: bool = row.try_get("enabled")?;
        if !enabled {
            return Err(AppError::Forbidden);
        }
        let route_mode: String = row.try_get("route_mode")?;
        if route_mode == "smart" {
            return resolve_smart_project_model(pool, project_id, &row, context).await;
        }
        return Ok(ResolvedProjectModel {
            external_model: row.try_get("model")?,
            target_model: row.try_get("target_model")?,
            target_channel_id: row.try_get("target_channel_id")?,
        });
    }

    if project_has_models(pool, project_id).await? {
        return Err(AppError::Forbidden);
    }

    Ok(ResolvedProjectModel {
        external_model: requested_model.clone(),
        target_model: requested_model,
        target_channel_id: None,
    })
}

async fn resolve_smart_project_model(
    pool: &PgPool,
    project_id: DbId,
    row: &sqlx::postgres::PgRow,
    context: Option<ProjectModelRequestContext>,
) -> AppResult<ResolvedProjectModel> {
    let started = Instant::now();
    let project_model_id: DbId = row.try_get("id")?;
    let requested_model: String = row.try_get("model")?;
    let fallback_model: String = row.try_get("target_model")?;
    let fallback_channel_id: Option<DbId> = row.try_get("target_channel_id")?;
    let config = routing_config_from_value(row.try_get("routing_config")?)?;
    let decision = classify_request(context.as_ref(), &config);
    let candidates = list_project_model_candidates(pool, project_model_id).await?;
    let (selected, decision_source, reason) = choose_smart_candidate(&candidates, &decision.tier)
        .unwrap_or_else(|| {
            (
                None,
                "fallback".to_string(),
                format!(
                    "no enabled candidate for tier {}; using fallback model",
                    decision.tier
                ),
            )
        });

    let (target_model, target_channel_id) = selected
        .map(|candidate| (candidate.target_model, candidate.target_channel_id))
        .unwrap_or((fallback_model, fallback_channel_id));
    validate_target(pool, project_id, target_channel_id, &target_model).await?;
    record_routing_decision(
        pool,
        RoutingDecisionInsert {
            project_id,
            project_model_id,
            requested_model: &requested_model,
            selected_model: &target_model,
            selected_channel_id: target_channel_id,
            decision_source: &decision_source,
            tier: &decision.tier,
            confidence: decision.confidence,
            reason: if reason.is_empty() {
                &decision.reason
            } else {
                &reason
            },
            latency_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
        },
    )
    .await?;
    Ok(ResolvedProjectModel {
        external_model: requested_model,
        target_model,
        target_channel_id,
    })
}

struct RoutingDecision {
    tier: String,
    confidence: f64,
    reason: String,
}

fn classify_request(
    context: Option<&ProjectModelRequestContext>,
    config: &ProjectModelRoutingConfig,
) -> RoutingDecision {
    let Some(context) = context else {
        return RoutingDecision {
            tier: config.default_tier.clone(),
            confidence: 0.4,
            reason: "missing request context".to_string(),
        };
    };
    let text = context.text.to_ascii_lowercase();
    let text_chars = context.text.chars().count();
    let has_code = text.contains("```")
        || text.contains(" traceback ")
        || text.contains(" exception")
        || text.contains("function ")
        || text.contains("class ")
        || text.contains("SELECT ".to_ascii_lowercase().as_str());
    let has_complex_keywords = [
        "推理",
        "证明",
        "架构",
        "复杂",
        "debug",
        "调试",
        "优化",
        "数学",
        "算法",
        "设计方案",
        "reason",
        "architecture",
        "analyze",
    ]
    .iter()
    .any(|keyword| text.contains(keyword));
    let reasoning_high = context
        .reasoning_effort
        .as_deref()
        .map(|value| value == "high" || value == "medium")
        .unwrap_or(false);
    if context.has_images || reasoning_high || has_complex_keywords || text_chars > 12_000 {
        return RoutingDecision {
            tier: "advanced".to_string(),
            confidence: 0.9,
            reason: "complex request signal".to_string(),
        };
    }
    if context.has_tools
        || context.has_response_format
        || has_code
        || context.message_count > 4
        || text_chars > 2_000
    {
        return RoutingDecision {
            tier: "standard".to_string(),
            confidence: 0.82,
            reason: "structured or medium complexity request".to_string(),
        };
    }
    RoutingDecision {
        tier: "simple".to_string(),
        confidence: 0.86,
        reason: "short plain text request".to_string(),
    }
}

fn choose_smart_candidate(
    candidates: &[ProjectModelCandidateRecord],
    tier: &str,
) -> Option<(Option<ProjectModelCandidateRecord>, String, String)> {
    let tier_candidates: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.enabled && candidate.tier == tier)
        .cloned()
        .collect();
    if tier_candidates.is_empty() {
        return None;
    }
    let highest_priority = tier_candidates
        .iter()
        .map(|candidate| candidate.priority)
        .max()?;
    let finalists: Vec<_> = tier_candidates
        .into_iter()
        .filter(|candidate| candidate.priority == highest_priority)
        .collect();
    let total_weight: i32 = finalists
        .iter()
        .map(|candidate| candidate.weight.max(1))
        .sum();
    let mut slot = rand::rng().random_range(0..total_weight);
    for candidate in finalists {
        slot -= candidate.weight.max(1);
        if slot < 0 {
            return Some((
                Some(candidate),
                "rules".to_string(),
                format!("selected {tier} candidate by priority and weight"),
            ));
        }
    }
    None
}

struct RoutingDecisionInsert<'a> {
    project_id: DbId,
    project_model_id: DbId,
    requested_model: &'a str,
    selected_model: &'a str,
    selected_channel_id: Option<DbId>,
    decision_source: &'a str,
    tier: &'a str,
    confidence: f64,
    reason: &'a str,
    latency_ms: i64,
}

async fn record_routing_decision(pool: &PgPool, item: RoutingDecisionInsert<'_>) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO routing_decision
            (project_id, project_model_id, requested_model, selected_model, selected_channel_id,
             decision_source, tier, confidence, reason, latency_ms)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind(item.project_id)
    .bind(item.project_model_id)
    .bind(item.requested_model)
    .bind(item.selected_model)
    .bind(item.selected_channel_id)
    .bind(item.decision_source)
    .bind(item.tier)
    .bind(item.confidence)
    .bind(item.reason)
    .bind(item.latency_ms)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn create_project_model(
    pool: &PgPool,
    project_id: DbId,
    req: UpsertProjectModelRequest,
) -> AppResult<ProjectModelRecord> {
    ensure_project_exists(pool, project_id).await?;
    let model = normalize_model(&req.model)?;
    let target_model = normalize_model(&req.target_model)?;
    let route_mode = normalize_route_mode(&req.route_mode)?;
    validate_routing_config(&req.routing_config)?;
    if route_mode == "smart" && model != req.routing_config.smart_model_name {
        return Err(AppError::BadRequest(format!(
            "smart route model must be {}",
            req.routing_config.smart_model_name
        )));
    }
    if route_mode == "smart" && req.candidates.is_empty() {
        return Err(AppError::BadRequest(
            "smart route requires at least one candidate".to_string(),
        ));
    }
    validate_target(pool, project_id, req.target_channel_id, &target_model).await?;
    for candidate in &req.candidates {
        validate_candidate(pool, project_id, candidate).await?;
    }
    let description = req.description.trim().to_string();
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"
        INSERT INTO project_model
            (project_id, model, target_model, target_channel_id, route_mode, routing_config, enabled, description)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(&model)
    .bind(&target_model)
    .bind(req.target_channel_id)
    .bind(&route_mode)
    .bind(sqlx::types::Json(&req.routing_config))
    .bind(req.enabled)
    .bind(description)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_project_model_write_error)?;
    let project_model_id: DbId = row.try_get("id")?;
    replace_project_model_candidates(&mut tx, project_model_id, &req.candidates).await?;
    tx.commit().await?;
    get_project_model(pool, project_id, project_model_id).await
}

fn map_project_model_write_error(err: sqlx::Error) -> AppError {
    if has_database_constraint(&err, "project_model_project_id_model_key") {
        return AppError::Conflict("同一项目下模型别名不能重复".to_string());
    }
    AppError::Sqlx(err)
}

fn has_database_constraint(err: &sqlx::Error, constraint: &str) -> bool {
    err.as_database_error()
        .and_then(|db_error| db_error.constraint())
        == Some(constraint)
}

pub async fn update_project_model(
    pool: &PgPool,
    project_id: DbId,
    current_model: &str,
    req: UpdateProjectModelRequest,
) -> AppResult<ProjectModelRecord> {
    let current_model = normalize_model(current_model)?;
    let existing = sqlx::query(
        "SELECT id, model, target_model, target_channel_id, route_mode, routing_config FROM project_model WHERE project_id = $1 AND model = $2",
    )
    .bind(project_id)
    .bind(&current_model)
    .fetch_optional(pool)
    .await?;
    let existing = existing.ok_or(AppError::NotFound)?;
    let id: DbId = existing.try_get("id")?;
    let model = req
        .model
        .as_deref()
        .map(normalize_model)
        .transpose()?
        .unwrap_or_else(|| existing.try_get("model").expect("model column"));
    let target_model = req
        .target_model
        .as_deref()
        .map(normalize_model)
        .transpose()?
        .unwrap_or_else(|| {
            existing
                .try_get("target_model")
                .expect("target_model column")
        });
    let target_channel_id = req.target_channel_id.unwrap_or_else(|| {
        existing
            .try_get("target_channel_id")
            .expect("target_channel_id column")
    });
    let route_mode: String = existing.try_get("route_mode")?;
    if req.candidates.is_some() && route_mode != "smart" {
        return Err(AppError::BadRequest(
            "only smart routes can update candidates".to_string(),
        ));
    }
    let routing_config = req.routing_config.unwrap_or(routing_config_from_value(
        existing.try_get("routing_config")?,
    )?);
    validate_routing_config(&routing_config)?;
    if let Some(candidates) = &req.candidates {
        if candidates.is_empty() {
            return Err(AppError::BadRequest(
                "smart route requires at least one candidate".to_string(),
            ));
        }
        for candidate in candidates {
            validate_candidate(pool, project_id, candidate).await?;
        }
    }
    validate_target(pool, project_id, target_channel_id, &target_model).await?;
    let mut tx = pool.begin().await?;
    sqlx::query(
        r#"
        UPDATE project_model
        SET model = $3,
            target_model = $4,
            target_channel_id = $5,
            enabled = COALESCE($6, enabled),
            description = COALESCE($7, description),
            routing_config = $8,
            updated_at = now()
        WHERE project_id = $1 AND id = $2
        "#,
    )
    .bind(project_id)
    .bind(id)
    .bind(model)
    .bind(target_model)
    .bind(target_channel_id)
    .bind(req.enabled)
    .bind(req.description.map(|value| value.trim().to_string()))
    .bind(sqlx::types::Json(&routing_config))
    .execute(&mut *tx)
    .await?;
    if let Some(candidates) = &req.candidates {
        replace_project_model_candidates(&mut tx, id, candidates).await?;
    }
    tx.commit().await?;
    get_project_model(pool, project_id, id).await
}

pub async fn delete_project_model(pool: &PgPool, project_id: DbId, model: &str) -> AppResult<()> {
    let model = normalize_model(model)?;
    let result = sqlx::query("DELETE FROM project_model WHERE project_id = $1 AND model = $2")
        .bind(project_id)
        .bind(model)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn get_project_model(
    pool: &PgPool,
    project_id: DbId,
    id: DbId,
) -> AppResult<ProjectModelRecord> {
    let row = sqlx::query(
        r#"
        SELECT pm.id, pm.project_id, pm.model, pm.target_model, pm.target_channel_id,
               c.name AS target_channel_name,
               pm.route_mode, pm.routing_config,
               pm.enabled, pm.description, pm.created_at, pm.updated_at
        FROM project_model pm
        LEFT JOIN channel c ON c.id = pm.target_channel_id
        WHERE pm.project_id = $1 AND pm.id = $2
        "#,
    )
    .bind(project_id)
    .bind(id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    project_model_from_row(pool, &row).await
}

async fn list_project_model_candidates(
    pool: &PgPool,
    project_model_id: DbId,
) -> AppResult<Vec<ProjectModelCandidateRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT pmc.id, pmc.project_model_id, pmc.target_model, pmc.target_channel_id,
               c.name AS target_channel_name,
               pmc.tier, pmc.priority, pmc.weight, pmc.enabled,
               pmc.created_at, pmc.updated_at
        FROM project_model_candidate pmc
        LEFT JOIN channel c ON c.id = pmc.target_channel_id
        WHERE pmc.project_model_id = $1
        ORDER BY pmc.tier ASC, pmc.priority DESC, pmc.id ASC
        "#,
    )
    .bind(project_model_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(project_model_candidate_from_row).collect()
}

async fn replace_project_model_candidates(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_model_id: DbId,
    candidates: &[UpsertProjectModelCandidateRequest],
) -> AppResult<()> {
    sqlx::query("DELETE FROM project_model_candidate WHERE project_model_id = $1")
        .bind(project_model_id)
        .execute(&mut **tx)
        .await?;
    for candidate in candidates {
        sqlx::query(
            r#"
            INSERT INTO project_model_candidate
                (project_model_id, target_model, target_channel_id, tier, priority, weight, enabled)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(project_model_id)
        .bind(candidate.target_model.trim())
        .bind(candidate.target_channel_id)
        .bind(candidate.tier.trim())
        .bind(candidate.priority)
        .bind(candidate.weight.max(1))
        .bind(candidate.enabled)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn normalize_model(value: &str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(AppError::BadRequest("model is required".to_string()));
    }
    if value.chars().count() > 160 {
        return Err(AppError::BadRequest(
            "model must be 160 characters or fewer".to_string(),
        ));
    }
    Ok(value.to_string())
}

fn normalize_route_mode(value: &str) -> AppResult<String> {
    match value.trim() {
        "direct" => Ok("direct".to_string()),
        "smart" => Ok("smart".to_string()),
        other => Err(AppError::BadRequest(format!("invalid route_mode: {other}"))),
    }
}

fn validate_tier(value: &str) -> AppResult<()> {
    match value.trim() {
        "simple" | "standard" | "advanced" => Ok(()),
        other => Err(AppError::BadRequest(format!("invalid tier: {other}"))),
    }
}

fn validate_routing_config(config: &ProjectModelRoutingConfig) -> AppResult<()> {
    validate_tier(&config.default_tier)?;
    if !(0.0..=1.0).contains(&config.low_confidence_threshold) {
        return Err(AppError::BadRequest(
            "low_confidence_threshold must be between 0 and 1".to_string(),
        ));
    }
    if config.classifier_enabled {
        let model = config
            .classifier_model
            .as_deref()
            .ok_or_else(|| AppError::BadRequest("classifier_model is required".to_string()))?;
        normalize_model(model)?;
    }
    Ok(())
}

async fn validate_candidate(
    pool: &PgPool,
    project_id: DbId,
    candidate: &UpsertProjectModelCandidateRequest,
) -> AppResult<()> {
    validate_tier(&candidate.tier)?;
    if candidate.weight < 1 {
        return Err(AppError::BadRequest(
            "candidate weight must be >= 1".to_string(),
        ));
    }
    let target_model = normalize_model(&candidate.target_model)?;
    validate_target(pool, project_id, candidate.target_channel_id, &target_model).await
}

async fn ensure_project_exists(pool: &PgPool, project_id: DbId) -> AppResult<()> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM project WHERE id = $1 AND deleted_at IS NULL)",
    )
    .bind(project_id)
    .fetch_one(pool)
    .await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    Ok(())
}

async fn validate_target(
    pool: &PgPool,
    project_id: DbId,
    target_channel_id: Option<DbId>,
    target_model: &str,
) -> AppResult<()> {
    if let Some(channel_id) = target_channel_id {
        let row = sqlx::query("SELECT provider FROM channel WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::BadRequest("target channel does not exist".to_string()))?;
        let provider: String = row.try_get("provider")?;
        let has_price = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM provider_price
                WHERE provider = $1 AND model = $2 AND enabled = TRUE
            )",
        )
        .bind(provider)
        .bind(target_model)
        .fetch_one(pool)
        .await?;
        if !has_price {
            return Err(AppError::BadRequest(
                "target channel model has no enabled price".to_string(),
            ));
        }
        return Ok(());
    }

    let has_any_price = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM provider_price
            WHERE model = $1 AND enabled = TRUE
        )",
    )
    .bind(target_model)
    .fetch_one(pool)
    .await?;
    if !has_any_price {
        return Err(AppError::BadRequest(format!(
            "price is not configured for model {target_model}"
        )));
    }
    let _ = project_id;
    Ok(())
}

async fn project_model_from_row(
    pool: &PgPool,
    row: &sqlx::postgres::PgRow,
) -> AppResult<ProjectModelRecord> {
    let id = row.try_get("id")?;
    Ok(ProjectModelRecord {
        id,
        project_id: row.try_get("project_id")?,
        model: row.try_get("model")?,
        target_model: row.try_get("target_model")?,
        target_channel_id: row.try_get("target_channel_id")?,
        target_channel_name: row.try_get("target_channel_name")?,
        route_mode: row.try_get("route_mode")?,
        routing_config: routing_config_from_value(row.try_get("routing_config")?)?,
        candidates: list_project_model_candidates(pool, id).await?,
        enabled: row.try_get("enabled")?,
        description: row.try_get("description")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn project_model_candidate_from_row(
    row: &sqlx::postgres::PgRow,
) -> AppResult<ProjectModelCandidateRecord> {
    Ok(ProjectModelCandidateRecord {
        id: row.try_get("id")?,
        project_model_id: row.try_get("project_model_id")?,
        target_model: row.try_get("target_model")?,
        target_channel_id: row.try_get("target_channel_id")?,
        target_channel_name: row.try_get("target_channel_name")?,
        tier: row.try_get("tier")?,
        priority: row.try_get("priority")?,
        weight: row.try_get("weight")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn routing_config_from_value(value: Value) -> AppResult<ProjectModelRoutingConfig> {
    if value.is_null() {
        return Ok(ProjectModelRoutingConfig::default());
    }
    serde_json::from_value(value).map_err(AppError::Json)
}

fn normalize_auto_configure_mode(value: &str) -> AppResult<String> {
    match value.trim() {
        "fill_missing" | "replace" | "keep" => Ok(value.trim().to_string()),
        other => Err(AppError::BadRequest(format!(
            "invalid auto configure mode: {other}"
        ))),
    }
}

async fn existing_smart_candidate_tiers(
    pool: &PgPool,
    project_id: DbId,
) -> AppResult<HashSet<String>> {
    let rows = sqlx::query(
        "SELECT DISTINCT pmc.tier
         FROM project_model_candidate pmc
         JOIN project_model pm ON pm.id = pmc.project_model_id
         WHERE pm.project_id = $1
           AND pm.route_mode = 'smart'
           AND pmc.enabled = TRUE",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| row.try_get::<String, _>("tier").ok())
        .collect())
}

async fn list_auto_configure_available_models(
    pool: &PgPool,
) -> AppResult<Vec<AutoConfigureAvailableModel>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT DISTINCT ON (cm.model, c.id)
               cm.model, c.provider, c.id AS channel_id, c.name AS channel_name, ce.protocol,
               pp.input_price_usd_micros, pp.output_price_usd_micros
        FROM channel_model cm
        JOIN channel c ON c.id = cm.channel_id
        JOIN provider p ON p.code = c.provider
        JOIN channel_endpoint ce ON ce.channel_id = c.id
            AND ce.enabled = TRUE
            AND ce.healthy = TRUE
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
        JOIN provider_price pp ON pp.provider = c.provider
                              AND pp.model = cm.model
                              AND pp.enabled = TRUE
                              AND pp.billing_meter = 'token'
                              AND {BILLABLE_PROVIDER_PRICE_CONDITION_PP}
        WHERE p.enabled = TRUE
          AND c.enabled = TRUE
          AND cm.enabled = TRUE
          AND cm.status = 'available'
          AND (
              cm.runtime_status = 'normal'
              OR (cm.runtime_status = 'cooldown' AND cm.cooldown_until <= now())
          )
          AND (
              (
                  c.use_credentials = FALSE
                  AND EXISTS (
                      SELECT 1 FROM channel_key ck
                      WHERE ck.channel_id = c.id
                        AND ck.enabled = TRUE
                        AND ck.healthy = TRUE
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
        ORDER BY cm.model ASC, c.id ASC, c.priority DESC,
                 CASE ce.protocol WHEN 'openai' THEN 0 WHEN 'openai_oauth' THEN 1 WHEN 'anthropic' THEN 2 ELSE 3 END
        "#,
    )))
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| {
            Ok(AutoConfigureAvailableModel {
                model: row.try_get("model")?,
                provider: row.try_get("provider")?,
                channel_id: row.try_get("channel_id")?,
                channel_name: row.try_get("channel_name")?,
                protocol: row
                    .try_get::<Option<String>, _>("protocol")?
                    .unwrap_or_else(|| "openai".to_string()),
                input_price_usd_micros: row.try_get("input_price_usd_micros")?,
                output_price_usd_micros: row.try_get("output_price_usd_micros")?,
            })
        })
        .collect()
}

async fn resolve_admin_text_model(
    state: &Arc<AppState>,
    model: &str,
    channel_id: Option<DbId>,
) -> AppResult<Option<(String, UpstreamProtocol, SelectedUpstream)>> {
    let rows = sqlx::query(
        r#"
        SELECT ce.protocol, c.id AS channel_id
        FROM channel c
        JOIN channel_endpoint ce ON ce.channel_id = c.id
        JOIN channel_model cm ON cm.channel_id = c.id
        WHERE cm.model = $1
          AND ($2::BIGINT IS NULL OR c.id = $2)
          AND ce.enabled = TRUE
          AND ce.healthy = TRUE
          AND ce.protocol IN ('openai', 'anthropic')
          AND (
              EXISTS (
                  SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                  WHERE btrim(endpoint_model.model) = cm.model
              )
              OR NOT EXISTS (
                  SELECT 1 FROM unnest(ce.models) AS endpoint_model(model)
                  WHERE btrim(endpoint_model.model) <> ''
              )
          )
        ORDER BY c.priority DESC,
                 CASE ce.protocol WHEN 'openai' THEN 0 WHEN 'anthropic' THEN 1 ELSE 2 END
        "#,
    )
    .bind(model)
    .bind(channel_id)
    .fetch_all(&state.db.pool)
    .await?;

    for row in rows {
        let protocol: String = row.try_get("protocol")?;
        let protocol = match protocol.as_str() {
            "openai" => UpstreamProtocol::Openai,
            "anthropic" => UpstreamProtocol::Anthropic,
            _ => continue,
        };
        let channel_id: DbId = row.try_get("channel_id")?;
        match state
            .selector
            .select_bound_channel_protocols(
                &state.db.pool,
                &state.secrets,
                &[protocol],
                model,
                channel_id,
                &[],
            )
            .await
        {
            Ok((_, upstream)) => return Ok(Some((model.to_string(), protocol, upstream))),
            Err(err) => {
                tracing::debug!(
                    model,
                    protocol = protocol.as_str(),
                    channel_id,
                    error = %err,
                    "configured admin text model is not currently callable"
                );
            }
        }
    }
    Ok(None)
}

async fn llm_auto_configure_suggestions(
    state: &Arc<AppState>,
    protocol: UpstreamProtocol,
    classifier_model: &str,
    upstream: &SelectedUpstream,
    available: &[AutoConfigureAvailableModel],
    existing_tiers: &HashSet<String>,
    mode: &str,
    max_candidates_per_tier: usize,
) -> AppResult<Vec<AutoSuggestion>> {
    let prompt = auto_configure_prompt(available, existing_tiers, mode, max_candidates_per_tier);
    let content = match protocol {
        UpstreamProtocol::Openai => {
            openai_auto_configure_content(state, classifier_model, upstream, prompt).await?
        }
        UpstreamProtocol::Anthropic => {
            anthropic_auto_configure_content(state, classifier_model, upstream, prompt).await?
        }
        UpstreamProtocol::OpenAiOauth => {
            return Err(AppError::BadRequest(
                "openai_oauth cannot be used as an auto configure classifier".to_string(),
            ))
        }
    };
    let parsed: LlmAutoConfigureResponse = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => serde_json::from_value(extract_json_object(&content)?)?,
    };
    validate_llm_auto_configure_response(parsed, available)
}

async fn openai_auto_configure_content(
    state: &Arc<AppState>,
    classifier_model: &str,
    upstream: &SelectedUpstream,
    prompt: String,
) -> AppResult<String> {
    let body = json!({
        "model": classifier_model,
        "temperature": 0,
        "response_format": { "type": "json_object" },
        "messages": [
            {
                "role": "system",
                "content": "You configure smart model routing. Return only valid JSON. Pick only models and channel IDs from the provided list."
            },
            {
                "role": "user",
                "content": prompt
            }
        ]
    });
    let response = state
        .http
        .post(upstream_url(&upstream.base_url, "/v1/chat/completions"))
        .bearer_auth(&upstream.secret)
        .header("content-type", "application/json")
        .body(Bytes::from(serde_json::to_vec(&body)?))
        .send()
        .await
        .map_err(|err| {
            AppError::UpstreamRequest(crate::error::UpstreamRequestError::from_reqwest(
                upstream.provider.clone(),
                &err,
            ))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = read_upstream_error_body(response).await;
        return Err(AppError::UpstreamUnavailable(format!(
            "auto configure model returned HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&body)
                .chars()
                .take(200)
                .collect::<String>()
        )));
    }
    let value: Value = response.json().await?;
    value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AppError::BadRequest("auto configure model response has no content".to_string())
        })
        .map(str::to_string)
}

async fn anthropic_auto_configure_content(
    state: &Arc<AppState>,
    classifier_model: &str,
    upstream: &SelectedUpstream,
    prompt: String,
) -> AppResult<String> {
    let body = json!({
        "model": classifier_model,
        "temperature": 0,
        "max_tokens": 1200,
        "system": "You configure smart model routing. Return only valid JSON. Pick only models and channel IDs from the provided list.",
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ]
    });
    let response = state
        .http
        .post(upstream_url(&upstream.base_url, "/v1/messages"))
        .header("x-api-key", &upstream.secret)
        .header("anthropic-version", DEFAULT_ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .body(Bytes::from(serde_json::to_vec(&body)?))
        .send()
        .await
        .map_err(|err| {
            AppError::UpstreamRequest(crate::error::UpstreamRequestError::from_reqwest(
                upstream.provider.clone(),
                &err,
            ))
        })?;
    if !response.status().is_success() {
        let status = response.status();
        let body = read_upstream_error_body(response).await;
        return Err(AppError::UpstreamUnavailable(format!(
            "auto configure model returned HTTP {}: {}",
            status.as_u16(),
            String::from_utf8_lossy(&body)
                .chars()
                .take(200)
                .collect::<String>()
        )));
    }
    let value: Value = response.json().await?;
    let content = value
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<String>()
        })
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| {
            AppError::BadRequest("auto configure model response has no content".to_string())
        })?;
    Ok(content)
}

fn auto_configure_prompt(
    available: &[AutoConfigureAvailableModel],
    existing_tiers: &HashSet<String>,
    mode: &str,
    max_candidates_per_tier: usize,
) -> String {
    let models = available
        .iter()
        .map(|item| {
            json!({
                "model": item.model,
                "provider": item.provider,
                "protocol": item.protocol,
                "channel_id": item.channel_id,
                "channel_name": item.channel_name,
                "input_price_usd_micros": item.input_price_usd_micros,
                "output_price_usd_micros": item.output_price_usd_micros
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "goal": "Recommend smart model routing candidates for simple, standard, and advanced tiers.",
        "rules": [
            "simple: low cost, fast, good for short common questions",
            "standard: balanced quality and cost, good for coding and structured output",
            "advanced: strongest reasoning, good for complex analysis, architecture, hard debugging, and math",
            "Use only provided model names and channel_id values.",
            "Prefer different models for different tiers when possible.",
            "Return at most one candidate per tier unless requested otherwise."
        ],
        "mode": mode,
        "existing_tiers": existing_tiers.iter().cloned().collect::<Vec<_>>(),
        "max_candidates_per_tier": max_candidates_per_tier,
        "available_models": models,
        "output_schema": {
            "simple": { "model": "string", "channel_id": "number_or_null", "reason": "string" },
            "standard": { "model": "string", "channel_id": "number_or_null", "reason": "string" },
            "advanced": { "model": "string", "channel_id": "number_or_null", "reason": "string" }
        }
    });
    payload.to_string()
}

fn extract_json_object(content: &str) -> AppResult<Value> {
    let start = content
        .find('{')
        .ok_or_else(|| AppError::BadRequest("auto configure output is not JSON".to_string()))?;
    let end = content
        .rfind('}')
        .ok_or_else(|| AppError::BadRequest("auto configure output is not JSON".to_string()))?;
    serde_json::from_str(&content[start..=end]).map_err(AppError::Json)
}

fn validate_llm_auto_configure_response(
    response: LlmAutoConfigureResponse,
    available: &[AutoConfigureAvailableModel],
) -> AppResult<Vec<AutoSuggestion>> {
    let mut suggestions = Vec::new();
    for (tier, item) in [
        ("simple", response.simple),
        ("standard", response.standard),
        ("advanced", response.advanced),
    ] {
        let Some(item) = item else {
            continue;
        };
        let model = normalize_model(&item.model)?;
        let available_item = find_available_auto_config_model(available, &model, item.channel_id)
            .ok_or_else(|| {
            AppError::BadRequest(format!(
                "auto configure suggested unavailable model {model}"
            ))
        })?;
        suggestions.push(AutoSuggestion {
            tier: tier.to_string(),
            target_model: available_item.model.clone(),
            target_channel_id: item.channel_id,
            target_channel_name: item
                .channel_id
                .and(Some(available_item.channel_name.clone())),
            reason: item.reason.unwrap_or_else(|| tier_default_reason(tier)),
        });
    }
    Ok(suggestions)
}

fn find_available_auto_config_model<'a>(
    available: &'a [AutoConfigureAvailableModel],
    model: &str,
    channel_id: Option<DbId>,
) -> Option<&'a AutoConfigureAvailableModel> {
    available.iter().find(|item| {
        item.model == model && channel_id.map(|id| id == item.channel_id).unwrap_or(true)
    })
}

fn filter_auto_configure_mode(
    suggestions: Vec<AutoSuggestion>,
    existing_tiers: &HashSet<String>,
    mode: &str,
) -> Vec<AutoSuggestion> {
    match mode {
        "fill_missing" => suggestions
            .into_iter()
            .filter(|item| !existing_tiers.contains(&item.tier))
            .collect(),
        "keep" => Vec::new(),
        _ => suggestions,
    }
}

fn tier_default_reason(tier: &str) -> String {
    match tier {
        "simple" => "适合简单问答和低成本请求".to_string(),
        "advanced" => "适合复杂推理、架构设计和疑难问题".to_string(),
        _ => "适合日常代码、结构化输出和中等复杂任务".to_string(),
    }
}
