use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

use crate::config::{self, DbPoolRole, DbPoolSettings};

pub async fn create_pool(config: &config::Config) -> Result<PgPool> {
    create_pool_with_settings(
        &config.database_url,
        DbPoolSettings {
            max_connections: config.db_max_connections,
            min_connections: config.db_min_connections,
            acquire_timeout_seconds: config.db_acquire_timeout_seconds,
            idle_timeout_seconds: config.db_idle_timeout_seconds,
            max_lifetime_seconds: config.db_max_lifetime_seconds,
        },
    )
    .await
}

pub async fn create_service_pool(config: &config::Config, role: DbPoolRole) -> Result<PgPool> {
    create_pool_with_settings(&config.database_url, config.db_pool_settings(role)).await
}

async fn create_pool_with_settings(database_url: &str, settings: DbPoolSettings) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(settings.max_connections)
        .min_connections(settings.min_connections)
        .acquire_timeout(Duration::from_secs(settings.acquire_timeout_seconds))
        .idle_timeout(Some(Duration::from_secs(settings.idle_timeout_seconds)))
        .max_lifetime(Some(Duration::from_secs(settings.max_lifetime_seconds)))
        .connect(database_url)
        .await?;
    Ok(pool)
}
