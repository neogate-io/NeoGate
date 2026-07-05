use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Row};

use crate::{
    auth::UserAuth, billing::BILLABLE_PROVIDER_PRICE_CONDITION_PP, error::AppError,
    error::AppResult, AppState,
};

use super::selector::UpstreamProtocol;

#[derive(Debug, Deserialize)]
pub(super) struct ListModelsQuery {
    limit: Option<usize>,
    before_id: Option<String>,
    after_id: Option<String>,
}

#[derive(Debug)]
struct AvailableModel {
    id: String,
    owned_by: String,
}

#[derive(Debug, Serialize)]
pub(super) struct OpenAiModelList {
    object: &'static str,
    data: Vec<OpenAiModel>,
}

#[derive(Debug, Serialize)]
pub(super) struct OpenAiModel {
    id: String,
    object: &'static str,
    created: i64,
    owned_by: String,
}

#[derive(Debug, Serialize)]
pub(super) struct AnthropicModelList {
    data: Vec<AnthropicModel>,
    first_id: Option<String>,
    has_more: bool,
    last_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AnthropicModel {
    id: String,
    #[serde(rename = "type")]
    model_type: &'static str,
    display_name: String,
    created_at: &'static str,
}

pub(super) async fn list_openai_models(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
) -> AppResult<Json<OpenAiModelList>> {
    let models = available_openai_models(&state, &auth).await?;
    Ok(Json(OpenAiModelList {
        object: "list",
        data: models.into_iter().map(openai_model).collect(),
    }))
}

pub(super) async fn retrieve_openai_model(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Path(model_id): Path<String>,
) -> AppResult<Json<OpenAiModel>> {
    let models = available_openai_models(&state, &auth).await?;
    let model = models
        .into_iter()
        .find(|model| model.id == model_id)
        .ok_or(AppError::NotFound)?;

    Ok(Json(openai_model(model)))
}

async fn available_openai_models(
    state: &AppState,
    auth: &UserAuth,
) -> AppResult<Vec<AvailableModel>> {
    available_models(state, auth, None).await
}

fn openai_model(model: AvailableModel) -> OpenAiModel {
    OpenAiModel {
        id: model.id,
        object: "model",
        created: 0,
        owned_by: model.owned_by,
    }
}

pub(super) async fn list_anthropic_models(
    State(state): State<Arc<AppState>>,
    auth: UserAuth,
    Query(query): Query<ListModelsQuery>,
) -> AppResult<Json<AnthropicModelList>> {
    let models = available_models(&state, &auth, None).await?;
    Ok(Json(anthropic_model_list(models, query)))
}

async fn available_models(
    state: &AppState,
    auth: &UserAuth,
    protocols: Option<&[UpstreamProtocol]>,
) -> AppResult<Vec<AvailableModel>> {
    let project_models =
        crate::project::models::list_project_models(&state.db.pool, auth.project_id).await?;
    if !project_models.is_empty() {
        return Ok(project_models
            .into_iter()
            .filter(|model| model.enabled)
            .map(|model| AvailableModel {
                id: model.model,
                owned_by: "project".to_string(),
            })
            .collect());
    }

    let protocols = protocols.map(|items| {
        items
            .iter()
            .map(|protocol| protocol.as_str().to_string())
            .collect::<Vec<_>>()
    });
    let rows = sqlx::query(AssertSqlSafe(format!(
        r#"
        SELECT model, MIN(provider) AS owned_by
        FROM (
            SELECT
                c.provider,
                cm.model,
                pp.billing_meter,
                pp.unit_price_micros,
                pp.input_price_micros,
                pp.output_price_micros
            FROM channel c
            JOIN provider p ON p.code = c.provider
            JOIN channel_endpoint ce ON ce.channel_id = c.id
            JOIN channel_model cm ON cm.channel_id = c.id
            JOIN provider_price pp
             ON pp.provider = c.provider
             AND pp.model = cm.model
             AND pp.enabled = TRUE
             AND {BILLABLE_PROVIDER_PRICE_CONDITION_PP}
            WHERE p.enabled = TRUE
              AND c.enabled = TRUE
              AND ($1::TEXT[] IS NULL OR ce.protocol = ANY($1))
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
                  (
                      c.use_credentials = FALSE
                      AND EXISTS (
                          SELECT 1
                          FROM channel_key ck
                          WHERE ck.channel_id = c.id
                            AND ck.enabled = TRUE
                            AND ck.healthy = TRUE
                            AND (ck.cooldown_until IS NULL OR ck.cooldown_until <= now())
                      )
                  )
                  OR (
                      c.use_credentials = TRUE
                      AND EXISTS (
                          SELECT 1
                          FROM credential cr
                          WHERE cr.provider = c.provider
                            AND cr.enabled = TRUE
                      )
                  )
              )
        ) available
        GROUP BY model
        ORDER BY MAX(COALESCE(unit_price_micros, output_price_micros)) DESC,
                 MAX(output_price_micros) DESC,
                 MAX(input_price_micros) DESC,
                 model ASC
        "#
    )))
    .bind(protocols.as_deref())
    .fetch_all(&state.db.pool)
    .await?;

    rows.iter()
        .map(|row| {
            Ok(AvailableModel {
                id: row.try_get("model")?,
                owned_by: row.try_get("owned_by")?,
            })
        })
        .collect()
}

fn anthropic_model_list(models: Vec<AvailableModel>, query: ListModelsQuery) -> AnthropicModelList {
    let limit = query.limit.unwrap_or(20).clamp(1, 1000);
    let mut start = 0;

    if let Some(after_id) = query.after_id.as_deref() {
        if let Some(index) = models.iter().position(|model| model.id == after_id) {
            start = index + 1;
        }
    }

    if let Some(before_id) = query.before_id.as_deref() {
        if let Some(index) = models.iter().position(|model| model.id == before_id) {
            start = index.saturating_sub(limit);
        }
    }

    let end = (start + limit).min(models.len());
    let has_more = end < models.len();
    let data: Vec<_> = models[start..end]
        .iter()
        .map(|model| AnthropicModel {
            id: model.id.clone(),
            model_type: "model",
            display_name: display_model_name(&model.id),
            created_at: "1970-01-01T00:00:00Z",
        })
        .collect();
    let first_id = data.first().map(|model| model.id.clone());
    let last_id = data.last().map(|model| model.id.clone());

    AnthropicModelList {
        data,
        first_id,
        has_more,
        last_id,
    }
}

fn display_model_name(id: &str) -> String {
    id.split(['/', ':'])
        .next_back()
        .unwrap_or(id)
        .replace(['-', '_'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anthropic_model_list_paginates_after_id() {
        let models = vec![
            AvailableModel {
                id: "claude-a".to_string(),
                owned_by: "anthropic".to_string(),
            },
            AvailableModel {
                id: "claude-b".to_string(),
                owned_by: "anthropic".to_string(),
            },
            AvailableModel {
                id: "claude-c".to_string(),
                owned_by: "anthropic".to_string(),
            },
        ];
        let response = anthropic_model_list(
            models,
            ListModelsQuery {
                limit: Some(1),
                before_id: None,
                after_id: Some("claude-a".to_string()),
            },
        );

        assert_eq!(response.data[0].id, "claude-b");
        assert_eq!(response.first_id.as_deref(), Some("claude-b"));
        assert_eq!(response.last_id.as_deref(), Some("claude-b"));
        assert!(response.has_more);
    }

    #[test]
    fn display_model_name_uses_leaf_model_id() {
        assert_eq!(
            display_model_name("provider/claude-sonnet"),
            "claude sonnet"
        );
    }
}
