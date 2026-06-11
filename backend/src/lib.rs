mod admin;
mod app;
mod auth;
mod billing;
mod bootstrap;
mod config;
mod core;
mod health;
mod install;
mod payment;
mod policy;
mod provider;
mod relay;
mod task;
mod usage;
mod user;

pub use app::AppState;
pub use core::{cache, db, email, error, id, secrets};

pub async fn run() -> anyhow::Result<()> {
    app::run().await
}

