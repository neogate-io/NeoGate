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
    },
    time,
};
use uuid::Uuid;

use crate::{
    error::{AppError, AppResult},
    id::DbId,
    usage::{self, ActivityRecorder, UsageDailyRecorder, UsageInsert},
};

const BILLING_BATCH_SIZE: i64 = 100;
const BILLING_MAX_BATCHES_PER_TICK: usize = 10;
const BILLING_MAX_ATTEMPTS: i32 = 10;
const BILLING_OUTBOX_WRITE_ATTEMPTS: u32 = 7;
const BILLING_OUTBOX_INITIAL_RETRY_DELAY: Duration = Duration::from_millis(50);
const BILLING_OUTBOX_BACKGROUND_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);
const BILLING_OUTBOX_BACKGROUND_RETRY_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct BillingOutbox {
    pool: PgPool,
    health: BillingOutboxHealth,
    sender: Option<mpsc::Sender<UsageInsert>>,
    retry_sender: Option<mpsc::Sender<UsageInsert>>,
    activity: ActivityRecorder,
    daily: UsageDailyRecorder,
}

#[derive(Clone, Default)]
struct BillingOutboxHealth {
    failed_since: Arc<RwLock<Option<DateTime<Utc>>>>,
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

impl BillingOutbox {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            health: BillingOutboxHealth::default(),
            sender: None,
            retry_sender: None,
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
            sender: Some(sender),
            retry_sender: Some(retry_sender),
            activity,
            daily,
        };
        let worker = outbox.clone();
        tokio::spawn(async move {
            worker
                .run_worker(receiver, flush_interval, process_outbox)
                .await;
        });
        let retry_worker = outbox.clone();
        tokio::spawn(async move {
            retry_worker.run_retry_worker(retry_receiver).await;
        });
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

        let Some(sender) = &self.retry_sender else {
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

        let Some(sender) = &self.sender else {
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

    async fn run_worker(
        self,
        mut receiver: mpsc::Receiver<UsageInsert>,
        flush_interval: Duration,
        process_outbox: bool,
    ) {
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
                _ = interval.tick() => {
                    if process_outbox {
                        for _ in 0..BILLING_MAX_BATCHES_PER_TICK {
                            match process_billing_outbox_batch(&self.pool, &self.activity, &self.daily, BILLING_BATCH_SIZE).await {
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
             WHERE status = 'pending'
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
    let mut tx = pool.begin().await?;
    let rows = sqlx::query(
        "SELECT id, transaction_id, payload
         FROM billing
         WHERE status = 'pending'
         ORDER BY created_at ASC
         LIMIT $1
         FOR UPDATE SKIP LOCKED",
    )
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;

    let mut processed = 0;
    let mut processed_usages = Vec::with_capacity(rows.len());
    for row in rows {
        let id: DbId = row.try_get("id")?;
        let transaction_id: Uuid = row.try_get("transaction_id")?;
        let payload: serde_json::Value = row.try_get("payload")?;
        match process_billing_payload(&mut tx, id, transaction_id, payload).await {
            Ok(usage) => processed_usages.push(usage),
            Err(err) => {
                let _ = tx.rollback().await;
                tracing::warn!(billing_id = %id, "failed to process billing record: {err}");
                if let Err(record_err) = record_billing_failure(pool, id, &err).await {
                    tracing::warn!(
                        billing_id = %id,
                        "failed to record billing processing failure: {record_err}"
                    );
                }
                return Err(err);
            }
        }
        processed += 1;
    }
    tx.commit().await?;
    activity.record(&processed_usages);
    daily.record(&processed_usages);
    Ok(processed)
}

async fn process_billing_payload(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: DbId,
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

    usage::flush_usage(tx, std::slice::from_ref(&usage)).await?;

    sqlx::query(
        "UPDATE billing
         SET status = 'processed',
             processed_at = now(),
             last_error = NULL
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(usage)
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
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(BILLING_MAX_ATTEMPTS)
    .bind(err.to_string().chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}
