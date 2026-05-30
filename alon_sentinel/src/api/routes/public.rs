use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::Response,
    routing::get,
};
use chrono::Utc;
use futures::future::try_join_all;
use serde::Serialize;

use crate::{
    api::{error::ApiError, extractors::RequestContext, state::AppState},
    domain::{
        site_monitor_checks, site_monitor_incidents,
        site_monitor_incidents::SiteMonitorIncidentStatus, site_monitors, site_status_pages, sites,
    },
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/v1/public/status/{slug}", get(get_public_status_page))
}

#[derive(Serialize)]
struct PublicUptimeDayBucket {
    date: String,
    total: i64,
    success: i64,
}

#[derive(Serialize)]
struct PublicMonitorStatus {
    label: String,
    monitor_type: &'static str,
    status: &'static str,
    response_time_ms: Option<i32>,
    last_checked_at: Option<String>,
    uptime_7d: Option<f64>,
    uptime_30d: Option<f64>,
    uptime_history: Vec<PublicUptimeDayBucket>,
}

#[derive(Serialize)]
struct PublicOpenIncident {
    opened_at: String,
    monitor_label: String,
    monitor_type: &'static str,
}

#[derive(Serialize)]
struct PublicResolvedIncident {
    opened_at: String,
    resolved_at: String,
    monitor_label: String,
    monitor_type: &'static str,
    downtime_seconds: Option<i32>,
    failure_reason: Option<String>,
}

#[derive(Serialize)]
struct PublicStatusPageResponse {
    slug: String,
    page_title: String,
    overall_status: &'static str,
    show_monitor_details: bool,
    show_uptime_percentages: bool,
    monitors: Vec<PublicMonitorStatus>,
    uptime_7d: Option<f64>,
    uptime_30d: Option<f64>,
    open_incidents: Vec<PublicOpenIncident>,
    incident_history: Vec<PublicResolvedIncident>,
    last_updated: String,
}

fn json_bytes_response(bytes: Vec<u8>) -> Result<Response, ApiError> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .map_err(|e| ApiError::internal_error(e.into()))
}

async fn get_public_status_page(
    State(state): State<Arc<AppState>>,
    request_context: RequestContext,
    Path(slug): Path<String>,
) -> Result<Response, ApiError> {
    if !state
        .public_rate_limiter
        .check("status_page", &request_context.ip_address)
    {
        return Err(ApiError::too_many_requests("too many requests"));
    }

    if let Some(cached) = state.status_page_cache.get(&slug) {
        return json_bytes_response(cached);
    }

    let status_page = site_status_pages::repository::get_by_slug(&state.pool, &slug)
        .await
        .map_err(ApiError::internal_error)?
        .filter(|p| p.is_enabled)
        .ok_or_else(|| ApiError::not_found("status page not found"))?;

    let site = sites::repository::get_site_by_id(&state.pool, status_page.site_id)
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("status page not found"))?;

    let (http_monitors, ssl_monitors, heartbeat_monitors, tcp_monitors, dns_monitors) =
        tokio::try_join!(
            site_monitors::repository::list_http_monitors_by_site_id(&state.pool, site.id),
            site_monitors::repository::list_ssl_monitors_by_site_id(&state.pool, site.id),
            site_monitors::repository::list_heartbeat_monitors_by_site_id(&state.pool, site.id),
            site_monitors::repository::list_tcp_monitors_by_site_id(&state.pool, site.id),
            site_monitors::repository::list_dns_monitors_by_site_id(&state.pool, site.id),
        )
        .map_err(ApiError::internal_error)?;

    let all_monitors: Vec<&site_monitors::SiteMonitor> = http_monitors
        .iter()
        .chain(ssl_monitors.iter())
        .chain(heartbeat_monitors.iter())
        .chain(tcp_monitors.iter())
        .chain(dns_monitors.iter())
        .collect();

    let all_active: Vec<&site_monitors::SiteMonitor> = all_monitors
        .iter()
        .filter(|m| m.is_active)
        .copied()
        .collect();

    let all_active_ids: Vec<i64> = all_active.iter().map(|m| m.id).collect();
    let overall_status = derive_overall_status(&all_active);

    // Open + resolved incidents in parallel
    let (open_incidents_raw, resolved_incidents_raw) = tokio::try_join!(
        site_monitor_incidents::repository::list_by_site_id(
            &state.pool,
            site.id,
            &site_monitor_incidents::IncidentCursorQuery {
                cursor_opened_at: None,
                cursor_id: None,
                status: Some(SiteMonitorIncidentStatus::Open),
                limit: 10,
            },
        ),
        site_monitor_incidents::repository::list_by_site_id(
            &state.pool,
            site.id,
            &site_monitor_incidents::IncidentCursorQuery {
                cursor_opened_at: None,
                cursor_id: None,
                status: Some(SiteMonitorIncidentStatus::Resolved),
                limit: 10,
            },
        ),
    )
    .map_err(ApiError::internal_error)?;

    // Per-monitor uptime data — only fetched when the toggle is on and monitors exist
    let (uptime_7d, uptime_30d, monitor_uptime_map, monitor_history_map) =
        if status_page.show_uptime_percentages && !all_active_ids.is_empty() {
            let since_7d = Utc::now() - chrono::Duration::days(7);
            let since_30d = Utc::now() - chrono::Duration::days(30);
            let pool = &state.pool;
            let ids = &all_active_ids;

            let (s7, s30, per_uptime, per_history) = tokio::try_join!(
                site_monitor_checks::repository::get_site_uptime_stats(pool, site.id, since_7d),
                site_monitor_checks::repository::get_site_uptime_stats(pool, site.id, since_30d),
                try_join_all(ids.iter().copied().map(|id| {
                    site_monitor_checks::repository::get_monitor_uptime_stats(pool, id)
                })),
                try_join_all(ids.iter().copied().map(|id| {
                    site_monitor_checks::repository::get_monitor_daily_uptime_stats(pool, id, 90)
                })),
            )
            .map_err(ApiError::internal_error)?;

            let pct = |total: i64, success: i64| -> Option<f64> {
                if total > 0 {
                    Some(success as f64 / total as f64 * 100.0)
                } else {
                    None
                }
            };

            let u7d = pct(s7.total_checks, s7.successful_checks);
            let u30d = pct(s30.total_checks, s30.successful_checks);

            let uptime_map: HashMap<i64, site_monitor_checks::repository::MonitorUptimeStats> =
                ids.iter().copied().zip(per_uptime).collect();
            let history_map: HashMap<i64, Vec<site_monitor_checks::repository::DailyUptimeBucket>> =
                ids.iter().copied().zip(per_history).collect();

            (u7d, u30d, uptime_map, history_map)
        } else {
            (None, None, HashMap::new(), HashMap::new())
        };

    let pct = |total: i64, success: i64| -> Option<f64> {
        if total > 0 {
            Some(success as f64 / total as f64 * 100.0)
        } else {
            None
        }
    };

    let monitors: Vec<PublicMonitorStatus> = if status_page.show_monitor_details {
        all_monitors
            .iter()
            .map(|m| {
                let (uptime_7d_m, uptime_30d_m, history) =
                    if let Some(u) = monitor_uptime_map.get(&m.id) {
                        let hist = monitor_history_map
                            .get(&m.id)
                            .map(|h| {
                                h.iter()
                                    .map(|b| PublicUptimeDayBucket {
                                        date: b.date.clone(),
                                        total: b.total_checks,
                                        success: b.successful_checks,
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        (
                            pct(u.total_7d, u.success_7d),
                            pct(u.total_30d, u.success_30d),
                            hist,
                        )
                    } else {
                        (None, None, vec![])
                    };

                let (label, monitor_type) = monitor_label(m);
                PublicMonitorStatus {
                    label,
                    monitor_type,
                    status: monitor_status(m),
                    response_time_ms: m.last_response_time_ms,
                    last_checked_at: m.last_checked_at.map(|t| t.to_rfc3339()),
                    uptime_7d: uptime_7d_m,
                    uptime_30d: uptime_30d_m,
                    uptime_history: history,
                }
            })
            .collect()
    } else {
        vec![]
    };

    let open_incidents: Vec<PublicOpenIncident> = open_incidents_raw
        .iter()
        .map(|inc| {
            let (label, monitor_type) = incident_label(inc);
            PublicOpenIncident {
                opened_at: inc.opened_at.to_rfc3339(),
                monitor_label: label,
                monitor_type,
            }
        })
        .collect();

    let incident_history: Vec<PublicResolvedIncident> = resolved_incidents_raw
        .iter()
        .filter_map(|inc| {
            let resolved_at = inc.resolved_at?;
            let (label, monitor_type) = incident_label(inc);
            Some(PublicResolvedIncident {
                opened_at: inc.opened_at.to_rfc3339(),
                resolved_at: resolved_at.to_rfc3339(),
                monitor_label: label,
                monitor_type,
                downtime_seconds: inc.downtime_seconds,
                failure_reason: inc.opened_failure_reason.clone(),
            })
        })
        .collect();

    let page_title = status_page
        .page_title
        .clone()
        .unwrap_or_else(|| site.name.clone());

    let response_body = PublicStatusPageResponse {
        slug: status_page.slug.clone(),
        page_title,
        overall_status,
        show_monitor_details: status_page.show_monitor_details,
        show_uptime_percentages: status_page.show_uptime_percentages,
        monitors,
        uptime_7d,
        uptime_30d,
        open_incidents,
        incident_history,
        last_updated: Utc::now().to_rfc3339(),
    };

    let bytes =
        serde_json::to_vec(&response_body).map_err(|e| ApiError::internal_error(e.into()))?;
    state.status_page_cache.set(slug, bytes.clone());

    json_bytes_response(bytes)
}

fn monitor_label(m: &site_monitors::SiteMonitor) -> (String, &'static str) {
    match m.monitor_type {
        site_monitors::SiteMonitorType::Tcp => (
            format!(
                "TCP {}:{}",
                m.tcp_target_host.as_deref().unwrap_or("?"),
                m.tcp_target_port.unwrap_or(0)
            ),
            "tcp",
        ),
        site_monitors::SiteMonitorType::Dns => (
            format!(
                "DNS {} {}",
                m.dns_record_type.as_deref().unwrap_or("?"),
                m.dns_hostname.as_deref().unwrap_or("?")
            ),
            "dns",
        ),
        _ => (m.target_url.clone(), m.monitor_type.as_str()),
    }
}

fn monitor_status(m: &site_monitors::SiteMonitor) -> &'static str {
    if !m.is_active {
        return "paused";
    }
    match m.last_is_success {
        None => "pending",
        Some(false) => "down",
        Some(true) => {
            if let (Some(actual), Some(threshold)) =
                (m.last_response_time_ms, m.max_response_time_ms)
                && actual > threshold
            {
                return "degraded";
            }
            "up"
        }
    }
}

fn derive_overall_status(active_monitors: &[&site_monitors::SiteMonitor]) -> &'static str {
    if active_monitors.is_empty() {
        return "unknown";
    }
    if active_monitors
        .iter()
        .any(|m| m.last_is_success == Some(false))
    {
        return "outage";
    }
    if active_monitors
        .iter()
        .any(|m| monitor_status(m) == "degraded")
    {
        return "degraded";
    }
    if active_monitors
        .iter()
        .all(|m| m.last_is_success == Some(true))
    {
        return "operational";
    }
    "unknown"
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{derive_overall_status, monitor_label, monitor_status};
    use crate::domain::site_monitors::{SiteMonitor, SiteMonitorType};

    fn build_monitor() -> SiteMonitor {
        let now = Utc::now();
        SiteMonitor {
            id: 1,
            site_id: 1,
            monitor_type: SiteMonitorType::Http,
            target_url: "https://example.com/health".to_string(),
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
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn monitor_label_uses_target_url_for_http_monitor() {
        let m = build_monitor();
        let (label, kind) = monitor_label(&m);
        assert_eq!(label, "https://example.com/health");
        assert_eq!(kind, "http");
    }

    #[test]
    fn monitor_label_formats_tcp_with_host_and_port() {
        let m = SiteMonitor {
            monitor_type: SiteMonitorType::Tcp,
            tcp_target_host: Some("db.example.com".to_string()),
            tcp_target_port: Some(5432),
            ..build_monitor()
        };
        let (label, kind) = monitor_label(&m);
        assert_eq!(label, "TCP db.example.com:5432");
        assert_eq!(kind, "tcp");
    }

    #[test]
    fn monitor_label_formats_tcp_with_placeholders_when_fields_absent() {
        let m = SiteMonitor {
            monitor_type: SiteMonitorType::Tcp,
            ..build_monitor()
        };
        let (label, kind) = monitor_label(&m);
        assert_eq!(label, "TCP ?:0");
        assert_eq!(kind, "tcp");
    }

    #[test]
    fn monitor_label_formats_dns_with_record_type_and_hostname() {
        let m = SiteMonitor {
            monitor_type: SiteMonitorType::Dns,
            dns_record_type: Some("A".to_string()),
            dns_hostname: Some("example.com".to_string()),
            ..build_monitor()
        };
        let (label, kind) = monitor_label(&m);
        assert_eq!(label, "DNS A example.com");
        assert_eq!(kind, "dns");
    }

    #[test]
    fn monitor_status_is_paused_when_inactive() {
        let m = SiteMonitor {
            is_active: false,
            last_is_success: Some(true),
            ..build_monitor()
        };
        assert_eq!(monitor_status(&m), "paused");
    }

    #[test]
    fn monitor_status_is_pending_before_first_check() {
        let m = build_monitor(); // last_is_success = None
        assert_eq!(monitor_status(&m), "pending");
    }

    #[test]
    fn monitor_status_is_down_when_last_check_failed() {
        let m = SiteMonitor {
            last_is_success: Some(false),
            ..build_monitor()
        };
        assert_eq!(monitor_status(&m), "down");
    }

    #[test]
    fn monitor_status_is_up_when_last_check_succeeded() {
        let m = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        assert_eq!(monitor_status(&m), "up");
    }

    #[test]
    fn monitor_status_is_degraded_when_response_time_exceeds_limit() {
        let m = SiteMonitor {
            last_is_success: Some(true),
            last_response_time_ms: Some(500),
            max_response_time_ms: Some(200),
            ..build_monitor()
        };
        assert_eq!(monitor_status(&m), "degraded");
    }

    #[test]
    fn monitor_status_is_up_when_response_time_is_within_limit() {
        let m = SiteMonitor {
            last_is_success: Some(true),
            last_response_time_ms: Some(199),
            max_response_time_ms: Some(200),
            ..build_monitor()
        };
        assert_eq!(monitor_status(&m), "up");
    }

    #[test]
    fn derive_overall_status_is_unknown_for_empty_list() {
        assert_eq!(derive_overall_status(&[]), "unknown");
    }

    #[test]
    fn derive_overall_status_is_operational_when_all_monitors_up() {
        let m1 = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        let m2 = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        assert_eq!(derive_overall_status(&[&m1, &m2]), "operational");
    }

    #[test]
    fn derive_overall_status_is_outage_when_any_monitor_down() {
        let up = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        let down = SiteMonitor {
            last_is_success: Some(false),
            ..build_monitor()
        };
        assert_eq!(derive_overall_status(&[&up, &down]), "outage");
    }

    #[test]
    fn derive_overall_status_is_degraded_when_any_monitor_exceeds_response_time() {
        let up = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        let slow = SiteMonitor {
            last_is_success: Some(true),
            last_response_time_ms: Some(500),
            max_response_time_ms: Some(200),
            ..build_monitor()
        };
        assert_eq!(derive_overall_status(&[&up, &slow]), "degraded");
    }

    #[test]
    fn derive_overall_status_prefers_outage_over_degraded() {
        let down = SiteMonitor {
            last_is_success: Some(false),
            ..build_monitor()
        };
        let slow = SiteMonitor {
            last_is_success: Some(true),
            last_response_time_ms: Some(500),
            max_response_time_ms: Some(200),
            ..build_monitor()
        };
        assert_eq!(derive_overall_status(&[&down, &slow]), "outage");
    }

    #[test]
    fn derive_overall_status_is_unknown_when_some_monitors_not_yet_checked() {
        let up = SiteMonitor {
            last_is_success: Some(true),
            ..build_monitor()
        };
        let pending = build_monitor(); // last_is_success = None
        assert_eq!(derive_overall_status(&[&up, &pending]), "unknown");
    }
}

fn incident_label(inc: &site_monitor_incidents::SiteMonitorIncident) -> (String, &'static str) {
    let monitor_type = inc.monitor_type.as_str();
    let label = match inc.monitor_type {
        site_monitors::SiteMonitorType::Tcp => format!("TCP {}", inc.target_url),
        site_monitors::SiteMonitorType::Dns => format!("DNS {}", inc.target_url),
        _ => inc.target_url.clone(),
    };
    (label, monitor_type)
}
