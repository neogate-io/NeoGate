use std::{collections::HashMap, sync::Arc};

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
            per_key
                .entry(user_key_id)
                .or_insert_with(|| Arc::new(Semaphore::new(self.key_limit)))
                .clone()
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
