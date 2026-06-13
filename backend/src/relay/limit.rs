use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use crate::{
    error::{AppError, AppResult},
    id::DbId,
};

#[derive(Clone)]
pub(crate) struct ImageSyncLimiter {
    global: Arc<Semaphore>,
    per_key: Arc<Mutex<HashMap<DbId, Arc<Semaphore>>>>,
    key_limit: usize,
    new_keys_since_prune: Arc<AtomicUsize>,
}

pub(crate) struct ImageSyncPermit {
    _global: OwnedSemaphorePermit,
    _key: OwnedSemaphorePermit,
}

impl ImageSyncLimiter {
    pub(crate) fn new(global_limit: usize, key_limit: usize) -> Self {
        Self {
            global: Arc::new(Semaphore::new(global_limit)),
            per_key: Arc::new(Mutex::new(HashMap::new())),
            key_limit,
            new_keys_since_prune: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) async fn try_acquire(&self, user_key_id: DbId) -> AppResult<ImageSyncPermit> {
        let global = self.global.clone().try_acquire_owned().map_err(|_| {
            AppError::RateLimited(
                "too many concurrent synchronous image requests; use /v1/responses with background=true for image generation".to_string(),
            )
        })?;
        let key_semaphore = {
            let mut per_key = self.per_key.lock().await;
            if let Some(semaphore) = per_key.get(&user_key_id) {
                semaphore.clone()
            } else {
                if self.new_keys_since_prune.fetch_add(1, Ordering::Relaxed)
                    >= IMAGE_SYNC_KEY_PRUNE_NEW_KEYS_INTERVAL
                {
                    self.new_keys_since_prune.store(0, Ordering::Relaxed);
                    prune_idle_key_limiters(&mut per_key, self.key_limit);
                }
                let semaphore = Arc::new(Semaphore::new(self.key_limit));
                per_key.insert(user_key_id, semaphore.clone());
                semaphore
            }
        };
        let key = key_semaphore.try_acquire_owned().map_err(|_| {
            AppError::RateLimited(
                "too many concurrent synchronous image requests for this API key; use /v1/responses with background=true for image generation".to_string(),
            )
        })?;
        Ok(ImageSyncPermit {
            _global: global,
            _key: key,
        })
    }
}

const IMAGE_SYNC_KEY_PRUNE_NEW_KEYS_INTERVAL: usize = 1024;

fn prune_idle_key_limiters(per_key: &mut HashMap<DbId, Arc<Semaphore>>, key_limit: usize) {
    per_key.retain(|_, semaphore| {
        Arc::strong_count(semaphore) > 1 || semaphore.available_permits() < key_limit
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ImageSyncLimiter {
        async fn per_key_len(&self) -> usize {
            self.per_key.lock().await.len()
        }
    }

    #[tokio::test]
    async fn prunes_idle_key_limiters_after_key_churn() {
        let limiter = ImageSyncLimiter::new(8, 1);

        for user_key_id in 1..=(IMAGE_SYNC_KEY_PRUNE_NEW_KEYS_INTERVAL as DbId + 1) {
            let permit = limiter.try_acquire(user_key_id).await.unwrap();
            drop(permit);
        }

        assert!(limiter.per_key_len().await <= 2);
    }

    #[tokio::test]
    async fn keeps_active_key_limiters_while_pruning() {
        let limiter = ImageSyncLimiter::new(8, 1);
        let active = limiter.try_acquire(1).await.unwrap();

        for user_key_id in 2..=(IMAGE_SYNC_KEY_PRUNE_NEW_KEYS_INTERVAL as DbId + 2) {
            let permit = limiter.try_acquire(user_key_id).await.unwrap();
            drop(permit);
        }

        assert!(limiter.per_key_len().await >= 1);
        drop(active);
    }
}
