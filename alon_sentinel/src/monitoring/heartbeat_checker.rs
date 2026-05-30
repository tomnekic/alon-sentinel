use chrono::{DateTime, Utc};

use crate::{domain::site_monitors::SiteMonitor, monitoring::http_checker::CheckResult};

pub fn check_heartbeat(site_monitor: &SiteMonitor, now: DateTime<Utc>) -> CheckResult {
    let grace_seconds = site_monitor.heartbeat_grace_seconds.unwrap_or(0).max(0);
    let allowed_age_seconds = i64::from(site_monitor.check_interval_seconds.max(0))
        .saturating_add(i64::from(grace_seconds));
    let reference_time = site_monitor
        .last_heartbeat_received_at
        .unwrap_or(site_monitor.created_at);
    let age_seconds = now
        .signed_duration_since(reference_time)
        .num_seconds()
        .max(0);

    if age_seconds <= allowed_age_seconds {
        return CheckResult {
            is_success: true,
            status_code: None,
            response_time_ms: None,
            failure_reason: None,
            error_message: None,
            certificate_metadata: None,
        };
    }

    let overdue_by_seconds = age_seconds.saturating_sub(allowed_age_seconds);
    let last_received_at = site_monitor
        .last_heartbeat_received_at
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "never".to_string());

    CheckResult {
        is_success: false,
        status_code: None,
        response_time_ms: None,
        failure_reason: Some("heartbeat_overdue".to_string()),
        error_message: Some(format!(
            "last heartbeat received at {last_received_at}; overdue by {overdue_by_seconds}s"
        )),
        certificate_metadata: None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};

    use super::check_heartbeat;
    use crate::domain::site_monitors::{SiteMonitor, SiteMonitorType};

    fn build_monitor() -> SiteMonitor {
        let now = Utc::now();
        SiteMonitor {
            id: 1,
            site_id: 1,
            monitor_type: SiteMonitorType::Heartbeat,
            target_url: "/v1/heartbeat/example".to_string(),
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
            heartbeat_token: Some("example".to_string()),
            heartbeat_grace_seconds: Some(15),
            http_check_timeout_seconds_override: None,
            http_check_max_attempts_override: None,
            http_check_retry_delays_ms_override: None,
            is_active: true,
            check_claimed_at: None,
            check_lease_until: None,
            check_claimed_by: None,
            last_checked_at: Some(now),
            last_successful_check_at: None,
            last_is_success: None,
            last_status_code: None,
            last_response_time_ms: None,
            last_failure_reason: None,
            last_error_message: None,
            last_heartbeat_received_at: Some(now),
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
    fn heartbeat_check_succeeds_with_recent_ping() {
        let monitor = build_monitor();
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(30));
        assert!(result.is_success);
    }

    #[test]
    fn heartbeat_check_fails_when_overdue() {
        let monitor = build_monitor();
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(90));
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("heartbeat_overdue"));
    }

    #[test]
    fn heartbeat_check_succeeds_at_exact_deadline() {
        let monitor = build_monitor(); // interval=60, grace=15 → allowed=75
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(75));
        assert!(result.is_success);
    }

    #[test]
    fn heartbeat_check_fails_one_second_past_deadline() {
        let monitor = build_monitor(); // allowed=75, age=76 → overdue by 1
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(76));
        assert!(!result.is_success);
        assert_eq!(result.failure_reason.as_deref(), Some("heartbeat_overdue"));
    }

    #[test]
    fn heartbeat_check_falls_back_to_created_at_when_no_heartbeat_received() {
        let mut monitor = build_monitor();
        monitor.last_heartbeat_received_at = None;
        // reference = created_at, age=30 < allowed=75 → success
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(30));
        assert!(result.is_success);
    }

    #[test]
    fn heartbeat_check_error_message_reports_never_when_no_heartbeat_received() {
        let mut monitor = build_monitor();
        monitor.last_heartbeat_received_at = None;
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(100));
        assert!(!result.is_success);
        let msg = result.error_message.unwrap();
        assert!(
            msg.contains("never"),
            "expected 'never' in message, got: {msg}"
        );
    }

    #[test]
    fn heartbeat_check_error_message_includes_last_heartbeat_timestamp() {
        let monitor = build_monitor(); // last_heartbeat_received_at = Some(now)
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(100));
        assert!(!result.is_success);
        let msg = result.error_message.unwrap();
        assert!(
            !msg.contains("never"),
            "should use actual timestamp, not 'never'"
        );
        assert!(msg.contains('T'), "should embed RFC3339 timestamp");
    }

    #[test]
    fn heartbeat_check_error_message_reports_overdue_duration_accurately() {
        let monitor = build_monitor(); // allowed=75, age=100 → overdue by 25
        let result = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(100));
        assert!(!result.is_success);
        let msg = result.error_message.unwrap();
        assert!(msg.contains("25s"), "expected '25s' in message, got: {msg}");
    }

    #[test]
    fn heartbeat_check_treats_absent_grace_period_as_zero() {
        let mut monitor = build_monitor();
        monitor.heartbeat_grace_seconds = None; // grace=0, allowed=60

        let at_deadline = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(60));
        assert!(at_deadline.is_success);

        let one_past = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(61));
        assert!(!one_past.is_success);
    }

    #[test]
    fn heartbeat_check_clamps_negative_interval_to_zero() {
        let mut monitor = build_monitor();
        monitor.check_interval_seconds = -60; // clamped to 0, allowed = 0+15 = 15

        let at_deadline = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(15));
        assert!(at_deadline.is_success);

        let one_past = check_heartbeat(&monitor, monitor.created_at + Duration::seconds(16));
        assert!(!one_past.is_success);
    }
}
