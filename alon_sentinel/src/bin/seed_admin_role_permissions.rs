use anyhow::Result;
use dotenvy::dotenv;
use std::env;
use tracing::info;

use alon_sentinel::{
    config, db,
    domain::{permissions, roles},
    logging,
};

const DEFAULT_ROLE_KEY: &str = "admin";

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = config::BootstrapConfig::from_args_and_env()?;
    let _log_guard = logging::init_logging(
        &bootstrap.log_dir,
        &bootstrap.log_level,
        bootstrap.log_max_files,
    );

    dotenv().ok();

    info!("Sentinel admin role permission seeding started");

    let config = config::Config::from_env()?;
    let pool = db::pool::create_pool(&config).await?;

    let role_key = env::var("SEED_ROLE_KEY").unwrap_or_else(|_| DEFAULT_ROLE_KEY.to_string());
    let role = roles::repository::get_role_by_key(&pool, &role_key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("role not found: {role_key}"))?;

    let all_permissions = permissions::repository::list_permissions(&pool).await?;
    let permission_ids = all_permissions
        .iter()
        .map(|permission| permission.id)
        .collect::<Vec<_>>();

    permissions::repository::replace_permissions_for_role(&pool, role.id, &permission_ids).await?;

    println!();
    println!("Sentinel role permission seeding completed.");
    println!("Role: {}", role.key);
    println!("Assigned permissions: {}", all_permissions.len());

    for permission in &all_permissions {
        println!("  - {}", permission.key);
    }

    Ok(())
}
