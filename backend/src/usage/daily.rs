use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use chrono::{NaiveDate, Utc};
use sqlx::{Postgres, QueryBuilder};

use crate::{billing::BillingMeter, error::AppResult, id::DbId};

use super::recorder::record::{input_tokens, output_tokens, total_tokens, usage_fields};
use super::UsageInsert;

const DAILY_FLUSH_MIN_INTERVAL: Duration = Duration::from_secs(5);
const DAILY_MAX_PENDING_AGGREGATES: usize = 10_000;

#[derive(Clone)]
pub struct UsageDailyRecorder {
    entries: Option<Arc<Mutex<HashMap<DailyUsageKey, DailyUsageAggregate>>>>,
}

impl UsageDailyRecorder {
    pub fn spawn(pool: sqlx::PgPool, flush_interval: Duration) -> Self {
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

async fn run_daily_worker(
    pool: sqlx::PgPool,
    entries: Arc<Mutex<HashMap<DailyUsageKey, DailyUsageAggregate>>>,
    flush_interval: Duration,
) {
    let mut interval = tokio::time::interval(flush_interval);
    loop {
        interval.tick().await;
        flush_daily_recorded_usage(&pool, &entries).await;
    }
}

async fn flush_daily_recorded_usage(
    pool: &sqlx::PgPool,
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

async fn flush_usage_daily_aggregates(
    tx: &mut sqlx::Transaction<'_, Postgres>,
    aggregates: &[DailyUsageAggregate],
) -> AppResult<()> {
    if aggregates.is_empty() {
        return Ok(());
    }

    let mut query_builder = QueryBuilder::<Postgres>::new(
        "INSERT INTO usage_daily
         (day, user_id, project_id, user_key_id, channel_id, channel_key_id, credential_id, model,
          request_count, success_count, error_count, streamed_count,
          latency_ms_total, first_response_ms_total, first_response_count,
          input_tokens, output_tokens, total_tokens, cache_in_tokens,
          cache_create_in_tokens, cache_create_5m_in_tokens,
          cache_create_1h_in_tokens, reason_out_tokens, audio_in_tokens,
          audio_out_tokens, billing_meter, billable_units, cost_micros)
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
            .push_bind(item.cost_micros);
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
              cost_micros = usage_daily.cost_micros + EXCLUDED.cost_micros,
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
    cost_micros: i64,
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
        let fields = usage_fields(item);
        let key = DailyUsageKey {
            day,
            user_id: item.user_id,
            project_id: item.project_id,
            user_key_id: item.user_key_id,
            channel_id: item.channel_id,
            channel_key_id: item.channel_key_id,
            credential_id: item.credential_id,
            model: item.model.clone().unwrap_or_default(),
            billing_meter: fields.billing_meter,
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
            billing_meter: fields.billing_meter,
            billable_units: fields.billable_units,
            cost_micros: fields.cost_micros.unwrap_or(0).max(0),
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
        self.cost_micros += other.cost_micros;
    }
}

fn usage_succeeded(item: &UsageInsert) -> bool {
    item.status_code.map_or_else(
        || item.error_summary.is_none(),
        |status| (200..400).contains(&status),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::recorder::tests::usage_insert;
    use super::*;

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
}
