use std::{sync::Arc, time::Instant};

use axum::{body::to_bytes, http::HeaderMap, response::Response};
use bytes::Bytes;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    auth::user_auth_for_key_id,
    billing::{parse_usage_from_bytes, TokenUsage},
    error::{AppError, AppResult},
    id::DbId,
    provider::openai,
    AppState,
};

use super::{AppRunOutcome, AppRuntime, IncomingAppMessage, APP_BODY_LIMIT_BYTES};

type RunLogResult = (
    &'static str,
    Option<i32>,
    Option<TokenUsage>,
    Option<String>,
);

pub(super) async fn run_app_message(
    state: Arc<AppState>,
    runtime: AppRuntime,
    message: IncomingAppMessage,
) -> AppResult<AppRunOutcome> {
    let started = Instant::now();
    if runtime.status != "enabled" || !runtime.endpoint_enabled {
        return Err(AppError::Forbidden);
    }
    if message.content.trim() == "/clear" {
        let conversation_id = ensure_conversation(&state, &runtime, &message).await?;
        sqlx::query("DELETE FROM app_message WHERE conversation_id = $1")
            .bind(conversation_id)
            .execute(&state.db.pool)
            .await?;
        return Ok(AppRunOutcome {
            conversation_id,
            message: "上下文已清空。".to_string(),
            trace_id: message.trace_id,
            duplicate: false,
        });
    }

    if let Some(message_id) = &message.external_message_id {
        if !record_delivery(&state, runtime.endpoint_id, message_id).await? {
            let conversation_id = ensure_conversation(&state, &runtime, &message).await?;
            insert_run_log(
                &state,
                &runtime,
                Some(conversation_id),
                &message,
                started.elapsed().as_millis() as i64,
                ("duplicate", None, None, None),
            )
            .await?;
            return Ok(AppRunOutcome {
                conversation_id,
                message: String::new(),
                trace_id: message.trace_id,
                duplicate: true,
            });
        }
    }

    let conversation_id = ensure_conversation(&state, &runtime, &message).await?;
    insert_app_message(
        &state,
        conversation_id,
        &runtime,
        message.external_message_id.as_deref(),
        "user",
        &message.content,
        &message.metadata,
    )
    .await?;
    let history = conversation_history(&state, conversation_id, runtime.context_turns).await?;
    let body = Bytes::from(serde_json::to_vec(&json!({
        "model": runtime.model,
        "messages": build_chat_messages(&runtime.system_prompt, &history),
        "max_tokens": runtime.max_output_tokens,
        "stream": false
    }))?);
    let auth = user_auth_for_key_id(&state, runtime.user_key_id).await?;
    let response =
        openai::openai_chat_completion_response(Arc::clone(&state), auth, HeaderMap::new(), body)
            .await;
    match response {
        Ok(response) => {
            let status = response.status();
            let bytes = body_bytes(response).await?;
            let answer = extract_chat_answer(&bytes)
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_string());
            let usage = parse_usage_from_bytes(&bytes, false);
            insert_app_message(
                &state,
                conversation_id,
                &runtime,
                None,
                "assistant",
                &answer,
                &json!({}),
            )
            .await?;
            update_app_activity(&state, runtime.app_id, runtime.endpoint_id).await?;
            insert_run_log(
                &state,
                &runtime,
                Some(conversation_id),
                &message,
                started.elapsed().as_millis() as i64,
                ("success", Some(status.as_u16() as i32), usage, None),
            )
            .await?;
            Ok(AppRunOutcome {
                conversation_id,
                message: answer,
                trace_id: message.trace_id,
                duplicate: false,
            })
        }
        Err(err) => {
            let summary = err.to_string();
            insert_run_log(
                &state,
                &runtime,
                Some(conversation_id),
                &message,
                started.elapsed().as_millis() as i64,
                ("failed", None, None, Some(summary)),
            )
            .await?;
            Err(err)
        }
    }
}

async fn body_bytes(response: Response) -> AppResult<Bytes> {
    let (_parts, body) = response.into_parts();
    to_bytes(body, APP_BODY_LIMIT_BYTES)
        .await
        .map_err(|err| AppError::BadRequest(format!("failed to read model response: {err}")))
}

fn build_chat_messages(system_prompt: &str, history: &[(String, String)]) -> Vec<Value> {
    let mut messages = Vec::new();
    if !system_prompt.trim().is_empty() {
        messages.push(json!({ "role": "system", "content": system_prompt }));
    }
    messages.extend(
        history
            .iter()
            .map(|(role, content)| json!({ "role": role, "content": content })),
    );
    messages
}

fn extract_chat_answer(bytes: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(bytes).ok()?;
    value
        .get("choices")?
        .get(0)?
        .get("message")?
        .get("content")?
        .as_str()
        .map(str::to_string)
}

async fn insert_run_log(
    state: &AppState,
    runtime: &AppRuntime,
    conversation_id: Option<DbId>,
    message: &IncomingAppMessage,
    latency_ms: i64,
    result: RunLogResult,
) -> AppResult<()> {
    let (status, status_code, usage, error_summary) = result;
    sqlx::query(
        r#"
        INSERT INTO app_run_log
            (app_id, endpoint_id, conversation_id, external_user_id, external_conversation_id,
             external_message_id, trace_id, app_type, model, status, status_code, latency_ms,
             input_tokens, output_tokens, total_tokens, error_summary)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
        "#,
    )
    .bind(runtime.app_id)
    .bind(runtime.endpoint_id)
    .bind(conversation_id)
    .bind(&message.external_user_id)
    .bind(&message.external_conversation_id)
    .bind(message.external_message_id.as_deref())
    .bind(&message.trace_id)
    .bind(&runtime.app_type)
    .bind(&runtime.model)
    .bind(status)
    .bind(status_code)
    .bind(latency_ms)
    .bind(usage.map(|usage| usage.input_tokens))
    .bind(usage.map(|usage| usage.output_tokens))
    .bind(usage.map(TokenUsage::total_tokens))
    .bind(error_summary)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn ensure_conversation(
    state: &AppState,
    runtime: &AppRuntime,
    message: &IncomingAppMessage,
) -> AppResult<DbId> {
    let row = sqlx::query(
        r#"
        INSERT INTO app_conversation
            (app_id, endpoint_id, external_user_id, external_conversation_id)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (app_id, endpoint_id, external_user_id, external_conversation_id)
        DO UPDATE SET updated_at = now()
        RETURNING id
        "#,
    )
    .bind(runtime.app_id)
    .bind(runtime.endpoint_id)
    .bind(&message.external_user_id)
    .bind(&message.external_conversation_id)
    .fetch_one(&state.db.pool)
    .await?;
    Ok(row.try_get("id")?)
}

async fn insert_app_message(
    state: &AppState,
    conversation_id: DbId,
    runtime: &AppRuntime,
    external_message_id: Option<&str>,
    role: &str,
    content: &str,
    metadata: &Value,
) -> AppResult<()> {
    sqlx::query(
        r#"
        INSERT INTO app_message
            (conversation_id, app_id, endpoint_id, external_message_id, role, content, metadata)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(conversation_id)
    .bind(runtime.app_id)
    .bind(runtime.endpoint_id)
    .bind(external_message_id)
    .bind(role)
    .bind(content)
    .bind(metadata)
    .execute(&state.db.pool)
    .await?;
    Ok(())
}

async fn conversation_history(
    state: &AppState,
    conversation_id: DbId,
    context_turns: i32,
) -> AppResult<Vec<(String, String)>> {
    let limit = (context_turns.max(0) as i64 * 2).max(2);
    let rows = sqlx::query(
        r#"
        SELECT role, content
        FROM (
            SELECT role, content, created_at, id
            FROM app_message
            WHERE conversation_id = $1 AND role IN ('user', 'assistant')
            ORDER BY created_at DESC, id DESC
            LIMIT $2
        ) recent
        ORDER BY created_at ASC, id ASC
        "#,
    )
    .bind(conversation_id)
    .bind(limit)
    .fetch_all(&state.db.pool)
    .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("role")?, row.try_get("content")?)))
        .collect()
}

async fn record_delivery(
    state: &AppState,
    endpoint_id: DbId,
    external_message_id: &str,
) -> AppResult<bool> {
    let result = sqlx::query(
        "INSERT INTO app_message_delivery (endpoint_id, external_message_id)
         VALUES ($1, $2)
         ON CONFLICT (endpoint_id, external_message_id) DO NOTHING",
    )
    .bind(endpoint_id)
    .bind(external_message_id)
    .execute(&state.db.pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

async fn update_app_activity(state: &AppState, app_id: DbId, endpoint_id: DbId) -> AppResult<()> {
    sqlx::query("UPDATE app SET last_active_at = now(), updated_at = now() WHERE id = $1")
        .bind(app_id)
        .execute(&state.db.pool)
        .await?;
    sqlx::query("UPDATE app_endpoint SET last_active_at = now(), updated_at = now() WHERE id = $1")
        .bind(endpoint_id)
        .execute(&state.db.pool)
        .await?;
    Ok(())
}
