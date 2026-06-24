use std::{
    collections::HashMap,
    sync::{Arc, RwLock as StdRwLock},
};

use chrono::{DateTime, Utc};

use crate::id::DbId;

/// 运行时学习到的「某 endpoint 的某 model 不支持 /v1/responses」结论。
///
/// 键仅到 `(endpoint_id, model)`：responses 支持是 endpoint+model 的固有属性，
/// 与凭证/路由形态无关；chat 路径完全不访问本缓存，因此不会误伤 /v1/chat/completions。
/// 仅缓存负向结论，到期后自愈——上游将来放开 responses 后自动恢复原生转发。
#[derive(Clone, Default)]
pub(crate) struct ResponsesSupportCache {
    entries: Arc<StdRwLock<HashMap<ResponsesSupportKey, DateTime<Utc>>>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ResponsesSupportKey {
    pub endpoint_id: DbId,
    pub model: String,
}

impl ResponsesSupportCache {
    pub(crate) fn is_unsupported(&self, endpoint_id: DbId, model: &str, now: DateTime<Utc>) -> bool {
        let key = ResponsesSupportKey {
            endpoint_id,
            model: model.to_string(),
        };
        let expired = {
            let entries = self
                .entries
                .read()
                .expect("responses support cache poisoned");
            match entries.get(&key) {
                Some(until) if *until > now => return true,
                Some(_) => true,
                None => return false,
            }
        };
        if expired {
            self.clear_expired(now);
        }
        false
    }

    pub(crate) fn mark_unsupported(
        &self,
        endpoint_id: DbId,
        model: &str,
        unsupported_until: DateTime<Utc>,
    ) {
        let key = ResponsesSupportKey {
            endpoint_id,
            model: model.to_string(),
        };
        let mut entries = self
            .entries
            .write()
            .expect("responses support cache poisoned");
        let now = Utc::now();
        entries.retain(|_, until| *until > now);
        entries.insert(key, unsupported_until);
    }

    pub(crate) fn clear_expired(&self, now: DateTime<Utc>) {
        let mut entries = self
            .entries
            .write()
            .expect("responses support cache poisoned");
        entries.retain(|_, until| *until > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn key(endpoint_id: DbId, model: &str) -> ResponsesSupportKey {
        ResponsesSupportKey {
            endpoint_id,
            model: model.to_string(),
        }
    }

    #[test]
    fn unsupported_is_true_within_ttl_and_false_after() {
        let cache = ResponsesSupportCache::default();
        let now = Utc::now();
        cache.mark_unsupported(7, "glm-5.2", now + Duration::seconds(10));

        assert!(cache.is_unsupported(7, "glm-5.2", now));
        assert!(!cache.is_unsupported(7, "qwen3.6-plus", now));
        assert!(!cache.is_unsupported(99, "glm-5.2", now));
        assert!(!cache.is_unsupported(7, "glm-5.2", now + Duration::seconds(11)));
    }

    #[test]
    fn clear_expired_removes_stale_entries() {
        let cache = ResponsesSupportCache::default();
        let now = Utc::now();
        cache.mark_unsupported(7, "glm-5.2", now + Duration::seconds(10));
        cache.mark_unsupported(8, "deepseek-v3.2", now - Duration::seconds(1));

        let later = now + Duration::seconds(11);
        cache.clear_expired(later);
        assert!(!cache.is_unsupported(7, "glm-5.2", later));
        assert!(!cache.is_unsupported(8, "deepseek-v3.2", later));
    }

    #[test]
    fn key_ignores_protocol_so_chat_path_is_unaffected() {
        // 键不含 protocol：本缓存只在 responses 路由决策处被查询，
        // chat 路径不访问，因此同 endpoint 的 chat 请求永远不会被这里影响。
        let cache = ResponsesSupportCache::default();
        let now = Utc::now();
        cache.mark_unsupported(7, "glm-5.2", now + Duration::seconds(10));

        assert_eq!(
            key(7, "glm-5.2"),
            ResponsesSupportKey {
                endpoint_id: 7,
                model: "glm-5.2".to_string()
            }
        );
        assert!(cache.is_unsupported(7, "glm-5.2", now));
    }
}
