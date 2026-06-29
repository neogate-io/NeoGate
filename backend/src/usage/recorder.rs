use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{NaiveDate, Utc};
use sqlx::{PgPool, Postgres, QueryBuilder, Row};
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    time,
};

use crate::{
    billing::{account, BillingCharge, BillingMeter, DebitPart, TokenUsage},
    error::AppResult,
    id::DbId,
};

use super::{KeyFailure, UsageInsert};

const USAGE_BATCH_SIZE: usize = 100;
const KEY_BATCH_SIZE: usize = 100;
const DAILY_FLUSH_MIN_INTERVAL: Duration = Duration::from_secs(5);
const ACTIVITY_FLUSH_MIN_INTERVAL: Duration = Duration::from_secs(10);
const ACTIVITY_MAX_PENDING_KEYS: usize = 10_000;
const DAILY_MAX_PENDING_AGGREGATES: usize = 10_000;

#[derive(Clone)]
pub struct UsageRecorder {
    sender: Option<mpsc::Sender<UsageItem>>,
}

#[derive(Clone)]
pub struct ActivityRecorder {
    entries: Option<Arc<Mutex<ActivityState>>>,
}

#[derive(Clone)]
pub struct UsageDailyRecorder {
    entries: Option<Arc<Mutex<HashMap<DailyUsageKey, DailyUsageAggregate>>>>,
}

#[derive(Clone, Default)]
struct ActivityState {
    channel_keys: HashSet<DbId>,
    user_keys: HashSet<DbId>,
    users: HashSet<DbId>,
}

struct UsageItem {
    usage: UsageInsert,
    failure: Option<KeyFailure>,
}

impl UsageRecorder {
    pub fn spawn(
        pool: PgPool,
        flush_interval: Duration,
        queue_size: usize,
        activity: ActivityRecorder,
        daily: UsageDailyRecorder,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(queue_size.max(1));
        tokio::spawn(run_worker(pool, receiver, flush_interval, activity, daily));
        Self {
            sender: Some(sender),
        }
    }

    pub fn disabled() -> Self {
        Self { sender: None }
    }

    pub async fn enqueue(&self, usage: UsageInsert, failure: Option<KeyFailure>) -> AppResult<()> {
        let Some(sender) = &self.sender else {
            return Ok(());
        };

        let item = UsageItem { usage, failure };
        match sender.try_send(item) {
            Ok(()) => {}
            Err(TrySendError::Full(_item)) => {
                tracing::warn!("relay usage queue is full; dropping non-billing usage record");
            }
            Err(TrySendError::Closed(_)) => {
                tracing::warn!("relay usage worker is closed; dropping usage record");
            }
        }
        Ok(())
    }
}

impl ActivityRecorder {
    pub fn spawn(pool: PgPool, flush_interval: Duration) -> Self {
        let entries = Arc::new(Mutex::new(ActivityState::default()));
        let worker_entries = Arc::clone(&entries);
        tokio::spawn(run_activity_worker(
            pool,
            worker_entries,
            flush_interval.max(ACTIVITY_FLUSH_MIN_INTERVAL),
        ));
        Self {
            entries: Some(entries),
        }
    }

    pub fn disabled() -> Self {
        Self { entries: None }
    }

    pub fn record(&self, usages: &[UsageInsert]) {
        let Some(entries) = &self.entries else {
            return;
        };
        if usages.is_empty() {
            return;
        }

        let mut entries = entries.lock().expect("activity recorder poisoned");
        for usage in usages {
            if let Some(channel_key_id) = usage.channel_key_id {
                insert_bounded_id(
                    &mut entries.channel_keys,
                    channel_key_id,
                    ACTIVITY_MAX_PENDING_KEYS,
                    "activity channel_key buffer",
                );
            }
            insert_bounded_id(
                &mut entries.user_keys,
                usage.user_key_id,
                ACTIVITY_MAX_PENDING_KEYS,
                "activity user_key buffer",
            );
            insert_bounded_id(
                &mut entries.users,
                usage.user_id,
                ACTIVITY_MAX_PENDING_KEYS,
                "activity user buffer",
            );
        }
    }
}

impl UsageDailyRecorder {
    pub fn spawn(pool: PgPool, flush_interval: Duration) -> Self {
        let entries = Arc::new(Mutex::new(HashMap::new()));
        let worker_entries = Arc::clone(&entries);
        tokio::spawn(run_daily_worker(
            pool,
            worker_entries,
            flush_interval.max(DAILY_FLUSH_MIN_INTERVAL),
        ));
        Self {
            entries: Some(entries),
        }
    }

    pub fn disabled() -> Self {
        Self { entries: None }
    }

    pub fn record(&self, usages: &[UsageInsert]) {
        let Some(entries) = &self.entries else {
            return;
        };
        if usages.is_empty() {
            return;
        }

        let day = Utc::now().date_naive();
        let aggregates = daily_usage_aggregates(day, usages);
        let mut entries = entries.lock().expect("usage daily recorder poisoned");
        merge_bounded_daily_usage_aggregates(&mut entries, aggregates);
    }
}

fn insert_bounded_id(values: &mut HashSet<DbId>, value: DbId, limit: usize, label: &'static str) {
    if values.contains(&value) || values.len() < limit {
        values.insert(value);
        return;
    }
    tracing::warn!(limit, "usage {label} is full; dropping aggregate key");
}

async fn run_worker(
    pool: PgPool,
    mut receiver: mpsc::Receiver<UsageItem>,
    flush_interval: Duration,
    activity: ActivityRecorder,
    daily: UsageDailyRecorder,
) {
    let mut interval = time::interval(flush_interval.max(Duration::from_millis(1)));
    let mut usages = Vec::with_capacity(USAGE_BATCH_SIZE);
    let mut failures: HashMap<DbId, KeyFailure> = HashMap::new();

    loop {
        tokio::select! {
            item = receiver.recv() => {
                match item {
                    Some(item) => {
                        if let Some(failure) = item.failure {
                            insert_bounded_failure(&mut failures, failure);
                        }
                        push_bounded_usage(&mut usages, item.usage);
                        if usages.len() >= USAGE_BATCH_SIZE
                            || failures.len() >= KEY_BATCH_SIZE
                        {
                            flush(
                                &pool,
                                &activity,
                                &daily,
                                &mut usages,
                                &mut failures,
                            )
                            .await;
                        }
                    }
                    None => {
                        flush(
                            &pool,
                            &activity,
                            &daily,
                            &mut usages,
                            &mut failures,
                        )
                        .await;
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                flush(
                    &pool,
                    &activity,
                    &daily,
                    &mut usages,
                    &mut failures,
                )
                .await;
            }
        }
    }
}

fn push_bounded_usage(usages: &mut Vec<UsageInsert>, usage: UsageInsert) {
    if usages.len() >= USAGE_BATCH_SIZE {
        tracing::warn!("relay usage flush buffer is full; dropping non-billing usage record");
        return;
    }
    usages.push(usage);
}

fn insert_bounded_failure(failures: &mut HashMap<DbId, KeyFailure>, failure: KeyFailure) {
    if failures.contains_key(&failure.channel_key_id) || failures.len() < KEY_BATCH_SIZE {
        failures.insert(failure.channel_key_id, failure);
        return;
    }
    tracing::warn!("relay key failure flush buffer is full; dropping key failure record");
}

async fn flush(
    pool: &PgPool,
    activity: &ActivityRecorder,
    daily: &UsageDailyRecorder,
    usages: &mut Vec<UsageInsert>,
    failures: &mut HashMap<DbId, KeyFailure>,
) {
    if usages.is_empty() && failures.is_empty() {
        return;
    }

    let Ok(mut tx) = pool.begin().await else {
        tracing::warn!("failed to begin relay usage flush transaction");
        return;
    };
    let result = async {
        if !failures.is_empty() {
            flush_key_failures(&mut tx, failures).await?;
        }
        if !usages.is_empty() {
            flush_usage(&mut tx, usages).await?;
        }
        tx.commit().await?;
        Ok::<(), crate::error::AppError>(())
    }
    .await;

    match result {
        Ok(()) => {
            activity.record(usages);
            daily.record(usages);
            failures.clear();
            usages.clear();
        }
        Err(err) => tracing::warn!("failed to flush relay usage records: {err}"),
    }
}

async fn run_daily_worker(
    pool: PgPool,
    entries: Arc<Mutex<HashMap<DailyUsageKey, DailyUsageAggregate>>>,
    flush_interval: Duration,
) {
    let mut interval = time::interval(flush_interval);
    loop {
        interval.tick().await;
        flush_daily_recorded_usage(&pool, &entries).await;
    }
}

async fn flush_daily_recorded_usage(
    pool: &PgPool,
    entries: &Arc<Mutex<HashMap<DailyUsageKey, DailyUsageAggregate>>>,
) {
    let pending = {
        let mut entries = entries.lock().expect("usage daily recorder poisoned");
        if entries.is_empty() {
            return;
        }
        std::mem::take(&mut *entries)
            .into_values()
            .collect::<Vec<_>>()
    };

    let result = async {
        let mut tx = pool.begin().await?;
        flush_usage_daily_aggregates(&mut tx, &pending).await?;
        tx.commit().await?;
        Ok::<(), crate::error::AppError>(())
    }
    .await;

    if let Err(err) = result {
        tracing::warn!("failed to flush daily usage aggregates: {err}");
        let mut entries = entries.lock().expect("usage daily recorder poisoned");
        let aggregates = pending
            .into_iter()
            .map(|aggregate| (aggregate.key.clone(), aggregate))
            .collect::<HashMap<_, _>>();
        merge_bounded_daily_usage_aggregates(&mut entries, aggregates);
    }
}

async fn run_activity_worker(
    pool: PgPool,
    entries: Arc<Mutex<ActivityState>>,
    flush_interval: Duration,
) {
    let mut interval = time::interval(flush_interval);
    loop {
        interval.tick().await;
        flush_recorded_activity(&pool, &entries).await;
    }
}

async fn flush_recorded_activity(pool: &PgPool, entries: &Arc<Mutex<ActivityState>>) {
    let pending = {
        let mut entries = entries.lock().expect("activity recorder poisoned");
        if entries.is_empty() {
            return;
        }
        std::mem::take(&mut *entries)
    };

    let result = async {
        let mut tx = pool.begin().await?;
        if !pending.channel_keys.is_empty() {
            flush_key_usage(&mut tx, &pending.channel_keys).await?;
        }
        if !pending.user_keys.is_empty() {
            flush_user_key_activity(&mut tx, &pending.user_keys).await?;
        }
        if !pending.users.is_empty() {
            flush_user_activity_ids(&mut tx, &pending.users).await?;
        }
        tx.commit().await?;
        Ok::<(), crate::error::AppError>(())
    }
    .await;

    if let Err(err) = result {
        tracing::warn!("failed to flush activity timestamps: {err}");
        let mut entries = entries.lock().expect("activity recorder poisoned");
        entries.merge_bounded(pending);
    }
}

pub(crate) async fn flush_key_usage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    key_used: &HashSet<DbId>,
) -> AppResult<()> {
    let key_ids: Vec<_> = key_used.iter().copied().collect();
    sqlx::query(
        "UPDATE channel_key SET last_used_at = now(), updated_at = now() WHERE id = ANY($1)",
    )
    .bind(key_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl ActivityState {
    fn is_empty(&self) -> bool {
        self.channel_keys.is_empty() && self.user_keys.is_empty() && self.users.is_empty()
    }

    fn merge_bounded(&mut self, other: Self) {
        for id in other.channel_keys {
            insert_bounded_id(
                &mut self.channel_keys,
                id,
                ACTIVITY_MAX_PENDING_KEYS,
                "activity channel_key buffer",
            );
        }
        for id in other.user_keys {
            insert_bounded_id(
                &mut self.user_keys,
                id,
                ACTIVITY_MAX_PENDING_KEYS,
                "activity user_key buffer",
            );
        }
        for id in other.users {
            insert_bounded_id(
                &mut self.users,
                id,
                ACTIVITY_MAX_PENDING_KEYS,
                "activity user buffer",
            );
        }
    }
}

async fn flush_user_key_activity(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_key_active: &HashSet<DbId>,
) -> AppResult<()> {
    let user_key_ids: Vec<_> = user_key_active.iter().copied().collect();
    sqlx::query(
        "UPDATE user_key SET last_active_at = now(), updated_at = now() WHERE id = ANY($1)",
    )
    .bind(&user_key_ids)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE project_member AS pm
         SET last_active_at = now(), updated_at = now()
         FROM user_key AS uk
         WHERE uk.id = ANY($1)
           AND uk.owner_user_id IS NOT NULL
           AND pm.project_id = uk.project_id
           AND pm.user_id = uk.owner_user_id",
    )
    .bind(&user_key_ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn flush_key_failures(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    failures: &HashMap<DbId, KeyFailure>,
) -> AppResult<()> {
    let mut query_builder = QueryBuilder::<Postgres>::new(
        "UPDATE channel_key AS ck
         SET cooldown_until = data.cooldown_until,
             last_error = data.error,
             updated_at = now()
         FROM (",
    );
    query_builder.push_values(failures.values(), |mut row, failure| {
        row.push_bind(failure.channel_key_id)
            .push_bind(failure.cooldown_until)
            .push_bind(failure.error.chars().take(500).collect::<String>());
    });
    query_builder.push(") AS data(id, cooldown_until, error) WHERE ck.id = data.id");
    query_builder.build().execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn flush_usage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    usages: &[UsageInsert],
) -> AppResult<()> {
    if usages.is_empty() {
        return Ok(());
    }

    let mut pending_usage = Vec::with_capacity(usages.len());

    for item in usages {
        if let Some(billing) = &item.billing {
            if billing.parts.is_empty() && billing.returned_parts.is_empty() {
                pending_usage.push(item);
                continue;
            }
            let row = insert_usage(tx, item).await?;
            let usage_id: DbId = row.try_get("id")?;
            let billing_parts = coalesce_debit_parts(&billing.parts);
            for part in &billing_parts {
                flush_billing_part(tx, usage_id, billing, part).await?;
            }
            for part in &billing.returned_parts {
                flush_returned_billing_part(tx, part).await?;
            }
        } else {
            pending_usage.push(item);
        }
    }

    flush_unbilled_usage(tx, &pending_usage).await?;
    Ok(())
}

async fn flush_user_activity_ids(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    user_ids: &HashSet<DbId>,
) -> AppResult<()> {
    if user_ids.is_empty() {
        return Ok(());
    }

    let user_ids: Vec<_> = user_ids.iter().copied().collect();
    sqlx::query(r#"UPDATE "user" SET last_active_at = now() WHERE id = ANY($1)"#)
        .bind(user_ids)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn flush_returned_billing_part(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    part: &DebitPart,
) -> AppResult<()> {
    account::mark_allocation_returned(tx, part.allocation_id, part.amount_micro_usd).await?;
    account::decrement_reserved(tx, &part.credit_account, part.amount_micro_usd).await?;
    Ok(())
}

async fn flush_unbilled_usage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    usages: &[&UsageInsert],
) -> AppResult<()> {
    if usages.is_empty() {
        return Ok(());
    }

    let mut query_builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO usage
         (user_id, project_id, user_key_id, channel_id, channel_key_id, credential_id,
          relay_trace_id, relay_attempt, relay_final,
          provider, model, upstream_model, status_code,
          streamed, latency_ms, first_response_ms, output_tokens_per_second, error_summary,
          input_tokens, output_tokens, total_tokens, cache_in_tokens,
          cache_create_in_tokens, cache_create_5m_in_tokens,
          cache_create_1h_in_tokens, reason_out_tokens, audio_in_tokens,
          audio_out_tokens, billing_meter, billable_units,
          cost_micro_usd, billing_status, billing_transaction_id)
         ",
    );
    query_builder.push_values(usages, |mut row, item| {
        row.push_bind(item.user_id)
            .push_bind(item.project_id)
            .push_bind(item.user_key_id)
            .push_bind(item.channel_id)
            .push_bind(item.channel_key_id)
            .push_bind(item.credential_id)
            .push_bind(item.relay_trace_id)
            .push_bind(item.relay_attempt)
            .push_bind(item.relay_final)
            .push_bind(&item.provider)
            .push_bind(item.model.as_deref())
            .push_bind(item.upstream_model.as_deref())
            .push_bind(item.status_code)
            .push_bind(item.streamed)
            .push_bind(item.latency_ms)
            .push_bind(item.first_response_ms)
            .push_bind(item.output_tokens_per_second)
            .push_bind(item.error_summary.as_deref())
            .push_bind(input_tokens(item))
            .push_bind(output_tokens(item))
            .push_bind(total_tokens(item))
            .push_bind(item.token_usage.and_then(|usage| usage.cached_input_tokens))
            .push_bind(
                item.token_usage
                    .and_then(|usage| usage.cache_creation_input_tokens),
            )
            .push_bind(
                item.token_usage
                    .and_then(|usage| usage.cache_creation_input_tokens_5m),
            )
            .push_bind(
                item.token_usage
                    .and_then(|usage| usage.cache_creation_input_tokens_1h),
            )
            .push_bind(
                item.token_usage
                    .and_then(|usage| usage.reasoning_output_tokens),
            )
            .push_bind(item.token_usage.and_then(|usage| usage.audio_input_tokens))
            .push_bind(item.token_usage.and_then(|usage| usage.audio_output_tokens))
            .push_bind(usage_billing_meter(item).as_str())
            .push_bind(usage_billable_units(item))
            .push_bind(item.billing.as_ref().map(|billing| billing.cost_micro_usd))
            .push_bind(
                item.billing
                    .as_ref()
                    .map(|billing| billing.status.as_str())
                    .unwrap_or("not_billed"),
            )
            .push_bind(item.billing.as_ref().map(|billing| billing.transaction_id));
    });
    query_builder.build().execute(&mut **tx).await?;
    Ok(())
}

async fn insert_usage(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    item: &UsageInsert,
) -> AppResult<sqlx::postgres::PgRow> {
    let row = sqlx::query(
        "INSERT INTO usage
         (user_id, project_id, user_key_id, channel_id, channel_key_id, credential_id,
          relay_trace_id, relay_attempt, relay_final,
          provider, model, upstream_model, status_code,
          streamed, latency_ms, first_response_ms, output_tokens_per_second, error_summary,
          input_tokens, output_tokens, total_tokens, cache_in_tokens,
          cache_create_in_tokens, cache_create_5m_in_tokens,
          cache_create_1h_in_tokens, reason_out_tokens, audio_in_tokens,
          audio_out_tokens, billing_meter, billable_units,
          cost_micro_usd, billing_status, billing_transaction_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                 $11, $12, $13, $14, $15, $16, $17, $18, $19, $20,
                 $21, $22, $23, $24, $25, $26, $27, $28, $29, $30,
                 $31, $32, $33)
         RETURNING id",
    )
    .bind(item.user_id)
    .bind(item.project_id)
    .bind(item.user_key_id)
    .bind(item.channel_id)
    .bind(item.channel_key_id)
    .bind(item.credential_id)
    .bind(item.relay_trace_id)
    .bind(item.relay_attempt)
    .bind(item.relay_final)
    .bind(&item.provider)
    .bind(item.model.as_deref())
    .bind(item.upstream_model.as_deref())
    .bind(item.status_code)
    .bind(item.streamed)
    .bind(item.latency_ms)
    .bind(item.first_response_ms)
    .bind(item.output_tokens_per_second)
    .bind(item.error_summary.as_deref())
    .bind(input_tokens(item))
    .bind(output_tokens(item))
    .bind(total_tokens(item))
    .bind(item.token_usage.and_then(|usage| usage.cached_input_tokens))
    .bind(
        item.token_usage
            .and_then(|usage| usage.cache_creation_input_tokens),
    )
    .bind(
        item.token_usage
            .and_then(|usage| usage.cache_creation_input_tokens_5m),
    )
    .bind(
        item.token_usage
            .and_then(|usage| usage.cache_creation_input_tokens_1h),
    )
    .bind(
        item.token_usage
            .and_then(|usage| usage.reasoning_output_tokens),
    )
    .bind(item.token_usage.and_then(|usage| usage.audio_input_tokens))
    .bind(item.token_usage.and_then(|usage| usage.audio_output_tokens))
    .bind(usage_billing_meter(item).as_str())
    .bind(usage_billable_units(item))
    .bind(item.billing.as_ref().map(|billing| billing.cost_micro_usd))
    .bind(
        item.billing
            .as_ref()
            .map(|billing| billing.status.as_str())
            .unwrap_or("not_billed"),
    )
    .bind(item.billing.as_ref().map(|billing| billing.transaction_id))
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

async fn flush_usage_daily_aggregates(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    aggregates: &[DailyUsageAggregate],
) -> AppResult<()> {
    if aggregates.is_empty() {
        return Ok(());
    }

    let mut query_builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO usage_daily
         (day, user_id, project_id, user_key_id, channel_id, channel_key_id, credential_id, provider, model,
          request_count, success_count, error_count, streamed_count,
          latency_ms_total, first_response_ms_total, first_response_count,
          input_tokens, output_tokens, total_tokens, cache_in_tokens,
          cache_create_in_tokens, cache_create_5m_in_tokens,
          cache_create_1h_in_tokens, reason_out_tokens, audio_in_tokens,
          audio_out_tokens, billing_meter, billable_units, cost_micro_usd)
         ",
    );
    query_builder.push_values(aggregates, |mut row, item| {
        row.push_bind(item.key.day)
            .push_bind(item.key.user_id)
            .push_bind(item.key.project_id)
            .push_bind(item.key.user_key_id)
            .push_bind(item.key.channel_id)
            .push_bind(item.key.channel_key_id)
            .push_bind(item.key.credential_id)
            .push_bind(&item.key.provider)
            .push_bind(&item.key.model)
            .push_bind(item.request_count)
            .push_bind(item.success_count)
            .push_bind(item.error_count)
            .push_bind(item.streamed_count)
            .push_bind(item.latency_ms_total)
            .push_bind(item.first_response_ms_total)
            .push_bind(item.first_response_count)
            .push_bind(item.input_tokens)
            .push_bind(item.output_tokens)
            .push_bind(item.total_tokens)
            .push_bind(item.cache_in_tokens)
            .push_bind(item.cache_create_in_tokens)
            .push_bind(item.cache_create_5m_in_tokens)
            .push_bind(item.cache_create_1h_in_tokens)
            .push_bind(item.reason_out_tokens)
            .push_bind(item.audio_in_tokens)
            .push_bind(item.audio_out_tokens)
            .push_bind(item.billing_meter.as_str())
            .push_bind(item.billable_units)
            .push_bind(item.cost_micro_usd);
    });
    query_builder.push(
        " ON CONFLICT (
              day,
              COALESCE(user_id, '-1'::BIGINT),
              COALESCE(project_id, '-1'::BIGINT),
              COALESCE(user_key_id, '-1'::BIGINT),
              COALESCE(channel_id, '-1'::BIGINT),
              COALESCE(channel_key_id, '-1'::BIGINT),
              COALESCE(credential_id, '-1'::BIGINT),
              provider,
              model,
              billing_meter
          )
          DO UPDATE SET
              request_count = usage_daily.request_count + EXCLUDED.request_count,
              success_count = usage_daily.success_count + EXCLUDED.success_count,
              error_count = usage_daily.error_count + EXCLUDED.error_count,
              streamed_count = usage_daily.streamed_count + EXCLUDED.streamed_count,
              latency_ms_total = usage_daily.latency_ms_total + EXCLUDED.latency_ms_total,
              first_response_ms_total = usage_daily.first_response_ms_total + EXCLUDED.first_response_ms_total,
              first_response_count = usage_daily.first_response_count + EXCLUDED.first_response_count,
              input_tokens = usage_daily.input_tokens + EXCLUDED.input_tokens,
              output_tokens = usage_daily.output_tokens + EXCLUDED.output_tokens,
              total_tokens = usage_daily.total_tokens + EXCLUDED.total_tokens,
              cache_in_tokens = usage_daily.cache_in_tokens + EXCLUDED.cache_in_tokens,
              cache_create_in_tokens = usage_daily.cache_create_in_tokens + EXCLUDED.cache_create_in_tokens,
              cache_create_5m_in_tokens = usage_daily.cache_create_5m_in_tokens + EXCLUDED.cache_create_5m_in_tokens,
              cache_create_1h_in_tokens = usage_daily.cache_create_1h_in_tokens + EXCLUDED.cache_create_1h_in_tokens,
              reason_out_tokens = usage_daily.reason_out_tokens + EXCLUDED.reason_out_tokens,
              audio_in_tokens = usage_daily.audio_in_tokens + EXCLUDED.audio_in_tokens,
              audio_out_tokens = usage_daily.audio_out_tokens + EXCLUDED.audio_out_tokens,
              billable_units = usage_daily.billable_units + EXCLUDED.billable_units,
              cost_micro_usd = usage_daily.cost_micro_usd + EXCLUDED.cost_micro_usd,
              updated_at = now()",
    );
    query_builder.build().execute(&mut **tx).await?;
    Ok(())
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct DailyUsageKey {
    day: NaiveDate,
    user_id: DbId,
    project_id: DbId,
    user_key_id: DbId,
    channel_id: DbId,
    channel_key_id: Option<DbId>,
    credential_id: Option<DbId>,
    provider: String,
    model: String,
    billing_meter: BillingMeter,
}

#[derive(Clone, Debug)]
struct DailyUsageAggregate {
    key: DailyUsageKey,
    request_count: i64,
    success_count: i64,
    error_count: i64,
    streamed_count: i64,
    latency_ms_total: i64,
    first_response_ms_total: i64,
    first_response_count: i64,
    input_tokens: i64,
    output_tokens: i64,
    total_tokens: i64,
    cache_in_tokens: i64,
    cache_create_in_tokens: i64,
    cache_create_5m_in_tokens: i64,
    cache_create_1h_in_tokens: i64,
    reason_out_tokens: i64,
    audio_in_tokens: i64,
    audio_out_tokens: i64,
    billing_meter: BillingMeter,
    billable_units: i64,
    cost_micro_usd: i64,
}

fn daily_usage_aggregates(
    day: NaiveDate,
    usages: &[UsageInsert],
) -> HashMap<DailyUsageKey, DailyUsageAggregate> {
    let mut aggregates: HashMap<DailyUsageKey, DailyUsageAggregate> =
        HashMap::with_capacity(usages.len());
    for usage in usages {
        let item = DailyUsageAggregate::from_usage(day, usage);
        aggregates
            .entry(item.key.clone())
            .and_modify(|existing| existing.add(&item))
            .or_insert(item);
    }
    aggregates
}

fn merge_bounded_daily_usage_aggregates(
    target: &mut HashMap<DailyUsageKey, DailyUsageAggregate>,
    delta: HashMap<DailyUsageKey, DailyUsageAggregate>,
) {
    for item in delta.into_values() {
        if let Some(existing) = target.get_mut(&item.key) {
            existing.add(&item);
        } else if target.len() < DAILY_MAX_PENDING_AGGREGATES {
            target.insert(item.key.clone(), item);
        } else {
            tracing::warn!(
                limit = DAILY_MAX_PENDING_AGGREGATES,
                "usage daily aggregate buffer is full; dropping aggregate key"
            );
        }
    }
}

impl DailyUsageAggregate {
    fn from_usage(day: NaiveDate, item: &UsageInsert) -> Self {
        let key = DailyUsageKey {
            day,
            user_id: item.user_id,
            project_id: item.project_id,
            user_key_id: item.user_key_id,
            channel_id: item.channel_id,
            channel_key_id: item.channel_key_id,
            credential_id: item.credential_id,
            provider: item.provider.clone(),
            model: item.model.clone().unwrap_or_default(),
            billing_meter: usage_billing_meter(item),
        };
        let success_count = if usage_succeeded(item) { 1_i64 } else { 0_i64 };
        let error_count = 1_i64 - success_count;
        Self {
            key,
            request_count: 1,
            success_count,
            error_count,
            streamed_count: if item.streamed { 1 } else { 0 },
            latency_ms_total: item.latency_ms.max(0),
            first_response_ms_total: item.first_response_ms.unwrap_or(0).max(0),
            first_response_count: if item.first_response_ms.is_some() {
                1
            } else {
                0
            },
            input_tokens: input_tokens(item).unwrap_or(0).max(0),
            output_tokens: output_tokens(item).unwrap_or(0).max(0),
            total_tokens: total_tokens(item).unwrap_or(0).max(0),
            cache_in_tokens: item
                .token_usage
                .and_then(|usage| usage.cached_input_tokens)
                .unwrap_or(0)
                .max(0),
            cache_create_in_tokens: item
                .token_usage
                .and_then(|usage| usage.cache_creation_input_tokens)
                .unwrap_or(0)
                .max(0),
            cache_create_5m_in_tokens: item
                .token_usage
                .and_then(|usage| usage.cache_creation_input_tokens_5m)
                .unwrap_or(0)
                .max(0),
            cache_create_1h_in_tokens: item
                .token_usage
                .and_then(|usage| usage.cache_creation_input_tokens_1h)
                .unwrap_or(0)
                .max(0),
            reason_out_tokens: item
                .token_usage
                .and_then(|usage| usage.reasoning_output_tokens)
                .unwrap_or(0)
                .max(0),
            audio_in_tokens: item
                .token_usage
                .and_then(|usage| usage.audio_input_tokens)
                .unwrap_or(0)
                .max(0),
            audio_out_tokens: item
                .token_usage
                .and_then(|usage| usage.audio_output_tokens)
                .unwrap_or(0)
                .max(0),
            billing_meter: usage_billing_meter(item),
            billable_units: usage_billable_units(item),
            cost_micro_usd: item
                .billing
                .as_ref()
                .map(|billing| billing.cost_micro_usd)
                .unwrap_or(0)
                .max(0),
        }
    }

    fn add(&mut self, other: &Self) {
        self.request_count += other.request_count;
        self.success_count += other.success_count;
        self.error_count += other.error_count;
        self.streamed_count += other.streamed_count;
        self.latency_ms_total += other.latency_ms_total;
        self.first_response_ms_total += other.first_response_ms_total;
        self.first_response_count += other.first_response_count;
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.cache_in_tokens += other.cache_in_tokens;
        self.cache_create_in_tokens += other.cache_create_in_tokens;
        self.cache_create_5m_in_tokens += other.cache_create_5m_in_tokens;
        self.cache_create_1h_in_tokens += other.cache_create_1h_in_tokens;
        self.reason_out_tokens += other.reason_out_tokens;
        self.audio_in_tokens += other.audio_in_tokens;
        self.audio_out_tokens += other.audio_out_tokens;
        self.billable_units += other.billable_units;
        self.cost_micro_usd += other.cost_micro_usd;
    }
}

fn usage_succeeded(item: &UsageInsert) -> bool {
    item.status_code
        .map(|status| (200..400).contains(&status))
        .unwrap_or_else(|| item.error_summary.is_none())
}

fn input_tokens(item: &UsageInsert) -> Option<i64> {
    item.token_usage
        .map(|usage| usage.input_tokens)
        .or_else(|| {
            item.billing
                .as_ref()
                .and_then(|billing| billing.input_tokens)
        })
}

fn output_tokens(item: &UsageInsert) -> Option<i64> {
    item.token_usage
        .map(|usage| usage.output_tokens)
        .or_else(|| {
            item.billing
                .as_ref()
                .and_then(|billing| billing.output_tokens)
        })
}

fn total_tokens(item: &UsageInsert) -> Option<i64> {
    item.token_usage.map(TokenUsage::total_tokens).or_else(|| {
        item.billing
            .as_ref()
            .and_then(|billing| billing.total_tokens)
    })
}

fn usage_billing_meter(item: &UsageInsert) -> BillingMeter {
    item.billing
        .as_ref()
        .map(|billing| billing.billing_meter)
        .unwrap_or(item.billing_meter)
}

fn usage_billable_units(item: &UsageInsert) -> i64 {
    item.billing
        .as_ref()
        .map(|billing| billing.billable_units)
        .unwrap_or(item.billable_units)
        .max(0)
}

async fn flush_billing_part(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    usage_id: DbId,
    billing: &BillingCharge,
    part: &DebitPart,
) -> AppResult<()> {
    account::mark_allocation_consumed(tx, part.allocation_id, part.amount_micro_usd).await?;
    let balance_after =
        account::debit_reserved_balance(tx, &part.credit_account, part.amount_micro_usd).await?;

    sqlx::query(
        "INSERT INTO credit_ledger
         (credit_account_id, amount_micro_usd, balance_after_micro_usd, reason,
          usage_id, allocation_id, transaction_id, metadata)
         VALUES ($1, $2, $3, 'usage', $4, $5, $6, $7)",
    )
    .bind(part.credit_account.id)
    .bind(-part.amount_micro_usd)
    .bind(balance_after)
    .bind(usage_id)
    .bind(part.allocation_id)
    .bind(billing.transaction_id)
    .bind(serde_json::json!({
        "billing_status": billing.status,
        "billing_meter": billing.billing_meter,
        "billable_units": billing.billable_units,
        "input_tokens": billing.input_tokens,
        "output_tokens": billing.output_tokens,
        "total_tokens": billing.total_tokens
    }))
    .execute(&mut **tx)
    .await?;

    Ok(())
}

fn coalesce_debit_parts(parts: &[DebitPart]) -> Vec<DebitPart> {
    let mut coalesced: Vec<DebitPart> = Vec::new();
    let mut indexes: HashMap<(DbId, DbId), usize> = HashMap::new();

    for part in parts {
        if part.amount_micro_usd <= 0 {
            continue;
        }

        let key = (part.credit_account.id, part.allocation_id);
        if let Some(&index) = indexes.get(&key) {
            coalesced[index].amount_micro_usd += part.amount_micro_usd;
        } else {
            indexes.insert(key, coalesced.len());
            coalesced.push(part.clone());
        }
    }

    coalesced
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn billing_charge() -> BillingCharge {
        BillingCharge {
            transaction_id: Uuid::new_v4(),
            input_tokens: Some(10),
            output_tokens: Some(20),
            total_tokens: Some(30),
            billing_meter: BillingMeter::Token,
            billable_units: 30,
            cost_micro_usd: 300,
            status: "billed".to_string(),
            parts: Vec::new(),
            returned_parts: Vec::new(),
        }
    }

    fn usage_insert(id: DbId) -> UsageInsert {
        UsageInsert {
            user_id: id,
            project_id: id,
            user_key_id: id,
            channel_id: id,
            channel_key_id: Some(id),
            credential_id: None,
            relay_trace_id: None,
            relay_attempt: 1,
            relay_final: true,
            provider: "openai".to_string(),
            model: Some("gpt-4.1".to_string()),
            upstream_model: Some("gpt-4.1".to_string()),
            status_code: Some(200),
            streamed: false,
            latency_ms: 123,
            first_response_ms: None,
            output_tokens_per_second: None,
            error_summary: None,
            token_usage: None,
            billing_meter: BillingMeter::Token,
            billable_units: 0,
            billing: None,
        }
    }

    fn key_failure(id: DbId) -> KeyFailure {
        KeyFailure {
            channel_key_id: id,
            cooldown_until: Utc::now(),
            error: "upstream error".to_string(),
        }
    }

    #[test]
    fn billing_usage_payload_round_trips() {
        let charge = billing_charge();
        let usage = UsageInsert {
            user_id: 1,
            project_id: 1,
            user_key_id: 2,
            channel_id: 3,
            channel_key_id: Some(4),
            credential_id: None,
            relay_trace_id: None,
            relay_attempt: 1,
            relay_final: true,
            provider: "openai".to_string(),
            model: Some("gpt-4.1".to_string()),
            upstream_model: Some("gpt-4.1".to_string()),
            status_code: Some(200),
            streamed: false,
            latency_ms: 123,
            first_response_ms: None,
            output_tokens_per_second: None,
            error_summary: None,
            token_usage: Some(TokenUsage {
                input_tokens: 10,
                output_tokens: 20,
                cached_input_tokens: Some(5),
                cache_creation_input_tokens: None,
                cache_creation_input_tokens_5m: None,
                cache_creation_input_tokens_1h: None,
                reasoning_output_tokens: Some(3),
                audio_input_tokens: None,
                audio_output_tokens: None,
            }),
            billing_meter: BillingMeter::Token,
            billable_units: 30,
            billing: Some(charge.clone()),
        };

        let payload = serde_json::to_value(&usage).unwrap();
        let decoded: UsageInsert = serde_json::from_value(payload).unwrap();

        assert_eq!(
            decoded.billing.unwrap().transaction_id,
            charge.transaction_id
        );
        assert_eq!(decoded.provider, "openai");
        assert_eq!(decoded.channel_key_id, Some(4));
    }

    #[test]
    fn usage_flush_buffer_is_bounded() {
        let mut usages = Vec::new();
        for id in 0..USAGE_BATCH_SIZE as DbId {
            push_bounded_usage(&mut usages, usage_insert(id));
        }

        push_bounded_usage(&mut usages, usage_insert(999));

        assert_eq!(usages.len(), USAGE_BATCH_SIZE);
        assert!(!usages.iter().any(|usage| usage.user_id == 999));
    }

    #[test]
    fn key_failure_flush_buffer_is_bounded_but_updates_existing_key() {
        let mut failures = HashMap::new();
        for id in 0..KEY_BATCH_SIZE as DbId {
            insert_bounded_failure(&mut failures, key_failure(id));
        }

        insert_bounded_failure(&mut failures, key_failure(999));
        insert_bounded_failure(
            &mut failures,
            KeyFailure {
                channel_key_id: 1,
                cooldown_until: Utc::now(),
                error: "newer error".to_string(),
            },
        );

        assert_eq!(failures.len(), KEY_BATCH_SIZE);
        assert!(!failures.contains_key(&999));
        assert_eq!(failures.get(&1).unwrap().error, "newer error");
    }

    #[test]
    fn activity_state_merge_is_bounded() {
        let mut state = ActivityState::default();
        for id in 0..ACTIVITY_MAX_PENDING_KEYS as DbId {
            state.users.insert(id);
        }
        let mut incoming = ActivityState::default();
        incoming.users.insert(999_999);

        state.merge_bounded(incoming);

        assert_eq!(state.users.len(), ACTIVITY_MAX_PENDING_KEYS);
        assert!(!state.users.contains(&999_999));
    }

    #[test]
    fn daily_usage_aggregate_merge_is_bounded_but_updates_existing_key() {
        let day = Utc::now().date_naive();
        let mut target = HashMap::new();
        for id in 0..DAILY_MAX_PENDING_AGGREGATES as DbId {
            let mut aggregate = DailyUsageAggregate::from_usage(day, &usage_insert(id));
            aggregate.request_count = 1;
            target.insert(aggregate.key.clone(), aggregate);
        }

        let mut delta = HashMap::new();
        let dropped = DailyUsageAggregate::from_usage(day, &usage_insert(999_999));
        delta.insert(dropped.key.clone(), dropped);
        let mut existing = DailyUsageAggregate::from_usage(day, &usage_insert(1));
        existing.request_count = 7;
        delta.insert(existing.key.clone(), existing);

        merge_bounded_daily_usage_aggregates(&mut target, delta);

        assert_eq!(target.len(), DAILY_MAX_PENDING_AGGREGATES);
        let existing_key = DailyUsageAggregate::from_usage(day, &usage_insert(1)).key;
        let dropped_key = DailyUsageAggregate::from_usage(day, &usage_insert(999_999)).key;
        assert_eq!(target.get(&existing_key).unwrap().request_count, 8);
        assert!(!target.contains_key(&dropped_key));
    }

    #[test]
    fn coalesces_debit_parts_by_credit_account_and_allocation() {
        let parts = vec![
            serde_json::from_value(serde_json::json!({
                "credit_account": { "id": 7 },
                "allocation_id": 101,
                "amount_micro_usd": 30,
                "generation": 1
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "credit_account": { "id": 7 },
                "allocation_id": 101,
                "amount_micro_usd": 20,
                "generation": 2
            }))
            .unwrap(),
            serde_json::from_value(serde_json::json!({
                "credit_account": { "id": 7 },
                "allocation_id": 102,
                "amount_micro_usd": 5,
                "generation": 3
            }))
            .unwrap(),
        ];

        let coalesced = coalesce_debit_parts(&parts);

        assert_eq!(coalesced.len(), 2);
        assert_eq!(coalesced[0].allocation_id, 101);
        assert_eq!(coalesced[0].amount_micro_usd, 50);
        assert_eq!(coalesced[1].allocation_id, 102);
        assert_eq!(coalesced[1].amount_micro_usd, 5);
    }
}
