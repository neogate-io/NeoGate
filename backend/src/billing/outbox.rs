use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row};
use tokio::{
    sync::mpsc::{self, error::TryRecvError, Receiver},
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
/// 后台 retry worker 的最大重试次数。超过后将整批记录逐条提交至 billing outbox
/// 表（process worker 负责处理），避免单条损坏记录永久阻塞整个 retry batch。
const BILLING_RETRY_WORKER_MAX_ATTEMPTS: u32 = 20;

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
    /// Dead-lettered records that exhausted all retry attempts. Reported for
    /// observability only — they are not retried and must NOT gate readiness,
    /// otherwise stale data rows can permanently block service startup.
    pub failed_count: i64,
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
                // 用监督壳包裹：worker panic 后记录 error 并自动重启，而非静默退出。
                tokio::spawn(async move {
                    loop {
                        let worker = process_worker.clone();
                        let result = tokio::spawn(async move {
                            worker.run_process_worker(flush_interval).await;
                        })
                        .await;
                        match result {
                            Ok(()) => break, // 正常关机（channel 关闭等），不重启
                            Err(err) if err.is_panic() => {
                                tracing::error!(
                                    "billing process worker panicked; restarting after delay: {err:?}"
                                );
                                time::sleep(Duration::from_millis(200)).await;
                            }
                            Err(err) => {
                                tracing::warn!("billing process worker exited: {err:?}");
                                break;
                            }
                        }
                    }
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

    /// Atomically persists an async task's billing intent and transitions the
    /// task out of `held`. A crash can therefore leave either both operations
    /// committed or neither operation committed.
    pub async fn enqueue_task_durable(
        &self,
        task_id: DbId,
        usage: &UsageInsert,
    ) -> AppResult<bool> {
        let Some(billing) = &usage.billing else {
            return Ok(false);
        };
        let payload = serde_json::to_value(usage)?;
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO billing (transaction_id, payload)
             VALUES ($1, $2)
             ON CONFLICT (transaction_id) DO NOTHING
             RETURNING id",
        )
        .bind(billing.transaction_id)
        .bind(&payload)
        .fetch_optional(&mut *tx)
        .await?;
        if inserted.is_none() {
            let existing_payload: serde_json::Value = sqlx::query_scalar(
                "SELECT payload
                 FROM billing
                 WHERE transaction_id = $1
                 FOR UPDATE",
            )
            .bind(billing.transaction_id)
            .fetch_one(&mut *tx)
            .await?;
            if existing_payload != payload {
                tx.rollback().await?;
                return Err(AppError::Conflict(
                    "billing transaction payload does not match the existing record".to_string(),
                ));
            }
        }
        let updated = sqlx::query(
            "UPDATE task_upstream
             SET billing_status = 'settled', updated_at = now()
             WHERE id = $1 AND billing_status = 'held'
             RETURNING billing_hold",
        )
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = updated {
            let hold_value: Option<serde_json::Value> = row.try_get("billing_hold")?;
            let hold = hold_value
                .map(serde_json::from_value::<crate::billing::DebitHold>)
                .transpose()?
                .ok_or_else(|| {
                    AppError::Conflict("async task billing hold is missing".to_string())
                })?;
            if hold.transaction_id != billing.transaction_id {
                tx.rollback().await?;
                return Err(AppError::Conflict(
                    "async task hold transaction does not match billing transaction".to_string(),
                ));
            }
        } else {
            let status: Option<String> = sqlx::query_scalar(
                "SELECT billing_status
                 FROM task_upstream
                 WHERE id = $1
                 FOR UPDATE",
            )
            .bind(task_id)
            .fetch_optional(&mut *tx)
            .await?;
            if status.as_deref() == Some("settled") {
                tx.commit().await?;
                self.health.record_success();
                return Ok(false);
            }
            tx.rollback().await?;
            return Err(match status {
                Some(status) => AppError::Conflict(format!(
                    "async task billing status is {status}; expected held or settled"
                )),
                None => AppError::NotFound,
            });
        }
        tx.commit().await?;
        self.health.record_success();
        Ok(true)
    }

    async fn enqueue_retry(&self, usage: UsageInsert) -> AppResult<()> {
        if usage.billing.is_none() {
            return Ok(());
        }

        let sender = self.retry_worker.sender();
        let Some(sender) = sender else {
            self.health.record_failure();
            return Err(AppError::UpstreamUnavailable(
                "billing outbox retry queue is unavailable".to_string(),
            ));
        };
        sender.send(usage).await.map_err(|_| {
            self.health.record_failure();
            AppError::UpstreamUnavailable("billing outbox retry queue is closed".to_string())
        })
    }

    pub async fn enqueue_or_retry(&self, usage: UsageInsert) -> AppResult<()> {
        if usage.billing.is_none() {
            return Ok(());
        }

        let sender = self.worker.sender();
        let Some(sender) = sender else {
            return self.enqueue_retry(usage).await;
        };
        sender.send(usage).await.map_err(|_| {
            self.health.record_failure();
            AppError::UpstreamUnavailable("billing outbox queue is closed".to_string())
        })
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
                    let batch = drain_batch_from_receiver(&mut receiver, usage, BILLING_BATCH_SIZE as usize);

                    match persist_billing_usages_with_retry(&self.pool, &batch).await {
                        Ok(()) => self.health.record_success(),
                        Err(err) => {
                            self.health.record_failure();
                            tracing::error!(
                                count = batch.len(),
                                "failed to persist relay billing usage batch from queue; queueing bounded background retry: {err}"
                            );
                            for usage in batch {
                                if let Err(err) = self.enqueue_retry(usage).await {
                                    tracing::error!("failed to enqueue durable billing retry: {err}");
                                }
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
            let batch =
                drain_batch_from_receiver(&mut receiver, usage, BILLING_BATCH_SIZE as usize);

            let mut delay = BILLING_OUTBOX_BACKGROUND_RETRY_INITIAL_DELAY;
            let mut attempt = 0u32;
            loop {
                match persist_billing_usages(&self.pool, &batch).await {
                    Ok(()) => {
                        self.health.record_success();
                        break;
                    }
                    Err(err) => {
                        self.health.record_failure();
                        attempt += 1;
                        if attempt >= BILLING_RETRY_WORKER_MAX_ATTEMPTS {
                            // 超过最大重试次数后逐条写入 billing outbox 表，由
                            // process worker 再次处理，避免损坏记录永久阻塞整个 batch
                            tracing::error!(
                                count = batch.len(),
                                attempt,
                                "billing retry batch exceeded max attempts; falling back to per-record persist: {err}"
                            );
                            for item in &batch {
                                if let Err(fb_err) =
                                    persist_billing_usage_with_retry(&self.pool, item).await
                                {
                                    tracing::error!(
                                        "failed to fall back per-record billing persist: {fb_err}"
                                    );
                                }
                            }
                            break;
                        }
                        tracing::error!(
                            count = batch.len(),
                            attempt,
                            max_attempts = BILLING_RETRY_WORKER_MAX_ATTEMPTS,
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
        *self.sender.write().unwrap_or_else(|e| e.into_inner()) = Some(sender);
        *self.handle.write().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    }

    fn sender(&self) -> Option<mpsc::Sender<UsageInsert>> {
        self.sender
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    async fn close_and_wait(&self) {
        self.sender
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        let handle = self
            .handle
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(handle) = handle {
            let _ = handle.await;
        }
    }
}

impl BillingOutboxHealth {
    fn record_success(&self) {
        *self.failed_since.write().unwrap_or_else(|e| e.into_inner()) = None;
    }

    fn record_failure(&self) {
        let mut failed_since = self.failed_since.write().unwrap_or_else(|e| e.into_inner());
        if failed_since.is_none() {
            *failed_since = Some(Utc::now());
        }
    }

    fn status(&self) -> BillingOutboxWriteStatus {
        let failed_since = *self.failed_since.read().unwrap_or_else(|e| e.into_inner());
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
        // Only `pending` rows gate readiness — the drain worker retries them, so
        // an aging pending backlog signals "the drain can't keep up". `failed`
        // rows are dead-lettered (never retried) and are counted separately for
        // observability; they must not block startup on stale data.
        "WITH pending AS (
             SELECT created_at
             FROM billing
             WHERE status = 'pending'
             ORDER BY created_at ASC
             LIMIT $1
         )
         SELECT COUNT(*)::BIGINT AS pending_count,
                COALESCE(EXTRACT(EPOCH FROM (now() - MIN(created_at)))::BIGINT, 0)
                    AS oldest_pending_age_seconds,
                (SELECT COUNT(*) FROM (
                    SELECT 1 FROM billing WHERE status = 'failed' LIMIT $1
                ) f)::BIGINT AS failed_count
         FROM pending",
    )
    .bind(sample_limit)
    .fetch_one(pool)
    .await?;

    Ok(BillingOutboxBacklogStatus {
        pending_count: row.try_get("pending_count")?,
        oldest_pending_age_seconds: row.try_get("oldest_pending_age_seconds")?,
        failed_count: row.try_get("failed_count")?,
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

/// run_worker / run_retry_worker 共用的批量收集辅助：先放入第一条，再尽量从 channel 排空，
/// 上限 `max_size`。消除两个函数原来的重复 while + try_recv 逻辑。
fn drain_batch_from_receiver<T>(
    receiver: &mut mpsc::Receiver<T>,
    first: T,
    max_size: usize,
) -> Vec<T> {
    let mut batch = vec![first];
    while batch.len() < max_size {
        match receiver.try_recv() {
            Ok(item) => batch.push(item),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
        }
    }
    batch
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
    // 不再跨 chunk 累积 selected_ids：
    // - 成功处理的记录已被标记为 'processed'，被 WHERE status='pending' 自然过滤；
    // - FOR UPDATE SKIP LOCKED 防止并发 worker 重复获取；
    // - 失败记录的 attempts 递增，ORDER BY attempts ASC 让它们排在后面。
    // 原来的累积方案在大批量时会产生最多 40×500=20000 条的 IN 子句，显著增加 PG 开销。
    while remaining > 0 {
        let chunk_limit = remaining.min(BILLING_PROCESS_CHUNK_SIZE);
        let result = process_billing_outbox_chunk(pool, activity, daily, chunk_limit, &[]).await?;
        processed += result.processed;
        remaining -= result.selected as i64;
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
                if let Err(rb_err) = tx.rollback().await {
                    tracing::warn!(billing_id = %failed_id, "failed to rollback billing transaction: {rb_err}");
                }
                tracing::warn!(
                    billing_id = %failed_id,
                    selected,
                    "failed to decode billing chunk; retrying selected records individually: {err}"
                );
                let processed =
                    process_billing_records_individually(pool, activity, daily, records).await?;
                return Ok(BillingOutboxChunkResult {
                    selected,
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
        if let Err(rb_err) = tx.rollback().await {
            tracing::warn!(selected, "failed to rollback billing transaction: {rb_err}");
        }
        tracing::warn!(
            selected,
            "failed to process billing chunk; retrying selected records individually: {err}"
        );
        let processed =
            process_billing_records_individually(pool, activity, daily, records).await?;
        return Ok(BillingOutboxChunkResult {
            selected,
            processed,
        });
    }

    tx.commit().await?;
    activity.record(&processed_usages);
    daily.record(&processed_usages);
    Ok(BillingOutboxChunkResult {
        selected,
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
         WHERE status = 'pending'
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
         WHERE id = $1 AND status = 'pending'
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
         WHERE id = ANY($1) AND status = 'pending'",
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
         WHERE id = $1 AND status IN ('pending', 'failed')",
    )
    .bind(id)
    .bind(BILLING_MAX_ATTEMPTS)
    .bind(err.to_string().chars().take(500).collect::<String>())
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::billing::{BillingCharge, BillingChargeStatus, BillingMeter};

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
            model: Some("gpt-4.1".to_string()),
            upstream_model: Some("gpt-4.1".to_string()),
            routing_phase: "relay".to_string(),
            routing: None,
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
                cost_micros: 0,
                status: BillingChargeStatus::Billed,
                parts: Vec::new(),
                returned_parts: Vec::new(),
            }),
        }
    }

    #[tokio::test]
    async fn new_outbox_without_workers_reports_unavailable_queue() {
        let pool = PgPool::connect_lazy("postgres://neogate:neogate@localhost/neogate").unwrap();
        let outbox = BillingOutbox::new(pool);
        assert!(outbox
            .enqueue_or_retry(usage_with_billing(1))
            .await
            .is_err());

        assert!(outbox
            .worker
            .sender
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_none());
    }
}
