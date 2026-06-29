use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde_json::Value;
use sqlx::{PgPool, Row};

use crate::{
    auth::UserAuth,
    billing::{CreditAccountId, DebitHold, TokenUsage},
    error::{AppError, AppResult},
    id::DbId,
    secrets::SecretStore,
    AppState,
};

pub(crate) use super::types::{
    NewUpstreamTask, TaskBillingContext, UpstreamTask, UpstreamTaskType, UsageSummary,
};
use crate::relay::selector::{SelectedUpstream, UpstreamProtocol};

pub(crate) struct UpstreamTaskUpdate {
    pub task_id: DbId,
    pub task_type: UpstreamTaskType,
    pub upstream_task_id: String,
    pub status: String,
    pub terminal: bool,
    pub metadata: Value,
    pub usage: Option<TokenUsage>,
    pub poll_interval: Duration,
}

pub(crate) async fn insert_task(
    pool: &PgPool,
    task: NewUpstreamTask<'_>,
    poll_interval: Duration,
    retention: Duration,
) -> AppResult<()> {
    let protocol = match task.protocol {
        UpstreamProtocol::Openai => "openai",
        UpstreamProtocol::Anthropic => "anthropic",
        UpstreamProtocol::OpenAiOauth => {
            return Err(AppError::BadRequest(
                "openai_oauth async tasks are not supported".to_string(),
            ))
        }
    };
    let hold = serde_json::to_value(task.hold)?;
    let next_poll_at = (!task.terminal).then(|| next_poll_at(poll_interval));
    let expires_at = Utc::now()
        + ChronoDuration::from_std(retention)
            .unwrap_or_else(|_| ChronoDuration::seconds(2_592_000));
    sqlx::query(
        r#"
        INSERT INTO task_upstream (
            task_type, upstream_task_id, user_id, project_id, user_key_id,
            protocol, provider, model, upstream_model,
            channel_id, channel_endpoint_id, channel_key_id, credential_id, upstream_base_url,
            status, terminal, billing_hold, upstream_metadata, next_poll_at, expires_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
        ON CONFLICT (task_type, provider, upstream_task_id) DO UPDATE
        SET status = EXCLUDED.status,
            terminal = EXCLUDED.terminal,
            upstream_metadata = EXCLUDED.upstream_metadata,
            updated_at = now()
        "#,
    )
    .bind(task.task_type.as_str())
    .bind(task.upstream_task_id)
    .bind(task.auth.user_id)
    .bind(task.auth.project_id)
    .bind(task.auth.user_key_id)
    .bind(protocol)
    .bind(&task.upstream.provider)
    .bind(task.model)
    .bind(task.upstream_model)
    .bind(task.upstream.channel_id)
    .bind(task.upstream.channel_endpoint_id)
    .bind(task.upstream.channel_key_id)
    .bind(task.upstream.credential_id)
    .bind(&task.upstream.base_url)
    .bind(task.status)
    .bind(task.terminal)
    .bind(hold)
    .bind(task.upstream_metadata)
    .bind(next_poll_at)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn fetch_task_for_auth(
    state: &AppState,
    auth: &UserAuth,
    task_type: UpstreamTaskType,
    upstream_task_id: &str,
) -> AppResult<(UpstreamTask, SelectedUpstream)> {
    let task = fetch_task(
        &state.db.pool,
        auth.user_key_id,
        task_type,
        upstream_task_id,
    )
    .await?;
    let upstream = task
        .selected_upstream(&state.db.pool, &state.secrets)
        .await?;
    Ok((task, upstream))
}

pub(crate) async fn claim_due_tasks(
    pool: &PgPool,
    limit: i64,
    poll_interval: Duration,
) -> AppResult<Vec<UpstreamTask>> {
    let next_poll_at = next_poll_at(poll_interval);
    let rows = sqlx::query(
        r#"
        WITH due AS (
            SELECT id
            FROM task_upstream
            WHERE terminal = FALSE
              AND next_poll_at IS NOT NULL
              AND next_poll_at <= now()
            ORDER BY next_poll_at ASC, id ASC
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE task_upstream task
        SET next_poll_at = $2,
            updated_at = now()
        FROM due
        WHERE task.id = due.id
        RETURNING task.id, task.task_type, task.upstream_task_id, task.user_id, task.project_id, task.user_key_id,
                  task.provider, task.model, task.upstream_model, task.channel_id, task.channel_endpoint_id,
                  task.channel_key_id, task.credential_id, task.upstream_base_url,
                  task.status, task.terminal, task.upstream_metadata, task.created_at
        "#,
    )
    .bind(limit)
    .bind(next_poll_at)
    .fetch_all(pool)
    .await?;
    rows.iter().map(task_from_row).collect()
}

pub(crate) async fn fetch_stale_terminal_held_tasks(
    pool: &PgPool,
    stale_before: DateTime<Utc>,
    limit: i64,
) -> AppResult<Vec<UpstreamTask>> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_type, upstream_task_id, user_id, project_id, user_key_id,
               provider, model, upstream_model, channel_id, channel_endpoint_id, channel_key_id, credential_id,
               upstream_base_url, status, terminal, upstream_metadata, created_at
        FROM task_upstream
        WHERE terminal = TRUE
          AND billing_status = 'held'
          AND usage_summary = '{}'::JSONB
          AND updated_at <= $1
        ORDER BY updated_at ASC, id ASC
        LIMIT $2
        "#,
    )
    .bind(stale_before)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    rows.iter().map(task_from_row).collect()
}

pub(crate) async fn list_tasks_for_auth(
    pool: &PgPool,
    auth: &UserAuth,
    task_type: UpstreamTaskType,
    limit: i64,
    after_id: Option<&str>,
    before_id: Option<&str>,
) -> AppResult<Vec<UpstreamTask>> {
    let rows = sqlx::query(
        r#"
        SELECT id, task_type, upstream_task_id, user_id, project_id, user_key_id,
               provider, model, upstream_model, channel_id, channel_endpoint_id, channel_key_id, credential_id,
               upstream_base_url, status, terminal, upstream_metadata, created_at
        FROM task_upstream
        WHERE user_key_id = $1
          AND task_type = $2
          AND (
              $4::TEXT IS NULL
              OR (created_at, id) < (
                  SELECT created_at, id
                  FROM task_upstream
                  WHERE user_key_id = $1
                    AND task_type = $2
                    AND upstream_task_id = $4
                  LIMIT 1
              )
          )
          AND (
              $5::TEXT IS NULL
              OR (created_at, id) > (
                  SELECT created_at, id
                  FROM task_upstream
                  WHERE user_key_id = $1
                    AND task_type = $2
                    AND upstream_task_id = $5
                  LIMIT 1
              )
          )
        ORDER BY created_at DESC, id DESC
        LIMIT $3
        "#,
    )
    .bind(auth.user_key_id)
    .bind(task_type.as_str())
    .bind(limit.clamp(1, 1000))
    .bind(after_id)
    .bind(before_id)
    .fetch_all(pool)
    .await?;
    rows.iter().map(task_from_row).collect()
}

pub(crate) async fn delete_expired_terminal_tasks(pool: &PgPool, limit: i64) -> AppResult<u64> {
    let result = sqlx::query(
        r#"
        WITH expired AS (
            SELECT id
            FROM task_upstream
            WHERE terminal = TRUE
              AND billing_status IN ('settled', 'released', 'failed')
              AND expires_at IS NOT NULL
              AND expires_at <= now()
            ORDER BY expires_at ASC, id ASC
            LIMIT $1
        )
        DELETE FROM task_upstream task
        USING expired
        WHERE task.id = expired.id
        "#,
    )
    .bind(limit.max(1))
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub(crate) async fn billing_context(
    pool: &PgPool,
    task: &UpstreamTask,
) -> AppResult<TaskBillingContext> {
    let Some(model) = task.model.as_deref() else {
        return Err(AppError::BadRequest(
            "async task is missing model for billing".to_string(),
        ));
    };
    let row = sqlx::query(
        r#"
        SELECT
            pw.id AS project_credit_account_id,
            ukw.id AS user_key_credit_account_id,
            ukmw.id AS user_key_model_credit_account_id,
            ug.code AS user_group
        FROM task_upstream task
        JOIN "user" u ON u.id = task.user_id
        JOIN user_group ug ON ug.id = u.user_group_id
        JOIN project p ON p.id = task.project_id
        JOIN credit_account pw ON pw.owner_type = 'project' AND pw.owner_id = p.id
        JOIN credit_account ukw ON ukw.owner_type = 'user_key' AND ukw.owner_id = task.user_key_id
        LEFT JOIN user_key_model ukm
            ON ukm.user_key_id = task.user_key_id
           AND ukm.model = $2
           AND ukm.enabled = TRUE
        LEFT JOIN credit_account ukmw
            ON ukmw.owner_type = 'user_key_model'
           AND ukmw.owner_id = ukm.id
        WHERE task.id = $1
        "#,
    )
    .bind(task.id)
    .bind(model)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    Ok(TaskBillingContext {
        user_id: task.user_id,
        project_id: task.project_id,
        user_key_id: task.user_key_id,
        project_credit_account: CreditAccountId::new(row.try_get("project_credit_account_id")?),
        user_key_credit_account: CreditAccountId::new(row.try_get("user_key_credit_account_id")?),
        user_key_model_credit_account: row
            .try_get::<Option<DbId>, _>("user_key_model_credit_account_id")?
            .map(CreditAccountId::new),
        user_group: row.try_get("user_group")?,
    })
}

pub(crate) async fn fetch_task(
    pool: &PgPool,
    user_key_id: DbId,
    task_type: UpstreamTaskType,
    upstream_task_id: &str,
) -> AppResult<UpstreamTask> {
    let row = sqlx::query(
        r#"
        SELECT id, task_type, upstream_task_id, user_id, project_id, user_key_id,
               provider, model, upstream_model, channel_id, channel_endpoint_id, channel_key_id, credential_id,
               upstream_base_url, status, terminal, upstream_metadata, created_at
        FROM task_upstream
        WHERE user_key_id = $1
          AND task_type = $2
          AND upstream_task_id = $3
        "#,
    )
    .bind(user_key_id)
    .bind(task_type.as_str())
    .bind(upstream_task_id)
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::NotFound)?;
    task_from_row(&row)
}

impl UpstreamTask {
    pub(crate) async fn selected_upstream(
        &self,
        pool: &PgPool,
        secrets: &SecretStore,
    ) -> AppResult<SelectedUpstream> {
        let Some(channel_key_id) = self.channel_key_id else {
            return Err(AppError::BadRequest(
                "async task was created with an unsupported credential upstream".to_string(),
            ));
        };
        let row = sqlx::query(
            r#"
            SELECT ck.secret_ciphertext, c.name AS channel_name
            FROM channel_key ck
            JOIN channel c ON c.id = ck.channel_id
            WHERE ck.id = $1
            "#,
        )
        .bind(channel_key_id)
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::NotFound)?;
        let secret_ciphertext: String = row.try_get("secret_ciphertext")?;
        let channel_name: String = row.try_get("channel_name")?;
        Ok(SelectedUpstream {
            channel_id: self.channel_id,
            channel_endpoint_id: self.channel_endpoint_id,
            channel_key_id: self.channel_key_id,
            credential_id: self.credential_id,
            provider: self.provider.clone(),
            channel_name,
            base_url: self.upstream_base_url.clone(),
            responses_chat_fallback: false,
            secret: secrets.plaintext(channel_key_id, &secret_ciphertext)?,
            account_id: None,
        })
    }
}

pub(crate) async fn update_task_from_upstream_value(
    pool: &PgPool,
    update: UpstreamTaskUpdate,
) -> AppResult<()> {
    let usage_summary = UsageSummary::value_from_usage(update.usage)?;
    let next_poll_at = (!update.terminal).then(|| next_poll_at(update.poll_interval));
    sqlx::query(
        r#"
        UPDATE task_upstream
        SET status = $3,
            terminal = $4,
            upstream_metadata = $5,
            usage_summary = CASE WHEN $6::JSONB = '{}'::JSONB THEN usage_summary ELSE $6 END,
            last_polled_at = now(),
            next_poll_at = $7,
            updated_at = now()
        WHERE task_type = $1
          AND upstream_task_id = $2
          AND id = $8
        "#,
    )
    .bind(update.task_type.as_str())
    .bind(update.upstream_task_id)
    .bind(update.status)
    .bind(update.terminal)
    .bind(update.metadata)
    .bind(usage_summary)
    .bind(next_poll_at)
    .bind(update.task_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(crate) async fn mark_billing_status(
    pool: &PgPool,
    task_id: DbId,
    from_status: &str,
    to_status: &str,
) -> AppResult<Option<DebitHold>> {
    let row = sqlx::query(
        r#"
        UPDATE task_upstream
        SET billing_status = $3,
            updated_at = now()
        WHERE id = $1
          AND billing_status = $2
        RETURNING billing_hold
        "#,
    )
    .bind(task_id)
    .bind(from_status)
    .bind(to_status)
    .fetch_optional(pool)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let value: Option<Value> = row.try_get("billing_hold")?;
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

fn task_from_row(row: &sqlx::postgres::PgRow) -> AppResult<UpstreamTask> {
    let task_type: String = row.try_get("task_type")?;
    Ok(UpstreamTask {
        id: row.try_get("id")?,
        task_type: match task_type.as_str() {
            "openai_response" => UpstreamTaskType::OpenAiResponse,
            "neogate_response" => UpstreamTaskType::NeogateResponse,
            "anthropic_message_batch" => UpstreamTaskType::AnthropicMessageBatch,
            other => return Err(AppError::BadRequest(format!("invalid task type: {other}"))),
        },
        upstream_task_id: row.try_get("upstream_task_id")?,
        user_id: row.try_get("user_id")?,
        project_id: row.try_get("project_id")?,
        user_key_id: row.try_get("user_key_id")?,
        provider: row.try_get("provider")?,
        model: row.try_get("model")?,
        upstream_model: row
            .try_get::<Option<String>, _>("upstream_model")?
            .or_else(|| row.try_get("model").ok()),
        channel_id: row.try_get("channel_id")?,
        channel_endpoint_id: row.try_get("channel_endpoint_id")?,
        channel_key_id: row.try_get("channel_key_id")?,
        credential_id: row.try_get("credential_id")?,
        upstream_base_url: row.try_get("upstream_base_url")?,
        status: row.try_get("status")?,
        terminal: row.try_get("terminal")?,
        upstream_metadata: row.try_get("upstream_metadata")?,
        created_at: row.try_get("created_at")?,
    })
}

fn next_poll_at(interval: Duration) -> DateTime<Utc> {
    Utc::now()
        + ChronoDuration::from_std(interval.max(Duration::from_secs(1)))
            .unwrap_or_else(|_| ChronoDuration::seconds(30))
}
