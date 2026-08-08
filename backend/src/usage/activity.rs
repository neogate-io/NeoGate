use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use sqlx::{PgPool, Postgres};

use crate::{error::AppResult, id::DbId};

use super::UsageInsert;

const ACTIVITY_FLUSH_MIN_INTERVAL: Duration = Duration::from_secs(10);
const ACTIVITY_MAX_PENDING_KEYS: usize = 10_000;

#[derive(Clone)]
pub struct ActivityRecorder {
    entries: Option<Arc<Mutex<ActivityState>>>,
}

#[derive(Clone, Default)]
struct ActivityState {
    channel_keys: HashSet<DbId>,
    user_keys: HashSet<DbId>,
    users: HashSet<DbId>,
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

        let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
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

async fn run_activity_worker(
    pool: PgPool,
    entries: Arc<Mutex<ActivityState>>,
    flush_interval: Duration,
) {
    let mut interval = tokio::time::interval(flush_interval);
    loop {
        interval.tick().await;
        flush_recorded_activity(&pool, &entries).await;
    }
}

async fn flush_recorded_activity(pool: &PgPool, entries: &Arc<Mutex<ActivityState>>) {
    let pending = {
        let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
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
        let mut entries = entries.lock().unwrap_or_else(|e| e.into_inner());
        entries.merge_bounded(pending);
    }
}

async fn flush_key_usage(
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

fn insert_bounded_id(values: &mut HashSet<DbId>, value: DbId, limit: usize, label: &'static str) {
    if values.contains(&value) || values.len() < limit {
        values.insert(value);
        return;
    }
    tracing::warn!(limit, "usage {label} is full; dropping aggregate key");
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
