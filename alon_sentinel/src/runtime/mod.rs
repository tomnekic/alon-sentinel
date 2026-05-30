use anyhow::Result;
use axum::http::{HeaderValue, Method, header};
use dotenvy::dotenv;
use rand::random;
use sqlx::PgPool;
use std::{env, net::SocketAddr, path::PathBuf, process};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinSet;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tracing::{error, info};
use tracing_appender::non_blocking::WorkerGuard;

use crate::{
    api::{
        app::{AppState, build_router},
        rate_limit::{AuthRateLimiter, StatusPageCache},
    },
    auth::{AuthConfig, AuthTokenCache},
    config::{BootstrapConfig, Config, DbPoolRole},
    db, logging, worker,
};

struct RuntimeContext {
    config: Config,
    pool: PgPool,
    _log_guard: WorkerGuard,
}

pub async fn run_api() -> Result<()> {
    let runtime = initialize("API application", DbPoolRole::Api).await?;
    let bind_address = runtime.config.api_bind_address.clone();
    let listener = TcpListener::bind(&bind_address).await?;
    let app = build_router(AppState {
        pool: runtime.pool,
        auth_config: AuthConfig {
            access_token_ttl_seconds: runtime.config.access_token_ttl_seconds as i64,
        },
        auth_rate_limiter: AuthRateLimiter::new(
            runtime.config.auth_rate_limit_max_requests,
            std::time::Duration::from_secs(runtime.config.auth_rate_limit_window_seconds as u64),
        ),
        auth_token_cache: AuthTokenCache::new(),
        trust_proxy_headers: runtime.config.trust_proxy_headers,
        trusted_proxy_ips: runtime.config.trusted_proxy_ips.clone(),
        cookie_secure: runtime.config.cookie_secure,
        http_monitor_allow_private_targets: runtime.config.http_monitor_allow_private_targets,
        webhook_secret_encryption_key: runtime.config.webhook_secret_encryption_key,
        db_max_connections: runtime.config.api_db_max_connections,
        public_rate_limiter: AuthRateLimiter::new(60, std::time::Duration::from_secs(60)),
        status_page_cache: StatusPageCache::new(256, std::time::Duration::from_secs(30)),
    })
    .layer(build_cors_layer(&runtime.config.cors_allowed_origins)?);

    info!("API listening on {bind_address}");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        match wait_for_shutdown_signal().await {
            Ok(signal) => info!("API shutdown signal received: {signal}"),
            Err(error) => error!("API shutdown signal listener failed: {:?}", error),
        }
    })
    .await?;
    Ok(())
}

pub async fn run_worker() -> Result<()> {
    let runtime = initialize("Worker application", DbPoolRole::Worker).await?;
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut join_set = JoinSet::new();
    let worker_process_identity = build_worker_process_identity();

    for i in 0..runtime.config.site_worker_count {
        let pool = runtime.pool.clone();
        let config = runtime.config.clone();
        let worker_id = build_worker_id(i, &worker_process_identity);
        let shutdown_rx = shutdown_rx.clone();

        join_set.spawn(async move {
            if let Err(error) =
                worker::site_worker::run(&pool, &config, &worker_id, shutdown_rx).await
            {
                error!("Worker {} failed: {:?}", worker_id, error);
            }
        });
    }

    let shutdown_signal = wait_for_shutdown_signal().await?;
    info!("Shutdown signal received: {shutdown_signal}");

    let _ = shutdown_tx.send(true);

    while let Some(result) = join_set.join_next().await {
        if let Err(error) = result {
            error!("Worker task join error: {:?}", error);
        }
    }

    info!("Worker application stopped");
    Ok(())
}

fn build_worker_id(worker_index: usize, worker_process_identity: &str) -> String {
    format!("site-worker-{worker_index}-{worker_process_identity}")
}

fn build_worker_process_identity() -> String {
    let hostname = detect_hostname()
        .map(|value| sanitize_worker_id_component(&value))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown-host".to_string());

    format!("{hostname}-{}-{:016x}", process::id(), random::<u64>())
}

fn detect_hostname() -> Option<String> {
    env::var("HOSTNAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            env::var("COMPUTERNAME")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

fn sanitize_worker_id_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    let mut last_was_separator = false;

    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            sanitized.push(character.to_ascii_lowercase());
            last_was_separator = false;
        } else if !last_was_separator {
            sanitized.push('-');
            last_was_separator = true;
        }
    }

    sanitized.trim_matches('-').to_string()
}

async fn wait_for_shutdown_signal() -> Result<&'static str> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate())?;

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                result?;
                Ok("ctrl_c")
            }
            _ = terminate.recv() => Ok("sigterm"),
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
        Ok("ctrl_c")
    }
}

fn spawn_log_cleanup_task(log_dir: PathBuf, max_files: usize) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(24 * 3600));
        interval.tick().await; // consume the immediate first tick
        loop {
            interval.tick().await;
            logging::prune_old_log_files(&log_dir, max_files);
        }
    });
}

async fn initialize(service_name: &str, db_pool_role: DbPoolRole) -> Result<RuntimeContext> {
    let bootstrap = BootstrapConfig::from_args_and_env()?;
    let log_guard = logging::init_logging(
        &bootstrap.log_dir,
        &bootstrap.log_level,
        bootstrap.log_max_files,
    );
    spawn_log_cleanup_task(bootstrap.log_dir.clone(), bootstrap.log_max_files);

    dotenv().ok();

    info!("{service_name} started");

    let config = Config::from_env()?;
    let pool = db::pool::create_service_pool(&config, db_pool_role).await?;

    Ok(RuntimeContext {
        config,
        pool,
        _log_guard: log_guard,
    })
}

fn build_cors_layer(allowed_origins: &[String]) -> Result<CorsLayer> {
    let origins = allowed_origins
        .iter()
        .map(|origin| HeaderValue::from_str(origin))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let allow_origin = AllowOrigin::list(origins);

    Ok(CorsLayer::new()
        .allow_origin(allow_origin)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::ACCEPT,
            header::CONTENT_TYPE,
            header::USER_AGENT,
        ]))
}

#[cfg(test)]
mod tests {
    use super::{build_worker_id, sanitize_worker_id_component};

    #[test]
    fn sanitize_worker_id_component_normalizes_hostname() {
        assert_eq!(
            sanitize_worker_id_component("My Host_Name.example.com"),
            "my-host-name-example-com"
        );
    }

    #[test]
    fn build_worker_id_includes_process_identity() {
        assert_eq!(
            build_worker_id(3, "host-4242-deadbeef"),
            "site-worker-3-host-4242-deadbeef"
        );
    }
}
