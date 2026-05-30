use alon_sentinel::runtime;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    runtime::run_api().await
}
