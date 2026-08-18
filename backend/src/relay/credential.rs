use std::{collections::HashMap, time::Duration};

use chrono::{DateTime, Utc};
use sqlx::PgPool;
use tokio::{
    sync::mpsc::{self, error::TrySendError},
    time,
};

use crate::id::DbId;

const CREDENTIAL_MODEL_BATCH_SIZE: usize = 200;

#[derive(Clone)]
pub struct CredentialModelRecorder {
    sender: Option<mpsc::Sender<CredentialModelUpdate>>,
    pool: Option<PgPool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CredentialModelKey {
    credential_id: DbId,
    channel_endpoint_id: DbId,
    model: String,
}

#[derive(Debug, Clone)]
enum CredentialModelUpdate {
    Available {
        key: CredentialModelKey,
    },
    Unavailable {
        key: CredentialModelKey,
        unavailable_until: DateTime<Utc>,
        last_error: String,
        last_status_code: i32,
    },
}

#[derive(Debug, Clone)]
struct PendingCredentialModelUpdate {
    key: CredentialModelKey,
    status: PendingCredentialModelStatus,
    success_count: i64,
    failure_count: i64,
}

#[derive(Debug, Clone)]
enum PendingCredentialModelStatus {
    Available,
    Unavailable {
        unavailable_until: DateTime<Utc>,
        last_error: String,
        last_status_code: i32,
    },
}

impl CredentialModelRecorder {
    pub fn spawn(pool: PgPool, flush_interval: Duration, queue_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(queue_size.max(1));
        tokio::spawn(run_worker(pool.clone(), receiver, flush_interval));
        Self {
            sender: Some(sender),
            pool: Some(pool),
        }
    }

    pub fn disabled() -> Self {
        Self {
            sender: None,
            pool: None,
        }
    }

    pub fn record_available(&self, credential_id: DbId, channel_endpoint_id: DbId, model: &str) {
        let _ = self.enqueue(CredentialModelUpdate::Available {
            key: CredentialModelKey {
                credential_id,
                channel_endpoint_id,
                model: model.trim().to_string(),
            },
        });
    }

    pub async fn record_unavailable(
        &self,
        credential_id: DbId,
        channel_endpoint_id: DbId,
        model: &str,
        unavailable_until: DateTime<Utc>,
        last_error: &str,
        last_status_code: i32,
    ) {
        let update = CredentialModelUpdate::Unavailable {
            key: CredentialModelKey {
                credential_id,
                channel_endpoint_id,
                model: model.trim().to_string(),
            },
            unavailable_until,
            last_error: last_error.chars().take(500).collect(),
            last_status_code,
        };
        if let Err(update) = self.enqueue(update) {
            let Some(pool) = &self.pool else {
                return;
            };
            let update = pending_update(update);
            if let Err(err) = flush_one(pool, &update).await {
                tracing::warn!(
                    credential_id = update.key.credential_id,
                    channel_endpoint_id = update.key.channel_endpoint_id,
                    model = %update.key.model,
                    error = %err,
                    "failed to persist credential model unavailable state after queue failure"
                );
            }
        }
    }

    fn enqueue(&self, update: CredentialModelUpdate) -> Result<(), CredentialModelUpdate> {
        let Some(sender) = &self.sender else {
            return Err(update);
        };
        match sender.try_send(update) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(update)) => {
                tracing::warn!("credential model state queue is full; dropping state update");
                Err(update)
            }
            Err(TrySendError::Closed(update)) => {
                tracing::warn!("credential model state worker is closed; dropping state update");
                Err(update)
            }
        }
    }
}

async fn run_worker(
    pool: PgPool,
    mut receiver: mpsc::Receiver<CredentialModelUpdate>,
    flush_interval: Duration,
) {
    let mut interval = time::interval(flush_interval.max(Duration::from_millis(1)));
    let mut pending = HashMap::new();

    loop {
        tokio::select! {
            update = receiver.recv() => {
                match update {
                    Some(update) => {
                        merge_update(&mut pending, update);
                        if pending.len() >= CREDENTIAL_MODEL_BATCH_SIZE {
                            flush(&pool, &mut pending).await;
                        }
                    }
                    None => {
                        flush(&pool, &mut pending).await;
                        break;
                    }
                }
            }
            _ = interval.tick() => {
                flush(&pool, &mut pending).await;
            }
        }
    }
}

fn merge_update(
    pending: &mut HashMap<CredentialModelKey, PendingCredentialModelUpdate>,
    update: CredentialModelUpdate,
) {
    let merged = pending_update(update);
    let key = merged.key.clone();
    let entry = pending
        .entry(key.clone())
        .or_insert_with(|| PendingCredentialModelUpdate {
            key,
            status: PendingCredentialModelStatus::Available,
            success_count: 0,
            failure_count: 0,
        });
    entry.status = merged.status;
    entry.success_count += merged.success_count;
    entry.failure_count += merged.failure_count;
}

fn pending_update(update: CredentialModelUpdate) -> PendingCredentialModelUpdate {
    match update {
        CredentialModelUpdate::Available { key } => PendingCredentialModelUpdate {
            key,
            status: PendingCredentialModelStatus::Available,
            success_count: 1,
            failure_count: 0,
        },
        CredentialModelUpdate::Unavailable {
            key,
            unavailable_until,
            last_error,
            last_status_code,
        } => PendingCredentialModelUpdate {
            key,
            status: PendingCredentialModelStatus::Unavailable {
                unavailable_until,
                last_error,
                last_status_code,
            },
            success_count: 0,
            failure_count: 1,
        },
    }
}

async fn flush(
    pool: &PgPool,
    pending: &mut HashMap<CredentialModelKey, PendingCredentialModelUpdate>,
) {
    if pending.is_empty() {
        return;
    }

    let updates = std::mem::take(pending).into_values().collect::<Vec<_>>();

    // 按状态拆分成两批，分别做批量 upsert，避免原来逐条 N 次 round-trip。
    let (available, unavailable): (Vec<_>, Vec<_>) = updates
        .into_iter()
        .partition(|u| matches!(u.status, PendingCredentialModelStatus::Available));

    if !available.is_empty() {
        if let Err(err) = flush_available_batch(pool, &available).await {
            tracing::warn!(
                count = available.len(),
                error = %err,
                "failed to flush available credential model states"
            );
        }
    }
    if !unavailable.is_empty() {
        if let Err(err) = flush_unavailable_batch(pool, &unavailable).await {
            tracing::warn!(
                count = unavailable.len(),
                error = %err,
                "failed to flush unavailable credential model states"
            );
        }
    }
}

/// 批量 upsert 所有 Available 状态更新，单次 DB round-trip。
async fn flush_available_batch(
    pool: &PgPool,
    updates: &[PendingCredentialModelUpdate],
) -> sqlx::Result<()> {
    use sqlx::QueryBuilder;
    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "INSERT INTO credential_model
         (credential_id, channel_endpoint_id, model, status, unavailable_until,
          last_error, last_status_code, last_seen_at, success_count, failure_count)
         ",
    );
    qb.push_values(updates, |mut b, u| {
        b.push_bind(u.key.credential_id)
            .push_bind(u.key.channel_endpoint_id)
            .push_bind(&u.key.model)
            .push("'available'")
            .push("NULL")
            .push("NULL")
            .push("NULL")
            .push("now()")
            .push_bind(u.success_count)
            .push_bind(u.failure_count);
    });
    qb.push(
        " ON CONFLICT (credential_id, channel_endpoint_id, model)
          DO UPDATE SET
              status = 'available',
              unavailable_until = NULL,
              last_error = NULL,
              last_status_code = NULL,
              last_seen_at = now(),
              success_count = credential_model.success_count + EXCLUDED.success_count,
              failure_count = credential_model.failure_count + EXCLUDED.failure_count,
              updated_at = now()",
    );
    qb.build().execute(pool).await?;
    Ok(())
}

/// 批量 upsert 所有 Unavailable 状态更新，单次 DB round-trip。
async fn flush_unavailable_batch(
    pool: &PgPool,
    updates: &[PendingCredentialModelUpdate],
) -> sqlx::Result<()> {
    use sqlx::QueryBuilder;
    // unavailable 需要逐条处理不同的 unavailable_until/last_error/last_status_code，
    // 仍用 QueryBuilder 批量推送。
    let mut qb: QueryBuilder<sqlx::Postgres> = QueryBuilder::new(
        "INSERT INTO credential_model
         (credential_id, channel_endpoint_id, model, status, unavailable_until,
          last_error, last_status_code, last_seen_at, success_count, failure_count)
         ",
    );
    qb.push_values(updates, |mut b, u| {
        let (unavailable_until, last_error, last_status_code) = match &u.status {
            PendingCredentialModelStatus::Unavailable {
                unavailable_until,
                last_error,
                last_status_code,
            } => (*unavailable_until, last_error.as_str(), *last_status_code),
            PendingCredentialModelStatus::Available => {
                // 逻辑上不会走到这里（已按状态拆分）
                return;
            }
        };
        b.push_bind(u.key.credential_id)
            .push_bind(u.key.channel_endpoint_id)
            .push_bind(&u.key.model)
            .push("'unavailable'")
            .push_bind(unavailable_until)
            .push_bind(last_error)
            .push_bind(last_status_code)
            .push("now()")
            .push_bind(u.success_count)
            .push_bind(u.failure_count);
    });
    qb.push(
        " ON CONFLICT (credential_id, channel_endpoint_id, model)
          DO UPDATE SET
              status = 'unavailable',
              unavailable_until = EXCLUDED.unavailable_until,
              last_error = EXCLUDED.last_error,
              last_status_code = EXCLUDED.last_status_code,
              last_seen_at = now(),
              success_count = credential_model.success_count + EXCLUDED.success_count,
              failure_count = credential_model.failure_count + EXCLUDED.failure_count,
              updated_at = now()",
    );
    qb.build().execute(pool).await?;
    Ok(())
}

async fn flush_one(pool: &PgPool, update: &PendingCredentialModelUpdate) -> sqlx::Result<()> {
    match &update.status {
        PendingCredentialModelStatus::Available => {
            sqlx::query(
                "INSERT INTO credential_model
                 (credential_id, channel_endpoint_id, model, status, unavailable_until,
                  last_error, last_status_code, last_seen_at, success_count, failure_count)
                 VALUES ($1, $2, $3, 'available', NULL, NULL, NULL, now(), $4, $5)
                 ON CONFLICT (credential_id, channel_endpoint_id, model)
                 DO UPDATE SET
                     status = 'available',
                     unavailable_until = NULL,
                     last_error = NULL,
                     last_status_code = NULL,
                     last_seen_at = now(),
                     success_count = credential_model.success_count + EXCLUDED.success_count,
                     failure_count = credential_model.failure_count + EXCLUDED.failure_count,
                     updated_at = now()",
            )
            .bind(update.key.credential_id)
            .bind(update.key.channel_endpoint_id)
            .bind(&update.key.model)
            .bind(update.success_count)
            .bind(update.failure_count)
            .execute(pool)
            .await?;
        }
        PendingCredentialModelStatus::Unavailable {
            unavailable_until,
            last_error,
            last_status_code,
        } => {
            sqlx::query(
                "INSERT INTO credential_model
                 (credential_id, channel_endpoint_id, model, status, unavailable_until,
                  last_error, last_status_code, last_seen_at, success_count, failure_count)
                 VALUES ($1, $2, $3, 'unavailable', $4, $5, $6, now(), $7, $8)
                 ON CONFLICT (credential_id, channel_endpoint_id, model)
                 DO UPDATE SET
                     status = 'unavailable',
                     unavailable_until = EXCLUDED.unavailable_until,
                     last_error = EXCLUDED.last_error,
                     last_status_code = EXCLUDED.last_status_code,
                     last_seen_at = now(),
                     success_count = credential_model.success_count + EXCLUDED.success_count,
                     failure_count = credential_model.failure_count + EXCLUDED.failure_count,
                     updated_at = now()",
            )
            .bind(update.key.credential_id)
            .bind(update.key.channel_endpoint_id)
            .bind(&update.key.model)
            .bind(unavailable_until)
            .bind(last_error)
            .bind(last_status_code)
            .bind(update.success_count)
            .bind(update.failure_count)
            .execute(pool)
            .await?;
        }
    }
    Ok(())
}
