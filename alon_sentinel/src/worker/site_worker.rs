use anyhow::Result;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::{StreamExt, stream};
use rand::Rng;
use sqlx::PgPool;
use std::time::Instant;
use tokio::sync::{oneshot, watch};
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::domain::{site_monitor_checks, site_monitor_incidents, site_monitors};
use crate::monitoring::{dns_checker, heartbeat_checker, http_checker, tcp_checker};
use crate::notifications;

pub async fn run(
    pool: &PgPool,
    config: &Config,
    worker_id: &str,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let smtp_mailer = notifications::service::build_smtp_mailer(config).map_err(|e| {
        error!("Failed to build SMTP mailer: {e}");
        e
    })?;
    let claim_limit = site_monitor_claim_limit(config);
    let mut next_check_history_retention_at = Utc::now();

    loop {
        if *shutdown_rx.borrow() {
            info!("Worker {worker_id} stopping");
            break;
        }

        let mut processed_work = false;
        match site_monitors::repository::claim_site_monitors_due_for_check(
            pool,
            worker_id,
            claim_limit,
            config.lease_site_check_seconds as i64,
        )
        .await
        {
            Ok(site_monitors) => {
                processed_work = !site_monitors.is_empty();
                if let Err(e) =
                    check_site_monitors(pool, site_monitors, config, worker_id, shutdown_rx.clone())
                        .await
                {
                    error!("Error checking site monitors: {:?}", e);
                }
            }
            Err(e) => {
                error!("Error fetching site monitors due for check: {:?}", e);
            }
        };

        if *shutdown_rx.borrow() {
            info!("Worker {worker_id} received shutdown before notification delivery phase");
            break;
        }

        match notifications::service::deliver_due_notifications(
            pool,
            smtp_mailer.as_ref(),
            config,
            worker_id,
            shutdown_rx.clone(),
        )
        .await
        {
            Ok(deliveries_claimed) => {
                processed_work |= deliveries_claimed > 0;
            }
            Err(e) => {
                error!("Error delivering notifications: {:?}", e);
            }
        }

        if *shutdown_rx.borrow() {
            info!("Worker {worker_id} received shutdown before retention phase");
            break;
        }

        match prune_site_monitor_check_history_if_due(
            pool,
            config,
            worker_id,
            &mut next_check_history_retention_at,
        )
        .await
        {
            Ok(deleted_count) => {
                processed_work |= deleted_count > 0;
            }
            Err(error) => {
                error!("Error pruning site monitor check history: {:?}", error);
            }
        }

        if processed_work {
            continue;
        }

        let idle_delay =
            match next_worker_idle_delay(pool, config, Some(next_check_history_retention_at)).await
            {
                Ok(delay) => delay,
                Err(error) => {
                    error!("Error deriving worker idle delay: {:?}", error);
                    worker_max_poll_interval(config)
                }
            };

        tokio::select! {
            result = shutdown_rx.changed() => {
                match result {
                    Ok(_) => {
                        if *shutdown_rx.borrow() {
                            info!("Worker {worker_id} received shutdown");
                            break;
                        }
                    }
                    Err(_) => {
                        info!("Worker {worker_id} shutdown channel closed");
                        break;
                    }
                }
            }
            _ = sleep(idle_delay) => {}
        }
    }
    Ok(())
}

fn site_monitor_claim_limit(config: &Config) -> i64 {
    config
        .due_sites_batch_size
        .min(config.max_http_concurrent_checks) as i64
}

fn worker_max_poll_interval(config: &Config) -> Duration {
    Duration::from_millis(config.worker_max_poll_interval_ms)
}

async fn next_worker_idle_delay(
    pool: &PgPool,
    config: &Config,
    next_check_history_retention_at: Option<DateTime<Utc>>,
) -> Result<Duration> {
    let next_monitor_check_at = site_monitors::repository::next_claimable_check_at(pool).await?;
    let next_notification_delivery_at =
        crate::domain::notification_deliveries::repository::next_claimable_delivery_at(pool)
            .await?;

    Ok(derive_worker_idle_delay(
        Utc::now(),
        next_monitor_check_at,
        next_notification_delivery_at,
        next_check_history_retention_at,
        worker_max_poll_interval(config),
    ))
}

fn derive_worker_idle_delay(
    now: DateTime<Utc>,
    next_monitor_check_at: Option<DateTime<Utc>>,
    next_notification_delivery_at: Option<DateTime<Utc>>,
    next_check_history_retention_at: Option<DateTime<Utc>>,
    max_poll_interval: Duration,
) -> Duration {
    let next_wake_at = [
        next_monitor_check_at,
        next_notification_delivery_at,
        next_check_history_retention_at,
    ]
    .into_iter()
    .flatten()
    .min();

    match next_wake_at {
        Some(next_wake_at) if next_wake_at > now => (next_wake_at - now)
            .to_std()
            .unwrap_or(Duration::ZERO)
            .min(max_poll_interval),
        Some(_) => Duration::ZERO,
        None => max_poll_interval,
    }
}

async fn prune_site_monitor_check_history_if_due(
    pool: &PgPool,
    config: &Config,
    worker_id: &str,
    next_run_at: &mut DateTime<Utc>,
) -> Result<usize> {
    let now = Utc::now();
    if now < *next_run_at {
        return Ok(0);
    }

    let cutoff = now - ChronoDuration::days(config.site_monitor_check_retention_days as i64);
    let deleted_count = match site_monitor_checks::repository::prune_checks_older_than(
        pool,
        cutoff,
        config.site_monitor_check_retention_batch_size as i64,
    )
    .await?
    {
        Some(count) => count as usize,
        None => {
            *next_run_at = now
                + ChronoDuration::seconds(
                    config.site_monitor_check_retention_interval_seconds as i64,
                );
            return Ok(0);
        }
    };

    if deleted_count > 0 {
        info!(
            worker_id,
            deleted_count,
            retention_days = config.site_monitor_check_retention_days,
            "Pruned expired site monitor check history"
        );
    }

    *next_run_at = if deleted_count >= config.site_monitor_check_retention_batch_size {
        now
    } else {
        now + ChronoDuration::seconds(config.site_monitor_check_retention_interval_seconds as i64)
    };

    Ok(deleted_count)
}

async fn check_site_monitors(
    pool: &PgPool,
    site_monitors: Vec<site_monitors::SiteMonitor>,
    config: &Config,
    worker_id: &str,
    shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    stream::iter(site_monitors.into_iter().map(|site_monitor| {
        let retry_shutdown_rx = shutdown_rx.clone();
        let lease_shutdown_rx = shutdown_rx.clone();
        let pool = pool.clone();
        let worker_id = worker_id.to_string();
        let policy = resolve_monitor_check_policy(&site_monitor, config);
        let lease_plan =
            derive_site_monitor_lease_plan(&policy, config.lease_site_check_seconds as u64);

        async move {
            if !ensure_site_monitor_initial_lease(
                &pool,
                site_monitor.id,
                &worker_id,
                lease_plan.initial_lease,
            )
            .await
            {
                warn!(
                    site_monitor_id = site_monitor.id,
                    target_url = %site_monitor.target_url,
                    "Lost monitor claim before starting check execution"
                );
                return;
            }

            let (lease_stop_tx, lease_stop_rx) = oneshot::channel();
            let lease_heartbeat = tokio::spawn(heartbeat_site_monitor_claim(
                pool.clone(),
                site_monitor.id,
                worker_id.clone(),
                MonitorClaimControl {
                    lease_plan: lease_plan.clone(),
                    shutdown_rx: lease_shutdown_rx,
                    stop_rx: lease_stop_rx,
                },
            ));

            let execution =
                run_monitor_check_with_retries(&site_monitor, &policy, retry_shutdown_rx).await;
            let _ = lease_stop_tx.send(());
            if let Err(error) = lease_heartbeat.await {
                error!(
                    site_monitor_id = site_monitor.id,
                    target_url = %site_monitor.target_url,
                    "Lease heartbeat task join error: {:?}",
                    error
                );
            }

            match execution {
                MonitorCheckExecution::Completed(result) => {
                    match save_check_result(&pool, &site_monitor, &worker_id, &result).await {
                        Ok(_) => (),
                        Err(e) => error!(
                            "Error saving check result for site monitor {}: {:?}",
                            site_monitor.id, e
                        ),
                    }
                }
                MonitorCheckExecution::Cancelled => {
                    info!(
                        site_monitor_id = site_monitor.id,
                        target_url = %site_monitor.target_url,
                        "Cancelled HTTP check during shutdown; releasing monitor claim"
                    );
                    match site_monitors::repository::release_site_monitor_claim(
                        &pool,
                        site_monitor.id,
                        &worker_id,
                    )
                    .await
                    {
                        Ok(true) => (),
                        Ok(false) => warn!(
                            site_monitor_id = site_monitor.id,
                            target_url = %site_monitor.target_url,
                            "Monitor claim was already lost before cancellation cleanup"
                        ),
                        Err(e) => error!(
                            "Error releasing cancelled claim for site monitor {}: {:?}",
                            site_monitor.id, e
                        ),
                    }
                }
            }
        }
    }))
    .buffer_unordered(config.max_http_concurrent_checks)
    .for_each(|_| async {})
    .await;

    Ok(())
}

enum MonitorCheckExecution {
    Completed(CompletedMonitorCheck),
    Cancelled,
}

struct CompletedMonitorCheck {
    result: http_checker::CheckResult,
    attempt_count: usize,
    total_duration_ms: i32,
}

async fn run_monitor_check_with_retries(
    site_monitor: &site_monitors::SiteMonitor,
    policy: &ResolvedMonitorCheckPolicy,
    mut shutdown_rx: watch::Receiver<bool>,
) -> MonitorCheckExecution {
    let started_at = Instant::now();

    for attempt_index in 0..policy.max_attempts {
        let check_future = async {
            match site_monitor.monitor_type {
                site_monitors::SiteMonitorType::Http => {
                    http_checker::check_url(
                        &site_monitor.target_url,
                        site_monitor.expected_status_code,
                        http_checker::HttpAssertions {
                            body_must_contain: site_monitor.body_must_contain.as_deref(),
                            body_must_not_contain: site_monitor.body_must_not_contain.as_deref(),
                            body_must_contain_texts: site_monitor
                                .body_must_contain_texts
                                .as_deref()
                                .unwrap_or(&[]),
                            body_must_not_contain_texts: site_monitor
                                .body_must_not_contain_texts
                                .as_deref()
                                .unwrap_or(&[]),
                            json_path_exists: site_monitor
                                .json_path_exists
                                .as_deref()
                                .unwrap_or(&[]),
                            json_path_equals: site_monitor
                                .json_path_equals
                                .as_ref()
                                .map(|assertions| assertions.0.as_slice())
                                .unwrap_or(&[]),
                            json_path_not_equals: site_monitor
                                .json_path_not_equals
                                .as_ref()
                                .map(|assertions| assertions.0.as_slice())
                                .unwrap_or(&[]),
                            max_response_time_ms: site_monitor.max_response_time_ms,
                            required_header_name: site_monitor.required_header_name.as_deref(),
                            required_header_value: site_monitor.required_header_value.as_deref(),
                            header_assertions: site_monitor
                                .header_assertions
                                .as_ref()
                                .map(|assertions| assertions.0.as_slice())
                                .unwrap_or(&[]),
                            ssl_certificate_checks_enabled: false,
                            ssl_expiry_warning_days: None,
                        },
                        policy.timeout,
                        http_checker::HttpCheckConfig {
                            max_response_body_bytes: policy.max_response_body_bytes,
                            validate_public_target: !policy.allow_private_targets,
                        },
                    )
                    .await
                }
                site_monitors::SiteMonitorType::Ssl => {
                    http_checker::check_ssl_certificate(
                        &site_monitor.target_url,
                        site_monitor.ssl_expiry_warning_days,
                        policy.timeout,
                        !policy.allow_private_targets,
                    )
                    .await
                }
                site_monitors::SiteMonitorType::Heartbeat => {
                    heartbeat_checker::check_heartbeat(site_monitor, Utc::now())
                }
                site_monitors::SiteMonitorType::Tcp => {
                    let host = site_monitor.tcp_target_host.as_deref().unwrap_or("");
                    let port = site_monitor.tcp_target_port.unwrap_or(0).clamp(0, 65535) as u16;
                    tcp_checker::check_tcp(host, port, policy.timeout, !cfg!(test)).await
                }
                site_monitors::SiteMonitorType::Dns => {
                    dns_checker::check_dns(
                        &dns_checker::DnsCheckParams {
                            hostname: site_monitor.dns_hostname.as_deref().unwrap_or(""),
                            record_type: site_monitor.dns_record_type.as_deref().unwrap_or(""),
                            expected_value: site_monitor.dns_expected_value.as_deref(),
                            nameserver: site_monitor.dns_nameserver.as_deref(),
                        },
                        policy.timeout,
                        !cfg!(test),
                    )
                    .await
                }
            }
        };
        tokio::pin!(check_future);
        let result = loop {
            tokio::select! {
                result = &mut check_future => break result,
                changed = shutdown_rx.changed() => {
                    match changed {
                        Ok(_) if *shutdown_rx.borrow() => {
                            info!(
                                site_monitor_id = site_monitor.id,
                                monitor_type = site_monitor.monitor_type.as_str(),
                                target_url = %site_monitor.target_url,
                                attempt = attempt_index + 1,
                                max_attempts = policy.max_attempts,
                                "Shutdown interrupted an in-flight monitor check"
                            );
                            return MonitorCheckExecution::Cancelled
                        }
                        Ok(_) => continue,
                        Err(_) => {
                            info!(
                                site_monitor_id = site_monitor.id,
                                monitor_type = site_monitor.monitor_type.as_str(),
                                target_url = %site_monitor.target_url,
                                attempt = attempt_index + 1,
                                max_attempts = policy.max_attempts,
                                "Shutdown channel closed during an in-flight monitor check"
                            );
                            return MonitorCheckExecution::Cancelled
                        }
                    }
                }
            }
        };

        let should_retry =
            attempt_index + 1 < policy.max_attempts && is_retryable_check_result(&result);

        if !should_retry {
            let attempt_count = attempt_index + 1;
            if result.is_success {
                info!(
                    site_monitor_id = site_monitor.id,
                    monitor_type = site_monitor.monitor_type.as_str(),
                    target_url = %site_monitor.target_url,
                    attempt_count,
                    status_code = result.status_code,
                    response_time_ms = result.response_time_ms,
                    "Completed monitor check successfully"
                );
            } else {
                warn!(
                    site_monitor_id = site_monitor.id,
                    monitor_type = site_monitor.monitor_type.as_str(),
                    target_url = %site_monitor.target_url,
                    attempt_count,
                    status_code = result.status_code,
                    response_time_ms = result.response_time_ms,
                    failure_reason = result.failure_reason.as_deref().unwrap_or("unknown"),
                    error_message = result.error_message.as_deref().unwrap_or("n/a"),
                    "Completed monitor check with final failure"
                );
            }
            return MonitorCheckExecution::Completed(CompletedMonitorCheck {
                result,
                attempt_count: attempt_index + 1,
                total_duration_ms: started_at.elapsed().as_millis() as i32,
            });
        }

        if let Some(delay) = policy.retry_delay_for_attempt(attempt_index) {
            warn!(
                site_monitor_id = site_monitor.id,
                monitor_type = site_monitor.monitor_type.as_str(),
                target_url = %site_monitor.target_url,
                attempt = attempt_index + 1,
                next_attempt = attempt_index + 2,
                max_attempts = policy.max_attempts,
                status_code = result.status_code,
                response_time_ms = result.response_time_ms,
                failure_reason = result.failure_reason.as_deref().unwrap_or("unknown"),
                error_message = result.error_message.as_deref().unwrap_or("n/a"),
                retry_delay_ms = delay.as_millis() as u64,
                "Retrying transient monitor check failure"
            );
            if !wait_for_retry_delay_or_shutdown(delay, &mut shutdown_rx).await {
                info!(
                    site_monitor_id = site_monitor.id,
                    monitor_type = site_monitor.monitor_type.as_str(),
                    target_url = %site_monitor.target_url,
                    attempt = attempt_index + 1,
                    max_attempts = policy.max_attempts,
                    "Shutdown interrupted monitor retry backoff"
                );
                return MonitorCheckExecution::Cancelled;
            }
        } else {
            debug!(
                site_monitor_id = site_monitor.id,
                monitor_type = site_monitor.monitor_type.as_str(),
                target_url = %site_monitor.target_url,
                attempt = attempt_index + 1,
                max_attempts = policy.max_attempts,
                "No retry delay available; completing monitor check after current attempt"
            );
            return MonitorCheckExecution::Completed(CompletedMonitorCheck {
                result,
                attempt_count: attempt_index + 1,
                total_duration_ms: started_at.elapsed().as_millis() as i32,
            });
        }
    }

    unreachable!("http_check_max_attempts must be greater than 0")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ResolvedMonitorCheckPolicy {
    timeout: Duration,
    max_attempts: usize,
    retry_delays_ms: Vec<u64>,
    retry_jitter_percent: usize,
    max_response_body_bytes: usize,
    allow_private_targets: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SiteMonitorLeasePlan {
    initial_lease: Duration,
    heartbeat_interval: Duration,
}

impl ResolvedMonitorCheckPolicy {
    fn base_retry_delay_for_attempt(&self, attempt_index: usize) -> Option<Duration> {
        self.retry_delays_ms
            .get(attempt_index)
            .or_else(|| self.retry_delays_ms.last())
            .copied()
            .map(Duration::from_millis)
    }

    fn retry_delay_for_attempt(&self, attempt_index: usize) -> Option<Duration> {
        self.base_retry_delay_for_attempt(attempt_index)
            .map(|delay| jitter_retry_delay(delay, self.retry_jitter_percent))
    }
}

fn resolve_monitor_check_policy(
    site_monitor: &site_monitors::SiteMonitor,
    config: &Config,
) -> ResolvedMonitorCheckPolicy {
    let timeout_seconds = site_monitor
        .http_check_timeout_seconds_override
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(config.http_check_timeout_seconds as u64);
    let max_attempts = site_monitor
        .http_check_max_attempts_override
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .unwrap_or(config.http_check_max_attempts);
    let retry_delays_ms = site_monitor
        .http_check_retry_delays_ms_override
        .as_ref()
        .map(|values| {
            values
                .iter()
                .copied()
                .filter_map(|value| u64::try_from(value).ok())
                .filter(|value| *value > 0)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| config.http_check_retry_delays_ms.clone());

    ResolvedMonitorCheckPolicy {
        timeout: Duration::from_secs(timeout_seconds),
        max_attempts,
        retry_delays_ms,
        retry_jitter_percent: config.http_check_retry_jitter_percent,
        max_response_body_bytes: config.http_check_max_response_body_bytes,
        allow_private_targets: config.http_monitor_allow_private_targets,
    }
}

fn derive_site_monitor_lease_plan(
    policy: &ResolvedMonitorCheckPolicy,
    minimum_lease_seconds: u64,
) -> SiteMonitorLeasePlan {
    let estimated_attempt_time = policy.timeout.saturating_mul(policy.max_attempts as u32);
    let estimated_retry_delay = (0..policy.max_attempts.saturating_sub(1))
        .filter_map(|attempt_index| policy.base_retry_delay_for_attempt(attempt_index))
        .map(|delay| worst_case_retry_delay(delay, policy.retry_jitter_percent))
        .fold(Duration::ZERO, |acc, delay| acc.saturating_add(delay));
    let safety_buffer = (policy.timeout / 2).max(Duration::from_secs(2));
    let derived_lease = estimated_attempt_time
        .saturating_add(estimated_retry_delay)
        .saturating_add(safety_buffer);
    let minimum_lease = Duration::from_secs(minimum_lease_seconds);
    let initial_lease = derived_lease.max(minimum_lease);
    let heartbeat_interval = (initial_lease / 3).max(Duration::from_secs(1));

    SiteMonitorLeasePlan {
        initial_lease,
        heartbeat_interval,
    }
}

fn jitter_retry_delay(base_delay: Duration, jitter_percent: usize) -> Duration {
    if jitter_percent == 0 {
        return base_delay;
    }

    let base_delay_ms = base_delay.as_millis() as u64;
    let jitter_window_ms =
        ((base_delay_ms as u128 * jitter_percent as u128) / 100_u128).min(u64::MAX as u128) as u64;

    if jitter_window_ms == 0 {
        return base_delay;
    }

    let min_delay_ms = base_delay_ms.saturating_sub(jitter_window_ms);
    let max_delay_ms = base_delay_ms.saturating_add(jitter_window_ms);
    let jittered_delay_ms = rand::rng().random_range(min_delay_ms..=max_delay_ms);

    Duration::from_millis(jittered_delay_ms)
}

fn worst_case_retry_delay(base_delay: Duration, jitter_percent: usize) -> Duration {
    if jitter_percent == 0 {
        return base_delay;
    }

    let base_delay_ms = base_delay.as_millis() as u64;
    let jitter_window_ms =
        ((base_delay_ms as u128 * jitter_percent as u128) / 100_u128).min(u64::MAX as u128) as u64;

    Duration::from_millis(base_delay_ms.saturating_add(jitter_window_ms))
}

async fn wait_for_retry_delay_or_shutdown(
    delay: Duration,
    shutdown_rx: &mut watch::Receiver<bool>,
) -> bool {
    if *shutdown_rx.borrow() {
        return false;
    }

    tokio::select! {
        result = shutdown_rx.changed() => {
            match result {
                Ok(_) => !*shutdown_rx.borrow(),
                Err(_) => false,
            }
        }
        _ = sleep(delay) => true,
    }
}

fn is_retryable_check_result(result: &http_checker::CheckResult) -> bool {
    matches!(
        result.failure_reason.as_deref(),
        Some("timeout" | "connect_error")
    )
}

async fn save_check_result(
    pool: &PgPool,
    site_monitor: &site_monitors::SiteMonitor,
    worker_id: &str,
    completed_check: &CompletedMonitorCheck,
) -> Result<()> {
    let result = &completed_check.result;
    let mut transact = pool.begin().await?;
    let cert = result.certificate_metadata.as_ref();
    let site_monitor_check = site_monitor_checks::repository::create_site_monitor_check(
        &mut transact,
        site_monitor.id,
        &site_monitor_checks::CreateMonitorCheckParams {
            monitor_type: site_monitor.monitor_type,
            url_checked: &site_monitor.target_url,
            expected_status_code: match site_monitor.monitor_type {
                site_monitors::SiteMonitorType::Http => Some(site_monitor.expected_status_code),
                site_monitors::SiteMonitorType::Ssl
                | site_monitors::SiteMonitorType::Heartbeat
                | site_monitors::SiteMonitorType::Tcp
                | site_monitors::SiteMonitorType::Dns => None,
            },
            is_success: result.is_success,
            status_code: result.status_code,
            response_time_ms: result.response_time_ms,
            total_duration_ms: Some(completed_check.total_duration_ms),
            attempt_count: completed_check.attempt_count as i32,
            was_retried: completed_check.attempt_count > 1,
            failure_reason: result.failure_reason.as_deref(),
            error_message: result.error_message.as_deref(),
            certificate_expires_at: cert.map(|m| m.expires_at),
            certificate_days_remaining: cert.map(|m| m.days_remaining),
            certificate_issuer: cert.map(|m| m.issuer.as_str()),
            certificate_subject: cert.map(|m| m.subject.as_str()),
            certificate_domain: cert.map(|m| m.domain.as_str()),
        },
    )
    .await?;
    let incident_id =
        manage_incident_lifecycle(&mut transact, site_monitor, &site_monitor_check).await?;
    notifications::service::enqueue_site_monitor_notifications(
        &mut transact,
        site_monitor,
        &site_monitor_check,
        incident_id,
    )
    .await?;
    let updated = site_monitors::repository::update_site_monitor_last_check(
        &mut transact,
        site_monitor.id,
        worker_id,
        &site_monitors::MonitorLastCheckParams {
            is_success: result.is_success,
            status_code: result.status_code,
            response_time_ms: result.response_time_ms,
            failure_reason: result.failure_reason.as_deref(),
            error_message: result.error_message.as_deref(),
            certificate_expires_at: cert.map(|m| m.expires_at),
            certificate_days_remaining: cert.map(|m| m.days_remaining),
            certificate_issuer: cert.map(|m| m.issuer.as_str()),
            certificate_subject: cert.map(|m| m.subject.as_str()),
            certificate_domain: cert.map(|m| m.domain.as_str()),
        },
    )
    .await?;
    if !updated {
        anyhow::bail!(
            "worker {worker_id} no longer owns site monitor {} while saving check result",
            site_monitor.id
        );
    }
    transact.commit().await?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum IncidentAction {
    Open,
    UpdateExisting,
    ReopenMissing,
    Resolve,
    None,
}

fn classify_incident_action(
    last_is_success: Option<bool>,
    current_is_success: bool,
    has_open_incident: bool,
) -> IncidentAction {
    match (last_is_success, current_is_success) {
        (None | Some(true), false) => IncidentAction::Open,
        (Some(false), false) => {
            if has_open_incident {
                IncidentAction::UpdateExisting
            } else {
                IncidentAction::ReopenMissing
            }
        }
        (Some(false), true) => {
            if has_open_incident {
                IncidentAction::Resolve
            } else {
                IncidentAction::None
            }
        }
        _ => IncidentAction::None,
    }
}

async fn manage_incident_lifecycle(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    site_monitor: &site_monitors::SiteMonitor,
    site_monitor_check: &site_monitor_checks::SiteMonitorCheck,
) -> Result<Option<i64>> {
    let open_incident = if matches!(site_monitor.last_is_success, Some(false)) {
        site_monitor_incidents::repository::get_open_incident_for_monitor(transact, site_monitor.id)
            .await?
    } else {
        None
    };

    match classify_incident_action(
        site_monitor.last_is_success,
        site_monitor_check.is_success,
        open_incident.is_some(),
    ) {
        IncidentAction::Open | IncidentAction::ReopenMissing => {
            let incident = site_monitor_incidents::repository::open_incident(
                transact,
                site_monitor.site_id,
                &site_monitor_incidents::OpenIncidentParams {
                    site_monitor_id: site_monitor.id,
                    monitor_type: site_monitor.monitor_type,
                    target_url: &site_monitor.target_url,
                    expected_status_code: site_monitor.expected_status_code,
                    check_id: site_monitor_check.id,
                    checked_at: site_monitor_check.checked_at,
                    status_code: site_monitor_check.status_code,
                    failure_reason: site_monitor_check.failure_reason.as_deref(),
                    error_message: site_monitor_check.error_message.as_deref(),
                },
            )
            .await?;
            Ok(Some(incident.id))
        }
        IncidentAction::UpdateExisting => {
            let open_incident = open_incident.expect("UpdateExisting requires an open incident");
            site_monitor_incidents::repository::update_incident_failure(
                transact,
                open_incident.id,
                &site_monitor_incidents::IncidentFailureParams {
                    check_id: site_monitor_check.id,
                    checked_at: site_monitor_check.checked_at,
                    status_code: site_monitor_check.status_code,
                    failure_reason: site_monitor_check.failure_reason.as_deref(),
                    error_message: site_monitor_check.error_message.as_deref(),
                },
            )
            .await?;
            Ok(Some(open_incident.id))
        }
        IncidentAction::Resolve => {
            let open_incident = open_incident.expect("Resolve requires an open incident");
            site_monitor_incidents::repository::resolve_incident(
                transact,
                open_incident.id,
                &site_monitor_incidents::ResolveIncidentParams {
                    check_id: site_monitor_check.id,
                    checked_at: site_monitor_check.checked_at,
                    status_code: site_monitor_check.status_code,
                    response_time_ms: site_monitor_check.response_time_ms,
                },
            )
            .await?;
            Ok(Some(open_incident.id))
        }
        IncidentAction::None => Ok(None),
    }
}

async fn ensure_site_monitor_initial_lease(
    pool: &PgPool,
    site_monitor_id: i64,
    worker_id: &str,
    initial_lease: Duration,
) -> bool {
    let lease_seconds = duration_to_lease_seconds(initial_lease);

    match site_monitors::repository::extend_site_monitor_claim(
        pool,
        site_monitor_id,
        worker_id,
        lease_seconds,
    )
    .await
    {
        Ok(updated) => updated,
        Err(error) => {
            error!(
                site_monitor_id,
                worker_id, lease_seconds, "Failed to set initial monitor lease: {:?}", error
            );
            false
        }
    }
}

struct MonitorClaimControl {
    lease_plan: SiteMonitorLeasePlan,
    shutdown_rx: watch::Receiver<bool>,
    stop_rx: oneshot::Receiver<()>,
}

async fn heartbeat_site_monitor_claim(
    pool: PgPool,
    site_monitor_id: i64,
    worker_id: String,
    ctrl: MonitorClaimControl,
) {
    let MonitorClaimControl {
        lease_plan,
        mut shutdown_rx,
        mut stop_rx,
    } = ctrl;
    let lease_seconds = duration_to_lease_seconds(lease_plan.initial_lease);

    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            changed = shutdown_rx.changed() => {
                match changed {
                    Ok(_) if *shutdown_rx.borrow() => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            _ = sleep(lease_plan.heartbeat_interval) => {}
        }

        match site_monitors::repository::extend_site_monitor_claim(
            &pool,
            site_monitor_id,
            &worker_id,
            lease_seconds,
        )
        .await
        {
            Ok(true) => debug!(
                site_monitor_id,
                worker_id, lease_seconds, "Extended in-flight monitor lease"
            ),
            Ok(false) => {
                warn!(
                    site_monitor_id,
                    worker_id, "Monitor lease extension failed because ownership was lost"
                );
                break;
            }
            Err(error) => error!(
                site_monitor_id,
                worker_id, "Failed to extend monitor lease: {:?}", error
            ),
        }
    }
}

fn duration_to_lease_seconds(duration: Duration) -> i64 {
    duration
        .as_secs()
        .saturating_add(u64::from(duration.subsec_nanos() > 0))
        .max(1) as i64
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::{
            Arc,
            atomic::{AtomicU64, AtomicUsize, Ordering},
        },
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use super::{
        CompletedMonitorCheck, IncidentAction, MonitorCheckExecution, check_site_monitors,
        classify_incident_action, derive_site_monitor_lease_plan, derive_worker_idle_delay,
        is_retryable_check_result, jitter_retry_delay, prune_site_monitor_check_history_if_due,
        resolve_monitor_check_policy, run, run_monitor_check_with_retries, save_check_result,
        site_monitor_claim_limit, worker_max_poll_interval,
    };
    use crate::{
        config::Config,
        crypto::WebhookSecretEncryptionKey,
        domain::{
            notification_channels::{self, NotificationChannelType},
            notification_deliveries::{self, NotificationEventType},
            site_monitor_checks, site_monitors, sites,
        },
        monitoring::http_checker::CheckResult,
    };
    use anyhow::{Context, Result};
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use sqlx::{Executor, PgPool};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::watch,
        time::{sleep, timeout},
    };
    static NEXT_TEST_ID: AtomicU64 = AtomicU64::new(1);

    fn test_webhook_secret_encryption_key() -> WebhookSecretEncryptionKey {
        WebhookSecretEncryptionKey::from_hex(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        )
        .expect("test webhook secret encryption key should parse")
    }

    #[test]
    fn site_monitor_claim_limit_uses_smaller_batch_size() {
        let mut config = test_config();
        config.due_sites_batch_size = 4;
        config.max_http_concurrent_checks = 10;

        assert_eq!(site_monitor_claim_limit(&config), 4);
    }

    #[test]
    fn site_monitor_claim_limit_caps_claims_at_max_concurrency() {
        let mut config = test_config();
        config.due_sites_batch_size = 100;
        config.max_http_concurrent_checks = 10;

        assert_eq!(site_monitor_claim_limit(&config), 10);
    }

    #[test]
    fn derive_worker_idle_delay_uses_earliest_due_item() {
        let now = DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        let delay = derive_worker_idle_delay(
            now,
            Some(now + chrono::Duration::seconds(4)),
            Some(now + chrono::Duration::seconds(2)),
            None,
            Duration::from_secs(5),
        );

        assert_eq!(delay, Duration::from_secs(2));
    }

    #[test]
    fn derive_worker_idle_delay_caps_wait_at_max_poll_interval() {
        let now = DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        let delay = derive_worker_idle_delay(
            now,
            Some(now + chrono::Duration::seconds(30)),
            None,
            None,
            Duration::from_secs(5),
        );

        assert_eq!(delay, Duration::from_secs(5));
    }

    #[test]
    fn derive_worker_idle_delay_returns_zero_for_already_due_work() {
        let now = DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        let delay = derive_worker_idle_delay(
            now,
            Some(now - chrono::Duration::seconds(1)),
            None,
            None,
            Duration::from_secs(5),
        );

        assert_eq!(delay, Duration::ZERO);
    }

    #[test]
    fn derive_worker_idle_delay_considers_check_history_retention_run() {
        let now = DateTime::parse_from_rfc3339("2026-04-28T12:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        let delay = derive_worker_idle_delay(
            now,
            Some(now + chrono::Duration::seconds(30)),
            Some(now + chrono::Duration::seconds(20)),
            Some(now + chrono::Duration::seconds(3)),
            Duration::from_secs(10),
        );

        assert_eq!(delay, Duration::from_secs(3));
    }

    #[test]
    fn worker_max_poll_interval_uses_configured_milliseconds() {
        let mut config = test_config();
        config.worker_max_poll_interval_ms = 1500;

        assert_eq!(
            worker_max_poll_interval(&config),
            Duration::from_millis(1500)
        );
    }

    #[tokio::test]
    async fn claim_site_monitors_due_for_check_does_not_double_claim() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Claim Site", "https://claim.test.com")
            .await?;
        let monitor = test_db
            .seed_http_monitor(site.id, "https://claim.test.com/health")
            .await?;

        let first_claim = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-a",
            10,
            60,
        )
        .await?;
        let second_claim = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-b",
            10,
            60,
        )
        .await?;

        assert_eq!(first_claim.len(), 1);
        assert_eq!(first_claim[0].id, monitor.id);
        assert!(second_claim.is_empty());

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn save_check_result_persists_success_and_clears_lease() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Success Site", "https://success.test.com")
            .await?;
        let monitor = test_db
            .seed_http_monitor(site.id, "https://success.test.com/health")
            .await?;
        let claimed_monitor = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-success",
            10,
            60,
        )
        .await?
        .into_iter()
        .find(|claimed| claimed.id == monitor.id)
        .expect("monitor should be claimed");

        let completed_check = CompletedMonitorCheck {
            result: CheckResult {
                is_success: true,
                status_code: Some(200),
                response_time_ms: Some(85),
                failure_reason: None,
                error_message: None,
                certificate_metadata: None,
            },
            attempt_count: 1,
            total_duration_ms: 85,
        };

        save_check_result(
            &test_db.pool,
            &claimed_monitor,
            "worker-success",
            &completed_check,
        )
        .await?;

        let checks = site_monitor_checks::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &site_monitor_checks::CheckCursorQuery {
                cursor_checked_at: None,
                cursor_id: None,
                is_success: None,
                limit: 10,
            },
        )
        .await?;
        let refreshed_monitor = test_db
            .get_http_monitor_by_target_url(site.id, "https://success.test.com/health")
            .await?
            .expect("monitor should still exist");

        assert_eq!(checks.len(), 1);
        assert!(checks[0].is_success);
        assert_eq!(checks[0].status_code, Some(200));
        assert_eq!(checks[0].total_duration_ms, Some(85));
        assert_eq!(checks[0].attempt_count, 1);
        assert!(!checks[0].was_retried);
        assert_eq!(refreshed_monitor.last_is_success, Some(true));
        assert_eq!(refreshed_monitor.last_status_code, Some(200));
        assert_eq!(refreshed_monitor.last_response_time_ms, Some(85));
        assert_eq!(refreshed_monitor.last_failure_reason, None);
        assert!(refreshed_monitor.last_checked_at.is_some());
        assert!(refreshed_monitor.last_successful_check_at.is_some());
        assert!(refreshed_monitor.check_claimed_at.is_none());
        assert!(refreshed_monitor.check_lease_until.is_none());
        assert!(refreshed_monitor.check_claimed_by.is_none());

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn save_check_result_enqueues_failure_then_recovery_notifications() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Notify Site", "https://notify.test.com")
            .await?;
        let monitor = test_db
            .seed_http_monitor(site.id, "https://notify.test.com/health")
            .await?;
        let webhook_secret_ciphertext = test_webhook_secret_encryption_key()
            .encrypt_webhook_secret("test-secret")
            .expect("test webhook secret should encrypt");
        let _channel = notification_channels::repository::create_channel(
            &test_db.pool,
            &notification_channels::NotificationChannelParams {
                channel_type: NotificationChannelType::Webhook,
                name: "Notify Hook",
                destination: "https://hooks.test.com/notify",
                webhook_secret_ciphertext: Some(&webhook_secret_ciphertext),
                notify_on_failure: true,
                notify_on_recovery: true,
                is_active: true,
            },
        )
        .await?;

        let claimed_monitor = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-notify-a",
            10,
            60,
        )
        .await?
        .into_iter()
        .find(|claimed| claimed.id == monitor.id)
        .expect("monitor should be claimed for failure");

        let first_result = CompletedMonitorCheck {
            result: CheckResult {
                is_success: false,
                status_code: Some(503),
                response_time_ms: Some(140),
                failure_reason: Some("unexpected_status".to_string()),
                error_message: Some("Unexpected HTTP status: 503, expected: 200".to_string()),
                certificate_metadata: None,
            },
            attempt_count: 2,
            total_duration_ms: 440,
        };
        save_check_result(
            &test_db.pool,
            &claimed_monitor,
            "worker-notify-a",
            &first_result,
        )
        .await?;

        let after_failure_monitor = test_db
            .get_http_monitor_by_target_url(site.id, "https://notify.test.com/health")
            .await?
            .expect("monitor should exist after failure");
        sqlx::query(
            r#"
            UPDATE site_monitors
            SET
                check_claimed_at = NOW(),
                check_lease_until = NOW() + INTERVAL '60 seconds',
                check_claimed_by = 'worker-notify-b',
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(after_failure_monitor.id)
        .execute(&test_db.pool)
        .await?;

        let recovery_result = CompletedMonitorCheck {
            result: CheckResult {
                is_success: true,
                status_code: Some(200),
                response_time_ms: Some(95),
                failure_reason: None,
                error_message: None,
                certificate_metadata: None,
            },
            attempt_count: 1,
            total_duration_ms: 95,
        };
        save_check_result(
            &test_db.pool,
            &after_failure_monitor,
            "worker-notify-b",
            &recovery_result,
        )
        .await?;

        let deliveries = notification_deliveries::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &notification_deliveries::DeliveryCursorQuery {
                cursor_created_at: None,
                cursor_id: None,
                status: None,
                event_type: None,
                limit: 10,
            },
        )
        .await?;

        assert_eq!(deliveries.len(), 2);
        assert_eq!(deliveries[0].event_type, NotificationEventType::Recovery);
        assert_eq!(deliveries[1].event_type, NotificationEventType::Failure);
        assert_eq!(
            deliveries[0].status,
            notification_deliveries::NotificationDeliveryStatus::Pending
        );
        assert_eq!(
            deliveries[1].status,
            notification_deliveries::NotificationDeliveryStatus::Pending
        );
        let recovered_monitor = test_db
            .get_http_monitor_by_target_url(site.id, "https://notify.test.com/health")
            .await?
            .expect("monitor should exist after recovery");
        assert_eq!(recovered_monitor.last_failure_reason, None);
        let checks = site_monitor_checks::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &site_monitor_checks::CheckCursorQuery {
                cursor_checked_at: None,
                cursor_id: None,
                is_success: None,
                limit: 10,
            },
        )
        .await?;
        assert_eq!(checks[0].attempt_count, 1);
        assert!(!checks[0].was_retried);
        assert_eq!(checks[0].total_duration_ms, Some(95));
        assert_eq!(checks[1].attempt_count, 2);
        assert!(checks[1].was_retried);
        assert_eq!(checks[1].total_duration_ms, Some(440));

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn run_processes_due_monitor_and_stops_cleanly() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Run Site", "https://run.test.com")
            .await?;
        let url = spawn_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
        )
        .await?;
        let _monitor = test_db.seed_http_monitor(site.id, &url).await?;

        let config = test_config();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let pool = test_db.pool.clone();
        let worker =
            tokio::spawn(async move { run(&pool, &config, "worker-run", shutdown_rx).await });

        wait_for_checks(&test_db.pool, site.id, 1).await?;
        let _ = shutdown_tx.send(true);
        timeout(Duration::from_secs(10), worker).await???;

        let checks = site_monitor_checks::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &site_monitor_checks::CheckCursorQuery {
                cursor_checked_at: None,
                cursor_id: None,
                is_success: None,
                limit: 10,
            },
        )
        .await?;
        assert_eq!(checks.len(), 1);
        assert!(checks[0].is_success);

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn run_http_check_with_retries_retries_transient_failure_then_succeeds() -> Result<()> {
        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let url = spawn_delayed_http_server(attempt_counter.clone()).await?;
        let monitor = build_test_monitor(1, &url, 200);
        let mut config = test_config();
        config.http_check_retry_delays_ms = vec![200, 300];
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let policy = resolve_monitor_check_policy(&monitor, &config);
        let result = run_monitor_check_with_retries(&monitor, &policy, shutdown_rx).await;

        let MonitorCheckExecution::Completed(result) = result else {
            panic!("check should complete successfully");
        };
        assert!(result.result.is_success);
        assert_eq!(result.result.status_code, Some(200));
        assert!(result.total_duration_ms >= 0);
        assert!(result.attempt_count >= 1);
        assert!(result.attempt_count <= config.http_check_max_attempts);
        assert_eq!(attempt_counter.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn run_http_check_with_retries_does_not_retry_unexpected_status() -> Result<()> {
        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let url = spawn_counting_http_server(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 4\r\nConnection: close\r\n\r\nDown",
            attempt_counter.clone(),
        )
        .await?;
        let monitor = build_test_monitor(1, &url, 200);
        let config = test_config();
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);

        let policy = resolve_monitor_check_policy(&monitor, &config);
        let result = run_monitor_check_with_retries(&monitor, &policy, shutdown_rx).await;

        let MonitorCheckExecution::Completed(result) = result else {
            panic!("unexpected status should not be treated as cancellation");
        };
        assert!(!result.result.is_success);
        assert_eq!(
            result.result.failure_reason.as_deref(),
            Some("unexpected_status")
        );
        assert!(result.total_duration_ms >= 0);
        assert_eq!(result.attempt_count, 1);
        assert_eq!(attempt_counter.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[tokio::test]
    async fn run_http_check_with_retries_keeps_in_flight_check_when_watch_updates_to_false()
    -> Result<()> {
        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let url = spawn_slow_counting_http_server(
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
            Duration::from_millis(200),
            attempt_counter.clone(),
        )
        .await?;
        let monitor = build_test_monitor(1, &url, 200);
        let mut config = test_config();
        config.http_check_max_attempts = 1;
        config.http_check_retry_delays_ms.clear();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let policy = resolve_monitor_check_policy(&monitor, &config);

        let task = tokio::spawn(async move {
            run_monitor_check_with_retries(&monitor, &policy, shutdown_rx).await
        });

        sleep(Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(false);
        let result = timeout(Duration::from_secs(2), task).await??;

        let MonitorCheckExecution::Completed(result) = result else {
            panic!("check should complete without consuming an attempt");
        };
        assert!(result.result.is_success);
        assert_eq!(result.attempt_count, 1);
        assert_eq!(attempt_counter.load(Ordering::Relaxed), 1);

        Ok(())
    }

    #[test]
    fn is_retryable_check_result_only_retries_transient_transport_failures() {
        assert!(is_retryable_check_result(&CheckResult {
            is_success: false,
            status_code: None,
            response_time_ms: Some(100),
            failure_reason: Some("timeout".to_string()),
            error_message: Some("timed out".to_string()),
            certificate_metadata: None,
        }));
        assert!(is_retryable_check_result(&CheckResult {
            is_success: false,
            status_code: None,
            response_time_ms: Some(100),
            failure_reason: Some("connect_error".to_string()),
            error_message: Some("connection refused".to_string()),
            certificate_metadata: None,
        }));
        assert!(!is_retryable_check_result(&CheckResult {
            is_success: false,
            status_code: Some(503),
            response_time_ms: Some(100),
            failure_reason: Some("unexpected_status".to_string()),
            error_message: Some("service unavailable".to_string()),
            certificate_metadata: None,
        }));
    }

    #[test]
    fn resolve_http_check_policy_prefers_monitor_overrides() {
        let mut monitor = build_test_monitor(1, "https://test.com/health", 200);
        monitor.http_check_timeout_seconds_override = Some(7);
        monitor.http_check_max_attempts_override = Some(4);
        monitor.http_check_retry_delays_ms_override = Some(vec![150, 450]);
        let config = test_config();

        let policy = resolve_monitor_check_policy(&monitor, &config);

        assert_eq!(policy.timeout, Duration::from_secs(7));
        assert_eq!(policy.max_attempts, 4);
        assert_eq!(policy.retry_delays_ms, vec![150, 450]);
        assert_eq!(policy.retry_jitter_percent, 0);
        assert_eq!(
            policy.base_retry_delay_for_attempt(2),
            Some(Duration::from_millis(450))
        );
    }

    #[test]
    fn jitter_retry_delay_stays_within_expected_bounds() {
        let base_delay = Duration::from_millis(1_000);

        for _ in 0..50 {
            let jittered = jitter_retry_delay(base_delay, 20);
            assert!(jittered >= Duration::from_millis(800));
            assert!(jittered <= Duration::from_millis(1_200));
        }

        assert_eq!(jitter_retry_delay(base_delay, 0), base_delay);
    }

    #[test]
    fn derive_site_monitor_lease_plan_covers_retry_budget_and_buffer() {
        let policy = super::ResolvedMonitorCheckPolicy {
            timeout: Duration::from_secs(4),
            max_attempts: 3,
            retry_delays_ms: vec![1_000, 3_000],
            retry_jitter_percent: 20,
            max_response_body_bytes: 64 * 1024,
            allow_private_targets: false,
        };

        let lease_plan = derive_site_monitor_lease_plan(&policy, 5);

        assert_eq!(lease_plan.initial_lease, Duration::from_millis(18_800));
        assert_eq!(lease_plan.heartbeat_interval, lease_plan.initial_lease / 3);
    }

    #[tokio::test]
    async fn run_http_check_with_retries_stops_waiting_when_shutdown_is_requested() -> Result<()> {
        let monitor = build_test_monitor(1, "http://127.0.0.1:1", 200);
        let mut config = test_config();
        config.http_check_retry_delays_ms = vec![1_000, 1_000];
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let start = Instant::now();
        let policy = resolve_monitor_check_policy(&monitor, &config);

        let task = tokio::spawn(async move {
            run_monitor_check_with_retries(&monitor, &policy, shutdown_rx).await
        });

        sleep(Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(true);
        let result = timeout(Duration::from_millis(1_500), task).await??;

        assert!(matches!(result, MonitorCheckExecution::Cancelled));
        assert!(start.elapsed() < Duration::from_millis(1_500));

        Ok(())
    }

    #[tokio::test]
    async fn check_site_monitors_releases_claim_when_shutdown_cancels_in_flight_check() -> Result<()>
    {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Cancel Site", "https://cancel.test.com")
            .await?;
        let url = spawn_hanging_http_server().await?;
        let monitor = test_db.seed_http_monitor(site.id, &url).await?;
        let claimed_monitor = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-cancel",
            10,
            60,
        )
        .await?
        .into_iter()
        .find(|claimed| claimed.id == monitor.id)
        .expect("monitor should be claimed");
        let config = test_config();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn({
            let pool = test_db.pool.clone();
            async move {
                check_site_monitors(
                    &pool,
                    vec![claimed_monitor],
                    &config,
                    "worker-cancel",
                    shutdown_rx,
                )
                .await
            }
        });

        sleep(Duration::from_millis(50)).await;
        let _ = shutdown_tx.send(true);
        let _ = timeout(Duration::from_secs(2), task).await??;

        let checks = site_monitor_checks::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &site_monitor_checks::CheckCursorQuery {
                cursor_checked_at: None,
                cursor_id: None,
                is_success: None,
                limit: 10,
            },
        )
        .await?;
        let refreshed_monitor = test_db
            .get_http_monitor_by_target_url(site.id, &url)
            .await?
            .expect("monitor should still exist");

        assert!(checks.is_empty());
        assert!(refreshed_monitor.check_claimed_at.is_none());
        assert!(refreshed_monitor.check_lease_until.is_none());
        assert!(refreshed_monitor.check_claimed_by.is_none());
        assert!(refreshed_monitor.last_checked_at.is_none());

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn check_site_monitors_extends_lease_while_check_is_running() -> Result<()> {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Lease Site", "https://lease.test.com")
            .await?;
        let url = spawn_hanging_http_server().await?;
        let monitor = test_db.seed_http_monitor(site.id, &url).await?;
        let claimed_monitor = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-lease-a",
            10,
            1,
        )
        .await?
        .into_iter()
        .find(|claimed| claimed.id == monitor.id)
        .expect("monitor should be claimed");
        let mut config = test_config();
        config.lease_site_check_seconds = 1;
        config.http_check_timeout_seconds = 4;
        config.http_check_max_attempts = 1;
        config.http_check_retry_delays_ms.clear();
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let task = tokio::spawn({
            let pool = test_db.pool.clone();
            async move {
                check_site_monitors(
                    &pool,
                    vec![claimed_monitor],
                    &config,
                    "worker-lease-a",
                    shutdown_rx,
                )
                .await
            }
        });

        sleep(Duration::from_millis(1_500)).await;
        let competing_claim = site_monitors::repository::claim_site_monitors_due_for_check(
            &test_db.pool,
            "worker-lease-b",
            10,
            1,
        )
        .await?;

        assert!(competing_claim.is_empty());

        let _ = shutdown_tx.send(true);
        let _ = timeout(Duration::from_secs(3), task).await??;

        test_db.cleanup().await?;
        Ok(())
    }

    #[tokio::test]
    async fn prune_site_monitor_check_history_if_due_deletes_expired_rows_in_batches() -> Result<()>
    {
        let test_db = TestDb::spawn().await?;
        let site = test_db
            .seed_site("Retention Site", "https://retention.test.com")
            .await?;
        let monitor = test_db
            .seed_http_monitor(site.id, "https://retention.test.com/health")
            .await?;
        let old_checked_at = Utc::now() - ChronoDuration::days(120);
        let recent_checked_at = Utc::now() - ChronoDuration::days(10);
        test_db
            .seed_site_monitor_check(&monitor, old_checked_at, false)
            .await?;
        test_db
            .seed_site_monitor_check(&monitor, old_checked_at + ChronoDuration::minutes(1), true)
            .await?;
        test_db
            .seed_site_monitor_check(&monitor, old_checked_at + ChronoDuration::minutes(2), false)
            .await?;
        test_db
            .seed_site_monitor_check(&monitor, recent_checked_at, true)
            .await?;
        let mut config = test_config();
        config.site_monitor_check_retention_days = 90;
        config.site_monitor_check_retention_batch_size = 2;
        config.site_monitor_check_retention_interval_seconds = 3600;
        let mut next_run_at = Utc::now();

        let first_deleted = prune_site_monitor_check_history_if_due(
            &test_db.pool,
            &config,
            "worker-retention",
            &mut next_run_at,
        )
        .await?;

        assert_eq!(first_deleted, 2);
        assert_eq!(test_db.count_site_monitor_checks().await?, 2);

        let second_deleted = prune_site_monitor_check_history_if_due(
            &test_db.pool,
            &config,
            "worker-retention",
            &mut next_run_at,
        )
        .await?;

        assert_eq!(second_deleted, 1);
        assert_eq!(test_db.count_site_monitor_checks().await?, 1);

        let checks = site_monitor_checks::repository::list_by_site_id(
            &test_db.pool,
            site.id,
            &site_monitor_checks::CheckCursorQuery {
                cursor_checked_at: None,
                cursor_id: None,
                is_success: None,
                limit: 10,
            },
        )
        .await?;
        assert_eq!(checks.len(), 1);
        assert!(
            checks[0].checked_at
                >= Utc::now()
                    - ChronoDuration::days(config.site_monitor_check_retention_days as i64)
        );

        test_db.cleanup().await?;
        Ok(())
    }

    struct TestDb {
        admin_pool: PgPool,
        pool: PgPool,
        schema: String,
    }

    impl TestDb {
        async fn spawn() -> Result<Self> {
            dotenvy::dotenv().ok();

            let base_database_url = std::env::var("TEST_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .context("set TEST_DATABASE_URL or DATABASE_URL for worker tests")?;
            let admin_pool = PgPool::connect(&base_database_url).await?;
            let schema = unique_schema_name();
            admin_pool
                .execute(sqlx::query(sqlx::AssertSqlSafe(format!(
                    "CREATE SCHEMA {}",
                    schema
                ))))
                .await?;

            let schema_database_url = schema_database_url(&base_database_url, &schema)?;
            let pool = PgPool::connect(&schema_database_url).await?;
            apply_migrations(&pool).await?;

            Ok(Self {
                admin_pool,
                pool,
                schema,
            })
        }

        async fn cleanup(self) -> Result<()> {
            self.pool.close().await;
            self.admin_pool
                .execute(sqlx::query(sqlx::AssertSqlSafe(format!(
                    "DROP SCHEMA {} CASCADE",
                    self.schema
                ))))
                .await?;
            self.admin_pool.close().await;
            Ok(())
        }

        async fn seed_site(&self, name: &str, base_url: &str) -> Result<sites::Site> {
            sites::repository::create_site(&self.pool, name, base_url).await
        }

        async fn seed_http_monitor(
            &self,
            site_id: i64,
            target_url: &str,
        ) -> Result<site_monitors::SiteMonitor> {
            site_monitors::repository::create_http_site_monitor(
                &self.pool,
                site_id,
                &site_monitors::HttpMonitorParams {
                    target_url,
                    check_interval_seconds: 60,
                    expected_status_code: 200,
                    body_must_contain: None,
                    body_must_not_contain: None,
                    body_must_contain_texts: None,
                    body_must_not_contain_texts: None,
                    json_path_exists: None,
                    json_path_equals: None,
                    json_path_not_equals: None,
                    max_response_time_ms: None,
                    required_header_name: None,
                    required_header_value: None,
                    header_assertions: None,
                    ssl_certificate_checks_enabled: false,
                    ssl_expiry_warning_days: None,
                    http_check_timeout_seconds_override: None,
                    http_check_max_attempts_override: None,
                    http_check_retry_delays_ms_override: None,
                    is_active: true,
                },
            )
            .await
        }

        async fn get_http_monitor_by_target_url(
            &self,
            site_id: i64,
            target_url: &str,
        ) -> Result<Option<site_monitors::SiteMonitor>> {
            site_monitors::repository::get_http_monitor_by_site_id_and_target_url(
                &self.pool, site_id, target_url,
            )
            .await
        }

        async fn seed_site_monitor_check(
            &self,
            monitor: &site_monitors::SiteMonitor,
            checked_at: DateTime<Utc>,
            is_success: bool,
        ) -> Result<site_monitor_checks::SiteMonitorCheck> {
            let status_code = if is_success { Some(200) } else { Some(500) };
            let response_time_ms = Some(150);
            let error_message = (!is_success).then_some("retained failure");
            let failure_reason = (!is_success).then_some("unexpected_status");

            let check = sqlx::query_as::<_, site_monitor_checks::SiteMonitorCheck>(
                r#"
                INSERT INTO site_monitor_checks (
                    site_monitor_id,
                    checked_at,
                    monitor_type,
                    url_checked,
                    expected_status_code,
                    is_success,
                    status_code,
                    response_time_ms,
                    total_duration_ms,
                    attempt_count,
                    was_retried,
                    failure_reason,
                    error_message,
                    certificate_expires_at,
                    certificate_days_remaining,
                    certificate_issuer,
                    certificate_subject,
                    certificate_domain
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
                RETURNING
                    id,
                    site_monitor_id,
                    checked_at,
                    monitor_type,
                    url_checked,
                    expected_status_code,
                    is_success,
                    status_code,
                    response_time_ms,
                    total_duration_ms,
                    attempt_count,
                    was_retried,
                    failure_reason,
                    error_message,
                    certificate_expires_at,
                    certificate_days_remaining,
                    certificate_issuer,
                    certificate_subject,
                    certificate_domain
                "#,
            )
            .bind(monitor.id)
            .bind(checked_at)
            .bind(monitor.monitor_type)
            .bind(&monitor.target_url)
            .bind(Some(monitor.expected_status_code))
            .bind(is_success)
            .bind(status_code)
            .bind(response_time_ms)
            .bind(response_time_ms)
            .bind(1_i32)
            .bind(false)
            .bind(failure_reason)
            .bind(error_message)
            .bind(None::<DateTime<Utc>>)
            .bind(None::<i32>)
            .bind(None::<&str>)
            .bind(None::<&str>)
            .bind(None::<&str>)
            .fetch_one(&self.pool)
            .await?;

            Ok(check)
        }

        async fn count_site_monitor_checks(&self) -> Result<i64> {
            let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM site_monitor_checks")
                .fetch_one(&self.pool)
                .await?;

            Ok(count)
        }
    }

    fn test_config() -> Config {
        Config {
            database_url: "postgresql://unused".to_string(),
            api_bind_address: "127.0.0.1:3000".to_string(),
            access_token_ttl_seconds: 3600,
            cors_allowed_origins: Vec::new(),
            db_max_connections: 20,
            db_min_connections: 2,
            db_acquire_timeout_seconds: 5,
            db_idle_timeout_seconds: 600,
            db_max_lifetime_seconds: 1800,
            api_db_max_connections: 20,
            api_db_min_connections: 2,
            worker_db_max_connections: 20,
            worker_db_min_connections: 2,
            worker_max_poll_interval_ms: 5_000,
            site_worker_count: 1,
            max_http_concurrent_checks: 1,
            due_sites_batch_size: 10,
            lease_site_check_seconds: 60,
            http_check_timeout_seconds: 1,
            http_check_max_response_body_bytes: 64 * 1024,
            http_check_max_attempts: 3,
            http_check_retry_delays_ms: vec![10, 20],
            http_check_retry_jitter_percent: 0,
            site_monitor_check_retention_days: 90,
            site_monitor_check_retention_interval_seconds: 3600,
            site_monitor_check_retention_batch_size: 5000,
            due_notification_batch_size: 10,
            max_notification_concurrent_deliveries: 1,
            lease_notification_delivery_seconds: 60,
            notification_delivery_timeout_seconds: 1,
            notification_max_attempts: 3,
            notification_retry_base_seconds: 30,
            auth_rate_limit_max_requests: 10,
            auth_rate_limit_window_seconds: 60,
            trust_proxy_headers: false,
            trusted_proxy_ips: Vec::new(),
            cookie_secure: false,
            http_monitor_allow_private_targets: true,
            webhook_secret_encryption_key: test_webhook_secret_encryption_key(),
            smtp: None,
        }
    }

    async fn wait_for_checks(pool: &PgPool, site_id: i64, minimum_count: usize) -> Result<()> {
        for _ in 0..40 {
            let checks = site_monitor_checks::repository::list_by_site_id(
                pool,
                site_id,
                &site_monitor_checks::CheckCursorQuery {
                    cursor_checked_at: None,
                    cursor_id: None,
                    is_success: None,
                    limit: minimum_count as i64 + 5,
                },
            )
            .await?;

            if checks.len() >= minimum_count {
                return Ok(());
            }

            sleep(Duration::from_millis(150)).await;
        }

        anyhow::bail!("timed out waiting for worker checks")
    }

    async fn apply_migrations(pool: &PgPool) -> Result<()> {
        for path in migration_paths()? {
            let sql = fs::read_to_string(&path)
                .with_context(|| format!("failed to read migration {}", path.display()))?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(pool)
                .await?;
        }

        Ok(())
    }

    fn migration_paths() -> Result<Vec<PathBuf>> {
        let mut paths = fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations"))?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        paths.sort();
        Ok(paths)
    }

    fn unique_schema_name() -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_millis();
        let suffix = NEXT_TEST_ID.fetch_add(1, Ordering::Relaxed);
        format!("sentinel_worker_test_{}_{}", millis, suffix)
    }

    fn schema_database_url(base_database_url: &str, schema: &str) -> Result<String> {
        let mut url = reqwest::Url::parse(base_database_url)?;
        url.query_pairs_mut()
            .append_pair("options", &format!("-c search_path={},public", schema));
        Ok(url.to_string())
    }

    async fn spawn_http_server(response: &'static str) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept test connection");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write test response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_hanging_http_server() -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept hanging connection");
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            sleep(Duration::from_secs(5)).await;
            drop(stream);
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_delayed_http_server(attempt_counter: Arc<AtomicUsize>) -> Result<String> {
        let reserved_listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let address = reserved_listener.local_addr()?;
        drop(reserved_listener);

        tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            let listener = TcpListener::bind(address)
                .await
                .expect("bind delayed retry test server");
            let (mut second_stream, _) = listener
                .accept()
                .await
                .expect("accept delayed retry test connection");
            attempt_counter.fetch_add(1, Ordering::Relaxed);
            let mut buffer = [0_u8; 1024];
            let _ = second_stream.read(&mut buffer).await;
            second_stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .expect("write delayed retry success response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_counting_http_server(
        response: &'static str,
        attempt_counter: Arc<AtomicUsize>,
    ) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept counting connection");
            attempt_counter.fetch_add(1, Ordering::Relaxed);
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write counting response");
        });

        Ok(format!("http://{}", address))
    }

    async fn spawn_slow_counting_http_server(
        response: &'static str,
        response_delay: Duration,
        attempt_counter: Arc<AtomicUsize>,
    ) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;

        tokio::spawn(async move {
            let (mut stream, _) = listener
                .accept()
                .await
                .expect("accept slow counting connection");
            attempt_counter.fetch_add(1, Ordering::Relaxed);
            let mut buffer = [0_u8; 1024];
            let _ = stream.read(&mut buffer).await;
            sleep(response_delay).await;
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write slow counting response");
        });

        Ok(format!("http://{}", address))
    }

    #[test]
    fn classify_incident_action_opens_on_first_failure() {
        assert_eq!(
            classify_incident_action(None, false, false),
            IncidentAction::Open
        );
    }

    #[test]
    fn classify_incident_action_opens_after_previous_success() {
        assert_eq!(
            classify_incident_action(Some(true), false, false),
            IncidentAction::Open
        );
    }

    #[test]
    fn classify_incident_action_updates_existing_incident_on_ongoing_failure() {
        assert_eq!(
            classify_incident_action(Some(false), false, true),
            IncidentAction::UpdateExisting
        );
    }

    #[test]
    fn classify_incident_action_reopens_missing_incident_on_ongoing_failure() {
        assert_eq!(
            classify_incident_action(Some(false), false, false),
            IncidentAction::ReopenMissing
        );
    }

    #[test]
    fn classify_incident_action_resolves_open_incident_on_recovery() {
        assert_eq!(
            classify_incident_action(Some(false), true, true),
            IncidentAction::Resolve
        );
    }

    #[test]
    fn classify_incident_action_is_no_op_for_recovery_without_open_incident() {
        assert_eq!(
            classify_incident_action(Some(false), true, false),
            IncidentAction::None
        );
    }

    #[test]
    fn classify_incident_action_is_no_op_for_sustained_success() {
        assert_eq!(
            classify_incident_action(Some(true), true, false),
            IncidentAction::None
        );
        assert_eq!(
            classify_incident_action(None, true, false),
            IncidentAction::None
        );
    }

    fn build_test_monitor(
        id: i64,
        target_url: &str,
        expected_status_code: i32,
    ) -> site_monitors::SiteMonitor {
        let timestamp = chrono::Utc::now();

        site_monitors::SiteMonitor {
            id,
            site_id: 1,
            monitor_type: site_monitors::SiteMonitorType::Http,
            target_url: target_url.to_string(),
            check_interval_seconds: 60,
            expected_status_code,
            body_must_contain: None,
            body_must_not_contain: None,
            body_must_contain_texts: None,
            body_must_not_contain_texts: None,
            json_path_exists: None,
            json_path_equals: None,
            json_path_not_equals: None,
            max_response_time_ms: None,
            required_header_name: None,
            required_header_value: None,
            header_assertions: None,
            ssl_certificate_checks_enabled: false,
            ssl_expiry_warning_days: None,
            tcp_target_host: None,
            tcp_target_port: None,
            dns_hostname: None,
            dns_record_type: None,
            dns_expected_value: None,
            dns_nameserver: None,
            heartbeat_token: None,
            heartbeat_grace_seconds: None,
            http_check_timeout_seconds_override: None,
            http_check_max_attempts_override: None,
            http_check_retry_delays_ms_override: None,
            is_active: true,
            check_claimed_at: None,
            check_lease_until: None,
            check_claimed_by: None,
            last_checked_at: None,
            last_successful_check_at: None,
            last_is_success: None,
            last_status_code: None,
            last_response_time_ms: None,
            last_failure_reason: None,
            last_error_message: None,
            last_heartbeat_received_at: None,
            last_certificate_expires_at: None,
            last_certificate_days_remaining: None,
            last_certificate_issuer: None,
            last_certificate_subject: None,
            last_certificate_domain: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }
}
