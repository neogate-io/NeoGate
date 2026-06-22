use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};
use tokio::{
    sync::mpsc::{
        self,
        error::{TryRecvError, TrySendError},
        Receiver,
    },
    task::JoinHandle,
    time,
};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    usage::{self, ActivityRecorder, UsageDailyRecorder, UsageInsert},
};

const BILLING_BATCH_SIZE: i64 = 500;
const BILLING_PROCESS_CHUNK_SIZE: i64 = 500;
const BILLING_MAX_BATCHES_PER_TICK: usize = 40;
const BILLING_PROCESS_WORKERS: usize = 4;
const BILLING_MAX_ATTEMPTS: i32 = 10;
const BILLING_OUTBOX_WRITE_ATTEMPTS: u32 = 7;
const BILLING_OUTBOX_INITIAL_RETRY_DELAY: Duration = Duration::from_millis(50);
const BILLING_OUTBOX_BACKGROUND_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const BILLING_OUTBOX_BACKGROUND_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct BillingOutbox {
    pool: PgPool,
    health: BillingOutboxHealth,
    worker: WorkerSlot,
    retry_worker: WorkerSlot,
    activity: ActivityRecorder,
    daily: UsageDailyRecorder,
}

#[derive(Clone, Default)]
struct BillingOutboxHealth {
    failed_since: Arc<RwLock<Option<DateTime<Utc>>>>,
}

#[derive(Clone, Default)]
struct WorkerSlot {
    sender: Arc<RwLock<Option<mpsc::Sender<UsageInsert>>>>,
    handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BillingOutboxWriteStatus {
    pub healthy: bool,
    pub failed_since: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct BillingOutboxBacklogStatus {
    pub pending_count: i64,
    pub oldest_pending_age_seconds: i64,
}

struct PendingBillingRecord {
    id: DbId,
    transaction_id: Uuid,
    payload: serde_json::Value,
}

impl BillingOutbox {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            health: BillingOutboxHealth::default(),
            worker: WorkerSlot::default(),
            retry_worker: WorkerSlot::default(),
            activity: ActivityRecorder::disabled(),
            daily: UsageDailyRecorder::disabled(),
        }
    }

    pub fn spawn(
        pool: PgPool,
        flush_interval: Duration,
        queue_size: usize,
        activity: ActivityRecorder,
        daily: UsageDailyRecorder,
        process_outbox: bool,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(queue_size.max(1));
        let (retry_sender, retry_receiver) = mpsc::channel(queue_size.max(1));
        let outbox = Self {
            pool,
            health: BillingOutboxHealth::default(),
            worker: WorkerSlot::default(),
            retry_worker: WorkerSlot::default(),
            activity,
            daily,
        };
        let worker = outbox.clone();
        outbox.worker.set(
            sender,
            tokio::spawn(async move {
                worker.run_worker(receiver, flush_interval).await;
            }),
        );
        if process_outbox {
            for _ in 0..BILLING_PROCESS_WORKERS {
                let process_worker = outbox.clone();
                tokio::spawn(async move {
                    process_worker.run_process_worker(flush_interval).await;
                });
            }
        }
        let retry_worker = outbox.clone();
        outbox.retry_worker.set(
            retry_sender,
            tokio::spawn(async move {
                retry_worker.run_retry_worker(retry_receiver).await;
            }),
        );
        outbox
    }

    pub fn write_status(&self) -> BillingOutboxWriteStatus {
        self.health.status()
    }

    pub async fn enqueue(&self, usage: &UsageInsert) -> AppResult<()> {
        if usage.billing.is_none() {
            return Ok(());
        }

        match persist_billing_usage_with_retry(&self.pool, usage).await {
            Ok(()) => {
                self.health.record_success();
                Ok(())
            }
            Err(err) => {
                self.health.record_failure();
                Err(err)
            }
        }
    }

    fn enqueue_retry(&self, usage: UsageInsert) {
        if usage.billing.is_none() {
            return;
        }

        let sender = self.retry_worker.sender();
        let Some(sender) = sender else {
            self.health.record_failure();
            tracing::error!(
                "billing outbox retry queue is unavailable; dropping in-memory billing retry"
            );
            return;
        };

        match sender.try_send(usage) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.health.record_failure();
                tracing::error!(
                    "billing outbox retry queue is full; dropping in-memory billing retry"
                );
            }
            Err(TrySendError::Closed(_)) => {
                self.health.record_failure();
                tracing::error!(
                    "billing outbox retry queue is closed; dropping in-memory billing retry"
                );
            }
        }
    }

    pub fn enqueue_or_retry(&self, usage: UsageInsert) {
        if usage.billing.is_none() {
            return;
        }

        let sender = self.worker.sender();
        let Some(sender) = sender else {
            self.enqueue_retry(usage);
            return;
        };

        match sender.try_send(usage) {
            Ok(()) => {}
            Err(TrySendError::Full(usage)) => {
                self.health.record_failure();
                tracing::warn!("billing outbox queue is full; queueing bounded background retry");
                self.enqueue_retry(usage);
            }
            Err(TrySendError::Closed(usage)) => {
                self.health.record_failure();
                tracing::warn!("billing outbox queue is closed; queueing bounded background retry");
                self.enqueue_retry(usage);
            }
        }
    }

    pub async fn flush_pending(&self, timeout: Duration, process_outbox: bool) {
        let pool = self.pool.clone();
        let activity = self.activity.clone();
        let daily = self.daily.clone();
        let worker = self.worker.clone();
        let retry_worker = self.retry_worker.clone();
        match time::timeout(timeout, async move {
            worker.close_and_wait().await;
            retry_worker.close_and_wait().await;
            if process_outbox {
                drain_billing_outbox(&pool, &activity, &daily).await?;
            }
            Ok::<(), AppError>(())
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!("failed to flush billing outbox on shutdown: {err}"),
            Err(_) => tracing::warn!(
                timeout_ms = timeout.as_millis() as i64,
                "timed out flushing billing outbox on shutdown"
            ),
        }
    }

    async fn run_worker(self, mut receiver: Receiver<UsageInsert>, flush_interval: Duration) {
        let mut interval = time::interval(flush_interval.max(Duration::from_millis(1)));
        loop {
            tokio::select! {
                usage = receiver.recv() => {
                    let Some(usage) = usage else {
                        break;
                    };
                    let mut batch = vec![usage];
                    while batch.len() < BILLING_BATCH_SIZE as usize {
                        match receiver.try_recv() {
                            Ok(usage) => batch.push(usage),
                            Err(TryRecvError::Empty) => break,
                            Err(TryRecvError::Disconnected) => break,
                        }
                    }

                    match persist_billing_usages_with_retry(&self.pool, &batch).await {
                        Ok(()) => self.health.record_success(),
                        Err(err) => {
                            self.health.record_failure();
                            tracing::error!(
                                count = batch.len(),
                                "failed to persist relay billing usage batch from queue; queueing bounded background retry: {err}"
                            );
                            for usage in batch {
                                self.enqueue_retry(usage);
                            }
                        }
                    }
                }
                _ = interval.tick() => {}
            }
        }
    }

    async fn run_process_worker(self, flush_interval: Duration) {
        let mut interval = time::interval(flush_interval.max(Duration::from_millis(1)));
        loop {
            interval.tick().await;
            for _ in 0..BILLING_MAX_BATCHES_PER_TICK {
                match process_billing_outbox_batch(
                    &self.pool,
                    &self.activity,
                    &self.daily,
                    BILLING_BATCH_SIZE,
                )
                .await
                {
                    Ok(processed) if processed >= BILLING_BATCH_SIZE as u64 => {}
                    Ok(_) => break,
                    Err(err) => {
                        tracing::warn!("failed to process durable billing records: {err}");
                        break;
                    }
                }
            }
        }
    }

    async fn run_retry_worker(self, mut receiver: mpsc::Receiver<UsageInsert>) {
        while let Some(usage) = receiver.recv().await {
            let mut batch = vec![usage];
            while batch.len() < BILLING_BATCH_SIZE as usize {
                match receiver.try_recv() {
                    Ok(usage) => batch.push(usage),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => break,
                }
            }

            let mut delay = BILLING_OUTBOX_BACKGROUND_RETRY_INITIAL_DELAY;
            loop {
                match persist_billing_usages(&self.pool, &batch).await {
                    Ok(()) => {
                        self.health.record_success();
                        break;
                    }
                    Err(err) => {
                        self.health.record_failure();
                        tracing::error!(
                            count = batch.len(),
                            "failed to persist durable billing batch in bounded background retry: {err}"
                        );
                        time::sleep(delay).await;
                        delay = (delay * 2).min(BILLING_OUTBOX_BACKGROUND_RETRY_MAX_DELAY);
                    }
                }
            }
        }
    }
}

impl WorkerSlot {
    fn set(&self, sender: mpsc::Sender<UsageInsert>, handle: JoinHandle<()>) {
        *self.sender.write().expect("billing outbox sender poisoned") = Some(sender);
        *self
            .handle
            .write()
            .expect("billing outbox worker handle poisoned") = Some(handle);
    }

    fn sender(&self) -> Option<mpsc::Sender<UsageInsert>> {
        self.sender
            .read()
            .expect("billing outbox sender poisoned")
            .clone()
    }

    async fn close_and_wait(&self) {
        self.sender
            .write()
            .expect("billing outbox sender poisoned")
            .take();
        let handle = self
            .handle
            .write()
            .expect("billing outbox worker handle poisoned")
            .take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }
}

impl BillingOutboxHealth {
    fn record_success(&self) {
        *self
            .failed_since
            .write()
            .expect("billing outbox health poisoned") = None;
    }

    fn record_failure(&self) {
        let mut failed_since = self
            .failed_since
            .write()
            .expect("billing outbox health poisoned");
        if failed_since.is_none() {
            *failed_since = Some(Utc::now());
        }
    }

    fn status(&self) -> BillingOutboxWriteStatus {
        let failed_since = *self
            .failed_since
            .read()
            .expect("billing outbox health poisoned");
        BillingOutboxWriteStatus {
            healthy: failed_since.is_none(),
            failed_since,
        }
    }
}

pub async fn backlog_status(
    pool: &PgPool,
    max_pending: i64,
) -> AppResult<BillingOutboxBacklogStatus> {
    let sample_limit = max_pending.saturating_add(1).max(1);
    let row = sqlx::query(
        "WITH pending AS (
             SELECT created_at
             FROM billing
             WHERE status IN ('pending', 'failed')
             ORDER BY created_at ASC
             LIMIT $1
         )
         SELECT COUNT(*)::BIGINT AS pending_count,
                COALESCE(EXTRACT(EPOCH FROM (now() - MIN(created_at)))::BIGINT, 0)
                    AS oldest_pending_age_seconds
         FROM pending",
    )
    .bind(sample_limit)
    .fetch_one(pool)
    .await?;

    Ok(BillingOutboxBacklogStatus {
        pending_count: row.try_get("pending_count")?,
        oldest_pending_age_seconds: row.try_get("oldest_pending_age_seconds")?,
    })
}

async fn persist_billing_usage_with_retry(pool: &PgPool, usage: &UsageInsert) -> AppResult<()> {
    let mut delay = BILLING_OUTBOX_INITIAL_RETRY_DELAY;
    for attempt in 1..=BILLING_OUTBOX_WRITE_ATTEMPTS {
        match persist_billing_usage(pool, usage).await {
            Ok(()) => return Ok(()),
            Err(err) if attempt == BILLING_OUTBOX_WRITE_ATTEMPTS => return Err(err),
            Err(err) => {
                tracing::warn!(
                    attempt,
                    "failed to persist durable billing record; retrying: {err}"
                );
                time::sleep(delay).await;
                delay *= 2;
            }
        }
    }
    Ok(())
}

async fn persist_billing_usages_with_retry(pool: &PgPool, usages: &[UsageInsert]) -> AppResult<()> {
    let mut delay = BILLING_OUTBOX_INITIAL_RETRY_DELAY;
    for attempt in 1..=BILLING_OUTBOX_WRITE_ATTEMPTS {
        match persist_billing_usages(pool, usages).await {
            Ok(()) => return Ok(()),
            Err(err) if attempt == BILLING_OUTBOX_WRITE_ATTEMPTS => return Err(err),
            Err(err) => {
                tracing::warn!(
                    attempt,
                    count = usages.len(),
                    "failed to persist durable billing record batch; retrying: {err}"
                );
                time::sleep(delay).await;
                delay *= 2;
            }
        }
    }
    Ok(())
}

async fn persist_billing_usage(pool: &PgPool, usage: &UsageInsert) -> AppResult<()> {
    let Some(billing) = &usage.billing else {
        return Ok(());
    };
    let payload = serde_json::to_value(usage)?;

    sqlx::query(
        "INSERT INTO billing
         (transaction_id, payload)
         VALUES ($1, $2)
         ON CONFLICT (transaction_id) DO NOTHING",
    )
    .bind(billing.transaction_id)
    .bind(payload)
    .execute(pool)
    .await?;

    Ok(())
}

async fn persist_billing_usages(pool: &PgPool, usages: &[UsageInsert]) -> AppResult<()> {
    let mut rows = Vec::with_capacity(usages.len());
    for usage in usages {
        let Some(billing) = &usage.billing else {
            continue;
        };
        rows.push((billing.transaction_id, serde_json::to_value(usage)?));
    }
    if rows.is_empty() {
        return Ok(());
    }

    let mut query_builder = sqlx::QueryBuilder::new(
        "INSERT INTO billing
         (transaction_id, payload) ",
    );
    query_builder.push_values(rows, |mut row, (transaction_id, payload)| {
        row.push_bind(transaction_id).push_bind(payload);
    });
    query_builder.push(" ON CONFLICT (transaction_id) DO NOTHING");
    query_builder.build().execute(pool).await?;
    Ok(())
}

async fn process_billing_outbox_batch(
    pool: &PgPool,
    activity: &ActivityRecorder,
    daily: &UsageDailyRecorder,
    limit: i64,
) -> AppResult<u64> {
    let mut processed = 0;
    let mut remaining = limit.max(0);
    let mut selected_ids = Vec::new();
    while remaining > 0 {
        let chunk_limit = remaining.min(BILLING_PROCESS_CHUNK_SIZE);
        let result =
            process_billing_outbox_chunk(pool, activity, daily, chunk_limit, &selected_ids).await?;
        processed += result.processed;
        remaining -= result.selected as i64;
        selected_ids.extend(result.selected_ids);
        if result.selected < chunk_limit as u64 {
            break;
        }
    }
    Ok(processed)
}

async fn drain_billing_outbox(
    pool: &PgPool,
    activity: &ActivityRecorder,
    daily: &UsageDailyRecorder,
) -> AppResult<u64> {
    let mut total = 0;
    loop {
        let processed =
            process_billing_outbox_batch(pool, activity, daily, BILLING_BATCH_SIZE).await?;
        total += processed;
        if processed < BILLING_BATCH_SIZE as u64 {
            break;
        }
    }
    Ok(total)
}

struct BillingOutboxChunkResult {
    selected: u64,
    selected_ids: Vec<DbId>,
    processed: u64,
}

async fn process_billing_outbox_chunk(
    pool: &PgPool,
    activity: &ActivityRecorder,
    daily: &UsageDailyRecorder,
    limit: i64,
    excluded_ids: &[DbId],
) -> AppResult<BillingOutboxChunkResult> {
    let mut tx = pool.begin().await?;
    let records = fetch_pending_billing_records(&mut tx, limit, excluded_ids).await?;
    if records.is_empty() {
        tx.commit().await?;
        return Ok(BillingOutboxChunkResult {
            selected: 0,
            selected_ids: Vec::new(),
            processed: 0,
        });
    }

    let selected = records.len() as u64;
    let selected_ids = records.iter().map(|record| record.id).collect::<Vec<_>>();
    let mut processed_usages = Vec::with_capacity(records.len());
    for record in &records {
        match usage_from_billing_payload(record.transaction_id, record.payload.clone()) {
            Ok(usage) => processed_usages.push(usage),
            Err(err) => {
                let failed_id = record.id;
                let _ = tx.rollback().await;
                tracing::warn!(
                    billing_id = %failed_id,
                    selected,
                    "failed to decode billing chunk; retrying selected records individually: {err}"
                );
                let processed =
                    process_billing_records_individually(pool, activity, daily, records).await?;
                return Ok(BillingOutboxChunkResult {
                    selected,
                    selected_ids,
                    processed,
                });
            }
        }
    }

    if let Err(err) = async {
        usage::flush_usage(&mut tx, &processed_usages).await?;
        mark_billing_records_processed(&mut tx, &selected_ids).await?;
        Ok::<(), AppError>(())
    }
    .await
    {
        let _ = tx.rollback().await;
        tracing::warn!(
            selected,
            "failed to process billing chunk; retrying selected records individually: {err}"
        );
        let processed =
            process_billing_records_individually(pool, activity, daily, records).await?;
        return Ok(BillingOutboxChunkResult {
            selected,
            selected_ids,
            processed,
        });
    }

    tx.commit().await?;
    activity.record(&processed_usages);
    daily.record(&processed_usages);
    Ok(BillingOutboxChunkResult {
        selected,
        selected_ids,
        processed: processed_usages.len() as u64,
    })
}

async fn fetch_pending_billing_records(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    limit: i64,
    excluded_ids: &[DbId],
) -> AppResult<Vec<PendingBillingRecord>> {
    let rows = sqlx::query(
        "SELECT id, transaction_id, payload
         FROM billing
         WHERE status IN ('pending', 'failed')
           AND NOT (id = ANY($2::BIGINT[]))
         ORDER BY attempts ASC, created_at ASC
         LIMIT $1
         FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .bind(excluded_ids)
    .fetch_all(&mut **tx)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(PendingBillingRecord {
                id: row.try_get("id")?,
                transaction_id: row.try_get("transaction_id")?,
                payload: row.try_get("payload")?,
            })
        })
        .collect()
}

async fn process_billing_records_individually(
    pool: &PgPool,
    activity: &ActivityRecorder,
    daily: &UsageDailyRecorder,
    records: Vec<PendingBillingRecord>,
) -> AppResult<u64> {
    let mut processed = 0;
    let mut processed_usages = Vec::new();
    for record in records {
        if let Some(usage) = process_billing_outbox_record(pool, record.id).await? {
            processed += 1;
            processed_usages.push(usage);
        }
    }
    activity.record(&processed_usages);
    daily.record(&processed_usages);
    Ok(processed)
}

async fn process_billing_outbox_record(pool: &PgPool, id: DbId) -> AppResult<Option<UsageInsert>> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        "SELECT id, transaction_id, payload
         FROM billing
         WHERE id = $1 AND status IN ('pending', 'failed')
         FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };
    let record = PendingBillingRecord {
        id: row.try_get("id")?,
        transaction_id: row.try_get("transaction_id")?,
        payload: row.try_get("payload")?,
    };

    match process_billing_payload(&mut tx, record.id, record.transaction_id, record.payload).await {
        Ok(usage) => {
            tx.commit().await?;
            Ok(Some(usage))
        }
        Err(err) => {
            let _ = tx.rollback().await;
            tracing::warn!(billing_id = %record.id, "failed to process billing record: {err}");
            if let Err(record_err) = record_billing_failure(pool, record.id, &err).await {
                tracing::warn!(
                    billing_id = %record.id,
                    "failed to record billing processing failure: {record_err}"
                );
                return Err(record_err);
            }
            Ok(None)
        }
    }
}

fn usage_from_billing_payload(
    transaction_id: Uuid,
    payload: serde_json::Value,
) -> AppResult<UsageInsert> {
    let usage: UsageInsert = serde_json::from_value(payload)?;
    let billing = usage
        .billing
        .as_ref()
        .ok_or_else(|| AppError::BadRequest("billing payload is missing charge".to_string()))?;
    if billing.transaction_id != transaction_id {
        return Err(AppError::BadRequest(
            "billing payload transaction id does not match row transaction id".to_string(),
        ));
    }

    Ok(usage)
}

async fn mark_billing_records_processed(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: &[DbId],
) -> AppResult<()> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE billing
         SET status = 'processed',
             processed_at = now(),
             last_error = NULL
         WHERE id = ANY($1) AND status IN ('pending', 'failed')",
    )
    .bind(ids)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn process_billing_payload(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: DbId,
    transaction_id: Uuid,
    payload: serde_json::Value,
) -> AppResult<UsageInsert> {
    let usage = usage_from_billing_payload(transaction_id, payload)?;
    usage::flush_usage(tx, std::slice::from_ref(&usage)).await?;

    sqlx::query(
        "UPDATE billing
         SET status = 'processed',
             processed_at = now(),
             last_error = NULL
         WHERE id = $1 AND status IN ('pending', 'failed')",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(usage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::{BillingCharge, BillingMeter};

    fn usage_with_billing(id: DbId) -> UsageInsert {
        UsageInsert {
            user_id: id,
            project_id: id,
            user_key_id: id,
            channel_id: id,
            channel_key_id: None,
            credential_id: None,
            relay_trace_id: None,
            relay_attempt: 1,
            relay_final: true,
            provider: "openai".to_string(),
            model: Some("gpt-4.1".to_string()),
            status_code: Some(200),
            streamed: false,
            latency_ms: 1,
            first_response_ms: None,
            output_tokens_per_second: None,
            error_summary: None,
            token_usage: None,
            billing_meter: BillingMeter::Token,
            billable_units: 0,
            billing: Some(BillingCharge {
                transaction_id: Uuid::new_v4(),
                input_tokens: None,
                output_tokens: None,
                total_tokens: None,
                billing_meter: BillingMeter::Token,
                billable_units: 0,
                cost_micro_usd: 0,
                status: "billed".to_string(),
                parts: Vec::new(),
                returned_parts: Vec::new(),
            }),
        }
    }

    #[tokio::test]
    async fn new_outbox_without_workers_drops_to_retry_without_panicking() {
        let pool = PgPool::connect_lazy("postgres://neogate:neogate@localhost/neogate").unwrap();
        let outbox = BillingOutbox::new(pool);
        outbox.enqueue_or_retry(usage_with_billing(1));

        assert!(outbox
            .worker
            .sender
            .read()
            .expect("billing outbox sender poisoned")
            .is_none());
    }
}

async fn record_billing_failure(pool: &PgPool, id: DbId, err: &AppError) -> AppResult<()> {
    sqlx::query(
        "UPDATE billing
         SET attempts = attempts + 1,
             status = CASE
                 WHEN attempts + 1 >= $2 THEN 'failed'
                 ELSE status
             END,
             last_error = $3
         WHERE id = $1 AND status IN ('pending', 'failed')",
    )
    .bind(id)
    .bind(BILLING_MAX_ATTEMPTS)
    .bind(err.to_string().chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}
