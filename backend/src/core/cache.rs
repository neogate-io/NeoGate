use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};

use crate::{error::AppResult, id::DbId, AppState};

const CHANNEL_SUFFIX: &str = "cache_invalidation";

#[derive(Clone)]
pub struct CacheInvalidator {
    bus: CacheInvalidationBus,
}

#[derive(Clone)]
pub struct CacheInvalidationBus {
    publisher: Option<RedisInvalidationPublisher>,
}

#[derive(Clone)]
struct RedisInvalidationPublisher {
    manager: redis::aio::ConnectionManager,
    channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InvalidationEvent {
    User {
        id: DbId,
    },
    UserKey {
        id: DbId,
    },
    Routing,
    ChannelKeySecret {
        id: DbId,
    },
    ChannelKeyCooldown {
        id: DbId,
        cooldown_until: DateTime<Utc>,
    },
    Price {
        channel_id: DbId,
        model: String,
    },
}

impl CacheInvalidator {
    pub fn local() -> Self {
        Self {
            bus: CacheInvalidationBus::local(),
        }
    }

    pub async fn redis(
        redis_url: &str,
        key_prefix: &str,
    ) -> AppResult<(Self, RedisInvalidationListener)> {
        let (bus, listener) = CacheInvalidationBus::redis(redis_url, key_prefix).await?;
        Ok((Self { bus }, listener))
    }

    pub async fn invalidate(&self, state: &AppState, event: InvalidationEvent) {
        apply_invalidation(state, event.clone()).await;
        if let Err(err) = self.bus.publish(event).await {
            tracing::warn!("failed to publish cache invalidation event: {err}");
        }
    }
}

impl CacheInvalidationBus {
    pub fn local() -> Self {
        Self { publisher: None }
    }

    pub async fn redis(
        redis_url: &str,
        key_prefix: &str,
    ) -> AppResult<(Self, RedisInvalidationListener)> {
        let client = redis::Client::open(redis_url)?;
        let manager = client.get_connection_manager().await?;
        let channel = redis_channel(key_prefix);
        Ok((
            Self {
                publisher: Some(RedisInvalidationPublisher {
                    manager,
                    channel: channel.clone(),
                }),
            },
            RedisInvalidationListener { client, channel },
        ))
    }

    pub async fn publish(&self, event: InvalidationEvent) -> AppResult<()> {
        let Some(publisher) = &self.publisher else {
            return Ok(());
        };
        let payload = serde_json::to_string(&event)?;
        let mut conn = publisher.manager.clone();
        let _: usize = conn.publish(&publisher.channel, payload).await?;
        Ok(())
    }
}

pub struct RedisInvalidationListener {
    client: redis::Client,
    channel: String,
}

impl RedisInvalidationListener {
    pub fn spawn(self, state: Arc<AppState>) {
        tokio::spawn(async move {
            loop {
                if let Err(err) = self.listen_once(Arc::clone(&state)).await {
                    tracing::warn!("redis cache invalidation listener failed: {err}");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        });
    }

    async fn listen_once(&self, state: Arc<AppState>) -> AppResult<()> {
        let mut pubsub = self.client.get_async_pubsub().await?;
        pubsub.subscribe(&self.channel).await?;
        let mut stream = pubsub.on_message();
        while let Some(message) = stream.next().await {
            let payload: String = message.get_payload()?;
            match serde_json::from_str::<InvalidationEvent>(&payload) {
                Ok(event) => apply_invalidation(&state, event).await,
                Err(err) => tracing::warn!("invalid cache invalidation payload: {err}"),
            }
        }
        Ok(())
    }
}

pub async fn apply_invalidation(state: &AppState, event: InvalidationEvent) {
    match event {
        InvalidationEvent::User { id } => state.user_auth_cache.remove_user(id),
        InvalidationEvent::UserKey { id } => state.user_auth_cache.remove_user_key(id),
        InvalidationEvent::Routing => state.selector.invalidate().await,
        InvalidationEvent::ChannelKeySecret { id } => state.secrets.forget(id),
        InvalidationEvent::ChannelKeyCooldown { id, cooldown_until } => {
            state
                .selector
                .mark_key_failure_local(id, cooldown_until)
                .await;
        }
        InvalidationEvent::Price { channel_id, model } => {
            state.billing.invalidate_price(channel_id, &model);
        }
    }
}

fn redis_channel(key_prefix: &str) -> String {
    format!("{key_prefix}:{CHANNEL_SUFFIX}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_key_cooldown_event_round_trips() {
        let event = InvalidationEvent::ChannelKeyCooldown {
            id: 42,
            cooldown_until: Utc::now(),
        };

        let payload = serde_json::to_string(&event).unwrap();
        let decoded: InvalidationEvent = serde_json::from_str(&payload).unwrap();

        match decoded {
            InvalidationEvent::ChannelKeyCooldown { id, .. } => assert_eq!(id, 42),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
