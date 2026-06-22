use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;
use serde_json::Value;

use super::selector::UpstreamProtocol;
use crate::id::DbId;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ChannelAffinityKey {
    pub(crate) rule: &'static str,
    pub(crate) protocol: UpstreamProtocol,
    pub(crate) model: String,
    pub(crate) value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UpstreamAffinityTarget {
    pub(crate) channel_id: DbId,
    pub(crate) channel_endpoint_id: DbId,
    pub(crate) channel_key_id: Option<DbId>,
    pub(crate) credential_id: Option<DbId>,
}

#[derive(Clone)]
pub(crate) struct ChannelAffinityCache {
    enabled: bool,
    ttl: Duration,
    max_entries: usize,
    entries: Arc<DashMap<ChannelAffinityKey, ChannelAffinityEntry>>,
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
        }
    }

    pub(crate) fn get(&self, key: &ChannelAffinityKey) -> Option<UpstreamAffinityTarget> {
        if !self.enabled {
            return None;
        }
        let now = Instant::now();
        if let Some(entry) = self.entries.get(key) {
            if entry.expires_at > now {
                return Some(entry.target.clone());
            }
            drop(entry);
            self.entries.remove(key);
        }
        None
    }

    pub(crate) fn insert(&self, key: ChannelAffinityKey, target: UpstreamAffinityTarget) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
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
                expires_at: now + self.ttl,
            },
        );
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

    #[test]
    fn cache_returns_inserted_target_until_ttl_expires() {
        let cache = ChannelAffinityCache::new(true, Duration::from_millis(20), 10);
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-1"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();
        let target = UpstreamAffinityTarget {
            channel_id: 1,
            channel_endpoint_id: 2,
            channel_key_id: Some(3),
            credential_id: None,
        };

        cache.insert(key.clone(), target.clone());
        assert_eq!(cache.get(&key), Some(target));

        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn disabled_cache_ignores_entries() {
        let cache = ChannelAffinityCache::new(false, Duration::from_secs(60), 10);
        let value: Value = serde_json::from_str(r#"{"prompt_cache_key":"trace-1"}"#).unwrap();
        let key = openai_responses_affinity_key_from_value("gpt-5", &value).unwrap();
        cache.insert(
            key.clone(),
            UpstreamAffinityTarget {
                channel_id: 1,
                channel_endpoint_id: 2,
                channel_key_id: Some(3),
                credential_id: None,
            },
        );

        assert_eq!(cache.get(&key), None);
    }
}
