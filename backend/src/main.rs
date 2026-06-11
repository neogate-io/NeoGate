#[tokio::main]
async fn main() -> anyhow::Result<()> {
    neogate::run().await
}
