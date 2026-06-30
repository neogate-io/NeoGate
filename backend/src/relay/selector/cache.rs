use std::time::Duration;

use chrono::{DateTime, Utc};

use crate::id::DbId;

use super::{
    load::trim_runtime_secret_cache_for_insert, CachedRuntimeSecret, ModelBlockCache,
    ModelBlockKey, RoutingCache, RuntimeSecretCache,
};

impl RuntimeSecretCache {
    pub(super) fn get(&self, credential_id: DbId, ciphertext: &str) -> Option<CachedRuntimeSecret> {
        self.entries
            .read()
            .expect("credential runtime secret cache poisoned")
            .get(&credential_id)
            .filter(|cached| cached.ciphertext == ciphertext)
            .cloned()
    }

    pub(super) fn insert(&self, credential_id: DbId, runtime: CachedRuntimeSecret) {
        let mut entries = self
            .entries
            .write()
            .expect("credential runtime secret cache poisoned");
        trim_runtime_secret_cache_for_insert(&mut entries, credential_id);
        entries.insert(credential_id, runtime);
    }

    pub(super) fn clear(&self) {
        self.entries
            .write()
            .expect("credential runtime secret cache poisoned")
            .clear();
    }

    pub(super) fn remove(&self, credential_id: DbId) {
        self.entries
            .write()
            .expect("credential runtime secret cache poisoned")
            .remove(&credential_id);
    }
}

impl ModelBlockCache {
    pub(super) fn insert(&self, key: ModelBlockKey, blocked_until: DateTime<Utc>) {
        let mut entries = self.entries.write().expect("model block cache poisoned");
        let now = Utc::now();
        entries.retain(|_, until| *until > now);
        entries.insert(key, blocked_until);
    }

    pub(super) fn contains_active(&self, key: &ModelBlockKey, now: DateTime<Utc>) -> bool {
        let expired = {
            let entries = self.entries.read().expect("model block cache poisoned");
            match entries.get(key) {
                Some(blocked_until) if *blocked_until > now => return true,
                Some(_) => true,
                None => return false,
            }
        };
        if expired {
            self.clear_expired(now);
        }
        false
    }

    pub(super) fn is_empty(&self) -> bool {
        self.entries
            .read()
            .expect("model block cache poisoned")
            .is_empty()
    }

    pub(super) fn clear_expired(&self, now: DateTime<Utc>) {
        self.entries
            .write()
            .expect("model block cache poisoned")
            .retain(|_, until| *until > now);
    }
}

impl RoutingCache {
    pub(super) fn is_fresh(&self, ttl: Duration) -> bool {
        self.loaded_at
            .is_some_and(|loaded_at| loaded_at.elapsed() < ttl)
    }
}
