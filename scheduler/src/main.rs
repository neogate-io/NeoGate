mod app;
mod config;
mod jobs;
mod secrets;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    app::run().await
}
