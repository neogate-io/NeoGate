mod admin;
mod app;
mod auth;
mod bootstrap;
mod billing;
mod config;
mod core;
mod health;
mod payment;
mod policy;
mod relay;
mod task;
mod usage;
mod user;

pub use app::AppState;
pub use core::{cache, db, email, error, id, secrets};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app::run().await
}
