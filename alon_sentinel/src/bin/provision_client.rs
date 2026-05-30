use anyhow::Result;
use dotenvy::dotenv;
use std::env;
use tracing::info;

use alon_sentinel::{
    auth::AuthService,
    config, db,
    domain::api_auth::{self, ApiClientType},
    logging,
};

const DEFAULT_CLIENT_NAME: &str = "Sentinel API Client";
const DEFAULT_CLIENT_ID: &str = "sentinel-client";
const DEFAULT_CLIENT_SECRET: &str = "sentinel-local-client-secret";
const DEFAULT_CLIENT_DESCRIPTION: &str = "Local client for the Alon Sentinel API";
const REQUIRED_SCOPES: [&str; 2] = ["sites:read", "sites:write"];

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = config::BootstrapConfig::from_args_and_env()?;
    let _log_guard = logging::init_logging(
        &bootstrap.log_dir,
        &bootstrap.log_level,
        bootstrap.log_max_files,
    );

    dotenv().ok();

    info!("Sentinel installation provisioning started");

    let config = config::Config::from_env()?;
    let pool = db::pool::create_pool(&config).await?;

    let client_name = env_or_default("SEED_CLIENT_NAME", DEFAULT_CLIENT_NAME);
    let client_id = env_or_default("SEED_CLIENT_ID", DEFAULT_CLIENT_ID);
    let client_secret = env_or_default("SEED_CLIENT_SECRET", DEFAULT_CLIENT_SECRET);
    let client_description = env_or_default("SEED_CLIENT_DESCRIPTION", DEFAULT_CLIENT_DESCRIPTION);

    let client_secret_hash = AuthService::hash_client_secret(&client_secret)?;
    let secret_prefix = client_secret.chars().take(12).collect::<String>();

    let api_client = api_auth::repository::upsert_api_client(
        &pool,
        api_auth::repository::NewApiClient {
            name: &client_name,
            description: Some(&client_description),
            client_type: ApiClientType::InstallationClient,
            client_id: &client_id,
            client_secret_hash: &client_secret_hash,
            secret_prefix: &secret_prefix,
            created_by_user_id: Some("provision_client"),
        },
    )
    .await?;

    for scope in REQUIRED_SCOPES {
        api_auth::repository::create_api_client_scope(&pool, api_client.id, scope).await?;
    }

    println!();
    println!("Sentinel installation provisioning completed.");
    println!("API client:");
    println!("  name: {}", api_client.name);
    println!("  client_id: {}", api_client.client_id);
    println!("  scopes: {}", REQUIRED_SCOPES.join(", "));
    println!();
    println!("Use these values in any Sentinel client integration:");
    println!("SENTINEL_BASE_URL=http://{}", config.api_bind_address);
    println!("SENTINEL_CLIENT_ID={}", api_client.client_id);
    println!("SENTINEL_CLIENT_SECRET={}", client_secret);
    println!("SENTINEL_TIMEOUT=10");
    println!();
    println!("To get an access token:");
    println!("POST http://{}/v1/auth/token", config.api_bind_address);
    println!(
        r#"{{"client_id":"{}","client_secret":"{}"}}"#,
        api_client.client_id, client_secret
    );

    Ok(())
}

fn env_or_default(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}
