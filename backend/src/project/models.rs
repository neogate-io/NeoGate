use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

#[derive(Debug, Clone, Serialize)]
pub struct ProjectModelRecord {
    pub id: DbId,
    pub project_id: DbId,
    pub model: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
    pub target_channel_name: Option<String>,
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

#[derive(Debug, Deserialize)]
pub struct UpsertProjectModelRequest {
    pub model: String,
    pub target_model: String,
    pub target_channel_id: Option<DbId>,
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
    pub enabled: Option<bool>,
    pub description: Option<String>,
}

fn default_enabled() -> bool {
    true
}

pub async fn list_project_models(
    pool: &PgPool,
    project_id: DbId,
) -> AppResult<Vec<ProjectModelRecord>> {
    let rows = sqlx::query(
        r#"
        SELECT pm.id, pm.project_id, pm.model, pm.target_model, pm.target_channel_id,
               c.name AS target_channel_name,
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

    rows.iter().map(project_model_from_row).collect()
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

pub async fn resolve_project_model(
    pool: &PgPool,
    project_id: DbId,
    requested_model: &str,
) -> AppResult<ResolvedProjectModel> {
    let requested_model = normalize_model(requested_model)?;
    let row = sqlx::query(
        r#"
        SELECT model, target_model, target_channel_id, enabled,
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

pub async fn create_project_model(
    pool: &PgPool,
    project_id: DbId,
    req: UpsertProjectModelRequest,
) -> AppResult<ProjectModelRecord> {
    ensure_project_exists(pool, project_id).await?;
    let model = normalize_model(&req.model)?;
    let target_model = normalize_model(&req.target_model)?;
    validate_target(pool, project_id, req.target_channel_id, &target_model).await?;
    let description = req.description.trim().to_string();
    let row = sqlx::query(
        r#"
        INSERT INTO project_model
            (project_id, model, target_model, target_channel_id, enabled, description)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
    )
    .bind(project_id)
    .bind(model)
    .bind(target_model)
    .bind(req.target_channel_id)
    .bind(req.enabled)
    .bind(description)
    .fetch_one(pool)
    .await
    .map_err(map_project_model_write_error)?;
    get_project_model(pool, project_id, row.try_get("id")?).await
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
        "SELECT id, model, target_model, target_channel_id FROM project_model WHERE project_id = $1 AND model = $2",
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
    validate_target(pool, project_id, target_channel_id, &target_model).await?;
    sqlx::query(
        r#"
        UPDATE project_model
        SET model = $3,
            target_model = $4,
            target_channel_id = $5,
            enabled = COALESCE($6, enabled),
            description = COALESCE($7, description),
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
    .execute(pool)
    .await?;
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
    project_model_from_row(&row)
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

fn project_model_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProjectModelRecord> {
    Ok(ProjectModelRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        model: row.try_get("model")?,
        target_model: row.try_get("target_model")?,
        target_channel_id: row.try_get("target_channel_id")?,
        target_channel_name: row.try_get("target_channel_name")?,
        enabled: row.try_get("enabled")?,
        description: row.try_get("description")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
