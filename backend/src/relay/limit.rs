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
pub(crate) struct UserRequestLimiter {
    global: Option<Arc<Semaphore>>,
    per_user: Arc<Mutex<HashMap<DbId, Arc<Semaphore>>>>,
    user_limit: usize,
    global_limit: usize,
    new_users_since_prune: Arc<AtomicUsize>,
}

pub(crate) struct UserRequestPermit {
    _global: Option<OwnedSemaphorePermit>,
    _user: OwnedSemaphorePermit,
}

impl UserRequestLimiter {
    pub(crate) fn new(user_limit: usize, global_limit: usize) -> Self {
        Self {
            global: (global_limit > 0).then(|| Arc::new(Semaphore::new(global_limit))),
            per_user: Arc::new(Mutex::new(HashMap::new())),
            user_limit,
            global_limit,
            new_users_since_prune: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub(crate) async fn try_acquire(&self, user_id: DbId) -> AppResult<UserRequestPermit> {
        let global = match &self.global {
            Some(global) => Some(
                global
                    .clone()
                    .try_acquire_owned()
                    .map_err(|_| AppError::RateLimited(global_limit_message(self.global_limit)))?,
            ),
            None => None,
        };
        let user_semaphore = {
            let mut per_user = self.per_user.lock().await;
            if let Some(semaphore) = per_user.get(&user_id) {
                semaphore.clone()
            } else {
                if self.new_users_since_prune.fetch_add(1, Ordering::Relaxed)
                    >= USER_REQUEST_PRUNE_NEW_USERS_INTERVAL
                {
                    self.new_users_since_prune.store(0, Ordering::Relaxed);
                    prune_idle_user_limiters(&mut per_user, self.user_limit);
                }
                let semaphore = Arc::new(Semaphore::new(self.user_limit));
                per_user.insert(user_id, semaphore.clone());
                semaphore
            }
        };
        let user = user_semaphore
            .try_acquire_owned()
            .map_err(|_| AppError::RateLimited(user_limit_message(self.user_limit)))?;
        Ok(UserRequestPermit {
            _global: global,
            _user: user,
        })
    }
}

const USER_REQUEST_PRUNE_NEW_USERS_INTERVAL: usize = 1024;

fn prune_idle_user_limiters(per_user: &mut HashMap<DbId, Arc<Semaphore>>, user_limit: usize) {
    per_user.retain(|_, semaphore| {
        Arc::strong_count(semaphore) > 1 || semaphore.available_permits() < user_limit
    });
}

fn global_limit_message(limit: usize) -> String {
    format!(
        "Global concurrent request limit reached: maximum {limit} active model requests. Please wait for an active request to finish and retry."
    )
}

fn user_limit_message(limit: usize) -> String {
    format!(
        "User concurrent request limit reached: maximum {limit} active model requests per user. Please wait for an active request to finish and retry."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    impl UserRequestLimiter {
        async fn per_user_len(&self) -> usize {
            self.per_user.lock().await.len()
        }
    }

    #[tokio::test]
    async fn prunes_idle_user_limiters_after_user_churn() {
        let limiter = UserRequestLimiter::new(1, 8);

        for user_id in 1..=(USER_REQUEST_PRUNE_NEW_USERS_INTERVAL as DbId + 1) {
            let permit = limiter.try_acquire(user_id).await.unwrap();
            drop(permit);
        }

        assert!(limiter.per_user_len().await <= 2);
    }

    #[tokio::test]
    async fn keeps_active_user_limiters_while_pruning() {
        let limiter = UserRequestLimiter::new(1, 8);
        let active = limiter.try_acquire(1).await.unwrap();

        for user_id in 2..=(USER_REQUEST_PRUNE_NEW_USERS_INTERVAL as DbId + 2) {
            let permit = limiter.try_acquire(user_id).await.unwrap();
            drop(permit);
        }

        assert!(limiter.per_user_len().await >= 1);
        drop(active);
    }

    #[tokio::test]
    async fn user_limit_error_includes_configured_limit() {
        let limiter = UserRequestLimiter::new(1, 0);
        let _active = limiter.try_acquire(1).await.unwrap();
        let err = match limiter.try_acquire(1).await {
            Ok(_) => panic!("expected user concurrency limit error"),
            Err(err) => err,
        };

        assert!(
            matches!(err, AppError::RateLimited(message) if message.contains("maximum 1 active model requests per user"))
        );
    }

    #[tokio::test]
    async fn global_limit_error_includes_configured_limit() {
        let limiter = UserRequestLimiter::new(10, 1);
        let _active = limiter.try_acquire(1).await.unwrap();
        let err = match limiter.try_acquire(2).await {
            Ok(_) => panic!("expected global concurrency limit error"),
            Err(err) => err,
        };

        assert!(
            matches!(err, AppError::RateLimited(message) if message.contains("maximum 1 active model requests"))
        );
    }
}
