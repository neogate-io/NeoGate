use sqlx::{Postgres, Row, Transaction};

use crate::{
    billing::{account, CreditAccountType},
    error::{AppError, AppResult},
    id::DbId,
};

const DEFAULT_PROJECT_NAME: &str = "默认项目";

pub(crate) async fn ensure_default_project_for_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: DbId,
) -> AppResult<DbId> {
    if let Some(row) = sqlx::query(
        r#"
        SELECT id
        FROM project
        WHERE owner_user_id = $1
          AND is_default = TRUE
        ORDER BY id ASC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    {
        let project_id: DbId = row.try_get("id")?;
        sqlx::query(
            r#"
            UPDATE project
            SET status = 'enabled',
                deleted_at = NULL,
                updated_at = now()
            WHERE id = $1
            "#,
        )
        .bind(project_id)
        .execute(&mut **tx)
        .await?;
        ensure_default_project_relations(tx, user_id, project_id).await?;
        return Ok(project_id);
    }

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
    ensure_default_project_relations(tx, user_id, project_id).await?;
    Ok(project_id)
}

async fn ensure_default_project_relations(
    tx: &mut Transaction<'_, Postgres>,
    user_id: DbId,
    project_id: DbId,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO project_member (project_id, user_id, role)
        VALUES ($1, $2, 'owner')
        ON CONFLICT (project_id, user_id)
        DO UPDATE SET role = 'owner', updated_at = now()
        "#,
    )
    .bind(project_id)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    account::get_or_create_credit_account_for_update(tx, CreditAccountType::Project, project_id)
        .await?;
    Ok(())
}

pub(crate) async fn default_project_for_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: DbId,
) -> AppResult<DbId> {
    ensure_default_project_for_user(tx, user_id).await?;
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
