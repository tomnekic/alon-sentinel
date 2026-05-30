use axum::extract::FromRef;
use sqlx::PgPool;
use std::net::IpAddr;

use crate::{
    api::rate_limit::{AuthRateLimiter, StatusPageCache},
    auth::{AuthConfig, AuthTokenCache},
    crypto::WebhookSecretEncryptionKey,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub auth_config: AuthConfig,
    pub auth_rate_limiter: AuthRateLimiter,
    pub auth_token_cache: AuthTokenCache,
    pub trust_proxy_headers: bool,
    pub trusted_proxy_ips: Vec<IpAddr>,
    pub cookie_secure: bool,
    pub http_monitor_allow_private_targets: bool,
    pub webhook_secret_encryption_key: WebhookSecretEncryptionKey,
    pub db_max_connections: u32,
    pub public_rate_limiter: AuthRateLimiter,
    pub status_page_cache: StatusPageCache,
}

impl FromRef<AppState> for PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl FromRef<AppState> for AuthConfig {
    fn from_ref(state: &AppState) -> Self {
        state.auth_config
    }
}
