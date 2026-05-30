use anyhow::Result;
use dotenvy::dotenv;
use tracing::info;

use alon_sentinel::{config, db, logging};

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = config::BootstrapConfig::from_args_and_env()?;
    let _log_guard = logging::init_logging(
        &bootstrap.log_dir,
        &bootstrap.log_level,
        bootstrap.log_max_files,
    );

    dotenv().ok();

    info!("Sentinel migration runner started");

    let config = config::Config::from_env()?;
    let pool = db::pool::create_pool(&config).await?;
    let report = db::migrations::run_pending_migrations(&pool).await?;

    println!("Sentinel migrations completed.");
    println!("Applied: {}", report.applied.len());
    for filename in &report.applied {
        println!("  + {filename}");
    }

    println!("Skipped: {}", report.skipped.len());
    for filename in &report.skipped {
        println!("  = {filename}");
    }

    Ok(())
}
