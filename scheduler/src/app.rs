use std::{sync::Arc, time::Instant};

use anyhow::Context;
use sqlx::PgPool;
use tokio::time::{self, Duration, MissedTickBehavior};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::{
    cache::CacheInvalidator,
    config::Config,
    jobs::{self, JobCadence},
};

#[derive(Clone)]
pub(crate) struct AppContext {
    pub config: Arc<Config>,
    pub db: PgPool,
    pub http: reqwest::Client,
    pub secrets: crate::secrets::SecretStore,
    pub cache_invalidator: CacheInvalidator,
}

pub(crate) async fn run() -> anyhow::Result<()> {
    init_tracing();
    let context = wait_for_context().await?;

    tracing::info!(
        tick_interval_ms = context.config.tick_interval.as_millis(),
        channel_probe_interval_secs = context.config.channel_probe_interval.as_secs(),
        upstream_models_interval_secs = context.config.upstream_models_interval.as_secs(),
        "starting neogate scheduler"
    );

    run_loop(context).await
}

async fn wait_for_context() -> anyhow::Result<AppContext> {
    loop {
        match build_context().await {
            Ok(context) => return Ok(context),
            Err(err) => {
                tracing::warn!(
                    "scheduler runtime configuration is not ready; retrying in 60s: {err:#}"
                );
                tokio::select! {
                    _ = time::sleep(Duration::from_secs(60)) => {}
                    signal = tokio::signal::ctrl_c() => {
                        signal.context("failed to listen for shutdown signal")?;
                        tracing::info!("scheduler shutdown requested before startup completed");
                        anyhow::bail!("scheduler shutdown requested");
                    }
                }
            }
        }
    }
}

async fn build_context() -> anyhow::Result<AppContext> {
    load_dotenv();
    let config = Arc::new(Config::from_env()?);
    let http = reqwest::Client::builder()
        .connect_timeout(config.upstream_connect_timeout)
        .timeout(config.upstream_timeout)
        .build()
        .context("failed to build scheduler http client")?;
    let db = PgPool::connect(&config.database_url)
        .await
        .context("failed to connect scheduler database")?;
    let secrets = crate::secrets::SecretStore::new(&config.upstream_secret_key);
    let cache_invalidator =
        CacheInvalidator::new(config.redis_url.as_deref(), &config.redis_key_prefix)
            .await
            .context("failed to initialize scheduler cache invalidator")?;
    Ok(AppContext {
        config,
        db,
        http,
        secrets,
        cache_invalidator,
    })
}

async fn run_loop(context: AppContext) -> anyhow::Result<()> {
    let mut cadence = JobCadence::new(Instant::now(), &context.config);
    let mut ticker = time::interval(context.config.tick_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if let Err(err) = jobs::run_due(&context, &mut cadence).await {
                    tracing::warn!("scheduler tick failed: {err:#}");
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("failed to listen for shutdown signal")?;
                tracing::info!("scheduler shutdown requested");
                break;
            }
        }
    }

    Ok(())
}

fn load_dotenv() {
    if let Ok(path) = std::env::var("NEOGATE_ENV_FILE") {
        dotenvy::from_path(path).ok();
    }
    dotenvy::from_filename(".env").ok();
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("neogate_scheduler=info,info"));
    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
