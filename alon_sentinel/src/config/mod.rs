use anyhow::Result;
use std::{env, ffi::OsString, net::IpAddr, path::PathBuf};

use crate::crypto::WebhookSecretEncryptionKey;

type EnvGetter<'a> = dyn for<'b> Fn(&'b str) -> std::result::Result<String, env::VarError> + 'a;

const DEFAULT_SITE_WORKER_COUNT: usize = 3;
const DEFAULT_MAX_HTTP_CONCURRENT_CHECKS: usize = 10;
const DEFAULT_DUE_SITES_BATCH_SIZE: usize = 100;
const DEFAULT_DB_MAX_CONNECTIONS: u32 = 20;
const DEFAULT_DB_MIN_CONNECTIONS: u32 = 2;
const DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS: u64 = 5;
const DEFAULT_DB_IDLE_TIMEOUT_SECONDS: u64 = 600;
const DEFAULT_DB_MAX_LIFETIME_SECONDS: u64 = 1800;
const DEFAULT_WORKER_MAX_POLL_INTERVAL_MS: u64 = 5000;
const DEFAULT_LEASE_SITE_CHECK_SECONDS: usize = 60;
const DEFAULT_HTTP_CHECK_TIMEOUT_SECONDS: usize = 10;
const DEFAULT_HTTP_CHECK_MAX_RESPONSE_BODY_BYTES: usize = 64 * 1024;
const DEFAULT_HTTP_CHECK_MAX_ATTEMPTS: usize = 3;
const DEFAULT_HTTP_CHECK_RETRY_DELAYS_MS: [u64; 2] = [1000, 3000];
const DEFAULT_HTTP_CHECK_RETRY_JITTER_PERCENT: usize = 20;
const DEFAULT_SITE_MONITOR_CHECK_RETENTION_DAYS: usize = 90;
const DEFAULT_SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS: usize = 3600;
const DEFAULT_SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE: usize = 5000;
const DEFAULT_DUE_NOTIFICATION_BATCH_SIZE: usize = 100;
const DEFAULT_MAX_NOTIFICATION_CONCURRENT_DELIVERIES: usize = 10;
const DEFAULT_LEASE_NOTIFICATION_DELIVERY_SECONDS: usize = 60;
const DEFAULT_NOTIFICATION_DELIVERY_TIMEOUT_SECONDS: usize = 10;
const DEFAULT_NOTIFICATION_MAX_ATTEMPTS: usize = 5;
const DEFAULT_NOTIFICATION_RETRY_BASE_SECONDS: usize = 30;
const DEFAULT_AUTH_RATE_LIMIT_MAX_REQUESTS: usize = 10;
const DEFAULT_AUTH_RATE_LIMIT_WINDOW_SECONDS: usize = 60;
const DEFAULT_SMTP_PORT: u16 = 587;
const DEFAULT_API_BIND_ADDRESS: &str = "127.0.0.1:3000";
const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: usize = 3600;
const DEFAULT_TRUST_PROXY_HEADERS: bool = false;
const DEFAULT_COOKIE_SECURE: bool = true;
const DEFAULT_HTTP_MONITOR_ALLOW_PRIVATE_TARGETS: bool = false;
const DEFAULT_LOG_MAX_FILES: usize = 30;
const DEFAULT_CORS_ALLOWED_ORIGINS: [&str; 4] = [
    "http://127.0.0.1:5173",
    "http://localhost:5173",
    "http://127.0.0.1:4173",
    "http://localhost:4173",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPoolRole {
    Api,
    Worker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbPoolSettings {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout_seconds: u64,
    pub idle_timeout_seconds: u64,
    pub max_lifetime_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
    pub from_email: String,
    pub from_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub api_bind_address: String,
    pub access_token_ttl_seconds: usize,
    pub cors_allowed_origins: Vec<String>,
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_seconds: u64,
    pub db_idle_timeout_seconds: u64,
    pub db_max_lifetime_seconds: u64,
    pub api_db_max_connections: u32,
    pub api_db_min_connections: u32,
    pub worker_db_max_connections: u32,
    pub worker_db_min_connections: u32,
    pub worker_max_poll_interval_ms: u64,
    pub site_worker_count: usize,
    pub max_http_concurrent_checks: usize,
    pub due_sites_batch_size: usize,
    pub lease_site_check_seconds: usize,
    pub http_check_timeout_seconds: usize,
    pub http_check_max_response_body_bytes: usize,
    pub http_check_max_attempts: usize,
    pub http_check_retry_delays_ms: Vec<u64>,
    pub http_check_retry_jitter_percent: usize,
    pub site_monitor_check_retention_days: usize,
    pub site_monitor_check_retention_interval_seconds: usize,
    pub site_monitor_check_retention_batch_size: usize,
    pub due_notification_batch_size: usize,
    pub max_notification_concurrent_deliveries: usize,
    pub lease_notification_delivery_seconds: usize,
    pub notification_delivery_timeout_seconds: usize,
    pub notification_max_attempts: usize,
    pub notification_retry_base_seconds: usize,
    pub auth_rate_limit_max_requests: usize,
    pub auth_rate_limit_window_seconds: usize,
    pub trust_proxy_headers: bool,
    pub trusted_proxy_ips: Vec<IpAddr>,
    pub cookie_secure: bool,
    pub http_monitor_allow_private_targets: bool,
    pub webhook_secret_encryption_key: WebhookSecretEncryptionKey,
    pub smtp: Option<SmtpConfig>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let get_env = |key: &str| env::var(key);
        Self::from_env_getter(&get_env)
    }

    fn from_env_getter(get_env: &EnvGetter<'_>) -> Result<Self> {
        let database_url = get_env("DATABASE_URL")?;
        let api_bind_address =
            get_env("API_BIND_ADDRESS").unwrap_or_else(|_| DEFAULT_API_BIND_ADDRESS.to_string());
        let access_token_ttl_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "ACCESS_TOKEN_TTL_SECONDS",
            DEFAULT_ACCESS_TOKEN_TTL_SECONDS,
        )?;
        let cors_allowed_origins = parse_env_csv_or_default_with(
            get_env,
            "CORS_ALLOWED_ORIGINS",
            &DEFAULT_CORS_ALLOWED_ORIGINS,
        );
        let db_max_connections = parse_env_positive_u32_or_default_with(
            get_env,
            "DB_MAX_CONNECTIONS",
            DEFAULT_DB_MAX_CONNECTIONS,
        )?;
        let db_min_connections = parse_env_u32_or_default_with(
            get_env,
            "DB_MIN_CONNECTIONS",
            DEFAULT_DB_MIN_CONNECTIONS,
        )?;
        let db_acquire_timeout_seconds = parse_env_positive_u64_or_default_with(
            get_env,
            "DB_ACQUIRE_TIMEOUT_SECONDS",
            DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS,
        )?;
        let db_idle_timeout_seconds = parse_env_positive_u64_or_default_with(
            get_env,
            "DB_IDLE_TIMEOUT_SECONDS",
            DEFAULT_DB_IDLE_TIMEOUT_SECONDS,
        )?;
        let db_max_lifetime_seconds = parse_env_positive_u64_or_default_with(
            get_env,
            "DB_MAX_LIFETIME_SECONDS",
            DEFAULT_DB_MAX_LIFETIME_SECONDS,
        )?;
        let api_db_max_connections = parse_env_positive_u32_or_default_with(
            get_env,
            "API_DB_MAX_CONNECTIONS",
            db_max_connections,
        )?;
        let api_db_min_connections =
            parse_env_u32_or_default_with(get_env, "API_DB_MIN_CONNECTIONS", db_min_connections)?;
        let worker_db_max_connections = parse_env_positive_u32_or_default_with(
            get_env,
            "WORKER_DB_MAX_CONNECTIONS",
            db_max_connections,
        )?;
        let worker_db_min_connections = parse_env_u32_or_default_with(
            get_env,
            "WORKER_DB_MIN_CONNECTIONS",
            db_min_connections,
        )?;
        let worker_max_poll_interval_ms = parse_env_positive_u64_or_default_with(
            get_env,
            "WORKER_MAX_POLL_INTERVAL_MS",
            DEFAULT_WORKER_MAX_POLL_INTERVAL_MS,
        )?;

        let site_worker_count = parse_env_positive_usize_or_default_with(
            get_env,
            "SITE_WORKER_COUNT",
            DEFAULT_SITE_WORKER_COUNT,
        )?;

        let max_http_concurrent_checks = parse_env_positive_usize_or_default_with(
            get_env,
            "MAX_HTTP_CONCURRENT_CHECKS",
            DEFAULT_MAX_HTTP_CONCURRENT_CHECKS,
        )?;

        let due_sites_batch_size = parse_env_positive_usize_or_default_with(
            get_env,
            "DUE_SITES_BATCH_SIZE",
            DEFAULT_DUE_SITES_BATCH_SIZE,
        )?;

        let lease_site_check_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "LEASE_SITE_CHECK_SECONDS",
            DEFAULT_LEASE_SITE_CHECK_SECONDS,
        )?;
        let http_check_timeout_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "HTTP_CHECK_TIMEOUT_SECONDS",
            DEFAULT_HTTP_CHECK_TIMEOUT_SECONDS,
        )?;
        let http_check_max_response_body_bytes = parse_env_positive_usize_or_default_with(
            get_env,
            "HTTP_CHECK_MAX_RESPONSE_BODY_BYTES",
            DEFAULT_HTTP_CHECK_MAX_RESPONSE_BODY_BYTES,
        )?;
        let http_check_max_attempts = parse_env_positive_usize_or_default_with(
            get_env,
            "HTTP_CHECK_MAX_ATTEMPTS",
            DEFAULT_HTTP_CHECK_MAX_ATTEMPTS,
        )?;
        let http_check_retry_delays_ms = parse_env_positive_u64_csv_or_default_with(
            get_env,
            "HTTP_CHECK_RETRY_DELAYS_MS",
            &DEFAULT_HTTP_CHECK_RETRY_DELAYS_MS,
        )?;
        let http_check_retry_jitter_percent = parse_env_usize_or_default_with(
            get_env,
            "HTTP_CHECK_RETRY_JITTER_PERCENT",
            DEFAULT_HTTP_CHECK_RETRY_JITTER_PERCENT,
        )?;
        let site_monitor_check_retention_days = parse_env_positive_usize_or_default_with(
            get_env,
            "SITE_MONITOR_CHECK_RETENTION_DAYS",
            DEFAULT_SITE_MONITOR_CHECK_RETENTION_DAYS,
        )?;
        let site_monitor_check_retention_interval_seconds =
            parse_env_positive_usize_or_default_with(
                get_env,
                "SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS",
                DEFAULT_SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS,
            )?;
        let site_monitor_check_retention_batch_size = parse_env_positive_usize_or_default_with(
            get_env,
            "SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE",
            DEFAULT_SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE,
        )?;
        let due_notification_batch_size = parse_env_positive_usize_or_default_with(
            get_env,
            "DUE_NOTIFICATION_BATCH_SIZE",
            DEFAULT_DUE_NOTIFICATION_BATCH_SIZE,
        )?;
        let max_notification_concurrent_deliveries = parse_env_positive_usize_or_default_with(
            get_env,
            "MAX_NOTIFICATION_CONCURRENT_DELIVERIES",
            DEFAULT_MAX_NOTIFICATION_CONCURRENT_DELIVERIES,
        )?;
        let lease_notification_delivery_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "LEASE_NOTIFICATION_DELIVERY_SECONDS",
            DEFAULT_LEASE_NOTIFICATION_DELIVERY_SECONDS,
        )?;
        let notification_delivery_timeout_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "NOTIFICATION_DELIVERY_TIMEOUT_SECONDS",
            DEFAULT_NOTIFICATION_DELIVERY_TIMEOUT_SECONDS,
        )?;
        let notification_max_attempts = parse_env_positive_usize_or_default_with(
            get_env,
            "NOTIFICATION_MAX_ATTEMPTS",
            DEFAULT_NOTIFICATION_MAX_ATTEMPTS,
        )?;
        let notification_retry_base_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "NOTIFICATION_RETRY_BASE_SECONDS",
            DEFAULT_NOTIFICATION_RETRY_BASE_SECONDS,
        )?;
        let auth_rate_limit_max_requests = parse_env_positive_usize_or_default_with(
            get_env,
            "AUTH_RATE_LIMIT_MAX_REQUESTS",
            DEFAULT_AUTH_RATE_LIMIT_MAX_REQUESTS,
        )?;
        let auth_rate_limit_window_seconds = parse_env_positive_usize_or_default_with(
            get_env,
            "AUTH_RATE_LIMIT_WINDOW_SECONDS",
            DEFAULT_AUTH_RATE_LIMIT_WINDOW_SECONDS,
        )?;
        let trust_proxy_headers = parse_env_bool_or_default_with(
            get_env,
            "TRUST_PROXY_HEADERS",
            DEFAULT_TRUST_PROXY_HEADERS,
        )?;
        let trusted_proxy_ips =
            parse_env_ip_csv_or_default_with(get_env, "TRUSTED_PROXY_IPS", &[])?;
        let cookie_secure =
            parse_env_bool_or_default_with(get_env, "COOKIE_SECURE", DEFAULT_COOKIE_SECURE)?;
        let http_monitor_allow_private_targets = parse_env_bool_or_default_with(
            get_env,
            "HTTP_MONITOR_ALLOW_PRIVATE_TARGETS",
            DEFAULT_HTTP_MONITOR_ALLOW_PRIVATE_TARGETS,
        )?;

        validate_db_pool_settings(db_max_connections, db_min_connections)?;
        validate_db_pool_settings(api_db_max_connections, api_db_min_connections)?;
        validate_db_pool_settings(worker_db_max_connections, worker_db_min_connections)?;
        validate_http_check_retry_policy(http_check_max_attempts, &http_check_retry_delays_ms)?;
        validate_http_check_retry_jitter_percent(http_check_retry_jitter_percent)?;
        validate_trusted_proxy_settings(trust_proxy_headers, &trusted_proxy_ips)?;
        let webhook_secret_encryption_key = load_webhook_secret_encryption_key_with(get_env)?;
        let smtp = load_smtp_config_with(get_env)?;

        Ok(Self {
            database_url,
            api_bind_address,
            access_token_ttl_seconds,
            cors_allowed_origins,
            db_max_connections,
            db_min_connections,
            db_acquire_timeout_seconds,
            db_idle_timeout_seconds,
            db_max_lifetime_seconds,
            api_db_max_connections,
            api_db_min_connections,
            worker_db_max_connections,
            worker_db_min_connections,
            worker_max_poll_interval_ms,
            site_worker_count,
            max_http_concurrent_checks,
            due_sites_batch_size,
            lease_site_check_seconds,
            http_check_timeout_seconds,
            http_check_max_response_body_bytes,
            http_check_max_attempts,
            http_check_retry_delays_ms,
            http_check_retry_jitter_percent,
            site_monitor_check_retention_days,
            site_monitor_check_retention_interval_seconds,
            site_monitor_check_retention_batch_size,
            due_notification_batch_size,
            max_notification_concurrent_deliveries,
            lease_notification_delivery_seconds,
            notification_delivery_timeout_seconds,
            notification_max_attempts,
            notification_retry_base_seconds,
            auth_rate_limit_max_requests,
            auth_rate_limit_window_seconds,
            trust_proxy_headers,
            trusted_proxy_ips,
            cookie_secure,
            http_monitor_allow_private_targets,
            webhook_secret_encryption_key,
            smtp,
        })
    }

    pub fn db_pool_settings(&self, role: DbPoolRole) -> DbPoolSettings {
        let (max_connections, min_connections) = match role {
            DbPoolRole::Api => (self.api_db_max_connections, self.api_db_min_connections),
            DbPoolRole::Worker => (
                self.worker_db_max_connections,
                self.worker_db_min_connections,
            ),
        };

        DbPoolSettings {
            max_connections,
            min_connections,
            acquire_timeout_seconds: self.db_acquire_timeout_seconds,
            idle_timeout_seconds: self.db_idle_timeout_seconds,
            max_lifetime_seconds: self.db_max_lifetime_seconds,
        }
    }
}

fn parse_env_usize_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: usize,
) -> Result<usize> {
    match get_env(key) {
        Ok(value) => Ok(value.parse::<usize>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_positive_usize_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: usize,
) -> Result<usize> {
    let value = parse_env_usize_or_default_with(get_env, key, default)?;

    if value == 0 {
        anyhow::bail!("{key} must be greater than 0");
    }

    Ok(value)
}

fn parse_env_u32_or_default_with(get_env: &EnvGetter<'_>, key: &str, default: u32) -> Result<u32> {
    match get_env(key) {
        Ok(value) => Ok(value.parse::<u32>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_positive_u32_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: u32,
) -> Result<u32> {
    let value = parse_env_u32_or_default_with(get_env, key, default)?;

    if value == 0 {
        anyhow::bail!("{key} must be greater than 0");
    }

    Ok(value)
}

fn parse_env_u16_or_default_with(get_env: &EnvGetter<'_>, key: &str, default: u16) -> Result<u16> {
    match get_env(key) {
        Ok(value) => Ok(value.parse::<u16>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_u64_or_default_with(get_env: &EnvGetter<'_>, key: &str, default: u64) -> Result<u64> {
    match get_env(key) {
        Ok(value) => Ok(value.parse::<u64>()?),
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_positive_u64_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: u64,
) -> Result<u64> {
    let value = parse_env_u64_or_default_with(get_env, key, default)?;

    if value == 0 {
        anyhow::bail!("{key} must be greater than 0");
    }

    Ok(value)
}

fn parse_env_positive_u64_csv_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: &[u64],
) -> Result<Vec<u64>> {
    match get_env(key) {
        Ok(value) => {
            let mut values = Vec::new();

            for item in value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
            {
                let parsed = item.parse::<u64>()?;

                if parsed == 0 {
                    anyhow::bail!("{key} values must be greater than 0");
                }

                values.push(parsed);
            }

            Ok(values)
        }
        Err(env::VarError::NotPresent) => Ok(default.to_vec()),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_bool_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: bool,
) -> Result<bool> {
    match get_env(key) {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => anyhow::bail!("{key} must be a boolean"),
        },
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(err.into()),
    }
}

fn parse_env_ip_csv_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: &[IpAddr],
) -> Result<Vec<IpAddr>> {
    match get_env(key) {
        Ok(value) => value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(|item| {
                item.parse::<IpAddr>()
                    .map_err(|_| anyhow::anyhow!("{key} must contain valid IP addresses"))
            })
            .collect(),
        Err(env::VarError::NotPresent) => Ok(default.to_vec()),
        Err(err) => Err(err.into()),
    }
}

fn load_webhook_secret_encryption_key_with(
    get_env: &EnvGetter<'_>,
) -> Result<WebhookSecretEncryptionKey> {
    let value = get_env("WEBHOOK_SECRET_ENCRYPTION_KEY")
        .map_err(|_| anyhow::anyhow!("WEBHOOK_SECRET_ENCRYPTION_KEY is required"))?;
    let value = value.trim();

    if value.is_empty() {
        anyhow::bail!("WEBHOOK_SECRET_ENCRYPTION_KEY is required");
    }

    WebhookSecretEncryptionKey::from_hex(value)
}

fn validate_db_pool_settings(max_connections: u32, min_connections: u32) -> Result<()> {
    if min_connections > max_connections {
        anyhow::bail!("DB_MIN_CONNECTIONS must be less than or equal to DB_MAX_CONNECTIONS");
    }

    Ok(())
}

fn validate_trusted_proxy_settings(
    trust_proxy_headers: bool,
    trusted_proxy_ips: &[IpAddr],
) -> Result<()> {
    if trust_proxy_headers && trusted_proxy_ips.is_empty() {
        anyhow::bail!("TRUSTED_PROXY_IPS is required when TRUST_PROXY_HEADERS is enabled");
    }

    Ok(())
}

fn validate_http_check_retry_policy(max_attempts: usize, retry_delays_ms: &[u64]) -> Result<()> {
    let required_retry_count = max_attempts.saturating_sub(1);

    if retry_delays_ms.len() < required_retry_count {
        anyhow::bail!(
            "HTTP_CHECK_RETRY_DELAYS_MS must provide at least {required_retry_count} delay values for HTTP_CHECK_MAX_ATTEMPTS={max_attempts}"
        );
    }

    Ok(())
}

fn validate_http_check_retry_jitter_percent(jitter_percent: usize) -> Result<()> {
    if jitter_percent > 100 {
        anyhow::bail!("HTTP_CHECK_RETRY_JITTER_PERCENT must be between 0 and 100");
    }

    Ok(())
}

fn parse_env_csv_or_default_with(
    get_env: &EnvGetter<'_>,
    key: &str,
    default: &[&str],
) -> Vec<String> {
    match get_env(key) {
        Ok(value) => value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        Err(env::VarError::NotPresent) => {
            default.iter().map(|value| (*value).to_string()).collect()
        }
        Err(_) => default.iter().map(|value| (*value).to_string()).collect(),
    }
}

fn load_smtp_config_with(get_env: &EnvGetter<'_>) -> Result<Option<SmtpConfig>> {
    let host = match get_env("SMTP_HOST") {
        Ok(value) if !value.trim().is_empty() => value,
        Ok(_) => return Ok(None),
        Err(env::VarError::NotPresent) => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let port = parse_env_u16_or_default_with(get_env, "SMTP_PORT", DEFAULT_SMTP_PORT)?;
    let username = get_env("SMTP_USERNAME")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let password = get_env("SMTP_PASSWORD")
        .ok()
        .filter(|value| !value.is_empty());
    let from_email = get_env("SMTP_FROM_EMAIL")?;
    let from_name = get_env("SMTP_FROM_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty());

    Ok(Some(SmtpConfig {
        host,
        port,
        username,
        password,
        from_email,
        from_name,
    }))
}

pub struct BootstrapConfig {
    pub log_dir: PathBuf,
    pub log_level: String,
    pub log_max_files: usize,
}

impl BootstrapConfig {
    pub fn from_args_and_env() -> anyhow::Result<Self> {
        let args_os = env::args_os().skip(1).collect::<Vec<_>>();
        let args = env::args().skip(1).collect::<Vec<_>>();

        Ok(Self::from_sources(
            &args_os,
            &args,
            env::var_os("LOG_DIR"),
            env::var("LOG_LEVEL").ok(),
            env::var("LOG_MAX_FILES").ok(),
        ))
    }

    fn from_sources(
        args_os: &[OsString],
        args: &[String],
        env_log_dir: Option<OsString>,
        env_log_level: Option<String>,
        env_log_max_files: Option<String>,
    ) -> Self {
        let log_dir = args_os
            .windows(2)
            .find(|w| w[0] == "--log-dir")
            .map(|w| PathBuf::from(&w[1]))
            .or_else(|| env_log_dir.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("./logs"));

        let log_level = args
            .windows(2)
            .find(|w| w[0] == "--log-level")
            .map(|w| w[1].clone())
            .or(env_log_level)
            .unwrap_or_else(|| "info".to_string());

        let log_max_files = args
            .windows(2)
            .find(|w| w[0] == "--log-max-files")
            .and_then(|w| w[1].parse::<usize>().ok())
            .or_else(|| env_log_max_files.and_then(|v| v.parse::<usize>().ok()))
            .unwrap_or(DEFAULT_LOG_MAX_FILES);

        Self {
            log_dir,
            log_level,
            log_max_files,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX: &str =
        "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";
    type TestEnv = HashMap<&'static str, String>;

    #[test]
    fn parse_env_usize_or_default_returns_present_value() {
        with_config_env(|env| {
            set_env(env, "SITE_WORKER_COUNT", "7");
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };

            let value = parse_env_usize_or_default_with(&get_env, "SITE_WORKER_COUNT", 3)
                .expect("should parse present numeric value");

            assert_eq!(value, 7);
        });
    }

    #[test]
    fn parse_env_usize_or_default_returns_default_when_value_is_missing() {
        with_config_env(|env| {
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };
            let value = parse_env_usize_or_default_with(&get_env, "SITE_WORKER_COUNT", 3)
                .expect("should fall back to default");

            assert_eq!(value, 3);
        });
    }

    #[test]
    fn parse_env_usize_or_default_returns_error_for_invalid_value() {
        with_config_env(|env| {
            set_env(env, "SITE_WORKER_COUNT", "not-a-number");
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };

            let result = parse_env_usize_or_default_with(&get_env, "SITE_WORKER_COUNT", 3);

            assert!(result.is_err());
        });
    }

    #[test]
    fn parse_env_positive_usize_or_default_returns_error_for_zero() {
        with_config_env(|env| {
            set_env(env, "SITE_WORKER_COUNT", "0");
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };

            let result = parse_env_positive_usize_or_default_with(&get_env, "SITE_WORKER_COUNT", 3);

            assert!(result.is_err());
        });
    }

    #[test]
    fn parse_env_positive_u64_csv_or_default_returns_present_values() {
        with_config_env(|env| {
            set_env(env, "HTTP_CHECK_RETRY_DELAYS_MS", "250, 750, 1500");
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };

            let values = parse_env_positive_u64_csv_or_default_with(
                &get_env,
                "HTTP_CHECK_RETRY_DELAYS_MS",
                &[1000, 3000],
            )
            .expect("should parse CSV delay values");

            assert_eq!(values, vec![250, 750, 1500]);
        });
    }

    #[test]
    fn parse_env_positive_u64_csv_or_default_returns_error_for_zero_value() {
        with_config_env(|env| {
            set_env(env, "HTTP_CHECK_RETRY_DELAYS_MS", "1000,0");
            let get_env = |key: &str| match env.get(key) {
                Some(value) => Ok(value.clone()),
                None => Err(std::env::VarError::NotPresent),
            };

            let result = parse_env_positive_u64_csv_or_default_with(
                &get_env,
                "HTTP_CHECK_RETRY_DELAYS_MS",
                &[1000],
            );

            assert!(result.is_err());
        });
    }

    #[test]
    fn validate_http_check_retry_jitter_percent_rejects_values_above_one_hundred() {
        let result = validate_http_check_retry_jitter_percent(101);

        assert!(result.is_err());
    }

    #[test]
    fn from_env_returns_error_when_database_url_is_missing() {
        with_config_env(|env| {
            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_uses_defaults_for_optional_values() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "API_BIND_ADDRESS", "127.0.0.1:4000");
            set_env(env, "ACCESS_TOKEN_TTL_SECONDS", "7200");

            let config = config_from_env(env).expect("config should load with defaults");

            assert_eq!(
                config.database_url,
                "postgresql://postgres:secret@localhost/alon_sentinel_db"
            );
            assert_eq!(config.api_bind_address, "127.0.0.1:4000");
            assert_eq!(config.access_token_ttl_seconds, 7200);
            assert_eq!(config.db_max_connections, DEFAULT_DB_MAX_CONNECTIONS);
            assert_eq!(config.db_min_connections, DEFAULT_DB_MIN_CONNECTIONS);
            assert_eq!(
                config.db_acquire_timeout_seconds,
                DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS
            );
            assert_eq!(
                config.db_idle_timeout_seconds,
                DEFAULT_DB_IDLE_TIMEOUT_SECONDS
            );
            assert_eq!(
                config.db_max_lifetime_seconds,
                DEFAULT_DB_MAX_LIFETIME_SECONDS
            );
            assert_eq!(config.api_db_max_connections, DEFAULT_DB_MAX_CONNECTIONS);
            assert_eq!(config.api_db_min_connections, DEFAULT_DB_MIN_CONNECTIONS);
            assert_eq!(config.worker_db_max_connections, DEFAULT_DB_MAX_CONNECTIONS);
            assert_eq!(config.worker_db_min_connections, DEFAULT_DB_MIN_CONNECTIONS);
            assert_eq!(
                config.worker_max_poll_interval_ms,
                DEFAULT_WORKER_MAX_POLL_INTERVAL_MS
            );
            assert_eq!(config.site_worker_count, DEFAULT_SITE_WORKER_COUNT);
            assert_eq!(
                config.max_http_concurrent_checks,
                DEFAULT_MAX_HTTP_CONCURRENT_CHECKS
            );
            assert_eq!(config.due_sites_batch_size, DEFAULT_DUE_SITES_BATCH_SIZE);
            assert_eq!(
                config.lease_site_check_seconds,
                DEFAULT_LEASE_SITE_CHECK_SECONDS
            );
            assert_eq!(
                config.http_check_timeout_seconds,
                DEFAULT_HTTP_CHECK_TIMEOUT_SECONDS
            );
            assert_eq!(
                config.http_check_max_attempts,
                DEFAULT_HTTP_CHECK_MAX_ATTEMPTS
            );
            assert_eq!(
                config.http_check_retry_delays_ms,
                DEFAULT_HTTP_CHECK_RETRY_DELAYS_MS.to_vec()
            );
            assert_eq!(
                config.http_check_retry_jitter_percent,
                DEFAULT_HTTP_CHECK_RETRY_JITTER_PERCENT
            );
            assert_eq!(
                config.site_monitor_check_retention_days,
                DEFAULT_SITE_MONITOR_CHECK_RETENTION_DAYS
            );
            assert_eq!(
                config.site_monitor_check_retention_interval_seconds,
                DEFAULT_SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS
            );
            assert_eq!(
                config.site_monitor_check_retention_batch_size,
                DEFAULT_SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE
            );
            assert_eq!(
                config.due_notification_batch_size,
                DEFAULT_DUE_NOTIFICATION_BATCH_SIZE
            );
            assert_eq!(
                config.max_notification_concurrent_deliveries,
                DEFAULT_MAX_NOTIFICATION_CONCURRENT_DELIVERIES
            );
            assert_eq!(
                config.lease_notification_delivery_seconds,
                DEFAULT_LEASE_NOTIFICATION_DELIVERY_SECONDS
            );
            assert_eq!(
                config.notification_delivery_timeout_seconds,
                DEFAULT_NOTIFICATION_DELIVERY_TIMEOUT_SECONDS
            );
            assert_eq!(
                config.notification_max_attempts,
                DEFAULT_NOTIFICATION_MAX_ATTEMPTS
            );
            assert_eq!(
                config.notification_retry_base_seconds,
                DEFAULT_NOTIFICATION_RETRY_BASE_SECONDS
            );
            assert_eq!(
                config.auth_rate_limit_max_requests,
                DEFAULT_AUTH_RATE_LIMIT_MAX_REQUESTS
            );
            assert_eq!(
                config.auth_rate_limit_window_seconds,
                DEFAULT_AUTH_RATE_LIMIT_WINDOW_SECONDS
            );
            assert!(!config.trust_proxy_headers);
            assert!(config.trusted_proxy_ips.is_empty());
            assert_eq!(
                config.webhook_secret_encryption_key,
                WebhookSecretEncryptionKey::from_hex(TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX)
                    .expect("test webhook secret key should parse")
            );
            assert!(config.smtp.is_none());
        });
    }

    #[test]
    fn from_env_returns_error_when_webhook_secret_encryption_key_is_missing() {
        with_config_env(|env| {
            set_env(
                env,
                "DATABASE_URL",
                "postgresql://postgres:secret@localhost/alon_sentinel_db",
            );

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_loads_required_webhook_secret_encryption_key() {
        with_config_env(|env| {
            set_minimal_config_env(env);

            let config = config_from_env(env).expect("config should load webhook secret key");

            assert_eq!(
                config.webhook_secret_encryption_key,
                WebhookSecretEncryptionKey::from_hex(TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX)
                    .expect("test webhook secret key should parse")
            );
        });
    }

    #[test]
    fn from_env_loads_explicit_site_monitor_check_retention_settings() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "SITE_MONITOR_CHECK_RETENTION_DAYS", "30");
            set_env(env, "SITE_MONITOR_CHECK_RETENTION_INTERVAL_SECONDS", "1800");
            set_env(env, "SITE_MONITOR_CHECK_RETENTION_BATCH_SIZE", "2500");

            let config = config_from_env(env).expect("config should load retention settings");

            assert_eq!(config.site_monitor_check_retention_days, 30);
            assert_eq!(config.site_monitor_check_retention_interval_seconds, 1800);
            assert_eq!(config.site_monitor_check_retention_batch_size, 2500);
        });
    }

    #[test]
    fn from_env_loads_explicit_auth_rate_limit_settings() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "AUTH_RATE_LIMIT_MAX_REQUESTS", "25");
            set_env(env, "AUTH_RATE_LIMIT_WINDOW_SECONDS", "120");

            let config = config_from_env(env).expect("config should load auth rate limit settings");

            assert_eq!(config.auth_rate_limit_max_requests, 25);
            assert_eq!(config.auth_rate_limit_window_seconds, 120);
        });
    }

    #[test]
    fn from_env_loads_trusted_proxy_settings() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "TRUST_PROXY_HEADERS", "true");
            set_env(env, "TRUSTED_PROXY_IPS", "127.0.0.1, 10.0.0.10");

            let config = config_from_env(env).expect("config should load trusted proxy settings");

            assert!(config.trust_proxy_headers);
            assert_eq!(
                config.trusted_proxy_ips,
                vec![
                    "127.0.0.1"
                        .parse::<IpAddr>()
                        .expect("loopback should parse"),
                    "10.0.0.10"
                        .parse::<IpAddr>()
                        .expect("private IP should parse")
                ]
            );
        });
    }

    #[test]
    fn from_env_returns_error_when_trusted_proxy_headers_enabled_without_trusted_ips() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "TRUST_PROXY_HEADERS", "true");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_returns_error_for_invalid_trusted_proxy_ip() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "TRUSTED_PROXY_IPS", "not-an-ip");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_loads_smtp_when_host_is_present() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "SMTP_HOST", "smtp.test.com");
            set_env(env, "SMTP_PORT", "2525");
            set_env(env, "SMTP_USERNAME", "monitor");
            set_env(env, "SMTP_PASSWORD", "secret");
            set_env(env, "SMTP_FROM_EMAIL", "alerts@test.com");
            set_env(env, "SMTP_FROM_NAME", "Alon Sentinel");

            let config = config_from_env(env).expect("config should load smtp");
            let smtp = config.smtp.expect("smtp config should be present");

            assert_eq!(smtp.host, "smtp.test.com");
            assert_eq!(smtp.port, 2525);
            assert_eq!(smtp.username.as_deref(), Some("monitor"));
            assert_eq!(smtp.password.as_deref(), Some("secret"));
            assert_eq!(smtp.from_email, "alerts@test.com");
            assert_eq!(smtp.from_name.as_deref(), Some("Alon Sentinel"));
        });
    }

    #[test]
    fn from_env_returns_error_for_invalid_numeric_value() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "DB_MAX_CONNECTIONS", "not-a-number");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_loads_explicit_db_pool_settings() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "DB_MAX_CONNECTIONS", "25");
            set_env(env, "DB_MIN_CONNECTIONS", "4");
            set_env(env, "DB_ACQUIRE_TIMEOUT_SECONDS", "7");
            set_env(env, "DB_IDLE_TIMEOUT_SECONDS", "900");
            set_env(env, "DB_MAX_LIFETIME_SECONDS", "3600");
            set_env(env, "WORKER_MAX_POLL_INTERVAL_MS", "1500");

            let config =
                config_from_env(env).expect("config should load explicit db pool settings");

            assert_eq!(config.db_max_connections, 25);
            assert_eq!(config.db_min_connections, 4);
            assert_eq!(config.db_acquire_timeout_seconds, 7);
            assert_eq!(config.db_idle_timeout_seconds, 900);
            assert_eq!(config.db_max_lifetime_seconds, 3600);
            assert_eq!(config.api_db_max_connections, 25);
            assert_eq!(config.api_db_min_connections, 4);
            assert_eq!(config.worker_db_max_connections, 25);
            assert_eq!(config.worker_db_min_connections, 4);
            assert_eq!(config.worker_max_poll_interval_ms, 1500);
        });
    }

    #[test]
    fn from_env_loads_service_specific_db_pool_settings() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "DB_MAX_CONNECTIONS", "25");
            set_env(env, "DB_MIN_CONNECTIONS", "4");
            set_env(env, "API_DB_MAX_CONNECTIONS", "18");
            set_env(env, "API_DB_MIN_CONNECTIONS", "3");
            set_env(env, "WORKER_DB_MAX_CONNECTIONS", "6");
            set_env(env, "WORKER_DB_MIN_CONNECTIONS", "2");

            let config =
                config_from_env(env).expect("config should load service-specific db pool settings");

            assert_eq!(config.api_db_max_connections, 18);
            assert_eq!(config.api_db_min_connections, 3);
            assert_eq!(config.worker_db_max_connections, 6);
            assert_eq!(config.worker_db_min_connections, 2);
            assert_eq!(
                config.db_pool_settings(DbPoolRole::Api),
                DbPoolSettings {
                    max_connections: 18,
                    min_connections: 3,
                    acquire_timeout_seconds: DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS,
                    idle_timeout_seconds: DEFAULT_DB_IDLE_TIMEOUT_SECONDS,
                    max_lifetime_seconds: DEFAULT_DB_MAX_LIFETIME_SECONDS,
                }
            );
            assert_eq!(
                config.db_pool_settings(DbPoolRole::Worker),
                DbPoolSettings {
                    max_connections: 6,
                    min_connections: 2,
                    acquire_timeout_seconds: DEFAULT_DB_ACQUIRE_TIMEOUT_SECONDS,
                    idle_timeout_seconds: DEFAULT_DB_IDLE_TIMEOUT_SECONDS,
                    max_lifetime_seconds: DEFAULT_DB_MAX_LIFETIME_SECONDS,
                }
            );
        });
    }

    #[test]
    fn from_env_returns_error_for_zero_worker_setting() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "SITE_WORKER_COUNT", "0");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_returns_error_when_db_min_connections_exceeds_max() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "DB_MAX_CONNECTIONS", "3");
            set_env(env, "DB_MIN_CONNECTIONS", "4");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_returns_error_when_service_db_min_connections_exceeds_max() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "WORKER_DB_MAX_CONNECTIONS", "3");
            set_env(env, "WORKER_DB_MIN_CONNECTIONS", "4");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_returns_error_for_zero_http_timeout() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "HTTP_CHECK_TIMEOUT_SECONDS", "0");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_returns_error_when_retry_schedule_is_too_short() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "HTTP_CHECK_MAX_ATTEMPTS", "3");
            set_env(env, "HTTP_CHECK_RETRY_DELAYS_MS", "1000");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn from_env_allows_single_attempt_with_empty_retry_schedule() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "HTTP_CHECK_MAX_ATTEMPTS", "1");
            set_env(env, "HTTP_CHECK_RETRY_DELAYS_MS", "");

            let config = config_from_env(env).expect("single-attempt policy should be valid");

            assert_eq!(config.http_check_max_attempts, 1);
            assert!(config.http_check_retry_delays_ms.is_empty());
        });
    }

    #[test]
    fn from_env_returns_error_for_retry_jitter_above_one_hundred_percent() {
        with_config_env(|env| {
            set_minimal_config_env(env);
            set_env(env, "HTTP_CHECK_RETRY_JITTER_PERCENT", "101");

            let result = config_from_env(env);

            assert!(result.is_err());
        });
    }

    #[test]
    fn bootstrap_config_prefers_args_over_env() {
        let config = BootstrapConfig::from_sources(
            &[OsString::from("--log-dir"), OsString::from("custom-logs")],
            &[
                "--log-level".to_string(),
                "debug".to_string(),
                "--log-max-files".to_string(),
                "7".to_string(),
            ],
            Some(OsString::from("env-logs")),
            Some("warn".to_string()),
            Some("60".to_string()),
        );

        assert_eq!(config.log_dir, PathBuf::from("custom-logs"));
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.log_max_files, 7);
    }

    #[test]
    fn bootstrap_config_uses_env_when_args_are_missing() {
        let config = BootstrapConfig::from_sources(
            &[],
            &[],
            Some(OsString::from("env-logs")),
            Some("warn".to_string()),
            Some("14".to_string()),
        );

        assert_eq!(config.log_dir, PathBuf::from("env-logs"));
        assert_eq!(config.log_level, "warn");
        assert_eq!(config.log_max_files, 14);
    }

    #[test]
    fn bootstrap_config_uses_defaults_when_args_and_env_are_missing() {
        let config = BootstrapConfig::from_sources(&[], &[], None, None, None);

        assert_eq!(config.log_dir, PathBuf::from("./logs"));
        assert_eq!(config.log_level, "info");
        assert_eq!(config.log_max_files, DEFAULT_LOG_MAX_FILES);
    }

    fn with_config_env(test_fn: impl FnOnce(&mut TestEnv)) {
        let mut env = TestEnv::new();
        test_fn(&mut env);
    }

    fn set_minimal_config_env(env: &mut TestEnv) {
        set_env(
            env,
            "DATABASE_URL",
            "postgresql://postgres:secret@localhost/alon_sentinel_db",
        );
        set_env(
            env,
            "WEBHOOK_SECRET_ENCRYPTION_KEY",
            TEST_WEBHOOK_SECRET_ENCRYPTION_KEY_HEX,
        );
    }

    fn config_from_env(env: &TestEnv) -> Result<Config> {
        let get_env = |key: &str| match env.get(key) {
            Some(value) => Ok(value.clone()),
            None => Err(std::env::VarError::NotPresent),
        };

        Config::from_env_getter(&get_env)
    }

    fn set_env(env: &mut TestEnv, key: &'static str, value: &str) {
        env.insert(key, value.to_string());
    }
}
