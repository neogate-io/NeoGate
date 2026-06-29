mod admin;
mod app;
mod apps;
mod auth;
mod billing;
mod cli;
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
pub use core::{cache, db, email, error, id, input, pagination, secrets};

pub async fn run() -> anyhow::Result<()> {
    app::load_dotenv();
    if matches!(cli::handle_args().await?, cli::CliAction::Handled) {
        return Ok(());
    }
    app::run().await
}
