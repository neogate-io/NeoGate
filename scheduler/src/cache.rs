use anyhow::Result;
use redis::AsyncCommands;
use serde_json::json;

const CHANNEL_SUFFIX: &str = "cache_invalidation";

#[derive(Clone)]
pub(crate) struct CacheInvalidator {
    publisher: Option<RedisInvalidationPublisher>,
}

#[derive(Clone)]
struct RedisInvalidationPublisher {
    manager: redis::aio::ConnectionManager,
    channel: String,
}

impl CacheInvalidator {
    pub(crate) async fn new(redis_url: Option<&str>, key_prefix: &str) -> Result<Self> {
        let Some(redis_url) = redis_url else {
            return Ok(Self { publisher: None });
        };

        let client = redis::Client::open(redis_url)?;
        let manager = client.get_connection_manager().await?;
        Ok(Self {
            publisher: Some(RedisInvalidationPublisher {
                manager,
                channel: redis_channel(key_prefix),
            }),
        })
    }

    pub(crate) async fn invalidate_routing(&self) {
        let Some(publisher) = &self.publisher else {
            return;
        };

        if let Err(err) = publisher.publish_routing().await {
            tracing::warn!("failed to publish routing cache invalidation event: {err}");
        }
    }
}

impl RedisInvalidationPublisher {
    async fn publish_routing(&self) -> Result<()> {
        let mut conn = self.manager.clone();
        let payload = json!({ "type": "routing" }).to_string();
        let _: usize = conn.publish(&self.channel, payload).await?;
        Ok(())
    }
}

fn redis_channel(key_prefix: &str) -> String {
    format!("{key_prefix}:{CHANNEL_SUFFIX}")
}
