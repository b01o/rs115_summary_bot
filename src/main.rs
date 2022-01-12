use anyhow::Result;
use rs115_bot::app;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await?;
    Ok(())
}