use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Instant,
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::{
    admin::setting::resolve_default_text_model,
    billing::BILLABLE_PRICE_CONDITION_CP,
    config::DEFAULT_ANTHROPIC_VERSION,
    error::{AppError, AppResult},
    id::DbId,
    relay::{
        read_upstream_error_body,
        selector::{SelectedUpstream, SelectionConstraints, UpstreamProtocol},
        upstream_url,
    },
    AppState,
};

const ROUTING_LABEL_LIMIT: usize = 80;
const ROUTING_MATCHED_RULE_LIMIT: usize = 5;
const ROUTING_CANDIDATE_SUMMARY_LIMIT: usize = 3;

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
    pub routing: Option<UsageRoutingSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRoutingMatchedRule {
    pub id: String,
    pub category: String,
    pub weight: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRoutingCandidateScore {
    pub candidate_id: DbId,
    pub target_model: String,
    pub tier: String,
    pub priority: i32,
    pub weight: i32,
    pub score: i32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRoutingCandidateSummary {
    pub target_model: String,
    pub tier: String,
    pub priority: i32,
    pub weight: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRoutingSnapshot {
    pub project_id: DbId,
    pub project_model_id: DbId,
    pub requested_model: String,
    pub selected_model: String,
    pub selected_channel_id: Option<DbId>,
    pub decision_source: String,
    pub tier: String,
    pub task_type: String,
    pub confidence: f64,
    pub reason_code: String,
    pub matched_rule_ids: Vec<String>,
    pub candidate_summary: Vec<UsageRoutingCandidateSummary>,
    pub fallback_reason: Option<String>,
    pub classifier_model: Option<String>,
    pub latency_ms: i64,
}

impl UsageRoutingSnapshot {
    fn compact(mut self) -> Self {
        self.requested_model = truncate_chars(self.requested_model, ROUTING_LABEL_LIMIT);
        self.selected_model = truncate_chars(self.selected_model, ROUTING_LABEL_LIMIT);
        self.decision_source = truncate_chars(self.decision_source, ROUTING_LABEL_LIMIT);
        self.tier = truncate_chars(self.tier, ROUTING_LABEL_LIMIT);
        self.task_type = truncate_chars(self.task_type, ROUTING_LABEL_LIMIT);
        self.fallback_reason = self
            .fallback_reason
            .map(|value| truncate_chars(value, ROUTING_LABEL_LIMIT));
        self.classifier_model = self
            .classifier_model
            .map(|value| truncate_chars(value, ROUTING_LABEL_LIMIT));
        self.reason_code = truncate_chars(self.reason_code, ROUTING_LABEL_LIMIT);
        self.matched_rule_ids = self
            .matched_rule_ids
            .into_iter()
            .take(ROUTING_MATCHED_RULE_LIMIT)
            .map(|id| truncate_chars(id, ROUTING_LABEL_LIMIT))
            .collect();
        self.candidate_summary = self
            .candidate_summary
            .into_iter()
            .take(ROUTING_CANDIDATE_SUMMARY_LIMIT)
            .map(|candidate| UsageRoutingCandidateSummary {
                target_model: truncate_chars(candidate.target_model, ROUTING_LABEL_LIMIT),
                tier: truncate_chars(candidate.tier, ROUTING_LABEL_LIMIT),
                priority: candidate.priority,
                weight: candidate.weight,
            })
            .collect();
        self
    }
}

fn truncate_chars(value: String, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value;
    }
    value.chars().take(limit).collect()
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
    input_price_micros: Option<i64>,
    output_price_micros: Option<i64>,
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

    let ids = rows
        .iter()
        .map(|row| row.try_get("id"))
        .collect::<Result<Vec<DbId>, sqlx::Error>>()?;
    let mut candidates = list_project_model_candidates_for_models(pool, &ids).await?;
    rows.iter()
        .map(|row| {
            let id = row.try_get("id")?;
            project_model_from_row(row, candidates.remove(&id).unwrap_or_default())
        })
        .collect()
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
    let params = LlmAutoConfigureParams {
        protocol: classifier_protocol,
        classifier_model: &classifier_model,
        upstream: &upstream,
        available: &available,
        existing_tiers: &existing_tiers,
        mode: &mode,
        max_candidates_per_tier,
    };
    let mut suggestions = match llm_auto_configure_suggestions(state, params).await {
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
        let mut context = Self {
            has_tools: value
                .get("tools")
                .and_then(Value::as_array)
                .map(|items| !items.is_empty())
                .unwrap_or(false),
            has_response_format: value.get("response_format").is_some()
                || value
                    .get("text")
                    .and_then(|text| text.get("format"))
                    .is_some(),
            reasoning_effort: value
                .get("reasoning_effort")
                .or_else(|| {
                    value
                        .get("reasoning")
                        .and_then(|reasoning| reasoning.get("effort"))
                })
                .and_then(Value::as_str)
                .map(str::to_string),
            ..Default::default()
        };
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
        let model_name: String = row.try_get("model")?;
        if !enabled {
            return Err(AppError::Forbidden(format!(
                "model '{model_name}' is disabled for this project"
            )));
        }
        let route_mode: String = row.try_get("route_mode")?;
        if route_mode == "smart" {
            return resolve_smart_project_model(pool, project_id, &row, context).await;
        }
        return Ok(ResolvedProjectModel {
            external_model: row.try_get("model")?,
            target_model: row.try_get("target_model")?,
            target_channel_id: row.try_get("target_channel_id")?,
            routing: None,
        });
    }

    if project_has_models(pool, project_id).await? {
        return Err(AppError::Forbidden(format!(
            "model '{requested_model}' is not in the project's model allowlist"
        )));
    }

    Ok(ResolvedProjectModel {
        external_model: requested_model.clone(),
        target_model: requested_model,
        target_channel_id: None,
        routing: None,
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
    let mut decision = classify_request(context.as_ref(), &config);
    let candidates = list_project_model_candidates(pool, project_model_id).await?;
    let selection = choose_smart_candidate(&candidates, &decision.tier);
    let (target_model, target_channel_id, reason_code) = if let Some(selection) = selection {
        decision.candidate_scores = selection.scores;
        (
            selection.candidate.target_model,
            selection.candidate.target_channel_id,
            selection.reason_code,
        )
    } else {
        decision.decision_source = "fallback".to_string();
        decision.fallback_reason = Some(format!("no_enabled_candidate_for_{}", decision.tier));
        (fallback_model, fallback_channel_id, "fallback_no_candidate")
    };
    validate_target(pool, project_id, target_channel_id, &target_model).await?;
    let routing_reason_code = if reason_code.is_empty() {
        decision.reason_code.clone()
    } else {
        reason_code.to_string()
    };
    let routing = UsageRoutingSnapshot {
        project_id,
        project_model_id,
        requested_model: requested_model.clone(),
        selected_model: target_model.clone(),
        selected_channel_id: target_channel_id,
        decision_source: decision.decision_source,
        tier: decision.tier,
        task_type: decision.task_type,
        confidence: decision.confidence,
        reason_code: routing_reason_code,
        matched_rule_ids: decision
            .matched_rules
            .into_iter()
            .map(|rule| rule.id)
            .collect(),
        candidate_summary: decision
            .candidate_scores
            .into_iter()
            .map(|score| UsageRoutingCandidateSummary {
                target_model: score.target_model,
                tier: score.tier,
                priority: score.priority,
                weight: score.weight,
            })
            .collect(),
        fallback_reason: decision.fallback_reason,
        classifier_model: config.classifier_model,
        latency_ms: started.elapsed().as_millis().min(i64::MAX as u128) as i64,
    }
    .compact();
    Ok(ResolvedProjectModel {
        external_model: requested_model,
        target_model,
        target_channel_id,
        routing: Some(routing),
    })
}

struct RoutingDecision {
    tier: String,
    task_type: String,
    confidence: f64,
    reason_code: String,
    matched_rules: Vec<UsageRoutingMatchedRule>,
    candidate_scores: Vec<UsageRoutingCandidateScore>,
    decision_source: String,
    fallback_reason: Option<String>,
}

struct SmartCandidateSelection {
    candidate: ProjectModelCandidateRecord,
    reason_code: &'static str,
    scores: Vec<UsageRoutingCandidateScore>,
}

fn routing_rule(id: &str, category: &str, weight: i32, reason: &str) -> UsageRoutingMatchedRule {
    UsageRoutingMatchedRule {
        id: id.to_string(),
        category: category.to_string(),
        weight,
        reason: reason.to_string(),
    }
}

fn contains_any(text: &str, keywords: &[&str]) -> bool {
    keywords.iter().any(|keyword| text.contains(keyword))
}

fn classify_request(
    context: Option<&ProjectModelRequestContext>,
    config: &ProjectModelRoutingConfig,
) -> RoutingDecision {
    let Some(context) = context else {
        return RoutingDecision {
            tier: config.default_tier.clone(),
            task_type: "unknown".to_string(),
            confidence: 0.4,
            reason_code: "missing_context".to_string(),
            matched_rules: vec![routing_rule(
                "missing_context",
                "fallback",
                1,
                "request body was not available for smart routing",
            )],
            candidate_scores: Vec::new(),
            decision_source: "rules".to_string(),
            fallback_reason: Some("missing_request_context".to_string()),
        };
    };

    let text = context.text.to_ascii_lowercase();
    let text_chars = context.text.chars().count();
    let mut matched_rules = Vec::new();

    let has_code = text.contains("```")
        || contains_any(
            &text,
            &[
                " traceback ",
                " exception",
                "function ",
                "class ",
                "select ",
                "insert ",
                "update ",
                "delete from",
                "代码",
                "调试",
                "debug",
                "rust",
                "python",
                "typescript",
            ],
        );
    let has_reasoning_keywords = contains_any(
        &text,
        &[
            "推理",
            "证明",
            "架构",
            "复杂",
            "优化",
            "数学",
            "算法",
            "设计方案",
            "reason",
            "architecture",
            "analyze",
        ],
    );
    let has_translation = contains_any(&text, &["翻译", "translate", "translation"]);
    let has_summarization = contains_any(&text, &["总结", "摘要", "summarize", "summary"]);
    let has_extraction = contains_any(&text, &["提取", "抽取", "extract", "json"]);
    let reasoning_high = context
        .reasoning_effort
        .as_deref()
        .map(|value| value == "high" || value == "medium")
        .unwrap_or(false);

    if context.has_images {
        matched_rules.push(routing_rule(
            "has_images",
            "modality",
            5,
            "request contains image input",
        ));
    }
    if reasoning_high {
        matched_rules.push(routing_rule(
            "reasoning_effort",
            "reasoning",
            5,
            "request asks for medium or high reasoning effort",
        ));
    }
    if has_reasoning_keywords {
        matched_rules.push(routing_rule(
            "reasoning_keywords",
            "reasoning",
            4,
            "request contains reasoning, architecture, math, or analysis keywords",
        ));
    }
    if text_chars > 12_000 {
        matched_rules.push(routing_rule(
            "very_long_context",
            "length",
            5,
            "request text is longer than 12000 characters",
        ));
    } else if text_chars > 2_000 {
        matched_rules.push(routing_rule(
            "long_context",
            "length",
            3,
            "request text is longer than 2000 characters",
        ));
    }
    if context.has_tools {
        matched_rules.push(routing_rule(
            "has_tools",
            "tool_use",
            4,
            "request includes tool definitions",
        ));
    }
    if context.has_response_format {
        matched_rules.push(routing_rule(
            "has_response_format",
            "structured_output",
            3,
            "request asks for a structured response format",
        ));
    }
    if has_code {
        matched_rules.push(routing_rule(
            "code_signal",
            "code",
            3,
            "request contains code, SQL, stack trace, or debugging signals",
        ));
    }
    if context.message_count > 4 {
        matched_rules.push(routing_rule(
            "multi_turn_context",
            "conversation",
            2,
            "request contains more than four messages",
        ));
    }
    if has_translation {
        matched_rules.push(routing_rule(
            "translation_signal",
            "language",
            2,
            "request looks like a translation task",
        ));
    }
    if has_summarization {
        matched_rules.push(routing_rule(
            "summarization_signal",
            "summarization",
            2,
            "request looks like a summarization task",
        ));
    }
    if has_extraction {
        matched_rules.push(routing_rule(
            "extraction_signal",
            "extraction",
            2,
            "request looks like an extraction task",
        ));
    }

    let task_type = if context.has_images {
        "vision"
    } else if context.has_tools {
        "tool_use"
    } else if context.has_response_format {
        "structured_output"
    } else if reasoning_high || has_reasoning_keywords {
        "reasoning"
    } else if has_code {
        "code"
    } else if has_translation {
        "translation"
    } else if has_summarization {
        "summarization"
    } else if has_extraction {
        "extraction"
    } else if text_chars > 2_000 {
        "long_context"
    } else {
        "chat"
    }
    .to_string();

    let (tier, confidence, reason_code) =
        if context.has_images || reasoning_high || has_reasoning_keywords || text_chars > 12_000 {
            ("advanced".to_string(), 0.9, "complex_signal".to_string())
        } else if context.has_tools
            || context.has_response_format
            || has_code
            || context.message_count > 4
            || text_chars > 2_000
        {
            ("standard".to_string(), 0.82, "medium_signal".to_string())
        } else {
            if matched_rules.is_empty() {
                matched_rules.push(routing_rule(
                    "short_plain_text",
                    "chat",
                    1,
                    "short request without advanced routing signals",
                ));
            }
            ("simple".to_string(), 0.86, "simple_signal".to_string())
        };

    RoutingDecision {
        tier,
        task_type,
        confidence,
        reason_code,
        matched_rules,
        candidate_scores: Vec::new(),
        decision_source: "rules".to_string(),
        fallback_reason: None,
    }
}

fn choose_smart_candidate(
    candidates: &[ProjectModelCandidateRecord],
    tier: &str,
) -> Option<SmartCandidateSelection> {
    let tier_candidates: Vec<_> = candidates
        .iter()
        .filter(|candidate| candidate.enabled && candidate.tier == tier)
        .cloned()
        .collect();
    if tier_candidates.is_empty() {
        return None;
    }
    let scores: Vec<_> = tier_candidates
        .iter()
        .map(|candidate| UsageRoutingCandidateScore {
            candidate_id: candidate.id,
            target_model: candidate.target_model.clone(),
            tier: candidate.tier.clone(),
            priority: candidate.priority,
            weight: candidate.weight,
            score: candidate.priority.saturating_mul(100) + candidate.weight.max(1),
            reason: format!(
                "enabled {tier} candidate scored by priority {} and weight {}",
                candidate.priority,
                candidate.weight.max(1)
            ),
        })
        .collect();
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
            return Some(SmartCandidateSelection {
                candidate,
                reason_code: "selected_priority_weight",
                scores,
            });
        }
    }
    None
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
    let candidates = list_project_model_candidates(pool, id).await?;
    project_model_from_row(&row, candidates)
}

async fn list_project_model_candidates(
    pool: &PgPool,
    project_model_id: DbId,
) -> AppResult<Vec<ProjectModelCandidateRecord>> {
    Ok(
        list_project_model_candidates_for_models(pool, &[project_model_id])
            .await?
            .remove(&project_model_id)
            .unwrap_or_default(),
    )
}

async fn list_project_model_candidates_for_models(
    pool: &PgPool,
    project_model_ids: &[DbId],
) -> AppResult<HashMap<DbId, Vec<ProjectModelCandidateRecord>>> {
    if project_model_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        SELECT pmc.id, pmc.project_model_id, pmc.target_model, pmc.target_channel_id,
               c.name AS target_channel_name,
               pmc.tier, pmc.priority, pmc.weight, pmc.enabled,
               pmc.created_at, pmc.updated_at
        FROM project_model_candidate pmc
        LEFT JOIN channel c ON c.id = pmc.target_channel_id
        WHERE pmc.project_model_id = ANY($1)
        ORDER BY pmc.project_model_id ASC, pmc.tier ASC, pmc.priority DESC, pmc.id ASC
        "#,
    )
    .bind(project_model_ids)
    .fetch_all(pool)
    .await?;
    let mut grouped = HashMap::new();
    for row in &rows {
        let candidate = project_model_candidate_from_row(row)?;
        grouped
            .entry(candidate.project_model_id)
            .or_insert_with(Vec::new)
            .push(candidate);
    }
    Ok(grouped)
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
        let row = sqlx::query("SELECT id FROM channel WHERE id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::BadRequest("target channel does not exist".to_string()))?;
        let channel_id: DbId = row.try_get("id")?;
        let has_price = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS (
                SELECT 1 FROM channel_price
                WHERE channel_id = $1 AND model = $2 AND enabled = TRUE
            )",
        )
        .bind(channel_id)
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
            SELECT 1 FROM channel_price
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

fn project_model_from_row(
    row: &sqlx::postgres::PgRow,
    candidates: Vec<ProjectModelCandidateRecord>,
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
        candidates,
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
               cp.input_price_micros, cp.output_price_micros
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
        JOIN channel_price cp ON cp.channel_id = c.id
                              AND cp.model = cm.model
                              AND cp.enabled = TRUE
                              AND cp.billing_meter = 'token'
                              AND {BILLABLE_PRICE_CONDITION_CP}
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
                input_price_micros: row.try_get("input_price_micros")?,
                output_price_micros: row.try_get("output_price_micros")?,
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
                SelectionConstraints::default(),
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

struct LlmAutoConfigureParams<'a> {
    protocol: UpstreamProtocol,
    classifier_model: &'a str,
    upstream: &'a SelectedUpstream,
    available: &'a [AutoConfigureAvailableModel],
    existing_tiers: &'a HashSet<String>,
    mode: &'a str,
    max_candidates_per_tier: usize,
}

async fn llm_auto_configure_suggestions(
    state: &Arc<AppState>,
    params: LlmAutoConfigureParams<'_>,
) -> AppResult<Vec<AutoSuggestion>> {
    let prompt = auto_configure_prompt(
        params.available,
        params.existing_tiers,
        params.mode,
        params.max_candidates_per_tier,
    );
    let content = match params.protocol {
        UpstreamProtocol::Openai => {
            openai_auto_configure_content(state, params.classifier_model, params.upstream, prompt)
                .await?
        }
        UpstreamProtocol::Anthropic => {
            anthropic_auto_configure_content(
                state,
                params.classifier_model,
                params.upstream,
                prompt,
            )
            .await?
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
    validate_llm_auto_configure_response(parsed, params.available)
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
                "input_price_micros": item.input_price_micros,
                "output_price_micros": item.output_price_micros
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

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(context: ProjectModelRequestContext) -> RoutingDecision {
        classify_request(Some(&context), &ProjectModelRoutingConfig::default())
    }

    #[test]
    fn usage_routing_snapshot_is_compacted() {
        let long = "x".repeat(400);
        let snapshot = UsageRoutingSnapshot {
            project_id: 1,
            project_model_id: 2,
            requested_model: long.clone(),
            selected_model: long.clone(),
            selected_channel_id: Some(3),
            decision_source: long.clone(),
            tier: "advanced".to_string(),
            task_type: long.clone(),
            confidence: 0.9,
            reason_code: long.clone(),
            matched_rule_ids: (0..10)
                .map(|index| format!("rule-{index}-{long}"))
                .collect(),
            candidate_summary: (0..12)
                .map(|index| UsageRoutingCandidateSummary {
                    target_model: long.clone(),
                    tier: "advanced".to_string(),
                    priority: index,
                    weight: 1,
                })
                .collect(),
            fallback_reason: Some(long.clone()),
            classifier_model: Some(long),
            latency_ms: 12,
        }
        .compact();

        assert_eq!(
            snapshot.requested_model.chars().count(),
            ROUTING_LABEL_LIMIT
        );
        assert_eq!(snapshot.reason_code.chars().count(), ROUTING_LABEL_LIMIT);
        assert_eq!(snapshot.matched_rule_ids.len(), ROUTING_MATCHED_RULE_LIMIT);
        assert_eq!(
            snapshot.candidate_summary.len(),
            ROUTING_CANDIDATE_SUMMARY_LIMIT
        );
        assert_eq!(
            snapshot.matched_rule_ids[0].chars().count(),
            ROUTING_LABEL_LIMIT
        );
        assert_eq!(
            snapshot.candidate_summary[0].target_model.chars().count(),
            ROUTING_LABEL_LIMIT
        );
    }

    #[test]
    fn classify_short_plain_text_as_simple_chat() {
        let decision = classify(ProjectModelRequestContext {
            text: "你好，介绍一下你自己".to_string(),
            message_count: 1,
            ..Default::default()
        });

        assert_eq!(decision.tier, "simple");
        assert_eq!(decision.task_type, "chat");
        assert!(decision
            .matched_rules
            .iter()
            .any(|rule| rule.id == "short_plain_text"));
    }

    #[test]
    fn classify_tools_as_standard_tool_use() {
        let decision = classify(ProjectModelRequestContext {
            text: "请调用工具查询天气".to_string(),
            message_count: 1,
            has_tools: true,
            ..Default::default()
        });

        assert_eq!(decision.tier, "standard");
        assert_eq!(decision.task_type, "tool_use");
        assert!(decision
            .matched_rules
            .iter()
            .any(|rule| rule.id == "has_tools"));
    }

    #[test]
    fn classify_response_format_as_structured_output() {
        let decision = classify(ProjectModelRequestContext {
            text: "请返回 JSON".to_string(),
            message_count: 1,
            has_response_format: true,
            ..Default::default()
        });

        assert_eq!(decision.tier, "standard");
        assert_eq!(decision.task_type, "structured_output");
    }

    #[test]
    fn classify_images_as_advanced_vision() {
        let decision = classify(ProjectModelRequestContext {
            text: "分析这张图片".to_string(),
            message_count: 1,
            has_images: true,
            ..Default::default()
        });

        assert_eq!(decision.tier, "advanced");
        assert_eq!(decision.task_type, "vision");
    }

    #[test]
    fn classify_reasoning_effort_as_advanced_reasoning() {
        let decision = classify(ProjectModelRequestContext {
            text: "请严谨证明这个结论".to_string(),
            message_count: 1,
            reasoning_effort: Some("high".to_string()),
            ..Default::default()
        });

        assert_eq!(decision.tier, "advanced");
        assert_eq!(decision.task_type, "reasoning");
    }

    #[test]
    fn classify_code_signal_as_standard_code() {
        let decision = classify(ProjectModelRequestContext {
            text: "```rust\nfn main() {}\n```\n这个 bug 怎么修？".to_string(),
            message_count: 1,
            ..Default::default()
        });

        assert_eq!(decision.tier, "standard");
        assert_eq!(decision.task_type, "code");
    }

    #[test]
    fn classify_missing_context_uses_default_tier() {
        let decision = classify_request(None, &ProjectModelRoutingConfig::default());

        assert_eq!(decision.tier, "standard");
        assert_eq!(decision.task_type, "unknown");
        assert_eq!(
            decision.fallback_reason.as_deref(),
            Some("missing_request_context")
        );
    }
}
