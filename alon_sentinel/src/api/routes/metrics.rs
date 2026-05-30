use std::{sync::Arc, time::Instant};

use axum::{Router, extract::State, http::header, response::IntoResponse, routing::get};
use prometheus::{
    Encoder, Gauge, HistogramOpts, HistogramVec, IntGauge, IntGaugeVec, Opts, Registry, TextEncoder,
};
use tracing::warn;

use crate::api::state::AppState;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/metrics", get(metrics_handler))
}

#[derive(sqlx::FromRow)]
struct MonitorTypeCount {
    monitor_type: String,
    count: i64,
}

#[derive(sqlx::FromRow)]
struct CheckResultCount {
    is_success: bool,
    count: i64,
}

#[derive(sqlx::FromRow)]
struct StatusCount {
    status: String,
    count: i64,
}

#[derive(sqlx::FromRow)]
struct CheckDurationRow {
    monitor_type: String,
    total_duration_ms: i32,
}

#[derive(sqlx::FromRow)]
struct CheckResponseTimeRow {
    monitor_type: String,
    response_time_ms: i32,
}

#[derive(sqlx::FromRow)]
struct CheckStatusCodeRow {
    monitor_type: String,
    status_code: String,
    count: i64,
}

#[derive(sqlx::FromRow)]
struct SslDaysRemainingRow {
    last_certificate_days_remaining: i32,
}

#[derive(sqlx::FromRow)]
struct NotificationLatencyRow {
    channel_type: String,
    latency_seconds: f64,
}

#[derive(sqlx::FromRow)]
struct NotificationRetryRow {
    channel_type: String,
    retried_deliveries: i64,
    extra_attempts: i64,
}

#[derive(sqlx::FromRow)]
struct ChannelTypeCount {
    channel_type: String,
    count: i64,
}

#[derive(sqlx::FromRow)]
struct CheckRetryRow {
    monitor_type: String,
    retried_checks: i64,
    extra_attempts: i64,
}

async fn metrics_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let scrape_started_at = Instant::now();
    let registry = Registry::new();

    let scrape_duration_seconds = Gauge::new(
        "alon_sentinel_scrape_duration_seconds",
        "Time taken to collect and encode all metrics, in seconds",
    )
    .expect("valid metric");
    registry
        .register(Box::new(scrape_duration_seconds.clone()))
        .expect("unique name");

    let monitors_active = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_monitors_active",
            "Number of active monitors by type",
        ),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(monitors_active.clone()))
        .expect("unique name");

    let incidents_open = IntGauge::new(
        "alon_sentinel_incidents_open",
        "Number of currently open (unresolved) incidents",
    )
    .expect("valid metric");
    registry
        .register(Box::new(incidents_open.clone()))
        .expect("unique name");

    let checks_recent = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_checks_recent",
            "Number of checks completed in the last 60 seconds",
        ),
        &["result"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(checks_recent.clone()))
        .expect("unique name");

    let notification_deliveries = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_notification_deliveries",
            "Number of notification deliveries by status",
        ),
        &["status"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(notification_deliveries.clone()))
        .expect("unique name");

    let db_pool_connections_max = IntGauge::new(
        "alon_sentinel_db_pool_connections_max",
        "Configured maximum number of connections in the API DB pool",
    )
    .expect("valid metric");
    registry
        .register(Box::new(db_pool_connections_max.clone()))
        .expect("unique name");

    let db_pool_connections_open = IntGauge::new(
        "alon_sentinel_db_pool_connections_open",
        "Current number of open connections in the API DB pool (idle + active)",
    )
    .expect("valid metric");
    registry
        .register(Box::new(db_pool_connections_open.clone()))
        .expect("unique name");

    let db_pool_connections_idle = IntGauge::new(
        "alon_sentinel_db_pool_connections_idle",
        "Current number of idle connections in the API DB pool",
    )
    .expect("valid metric");
    registry
        .register(Box::new(db_pool_connections_idle.clone()))
        .expect("unique name");

    db_pool_connections_max.set(state.db_max_connections as i64);
    db_pool_connections_open.set(state.pool.size() as i64);
    db_pool_connections_idle.set(state.pool.num_idle() as i64);

    let process_resident_memory_bytes = IntGauge::new(
        "alon_sentinel_process_resident_memory_bytes",
        "Resident set size of the API process in bytes",
    )
    .expect("valid metric");
    registry
        .register(Box::new(process_resident_memory_bytes.clone()))
        .expect("unique name");

    let process_virtual_memory_bytes = IntGauge::new(
        "alon_sentinel_process_virtual_memory_bytes",
        "Virtual memory size of the API process in bytes",
    )
    .expect("valid metric");
    registry
        .register(Box::new(process_virtual_memory_bytes.clone()))
        .expect("unique name");

    if let Some((rss, virt)) = read_process_memory_bytes() {
        process_resident_memory_bytes.set(rss);
        process_virtual_memory_bytes.set(virt);
    }

    let tokio_alive_tasks = IntGauge::new(
        "alon_sentinel_tokio_alive_tasks",
        "Number of tasks currently alive in the Tokio runtime",
    )
    .expect("valid metric");
    registry
        .register(Box::new(tokio_alive_tasks.clone()))
        .expect("unique name");

    let tokio_worker_threads = IntGauge::new(
        "alon_sentinel_tokio_worker_threads",
        "Number of worker threads in the Tokio runtime",
    )
    .expect("valid metric");
    registry
        .register(Box::new(tokio_worker_threads.clone()))
        .expect("unique name");

    let rt = tokio::runtime::Handle::current();
    let rt_metrics = rt.metrics();
    tokio_alive_tasks.set(rt_metrics.num_alive_tasks() as i64);
    tokio_worker_threads.set(rt_metrics.num_workers() as i64);

    let worker_queue_depth = IntGauge::new(
        "alon_sentinel_worker_queue_depth",
        "Number of active monitors currently due for a check (not leased by any worker)",
    )
    .expect("valid metric");
    registry
        .register(Box::new(worker_queue_depth.clone()))
        .expect("unique name");

    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
         FROM site_monitors sm \
         INNER JOIN sites s ON s.id = sm.site_id \
         WHERE sm.is_active = TRUE \
           AND s.is_active = TRUE \
           AND (sm.last_checked_at IS NULL \
                OR sm.last_checked_at <= NOW() - (sm.check_interval_seconds * INTERVAL '1 second')) \
           AND (sm.check_lease_until IS NULL OR sm.check_lease_until < NOW())",
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(count) => worker_queue_depth.set(count),
        Err(e) => warn!(error = ?e, "metrics: failed to query worker queue depth"),
    }

    let worker_active_leases = IntGauge::new(
        "alon_sentinel_worker_active_leases",
        "Number of monitors currently claimed and being checked by a worker",
    )
    .expect("valid metric");
    registry
        .register(Box::new(worker_active_leases.clone()))
        .expect("unique name");

    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) \
         FROM site_monitors \
         WHERE check_claimed_by IS NOT NULL \
           AND check_lease_until > NOW()",
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(count) => worker_active_leases.set(count),
        Err(e) => warn!(error = ?e, "metrics: failed to query worker active leases"),
    }

    match sqlx::query_as::<_, MonitorTypeCount>(
        "SELECT monitor_type::text AS monitor_type, COUNT(*) AS count \
         FROM site_monitors WHERE is_active = TRUE GROUP BY monitor_type",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                monitors_active
                    .with_label_values(&[&row.monitor_type])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query active monitors"),
    }

    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM site_monitor_incidents WHERE resolved_at IS NULL",
    )
    .fetch_one(&state.pool)
    .await
    {
        Ok(count) => incidents_open.set(count),
        Err(e) => warn!(error = ?e, "metrics: failed to query open incidents"),
    }

    match sqlx::query_as::<_, CheckResultCount>(
        "SELECT is_success, COUNT(*) AS count FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '60 seconds' GROUP BY is_success",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                let result_label = if row.is_success { "success" } else { "failure" };
                checks_recent
                    .with_label_values(&[result_label])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query recent checks"),
    }

    let check_duration_seconds = HistogramVec::new(
        HistogramOpts::new(
            "alon_sentinel_check_duration_seconds",
            "Total check duration including retries, in seconds (last 5 minutes)",
        )
        .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_duration_seconds.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, CheckDurationRow>(
        "SELECT monitor_type::text AS monitor_type, total_duration_ms \
         FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '5 minutes' \
           AND total_duration_ms IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                check_duration_seconds
                    .with_label_values(&[&row.monitor_type])
                    .observe(row.total_duration_ms as f64 / 1000.0);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query check durations"),
    }

    let ssl_days_remaining = prometheus::Histogram::with_opts(
        HistogramOpts::new(
            "alon_sentinel_ssl_certificate_days_remaining",
            "Days until SSL certificate expiry across active SSL monitors (current state)",
        )
        .buckets(vec![7.0, 14.0, 21.0, 30.0, 60.0, 90.0]),
    )
    .expect("valid metric");
    registry
        .register(Box::new(ssl_days_remaining.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, SslDaysRemainingRow>(
        "SELECT last_certificate_days_remaining \
         FROM site_monitors \
         WHERE monitor_type = 'ssl' \
           AND is_active = TRUE \
           AND last_certificate_days_remaining IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                ssl_days_remaining.observe(row.last_certificate_days_remaining as f64);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query SSL certificate expiry"),
    }

    let check_response_time_seconds = HistogramVec::new(
        HistogramOpts::new(
            "alon_sentinel_check_response_time_seconds",
            "Per-attempt response time in seconds, excluding retries (last 5 minutes)",
        )
        .buckets(vec![0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_response_time_seconds.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, CheckResponseTimeRow>(
        "SELECT monitor_type::text AS monitor_type, response_time_ms \
         FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '5 minutes' \
           AND response_time_ms IS NOT NULL",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                check_response_time_seconds
                    .with_label_values(&[&row.monitor_type])
                    .observe(row.response_time_ms as f64 / 1000.0);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query check response times"),
    }

    let check_status_codes = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_check_status_codes",
            "Number of checks by HTTP status code returned (last 5 minutes)",
        ),
        &["monitor_type", "status_code"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_status_codes.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, CheckStatusCodeRow>(
        "SELECT monitor_type::text AS monitor_type, \
                status_code::text AS status_code, \
                COUNT(*) AS count \
         FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '5 minutes' \
           AND status_code IS NOT NULL \
         GROUP BY monitor_type, status_code",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                check_status_codes
                    .with_label_values(&[&row.monitor_type, &row.status_code])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query check status codes"),
    }

    let check_retried = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_check_retried",
            "Number of checks that required at least one retry (last 5 minutes)",
        ),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_retried.clone()))
        .expect("unique name");

    let check_extra_attempts = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_check_extra_attempts",
            "Total extra attempts beyond the first across all retried checks (last 5 minutes)",
        ),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_extra_attempts.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, CheckRetryRow>(
        "SELECT monitor_type::text AS monitor_type, \
                COUNT(*) AS retried_checks, \
                SUM(attempt_count - 1)::bigint AS extra_attempts \
         FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '5 minutes' \
           AND was_retried = TRUE \
         GROUP BY monitor_type",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                check_retried
                    .with_label_values(&[&row.monitor_type])
                    .set(row.retried_checks);
                check_extra_attempts
                    .with_label_values(&[&row.monitor_type])
                    .set(row.extra_attempts);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query check retries"),
    }

    let check_timeouts = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_check_timeouts",
            "Number of checks that timed out (last 5 minutes)",
        ),
        &["monitor_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(check_timeouts.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, MonitorTypeCount>(
        "SELECT monitor_type::text AS monitor_type, COUNT(*) AS count \
         FROM site_monitor_checks \
         WHERE checked_at > NOW() - INTERVAL '5 minutes' \
           AND failure_reason = 'timeout' \
         GROUP BY monitor_type",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                check_timeouts
                    .with_label_values(&[&row.monitor_type])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query check timeouts"),
    }

    let notification_send_latency_seconds = HistogramVec::new(
        HistogramOpts::new(
            "alon_sentinel_notification_send_latency_seconds",
            "Time from notification enqueue to delivery, in seconds (last 5 minutes)",
        )
        .buckets(vec![0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
        &["channel_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(notification_send_latency_seconds.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, NotificationLatencyRow>(
        "SELECT nc.channel_type::text AS channel_type, \
                EXTRACT(EPOCH FROM (nd.delivered_at - nd.created_at))::float8 AS latency_seconds \
         FROM notification_deliveries nd \
         INNER JOIN notification_channels nc ON nc.id = nd.notification_channel_id \
         WHERE nd.delivered_at > NOW() - INTERVAL '5 minutes'",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                notification_send_latency_seconds
                    .with_label_values(&[&row.channel_type])
                    .observe(row.latency_seconds);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query notification send latency"),
    }

    let notification_retried = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_notification_retried",
            "Number of notification deliveries that required at least one retry (last 5 minutes)",
        ),
        &["channel_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(notification_retried.clone()))
        .expect("unique name");

    let notification_extra_attempts = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_notification_extra_attempts",
            "Total extra delivery attempts beyond the first across retried notifications (last 5 minutes)",
        ),
        &["channel_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(notification_extra_attempts.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, NotificationRetryRow>(
        "SELECT nc.channel_type::text AS channel_type, \
                COUNT(*) AS retried_deliveries, \
                SUM(nd.attempts - 1)::bigint AS extra_attempts \
         FROM notification_deliveries nd \
         INNER JOIN notification_channels nc ON nc.id = nd.notification_channel_id \
         WHERE nd.attempts > 1 \
           AND nd.updated_at > NOW() - INTERVAL '5 minutes' \
         GROUP BY nc.channel_type",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                notification_retried
                    .with_label_values(&[&row.channel_type])
                    .set(row.retried_deliveries);
                notification_extra_attempts
                    .with_label_values(&[&row.channel_type])
                    .set(row.extra_attempts);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query notification retries"),
    }

    let notification_delivery_failures = IntGaugeVec::new(
        Opts::new(
            "alon_sentinel_notification_delivery_failures",
            "Total permanently failed notification deliveries by channel type",
        ),
        &["channel_type"],
    )
    .expect("valid metric");
    registry
        .register(Box::new(notification_delivery_failures.clone()))
        .expect("unique name");

    match sqlx::query_as::<_, ChannelTypeCount>(
        "SELECT nc.channel_type::text AS channel_type, COUNT(*) AS count \
         FROM notification_deliveries nd \
         INNER JOIN notification_channels nc ON nc.id = nd.notification_channel_id \
         WHERE nd.status = 'failed' \
         GROUP BY nc.channel_type",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                notification_delivery_failures
                    .with_label_values(&[&row.channel_type])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query notification delivery failures"),
    }

    match sqlx::query_as::<_, StatusCount>(
        "SELECT status::text AS status, COUNT(*) AS count \
         FROM notification_deliveries GROUP BY status",
    )
    .fetch_all(&state.pool)
    .await
    {
        Ok(rows) => {
            for row in rows {
                notification_deliveries
                    .with_label_values(&[&row.status])
                    .set(row.count);
            }
        }
        Err(e) => warn!(error = ?e, "metrics: failed to query notification deliveries"),
    }

    scrape_duration_seconds.set(scrape_started_at.elapsed().as_secs_f64());

    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    if let Err(e) = encoder.encode(&registry.gather(), &mut buffer) {
        warn!(error = ?e, "metrics: failed to encode metrics");
    }

    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        buffer,
    )
}

fn read_process_memory_bytes() -> Option<(i64, i64)> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let mut rss_kb: Option<i64> = None;
        let mut vm_kb: Option<i64> = None;
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                rss_kb = rest
                    .trim()
                    .strip_suffix(" kB")
                    .and_then(|s| s.trim().parse().ok());
            } else if let Some(rest) = line.strip_prefix("VmSize:") {
                vm_kb = rest
                    .trim()
                    .strip_suffix(" kB")
                    .and_then(|s| s.trim().parse().ok());
            }
            if rss_kb.is_some() && vm_kb.is_some() {
                break;
            }
        }
        Some((rss_kb? * 1024, vm_kb? * 1024))
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
