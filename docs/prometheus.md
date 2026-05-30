# Prometheus Metrics

Alon Sentinel exposes Prometheus-compatible metrics from the API service.

```text
GET /metrics
```

The endpoint is intended for trusted scrape targets. The production Caddy
overlay blocks public access to `/metrics` on the API hostname by default. Run
Prometheus in the same private network, behind a VPN, or behind reverse-proxy
access controls if you need to scrape it from outside the host.

## Endpoint

For local or private-network scraping:

```bash
curl http://api:3000/metrics
```

For a local source build:

```bash
curl http://127.0.0.1:3000/metrics
```

## Sample Output

```text
# HELP alon_sentinel_monitors_active Number of active monitors by type
# TYPE alon_sentinel_monitors_active gauge
alon_sentinel_monitors_active{monitor_type="http"} 5

# HELP alon_sentinel_incidents_open Number of currently open (unresolved) incidents
# TYPE alon_sentinel_incidents_open gauge
alon_sentinel_incidents_open 0

# HELP alon_sentinel_check_response_time_seconds Per-attempt response time in seconds, excluding retries (last 5 minutes)
# TYPE alon_sentinel_check_response_time_seconds histogram
alon_sentinel_check_response_time_seconds_bucket{monitor_type="http",le="0.05"} 25
alon_sentinel_check_response_time_seconds_sum{monitor_type="http"} 0.052
alon_sentinel_check_response_time_seconds_count{monitor_type="http"} 25
```

## Metric Descriptions

| Metric | Type | Description |
| --- | --- | --- |
| `alon_sentinel_monitors_active` | Gauge | Active monitors by monitor type. |
| `alon_sentinel_incidents_open` | Gauge | Currently unresolved incidents. |
| `alon_sentinel_checks_recent` | Gauge | Checks completed in the last 60 seconds by result. |
| `alon_sentinel_check_duration_seconds` | Histogram | Total check duration including retries over the last 5 minutes. |
| `alon_sentinel_check_response_time_seconds` | Histogram | Per-attempt response time excluding retries over the last 5 minutes. |
| `alon_sentinel_check_status_codes` | Gauge | HTTP status-code counts over the last 5 minutes. |
| `alon_sentinel_check_retried` | Gauge | Checks that required at least one retry over the last 5 minutes. |
| `alon_sentinel_check_extra_attempts` | Gauge | Extra check attempts beyond the first over the last 5 minutes. |
| `alon_sentinel_check_timeouts` | Gauge | Timed-out checks over the last 5 minutes. |
| `alon_sentinel_notification_deliveries` | Gauge | Notification deliveries by status. |
| `alon_sentinel_notification_send_latency_seconds` | Histogram | Time from notification enqueue to delivery over the last 5 minutes. |
| `alon_sentinel_notification_retried` | Gauge | Notification deliveries that required at least one retry. |
| `alon_sentinel_notification_extra_attempts` | Gauge | Extra notification attempts beyond the first. |
| `alon_sentinel_notification_delivery_failures` | Gauge | Permanently failed notification deliveries by channel type. |
| `alon_sentinel_worker_queue_depth` | Gauge | Active monitors due for a check and not leased by a worker. |
| `alon_sentinel_worker_active_leases` | Gauge | Monitors currently claimed by a worker. |
| `alon_sentinel_db_pool_connections_max` | Gauge | Configured maximum API database pool size. |
| `alon_sentinel_db_pool_connections_open` | Gauge | Current open API database pool connections. |
| `alon_sentinel_db_pool_connections_idle` | Gauge | Current idle API database pool connections. |
| `alon_sentinel_process_resident_memory_bytes` | Gauge | API process resident memory. |
| `alon_sentinel_process_virtual_memory_bytes` | Gauge | API process virtual memory. |
| `alon_sentinel_tokio_alive_tasks` | Gauge | Alive Tokio runtime tasks. |
| `alon_sentinel_tokio_worker_threads` | Gauge | Tokio runtime worker threads. |
| `alon_sentinel_scrape_duration_seconds` | Gauge | Time needed to collect and encode metrics. |
| `alon_sentinel_ssl_certificate_days_remaining` | Histogram | Days until SSL certificate expiry across active SSL monitors. |

## Latency Queries

Check response-time percentiles:

```promql
histogram_quantile(
  0.50,
  sum(rate(alon_sentinel_check_response_time_seconds_bucket[5m])) by (le, monitor_type)
)
```

```promql
histogram_quantile(
  0.95,
  sum(rate(alon_sentinel_check_response_time_seconds_bucket[5m])) by (le, monitor_type)
)
```

```promql
histogram_quantile(
  0.99,
  sum(rate(alon_sentinel_check_response_time_seconds_bucket[5m])) by (le, monitor_type)
)
```

Average response time:

```promql
sum(rate(alon_sentinel_check_response_time_seconds_sum[5m])) by (monitor_type)
/
sum(rate(alon_sentinel_check_response_time_seconds_count[5m])) by (monitor_type)
```

## Prometheus Scrape Config

When Prometheus runs on the same Docker network:

```yaml
scrape_configs:
  - job_name: alon-sentinel
    metrics_path: /metrics
    static_configs:
      - targets:
          - api:3000
```

## Integrations

The metrics endpoint can feed:

- Prometheus
- Grafana
- Alertmanager
- VictoriaMetrics
- Thanos

Keep `/metrics` on a trusted network path. If you intentionally expose it
through a public reverse proxy, add IP allowlisting, VPN access, or proxy-level
authentication.
