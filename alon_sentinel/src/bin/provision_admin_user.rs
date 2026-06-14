use anyhow::{Result, bail};
use dotenvy::dotenv;
use std::env;
use tracing::info;

use alon_sentinel::{
    auth::AuthService,
    config, db,
    domain::{admin_users, roles},
    logging,
};

const DEFAULT_ADMIN_EMAIL: &str = "admin@localhost";
const DEFAULT_ADMIN_NAME: &str = "Sentinel Admin";
const DEFAULT_ADMIN_ROLE: &str = "admin";

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = config::BootstrapConfig::from_args_and_env()?;
    let _log_guard = logging::init_logging(
        &bootstrap.log_dir,
        &bootstrap.log_level,
        bootstrap.log_max_files,
    );

    dotenv().ok();

    info!("Sentinel admin user provisioning started");

    let config = config::Config::from_env()?;
    let pool = db::pool::create_pool(&config).await?;

    let email = env_or_default("SEED_ADMIN_EMAIL", DEFAULT_ADMIN_EMAIL);
    let display_name = env_or_default("SEED_ADMIN_NAME", DEFAULT_ADMIN_NAME);
    let password = env::var("SEED_ADMIN_PASSWORD")
        .map_err(|_| anyhow::anyhow!("SEED_ADMIN_PASSWORD must be set"))?;
    let role_key = env_or_default("SEED_ADMIN_ROLE", DEFAULT_ADMIN_ROLE);

    if password.trim().is_empty() {
        bail!("SEED_ADMIN_PASSWORD can not be empty");
    }

    if let Some(existing) = admin_users::repository::get_admin_user_by_email(&pool, &email).await? {
        println!();
        println!("Admin user '{}' already exists — skipping.", existing.email);
        println!("Change the password via the admin UI if needed.");
        println!();
        return Ok(());
    }

    let password_hash = AuthService::hash_password(&password)?;
    let user = admin_users::repository::upsert_admin_user(
        &pool,
        admin_users::repository::NewAdminUser {
            email: &email,
            display_name: &display_name,
            password_hash: &password_hash,
        },
    )
    .await?;

    let role = roles::repository::get_role_by_key(&pool, &role_key)
        .await?
        .ok_or_else(|| anyhow::anyhow!("role not found: {role_key}"))?;

    roles::repository::assign_role_to_admin_user(&pool, user.id, role.id).await?;

    println!();
    println!("==============================================");
    println!(" Alon Sentinel — admin credentials");
    println!("==============================================");
    println!("  Email:    {}", user.email);
    println!("  Password: {}", password);
    println!("==============================================");
    println!();
    println!("Change this password after your first login.");
    println!();

    Ok(())
}

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}
