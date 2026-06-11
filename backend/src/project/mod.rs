use sqlx::{Postgres, Row, Transaction};

use crate::{
    billing::{account, CreditAccountType},
    error::{AppError, AppResult},
    id::DbId,
};

const DEFAULT_PROJECT_NAME: &str = "默认项目";

pub(crate) async fn create_default_project_for_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: DbId,
) -> AppResult<DbId> {
    let row = sqlx::query(
        r#"
        INSERT INTO project (name, owner_user_id, status, is_default)
        VALUES ($1, $2, 'enabled', TRUE)
        RETURNING id
        "#,
    )
    .bind(DEFAULT_PROJECT_NAME)
    .bind(user_id)
    .fetch_one(&mut **tx)
    .await?;
    let project_id: DbId = row.try_get("id")?;
    sqlx::query(
        r#"
        INSERT INTO project_member (project_id, user_id, role)
        VALUES ($1, $2, 'owner')
        "#,
    )
    .bind(project_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    account::create_credit_account(tx, CreditAccountType::Project, project_id).await?;
    Ok(project_id)
}

pub(crate) async fn default_project_for_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: DbId,
) -> AppResult<DbId> {
    let row = sqlx::query(
        r#"
        SELECT id
        FROM project
        WHERE owner_user_id = $1
          AND is_default = TRUE
          AND status = 'enabled'
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::BadRequest("default project is missing".to_string()))?;
    Ok(row.try_get("id")?)
}
