use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::selector::UpstreamProtocol;
use crate::id::DbId;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ChannelAffinityKey {
    pub(crate) rule: &'static str,
    pub(crate) protocol: UpstreamProtocol,
    pub(crate) model: String,
    pub(crate) value: String,
}

const REDIS_KEY_VERSION: &str = "channel_affinity:v1";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct UpstreamAffinityTarget {
    pub(crate) channel_id: DbId,
    pub(crate) channel_endpoint_id: DbId,
    pub(crate) channel_key_id: Option<DbId>,
    pub(crate) credential_id: Option<DbId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChannelAffinityStatus {
    Miss,
    Local,
    Redis,
}

impl ChannelAffinityStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Miss => "miss",
            Self::Local => "local",
            Self::Redis => "redis",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChannelAffinityObservation {
    pub(crate) status: ChannelAffinityStatus,
    pub(crate) key_fingerprint: String,
}

#[derive(Clone)]
pub(crate) struct ChannelAffinityCache {
    enabled: bool,
    ttl: Duration,
    max_entries: usize,
    entries: Arc<DashMap<ChannelAffinityKey, ChannelAffinityEntry>>,
    redis: Option<RedisAffinityStore>,
}

#[derive(Clone)]
struct RedisAffinityStore {
    manager: redis::aio::ConnectionManager,
    key_prefix: String,
}

#[derive(Clone)]
struct ChannelAffinityEntry {
    target: UpstreamAffinityTarget,
    expires_at: Instant,
}

impl ChannelAffinityCache {
    pub(crate) fn new(enabled: bool, ttl: Duration, max_entries: usize) -> Self {
        Self {
            enabled,
            ttl,
            max_entries: max_entries.max(1),
            entries: Arc::new(DashMap::new()),
            redis: None,
        }
    }

    pub(crate) async fn with_redis(
        enabled: bool,
        ttl: Duration,
        max_entries: usize,
        client: &redis::Client,
        key_prefix: &str,
    ) -> Self {
        let mut cache = Self::new(enabled, ttl, max_entries);
        if !enabled {
            return cache;
        }
        match client.get_connection_manager().await {
            Ok(manager) => {
                cache.redis = Some(RedisAffinityStore {
                    manager,
                    key_prefix: format!("{key_prefix}:{REDIS_KEY_VERSION}"),
                });
            }
            Err(err) => {
                tracing::warn!("failed to initialize redis channel affinity cache: {err}");
            }
        }
        cache
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(crate) async fn get(
        &self,
        key: &ChannelAffinityKey,
    ) -> Option<(UpstreamAffinityTarget, ChannelAffinityStatus)> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        if let Some(entry) = self.entries.get(key) {
            if entry.expires_at > now {
                return Some((entry.target.clone(), ChannelAffinityStatus::Local));
            }
            // drop(entry) 释放分片锁后 remove 存在 TOCTOU 窗口：另一线程可能在两步之间
            // 插入新鲜条目而被误删。remove_if 在持锁下原子地校验后删除，消除该窗口。
            drop(entry);
            self.entries.remove_if(key, |_, e| e.expires_at <= now);
        }

        let redis = self.redis.as_ref()?;
        let redis_key = redis.key(key);
        let mut conn = redis.manager.clone();

        // GET + PTTL 合并为单次 pipeline，避免两次 Redis 往返。
        let (payload, ttl_ms): (Option<String>, i64) = match redis::pipe()
            .get(&redis_key)
            .pttl(&redis_key)
            .query_async(&mut conn)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!("failed to read channel affinity from redis: {err}");
                return None;
            }
        };

        let payload = payload?;
        let target = match serde_json::from_str::<UpstreamAffinityTarget>(&payload) {
            Ok(target) => target,
            Err(err) => {
                tracing::warn!("invalid channel affinity payload in redis: {err}");
                let _: Result<usize, _> = conn.del(redis_key).await;
                return None;
            }
        };

        if ttl_ms == -2 {
            return None; // key 已过期
        }
        if ttl_ms > 0 {
            self.insert_local(
                key.clone(),
                target.clone(),
                now,
                Duration::from_millis(ttl_ms as u64),
            );
        }
        Some((target, ChannelAffinityStatus::Redis))
    }

    pub(crate) async fn insert(&self, key: ChannelAffinityKey, target: UpstreamAffinityTarget) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        self.insert_local(key.clone(), target.clone(), now, self.ttl);

        let Some(redis) = &self.redis else {
            return;
        };
        let payload = match serde_json::to_string(&target) {
            Ok(payload) => payload,
            Err(err) => {
                tracing::warn!("failed to encode channel affinity for redis: {err}");
                return;
            }
        };
        let mut conn = redis.manager.clone();
        if let Err(err) = conn
            .set_ex::<_, _, ()>(redis.key(&key), payload, self.ttl.as_secs().max(1))
            .await
        {
            tracing::warn!("failed to write channel affinity to redis: {err}");
        }
    }

    fn insert_local(
        &self,
        key: ChannelAffinityKey,
        target: UpstreamAffinityTarget,
        now: Instant,
        ttl: Duration,
    ) {
        if self.entries.len() >= self.max_entries {
            self.entries.retain(|_, entry| entry.expires_at > now);
            while self.entries.len() >= self.max_entries && !self.entries.contains_key(&key) {
                let evict = self.entries.iter().next().map(|entry| entry.key().clone());
                let Some(evict) = evict else {
                    break;
                };
                self.entries.remove(&evict);
            }
        }
        self.entries.insert(
            key,
            ChannelAffinityEntry {
                target,
                expires_at: now + ttl,
            },
        );
    }
}

impl RedisAffinityStore {
    fn key(&self, key: &ChannelAffinityKey) -> String {
        format!("{}:{}", self.key_prefix, key.storage_fingerprint())
    }
}

impl ChannelAffinityKey {
    fn digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(self.rule.as_bytes());
        digest.update([0]);
        digest.update(self.protocol.as_str().as_bytes());
        digest.update([0]);
        digest.update(self.model.as_bytes());
        digest.update([0]);
        digest.update(self.value.as_bytes());
        digest.finalize().into()
    }

    fn storage_fingerprint(&self) -> String {
        hex::encode(self.digest())
    }

    pub(crate) fn fingerprint(&self) -> String {
        hex::encode(&self.digest()[..8])
    }
}

pub(crate) fn openai_responses_affinity_key_from_value(
    model: &str,
    value: &Value,
) -> Option<ChannelAffinityKey> {
    affinity_key_from_value(
        "openai_responses_prompt_cache_key",
        UpstreamProtocol::Openai,
        model,
        value,
        &["prompt_cache_key"],
    )
}

pub(crate) fn anthropic_messages_affinity_key_from_value(
    _model: &str,
    value: &Value,
) -> Option<ChannelAffinityKey> {
    affinity_key_from_value(
        "anthropic_messages_metadata_user_id",
        UpstreamProtocol::Anthropic,
        "",
        value,
        &["metadata", "user_id"],
    )
}

fn affinity_key_from_value(
    rule: &'static str,
    protocol: UpstreamProtocol,
    model: &str,
    value: &Value,
    path: &[&str],
) -> Option<ChannelAffinityKey> {
    let value = json_path_value(value, path)?;
    Some(ChannelAffinityKey {
        rule,
        protocol,
        model: model.to_string(),
        value,
    })
}

fn json_path_value(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    scalar_to_affinity_value(current)
}

fn scalar_to_affinity_value(value: &Value) -> Option<String> {
    let value = match value {
        Value::String(value) => value.trim().to_string(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => return None,
    };
    (!value.is_empty()).then_some(value)
}

impl From<&super::selector::SelectedUpstream> for UpstreamAffinityTarget {
    fn from(upstream: &super::selector::SelectedUpstream) -> Self {
        Self {
            channel_id: upstream.channel_id,
            channel_endpoint_id: upstream.channel_endpoint_id,
            channel_key_id: upstream.channel_key_id,
            credential_id: upstream.credential_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_openai_responses_prompt_cache_key() {
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-1"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();

        assert_eq!(key.rule, "openai_responses_prompt_cache_key");
        assert_eq!(key.model, "gpt-5");
        assert_eq!(key.value, "trace-1");
    }

    #[test]
    fn extracts_anthropic_metadata_user_id() {
        let value: Value = serde_json::from_str(r#"{"metadata":{"user_id":"user-1"}}"#).unwrap();
        let key = anthropic_messages_affinity_key_from_value("claude-sonnet-4", &value).unwrap();

        assert_eq!(key.rule, "anthropic_messages_metadata_user_id");
        assert_eq!(key.model, "");
        assert_eq!(key.value, "user-1");
    }

    #[test]
    fn anthropic_affinity_key_ignores_model_name() {
        let value: Value = serde_json::from_str(r#"{"metadata":{"user_id":"trace-1"}}"#).unwrap();
        let sonnet = anthropic_messages_affinity_key_from_value("claude-sonnet-4", &value).unwrap();
        let haiku = anthropic_messages_affinity_key_from_value("claude-haiku-4", &value).unwrap();

        assert_eq!(sonnet, haiku);
    }

    #[tokio::test]
    async fn cache_returns_inserted_target_until_ttl_expires() {
        let cache = ChannelAffinityCache::new(true, Duration::from_millis(20), 10);
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-1"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();
        let target = UpstreamAffinityTarget {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
        };

        cache.insert(key.clone(), target.clone()).await;
        assert_eq!(
            cache.get(&key).await,
            Some((target, ChannelAffinityStatus::Local))
        );

        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(cache.get(&key).await, None);
    }

    #[tokio::test]
    async fn disabled_cache_ignores_entries() {
        let cache = ChannelAffinityCache::new(false, Duration::from_secs(60), 10);
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-1"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();
        cache
            .insert(
                key.clone(),
                UpstreamAffinityTarget {
                    channel_id: 1,
                    channel_endpoint_id: 2,
                    channel_key_id: Some(3),
                    credential_id: None,
                },
            )
            .await;

        assert_eq!(cache.get(&key).await, None);
    }

    #[test]
    fn affinity_fingerprint_is_stable_and_hides_the_source_value() {
        let value: Value =
            serde_json::from_str(r#"{"metadata":{"user_id":"sensitive-session-value"}}"#).unwrap();
        let key = anthropic_messages_affinity_key_from_value("claude-sonnet-4", &value).unwrap();

        assert_eq!(key.fingerprint(), key.fingerprint());
        assert_eq!(key.fingerprint().len(), 16);
        assert_eq!(key.storage_fingerprint().len(), 64);
        assert!(!key.fingerprint().contains("sensitive"));
        assert!(!key.storage_fingerprint().contains("sensitive"));
    }

    #[tokio::test]
    async fn redis_cache_supports_cross_instance_hits_without_extending_ttl() {
        let Ok(redis_url) = std::env::var("NEOGATE_TEST_REDIS_URL") else {
            return;
        };
        let client = redis::Client::open(redis_url).unwrap();
        let prefix = format!("neogate-test-{}", uuid::Uuid::new_v4());
        let first =
            ChannelAffinityCache::with_redis(true, Duration::from_secs(2), 10, &client, &prefix)
                .await;
        let second =
            ChannelAffinityCache::with_redis(true, Duration::from_secs(2), 10, &client, &prefix)
                .await;
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-redis"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();
        let target = UpstreamAffinityTarget {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
        };

        first.insert(key.clone(), target.clone()).await;
        tokio::time::sleep(Duration::from_millis(1_200)).await;
        assert_eq!(
            second.get(&key).await,
            Some((target, ChannelAffinityStatus::Redis))
        );

        tokio::time::sleep(Duration::from_millis(950)).await;
        assert_eq!(second.get(&key).await, None);
    }
}
