use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{QueryBuilder, Row};

use crate::{
    billing::{account, CreditAccountType},
    error::{AppError, AppResult},
    id::DbId,
    AppState,
};

#[derive(Debug, Serialize)]
pub struct ProjectRecord {
    pub id: DbId,
    pub name: String,
    pub owner_user_id: DbId,
    pub owner_email: String,
    pub status: String,
    pub is_default: bool,
    pub member_count: i64,
    pub user_key_count: i64,
    pub balance_micro_usd: i64,
    pub reserved_micro_usd: i64,
    pub available_micro_usd: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ProjectMemberRecord {
    pub id: DbId,
    pub project_id: DbId,
    pub user_id: DbId,
    pub user_email: String,
    pub role: String,
    pub user_status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct ProjectPage {
    pub items: Vec<ProjectRecord>,
    pub limit: i64,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct ListProjectsQuery {
    pub search: Option<String>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub owner_user_id: DbId,
    #[serde(default = "default_enabled_status")]
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
    pub status: Option<String>,
}

fn default_enabled_status() -> String {
    "enabled".to_string()
}

pub async fn list_projects(state: &AppState, query: ListProjectsQuery) -> AppResult<ProjectPage> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let cursor = parse_created_id_cursor(query.cursor.as_deref())?;
    let search = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let status = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if let Some(status) = &status {
        validate_project_status(status)?;
    }

    let mut query_builder = QueryBuilder::new(
        r#"WITH page_projects AS (
               SELECT p.id, p.name, p.owner_user_id, p.status, p.is_default,
                      p.created_at, p.updated_at
               FROM project p
               JOIN "user" owner ON owner.id = p.owner_user_id"#,
    );

    let mut has_where = false;
    if let Some(search) = search {
        query_builder
            .push(" WHERE (p.name ILIKE ")
            .push_bind(format!("%{search}%"))
            .push(" OR owner.email::TEXT ILIKE ")
            .push_bind(format!("%{search}%"))
            .push(")");
        has_where = true;
    }

    if let Some(status) = status {
        query_builder
            .push(if has_where { " AND " } else { " WHERE " })
            .push("p.status = ")
            .push_bind(status);
        has_where = true;
    }

    if let Some((created_at, id)) = cursor {
        query_builder
            .push(if has_where { " AND " } else { " WHERE " })
            .push("(p.created_at, p.id) < (")
            .push_bind(created_at)
            .push(", ")
            .push_bind(id)
            .push(")");
    }

    query_builder
        .push(" ORDER BY p.created_at DESC, p.id DESC LIMIT ")
        .push_bind(limit + 1)
        .push(
            r#"
           )
           SELECT p.id, p.name, p.owner_user_id, owner.email AS owner_email,
                  p.status, p.is_default,
                  COALESCE(pm.member_count, 0) AS member_count,
                  COALESCE(uk.user_key_count, 0) AS user_key_count,
                  COALESCE(w.balance_micro_usd, 0) AS balance_micro_usd,
                  COALESCE(w.reserved_micro_usd, 0) AS reserved_micro_usd,
                  p.created_at, p.updated_at
           FROM page_projects p
           JOIN "user" owner ON owner.id = p.owner_user_id
           LEFT JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
           LEFT JOIN LATERAL (
               SELECT count(*) AS member_count
               FROM project_member member
               WHERE member.project_id = p.id
           ) pm ON TRUE
           LEFT JOIN LATERAL (
               SELECT count(*) AS user_key_count
               FROM user_key key
               WHERE key.project_id = p.id
           ) uk ON TRUE
           ORDER BY p.created_at DESC, p.id DESC"#,
        );

    let rows = query_builder.build().fetch_all(&state.db.pool).await?;
    let has_more = rows.len() > limit as usize;
    let rows = rows.into_iter().take(limit as usize).collect::<Vec<_>>();
    let next_cursor = has_more
        .then(|| rows.last())
        .flatten()
        .map(created_id_cursor_from_row)
        .transpose()?;

    Ok(ProjectPage {
        items: rows
            .iter()
            .map(project_from_row)
            .collect::<Result<_, _>>()?,
        limit,
        next_cursor,
        has_more,
    })
}

pub async fn create_project(
    state: &AppState,
    req: CreateProjectRequest,
) -> AppResult<ProjectRecord> {
    validate_project_status(&req.status)?;
    let name = normalize_project_name(&req.name)?;
    let mut tx = state.db.pool.begin().await?;
    ensure_user_exists_in_tx(&mut tx, req.owner_user_id).await?;
    let row = sqlx::query(
        "INSERT INTO project (name, owner_user_id, status, is_default)
         VALUES ($1, $2, $3, FALSE)
         RETURNING id",
    )
    .bind(name)
    .bind(req.owner_user_id)
    .bind(req.status)
    .fetch_one(&mut *tx)
    .await?;
    let project_id: DbId = row.try_get("id")?;
    sqlx::query(
        "INSERT INTO project_member (project_id, user_id, role)
         VALUES ($1, $2, 'owner')
         ON CONFLICT (project_id, user_id)
         DO UPDATE SET role = 'owner', updated_at = now()",
    )
    .bind(project_id)
    .bind(req.owner_user_id)
    .execute(&mut *tx)
    .await?;
    account::create_credit_account(&mut tx, CreditAccountType::Project, project_id).await?;
    tx.commit().await?;
    get_project(state, project_id).await
}

pub async fn update_project(
    state: &AppState,
    id: DbId,
    req: UpdateProjectRequest,
) -> AppResult<ProjectRecord> {
    if let Some(status) = &req.status {
        validate_project_status(status)?;
    }
    let name = req
        .name
        .as_deref()
        .map(normalize_project_name)
        .transpose()?;
    let row = sqlx::query(
        "UPDATE project
         SET name = COALESCE($2, name),
             status = COALESCE($3, status),
             updated_at = now()
         WHERE id = $1
         RETURNING id",
    )
    .bind(id)
    .bind(name)
    .bind(req.status.as_deref())
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    let project_id: DbId = row.try_get("id")?;
    invalidate_project_keys(state, project_id).await?;
    get_project(state, project_id).await
}

pub async fn delete_project(state: &AppState, id: DbId) -> AppResult<()> {
    let row = sqlx::query(
        "SELECT p.is_default,
                EXISTS(SELECT 1 FROM user_key uk WHERE uk.project_id = p.id) AS has_keys
         FROM project p
         WHERE p.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    if row.try_get::<bool, _>("is_default")? {
        return Err(AppError::BadRequest(
            "default project cannot be deleted".to_string(),
        ));
    }
    if row.try_get::<bool, _>("has_keys")? {
        return Err(AppError::BadRequest(
            "project with api keys cannot be deleted".to_string(),
        ));
    }
    let result = sqlx::query("DELETE FROM project WHERE id = $1")
        .bind(id)
        .execute(&state.db.pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn list_project_members(
    state: &AppState,
    project_id: DbId,
) -> AppResult<Vec<ProjectMemberRecord>> {
    let rows = sqlx::query(
        "SELECT pm.id, pm.project_id, pm.user_id, u.email AS user_email,
                pm.role, u.status AS user_status, pm.created_at, pm.updated_at
         FROM project_member pm
         JOIN \"user\" u ON u.id = pm.user_id
         WHERE pm.project_id = $1
         ORDER BY
           CASE pm.role
             WHEN 'owner' THEN 1
             WHEN 'admin' THEN 2
             WHEN 'member' THEN 3
             ELSE 4
           END,
           pm.created_at ASC,
           pm.id ASC",
    )
    .bind(project_id)
    .fetch_all(&state.db.pool)
    .await?;
    Ok(rows
        .iter()
        .map(project_member_from_row)
        .collect::<Result<_, _>>()?)
}

async fn get_project(state: &AppState, id: DbId) -> AppResult<ProjectRecord> {
    let row = sqlx::query(
        "SELECT p.id, p.name, p.owner_user_id, owner.email AS owner_email,
                p.status, p.is_default,
                COALESCE(pm.member_count, 0) AS member_count,
                COALESCE(uk.user_key_count, 0) AS user_key_count,
                COALESCE(w.balance_micro_usd, 0) AS balance_micro_usd,
                COALESCE(w.reserved_micro_usd, 0) AS reserved_micro_usd,
                p.created_at, p.updated_at
         FROM project p
         JOIN \"user\" owner ON owner.id = p.owner_user_id
         LEFT JOIN credit_account w ON w.owner_type = 'project' AND w.owner_id = p.id
         LEFT JOIN LATERAL (
             SELECT count(*) AS member_count
             FROM project_member member
             WHERE member.project_id = p.id
         ) pm ON TRUE
         LEFT JOIN LATERAL (
             SELECT count(*) AS user_key_count
             FROM user_key key
             WHERE key.project_id = p.id
         ) uk ON TRUE
         WHERE p.id = $1",
    )
    .bind(id)
    .fetch_optional(&state.db.pool)
    .await?
    .ok_or(AppError::NotFound)?;
    project_from_row(&row)
}

async fn ensure_user_exists_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: DbId,
) -> AppResult<()> {
    let exists = sqlx::query("SELECT id FROM \"user\" WHERE id = $1 FOR KEY SHARE")
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await?
        .is_some();
    if !exists {
        return Err(AppError::BadRequest(
            "owner user does not exist".to_string(),
        ));
    }
    Ok(())
}

async fn invalidate_project_keys(state: &AppState, project_id: DbId) -> AppResult<()> {
    let rows = sqlx::query("SELECT id FROM user_key WHERE project_id = $1")
        .bind(project_id)
        .fetch_all(&state.db.pool)
        .await?;
    for row in rows {
        let id: DbId = row.try_get("id")?;
        state
            .cache_invalidator
            .invalidate(state, crate::cache::InvalidationEvent::UserKey { id })
            .await;
    }
    Ok(())
}

fn parse_created_id_cursor(cursor: Option<&str>) -> AppResult<Option<(DateTime<Utc>, DbId)>> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let Some((created_at, id)) = cursor.rsplit_once('|') else {
        return Err(AppError::BadRequest("invalid cursor".to_string()));
    };
    let created_at = DateTime::parse_from_rfc3339(created_at)
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?
        .with_timezone(&Utc);
    let id = id
        .parse::<DbId>()
        .map_err(|_| AppError::BadRequest("invalid cursor".to_string()))?;
    Ok(Some((created_at, id)))
}

fn created_id_cursor_from_row(row: &sqlx::postgres::PgRow) -> Result<String, sqlx::Error> {
    let created_at: DateTime<Utc> = row.try_get("created_at")?;
    let id: DbId = row.try_get("id")?;
    Ok(format!("{}|{}", created_at.to_rfc3339(), id))
}

fn validate_project_status(status: &str) -> AppResult<()> {
    match status {
        "enabled" | "disabled" => Ok(()),
        other => Err(AppError::BadRequest(format!("invalid status: {other}"))),
    }
}

fn normalize_project_name(name: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("project name is required".to_string()));
    }
    if name.chars().count() > 80 {
        return Err(AppError::BadRequest(
            "project name must be 80 characters or fewer".to_string(),
        ));
    }
    Ok(name.to_string())
}

fn project_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProjectRecord> {
    let balance_micro_usd = row.try_get("balance_micro_usd")?;
    let reserved_micro_usd = row.try_get("reserved_micro_usd")?;
    Ok(ProjectRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        owner_user_id: row.try_get("owner_user_id")?,
        owner_email: row.try_get("owner_email")?,
        status: row.try_get("status")?,
        is_default: row.try_get("is_default")?,
        member_count: row.try_get("member_count")?,
        user_key_count: row.try_get("user_key_count")?,
        balance_micro_usd,
        reserved_micro_usd,
        available_micro_usd: balance_micro_usd - reserved_micro_usd,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn project_member_from_row(row: &sqlx::postgres::PgRow) -> AppResult<ProjectMemberRecord> {
    Ok(ProjectMemberRecord {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        user_id: row.try_get("user_id")?,
        user_email: row.try_get("user_email")?,
        role: row.try_get("role")?,
        user_status: row.try_get("user_status")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
