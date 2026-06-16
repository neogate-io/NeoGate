use std::time::{Duration, Instant};

use crate::{app::AppContext, config::Config};

mod channel_probe;
mod upstream_models;

pub(crate) struct JobCadence {
    next_channel_probe: Instant,
    next_upstream_models: Instant,
}

impl JobCadence {
    pub fn new(now: Instant, config: &Config) -> Self {
        Self {
            next_channel_probe: now,
            next_upstream_models: now + spread_initial_delay(config.upstream_models_interval),
        }
    }
}

pub(crate) async fn run_due(context: &AppContext, cadence: &mut JobCadence) -> anyhow::Result<()> {
    let now = Instant::now();

    if now >= cadence.next_channel_probe {
        channel_probe::run(context).await?;
        cadence.next_channel_probe = now + context.config.channel_probe_interval;
    }

    if now >= cadence.next_upstream_models {
        upstream_models::run(context).await?;
        cadence.next_upstream_models = now + context.config.upstream_models_interval;
    }

    Ok(())
}

fn spread_initial_delay(interval: Duration) -> Duration {
    interval.min(Duration::from_secs(15))
}
