use crate::{
    auth::UserAuth,
    billing::{
        BillableUsage, BillingAccounts, CreditAccountId, DebitHold, SettleRequest, TokenUsage,
    },
    error::AppResult,
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
    let billing_context = upstream::billing_context(&state.db.pool, &task).await?;
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
    let Some(hold) =
        upstream::mark_billing_status(&state.db.pool, task_id, "held", "released").await?
    else {
        return Ok(());
    };
    release_empty_hold(state, hold, context).await;
    Ok(())
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
    let target = if usage.is_some() {
        "settled"
    } else {
        "released"
    };
    let Some(hold) = upstream::mark_billing_status(&state.db.pool, task.id, "held", target).await?
    else {
        return Ok(());
    };
    if let Some(usage) = usage {
        let Some(model) = task.model.as_deref() else {
            fail_settled_task_billing(state, task.id, hold, "async task missing model").await?;
            return Ok(());
        };
        let upstream_model = task.upstream_model.as_deref().unwrap_or(model);
        let price = match state
            .billing
            .price_for(
                &state.db.pool,
                &task.provider,
                upstream_model,
                &billing_context.user_group,
            )
            .await
        {
            Ok(price) => price,
            Err(err) => {
                fail_settled_task_billing(state, task.id, hold, "async task price lookup error")
                    .await?;
                return Err(err);
            }
        };
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
                    usage: Some(BillableUsage::token(usage)),
                    price: &price,
                },
            )
            .await
        {
            Ok(billing) => billing,
            Err(err) => {
                fail_settled_task_billing(state, task.id, hold, "async task billing settle error")
                    .await?;
                return Err(err);
            }
        };
        state.billing_outbox.enqueue_or_retry(UsageInsert {
            user_id: billing_context.user_id,
            project_id: billing_context.project_id,
            user_key_id: billing_context.user_key_id,
            channel_id: upstream.channel_id,
            channel_key_id: upstream.channel_key_id,
            credential_id: upstream.credential_id,
            relay_trace_id: None,
            relay_attempt: 1,
            relay_final: true,
            model: Some(model.to_string()),
            upstream_model: Some(upstream_model.to_string()),
            routing_phase: "relay".to_string(),
            routing: None,
            status_code: Some(200),
            streamed: false,
            latency_ms: 0,
            first_response_ms: None,
            output_tokens_per_second: None,
            error_summary: None,
            token_usage: Some(usage),
            billing_meter: billing.billing_meter,
            billable_units: billing.billable_units,
            billing: Some(billing),
        });
    } else {
        release_empty_hold(state, hold, "async task terminal without usage").await;
    }
    Ok(())
}

async fn fail_settled_task_billing(
    state: &AppState,
    task_id: DbId,
    hold: DebitHold,
    context: &str,
) -> AppResult<()> {
    let _ = upstream::mark_billing_status(&state.db.pool, task_id, "settled", "failed").await?;
    release_empty_hold(state, hold, context).await;
    Ok(())
}

async fn release_empty_hold(state: &AppState, hold: DebitHold, context: &str) {
    if let Err(err) = state.billing.release_hold(&state.db.pool, hold).await {
        tracing::warn!("failed to release {context} hold: {err}");
    }
}
