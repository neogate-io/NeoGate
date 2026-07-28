use std::time::Duration;

pub(crate) mod billing;
pub(crate) mod jobs;
pub(crate) mod results;
pub(crate) mod spool;
pub(crate) mod types;
pub(crate) mod upstream;
pub(crate) mod worker;

pub(crate) const WORKER_TICK_INTERVAL: Duration = Duration::from_secs(1);
pub(crate) const POLL_INTERVAL: Duration = Duration::from_secs(5);
pub(crate) const ASSET_RETENTION: Duration = Duration::from_secs(3 * 24 * 60 * 60);
pub(crate) const REQUEST_SPOOL_TTL: Duration = Duration::from_secs(60 * 60);
pub(crate) const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);
pub(crate) const ORPHAN_CLEANUP_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
