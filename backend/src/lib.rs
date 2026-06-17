mod admin;
mod app;
mod apps;
mod auth;
mod billing;
mod config;
mod core;
mod health;
mod payment;
mod policy;
mod project;
mod provider;
mod relay;
mod setup;
mod task;
mod usage;
mod user;

pub use app::AppState;
pub use core::{cache, db, email, error, id, pagination, secrets};

pub async fn run() -> anyhow::Result<()> {
    app::run().await
}
