use chrono::{DateTime, Utc};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth::UserAuth,
    billing::{
        video, BillableUsage, Billing, BillingAccounts, BillingMeter, CreditAccountId, DebitHold,
        SettleRequest, TokenUsage, VideoBillingMode,
    },
    error::{AppError, AppResult},
    id::DbId,
    relay::selector::SelectedUpstream,
    usage::UsageInsert,
    AppState,
};

use super::upstream::{self, UpstreamTask, UpstreamTaskType};

pub(crate) async fn finalize_for_auth(
    state: &AppState,
    auth: &UserAuth,
    upstream_task_id: &str,
    task_type: UpstreamTaskType,
    usage: Option<TokenUsage>,
    terminal: bool,
) -> AppResult<()> {
    if !terminal {
        return Ok(());
    }
    let (task, upstream) =
        upstream::fetch_task_for_auth(state, auth, task_type, upstream_task_id).await?;
    let user_key_model_credit_account = task
        .model
        .as_deref()
        .and_then(|model| auth.model_credit_account(model))
        .cloned();
    finalize_loaded(
        state,
        &task,
        &upstream,
        AsyncTaskBillingContext {
            user_id: auth.user_id,
            project_id: auth.project_id,
            user_key_id: auth.user_key_id,
            project_credit_account: auth.project_credit_account.clone(),
            user_key_credit_account: auth.user_key_credit_account.clone(),
            user_key_model_credit_account,
            user_group: auth.user_group.clone(),
        },
        usage,
    )
    .await
}

pub(crate) async fn finalize_polled(
    state: &AppState,
    task: UpstreamTask,
    upstream: SelectedUpstream,
    usage: Option<TokenUsage>,
) -> AppResult<()> {
    let billing_context = match upstream::billing_context(&state.db.pool, &task).await {
        Ok(context) => context,
        Err(err) => {
            if permanent_billing_error(&err) {
                if let Some(hold) = upstream::held_billing_hold(&state.db.pool, task.id).await? {
                    fail_settled_task_billing(
                        state,
                        task.id,
                        hold,
                        "async task billing context error",
                    )
                    .await?;
                }
            }
            return Err(err);
        }
    };
    finalize_loaded(
        state,
        &task,
        &upstream,
        AsyncTaskBillingContext {
            user_id: billing_context.user_id,
            project_id: billing_context.project_id,
            user_key_id: billing_context.user_key_id,
            project_credit_account: billing_context.project_credit_account,
            user_key_credit_account: billing_context.user_key_credit_account,
            user_key_model_credit_account: billing_context.user_key_model_credit_account,
            user_group: billing_context.user_group,
        },
        usage,
    )
    .await
}

pub(crate) async fn release_task_hold_by_id(
    state: &AppState,
    task_id: DbId,
    context: &str,
) -> AppResult<()> {
    transition_and_release_task_hold(state, task_id, "released", None).await?;
    tracing::debug!(task_id, context, "released async task billing hold");
    Ok(())
}

pub(crate) async fn fail_task_hold_by_id(
    state: &AppState,
    task_id: DbId,
    context: &str,
) -> AppResult<()> {
    let Some(hold) = upstream::held_billing_hold(&state.db.pool, task_id).await? else {
        return Ok(());
    };
    fail_settled_task_billing(state, task_id, hold, context).await
}

struct AsyncTaskBillingContext {
    user_id: DbId,
    project_id: DbId,
    user_key_id: DbId,
    project_credit_account: CreditAccountId,
    user_key_credit_account: CreditAccountId,
    user_key_model_credit_account: Option<CreditAccountId>,
    user_group: String,
}

async fn finalize_loaded(
    state: &AppState,
    task: &UpstreamTask,
    upstream: &SelectedUpstream,
    billing_context: AsyncTaskBillingContext,
    usage: Option<TokenUsage>,
) -> AppResult<()> {
    let video_success = task.task_type == UpstreamTaskType::OpenAiVideo
        && openai_video_success_status(&task.status);
    let video_billing_metadata = (task.task_type == UpstreamTaskType::OpenAiVideo)
        .then(|| video::video_billing_metadata(&task.upstream_metadata))
        .flatten();
    let video_settlement = video_billing_metadata
        .as_ref()
        .and_then(|metadata| {
            video_success
                .then(|| video::settlement_usage_and_price(metadata, &task.upstream_metadata))
        })
        .flatten();
    if video_billing_metadata.is_some() && video_success && video_settlement.is_none() {
        tracing::warn!(
            task_id = task.id,
            upstream_task_id = %task.upstream_task_id,
            status = %task.status,
            "seedance official token video task finished without usage.total_tokens; releasing hold"
        );
    }
    let provider_video_seconds = (task.task_type == UpstreamTaskType::OpenAiVideo
        && video_success
        && video_billing_metadata.is_none())
    .then(|| video::provider_video_duration_seconds(&task.upstream_metadata))
    .flatten();
    let audio_seconds = completed_audio_seconds(task);
    let usage = if task.task_type == UpstreamTaskType::OpenAiVideo && !video_success {
        None
    } else {
        usage
    };
    let settle_without_usage = task.task_type == UpstreamTaskType::OpenAiVideo
        && video_billing_metadata.is_none()
        && video_success
        && usage.is_none();
    let image_count = completed_image_count(task.task_type, &task.status, &task.upstream_metadata);
    let should_settle = video_settlement.is_some()
        || usage.is_some()
        || provider_video_seconds.is_some()
        || audio_seconds.is_some()
        || image_count.is_some()
        || settle_without_usage;
    if !should_settle {
        release_task_hold_by_id(state, task.id, "async task terminal without usage").await?;
        if task.task_type == UpstreamTaskType::AudioTranscription {
            let model = task.model.clone();
            let upstream_model = task.upstream_model.clone().or_else(|| model.clone());
            state
                .usage
                .enqueue(
                    UsageInsert {
                        user_id: billing_context.user_id,
                        project_id: billing_context.project_id,
                        user_key_id: billing_context.user_key_id,
                        channel_id: upstream.channel_id,
                        channel_key_id: upstream.channel_key_id,
                        credential_id: upstream.credential_id,
                        relay_trace_id: async_task_relay_trace_id(&task.upstream_metadata),
                        relay_attempt: 1,
                        relay_final: true,
                        model,
                        upstream_model,
                        routing_phase: "relay".to_string(),
                        routing: None,
                        status_code: Some(502),
                        streamed: false,
                        latency_ms: async_task_latency_ms(task),
                        first_response_ms: None,
                        output_tokens_per_second: None,
                        error_summary: task
                            .upstream_metadata
                            .pointer("/result/error")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned),
                        token_usage: None,
                        billing_meter: BillingMeter::Audio,
                        billable_units: 0,
                        billing: None,
                    },
                    None,
                )
                .await?;
        }
        return Ok(());
    }

    let Some(hold) = upstream::held_billing_hold(&state.db.pool, task.id).await? else {
        return Ok(());
    };
    {
        let Some(model) = task.model.as_deref() else {
            fail_settled_task_billing(state, task.id, hold, "async task missing model").await?;
            return Ok(());
        };
        let upstream_model = task.upstream_model.as_deref().unwrap_or(model);
        let (billable_usage, price) = if let Some((billable_usage, price)) = video_settlement {
            (Some(billable_usage), price)
        } else {
            let price = match state
                .billing
                .price_for(
                    &state.db.pool,
                    task.channel_id,
                    upstream_model,
                    &billing_context.user_group,
                )
                .await
            {
                Ok(price) => price,
                Err(err) => {
                    if permanent_billing_error(&err) {
                        fail_settled_task_billing(
                            state,
                            task.id,
                            hold,
                            "async task price lookup error",
                        )
                        .await?;
                    }
                    return Err(err);
                }
            };
            let billable_usage = if task.task_type == UpstreamTaskType::NeogateResponse
                && price.billing_meter == BillingMeter::Image
            {
                image_count.map(BillableUsage::image)
            } else if task.task_type == UpstreamTaskType::OpenAiVideo
                && price.billing_meter == BillingMeter::Video
            {
                match price.video_billing_mode {
                    Some(VideoBillingMode::PerSecond) => provider_video_seconds
                        .map(BillableUsage::video_seconds)
                        .or_else(|| usage.map(BillableUsage::token)),
                    Some(VideoBillingMode::OfficialToken) => usage.map(BillableUsage::token),
                    None => usage.map(BillableUsage::token),
                }
            } else if task.task_type == UpstreamTaskType::AudioTranscription
                && price.billing_meter == BillingMeter::Audio
            {
                audio_seconds.map(BillableUsage::audio_seconds)
            } else {
                usage.map(BillableUsage::token)
            };
            (billable_usage, price)
        };
        let billable_token_usage = billable_usage.and_then(|usage| usage.token_usage);
        let record_token_usage = usage.or(billable_token_usage);
        let billing = match state
            .billing
            .settle(
                &state.db.pool,
                SettleRequest {
                    accounts: BillingAccounts {
                        user_id: billing_context.user_id,
                        project_id: billing_context.project_id,
                        user_key_id: billing_context.user_key_id,
                        user_key_model_credit_account: billing_context
                            .user_key_model_credit_account
                            .as_ref(),
                        user_key_credit_account: &billing_context.user_key_credit_account,
                        project_credit_account: &billing_context.project_credit_account,
                    },
                    hold: hold.clone(),
                    usage: billable_usage,
                    price: &price,
                    allow_supplemental: false,
                },
            )
            .await
        {
            Ok(billing) => billing,
            Err(err) => {
                if permanent_billing_error(&err) {
                    fail_settled_task_billing(
                        state,
                        task.id,
                        hold,
                        "async task billing settle error",
                    )
                    .await?;
                }
                return Err(err);
            }
        };
        let usage_insert = UsageInsert {
            user_id: billing_context.user_id,
            project_id: billing_context.project_id,
            user_key_id: billing_context.user_key_id,
            channel_id: upstream.channel_id,
            channel_key_id: upstream.channel_key_id,
            credential_id: upstream.credential_id,
            relay_trace_id: async_task_relay_trace_id(&task.upstream_metadata),
            relay_attempt: 1,
            relay_final: true,
            model: Some(model.to_string()),
            upstream_model: Some(upstream_model.to_string()),
            routing_phase: "relay".to_string(),
            routing: None,
            status_code: Some(200),
            streamed: false,
            latency_ms: async_task_latency_ms(task),
            first_response_ms: None,
            output_tokens_per_second: None,
            error_summary: None,
            token_usage: record_token_usage,
            billing_meter: billing.billing_meter,
            billable_units: billing.billable_units,
            billing: Some(billing),
        };
        state
            .billing_outbox
            .enqueue_task_durable(task.id, &usage_insert)
            .await?;
    }
    Ok(())
}

fn permanent_billing_error(err: &AppError) -> bool {
    matches!(
        err,
        AppError::BadRequest(_)
            | AppError::BadRequestWithCode { .. }
            | AppError::NotFound
            | AppError::Json(_)
    )
}

pub(crate) fn permanent_terminal_restore_error(err: &AppError) -> bool {
    matches!(
        err,
        AppError::BadRequest(_)
            | AppError::BadRequestWithCode { .. }
            | AppError::NotFound
            | AppError::Json(_)
            | AppError::Anyhow(_)
    )
}

fn completed_audio_seconds(task: &UpstreamTask) -> Option<i64> {
    if task.task_type != UpstreamTaskType::AudioTranscription || task.status != "completed" {
        return None;
    }
    task.upstream_metadata
        .pointer("/result/duration_seconds")
        .and_then(Value::as_f64)
        .filter(|duration| duration.is_finite() && *duration > 0.0 && *duration <= 43_200.0)
        .map(|duration| duration.ceil() as i64)
}

fn async_task_latency_ms(task: &UpstreamTask) -> i64 {
    let started_at = async_task_started_at(&task.upstream_metadata).unwrap_or(task.created_at);
    Utc::now()
        .signed_duration_since(started_at)
        .num_milliseconds()
        .max(0)
}

fn completed_image_count(
    task_type: UpstreamTaskType,
    status: &str,
    metadata: &Value,
) -> Option<i64> {
    if task_type != UpstreamTaskType::NeogateResponse || status != "completed" {
        return None;
    }
    metadata
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| assets.len() as i64)
        .filter(|count| *count > 0)
}

fn async_task_started_at(value: &Value) -> Option<DateTime<Utc>> {
    value
        .get("neogate")
        .and_then(|neogate| neogate.get("relay_started_at"))
        .and_then(Value::as_str)
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&Utc))
}

fn async_task_relay_trace_id(value: &Value) -> Option<Uuid> {
    value
        .get("neogate")
        .and_then(|neogate| neogate.get("relay_trace_id"))
        .and_then(Value::as_str)
        .and_then(|value| Uuid::parse_str(value).ok())
}

async fn fail_settled_task_billing(
    state: &AppState,
    task_id: DbId,
    hold: DebitHold,
    context: &str,
) -> AppResult<()> {
    transition_and_release_task_hold(state, task_id, "failed", Some(hold.transaction_id)).await?;
    tracing::warn!(task_id, context, "abandoned async task billing hold");
    Ok(())
}

fn openai_video_success_status(status: &str) -> bool {
    matches!(status, "completed" | "succeeded" | "success")
}

async fn transition_and_release_task_hold(
    state: &AppState,
    task_id: DbId,
    target_status: &str,
    expected_transaction_id: Option<Uuid>,
) -> AppResult<bool> {
    let mut tx = state.db.pool.begin().await?;
    let row = sqlx::query(
        "UPDATE task_upstream
         SET billing_status = $2, updated_at = now()
         WHERE id = $1 AND billing_status = 'held'
         RETURNING billing_hold",
    )
    .bind(task_id)
    .bind(target_status)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(false);
    };
    let hold_value: Option<Value> = sqlx::Row::try_get(&row, "billing_hold")?;
    if let Some(hold_value) = hold_value {
        let hold: DebitHold = serde_json::from_value(hold_value)?;
        if let Some(expected) = expected_transaction_id {
            if hold.transaction_id != expected {
                // transaction_id 不匹配说明存在逻辑错误，回滚并以错误上报而非静默继续
                if let Err(rb_err) = tx.rollback().await {
                    tracing::warn!(task_id, "failed to rollback after transaction_id mismatch: {rb_err}");
                }
                return Err(AppError::Conflict(format!(
                    "async task hold transaction_id mismatch: expected {expected}, got {}",
                    hold.transaction_id
                )));
            }
        }
        Billing::release_hold_in_transaction(&mut tx, &hold).await?;
    }
    tx.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        async_task_relay_trace_id, async_task_started_at, completed_audio_seconds,
        completed_image_count, openai_video_success_status, permanent_billing_error,
        permanent_terminal_restore_error,
    };
    use crate::error::AppError;
    use crate::task::upstream::{UpstreamTask, UpstreamTaskType};
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn completed_audio_duration_is_rounded_up() {
        let task = UpstreamTask {
            id: 1,
            task_type: UpstreamTaskType::AudioTranscription,
            upstream_task_id: "task-audio".to_string(),
            user_id: 1,
            project_id: 1,
            user_key_id: 1,
            provider: "qwen".to_string(),
            model: Some("speech-to-text".to_string()),
            upstream_model: Some("paraformer-v2".to_string()),
            channel_id: 1,
            channel_endpoint_id: 1,
            channel_key_id: Some(1),
            credential_id: None,
            upstream_base_url: "https://dashscope.aliyuncs.com".to_string(),
            adapter_hint: None,
            status: "completed".to_string(),
            terminal: true,
            upstream_metadata: json!({ "result": { "duration_seconds": 12.01 } }),
            created_at: Utc::now(),
        };
        assert_eq!(completed_audio_seconds(&task), Some(13));
    }

    #[test]
    fn seedance_success_statuses_are_billable_video_terminals() {
        assert!(openai_video_success_status("completed"));
        assert!(openai_video_success_status("succeeded"));
        assert!(openai_video_success_status("success"));
        assert!(!openai_video_success_status("failed"));
        assert!(!openai_video_success_status("cancelled"));
    }

    #[test]
    fn reads_async_task_relay_metadata() {
        let trace_id = Uuid::new_v4();
        let metadata = json!({
            "neogate": {
                "relay_trace_id": trace_id.to_string(),
                "relay_started_at": "2026-07-12T11:45:22Z"
            }
        });

        assert_eq!(async_task_relay_trace_id(&metadata), Some(trace_id));
        assert_eq!(
            async_task_started_at(&metadata),
            Utc.with_ymd_and_hms(2026, 7, 12, 11, 45, 22).single()
        );
    }

    #[test]
    fn ignores_invalid_async_task_relay_metadata() {
        let metadata = json!({
            "neogate": {
                "relay_trace_id": "not-a-uuid",
                "relay_started_at": "not-a-date"
            }
        });

        assert_eq!(async_task_relay_trace_id(&metadata), None);
        assert_eq!(async_task_started_at(&metadata), None);
    }

    #[test]
    fn only_permanent_billing_errors_abandon_async_task_hold() {
        assert!(permanent_billing_error(&AppError::BadRequest(
            "missing price".to_string()
        )));
        assert!(permanent_billing_error(&AppError::NotFound));
        assert!(!permanent_billing_error(&AppError::Sqlx(
            sqlx::Error::PoolTimedOut
        )));
        assert!(!permanent_billing_error(&AppError::UpstreamUnavailable(
            "outbox unavailable".to_string()
        )));
    }

    #[test]
    fn terminal_restore_classifies_missing_upstream_as_permanent() {
        assert!(permanent_terminal_restore_error(&AppError::NotFound));
        assert!(permanent_terminal_restore_error(&AppError::BadRequest(
            "missing channel key".to_string()
        )));
        assert!(permanent_terminal_restore_error(&AppError::Anyhow(
            anyhow::anyhow!("invalid encrypted secret")
        )));
        assert!(!permanent_terminal_restore_error(&AppError::Sqlx(
            sqlx::Error::PoolTimedOut
        )));
    }

    #[test]
    fn counts_completed_neogate_image_assets_for_billing() {
        let metadata = serde_json::json!({"assets": [{"index": 0}, {"index": 1}]});

        assert_eq!(
            completed_image_count(UpstreamTaskType::NeogateResponse, "completed", &metadata),
            Some(2)
        );
        assert_eq!(
            completed_image_count(UpstreamTaskType::NeogateResponse, "failed", &metadata),
            None
        );
        assert_eq!(
            completed_image_count(UpstreamTaskType::OpenAiVideo, "completed", &metadata),
            None
        );
    }
}
