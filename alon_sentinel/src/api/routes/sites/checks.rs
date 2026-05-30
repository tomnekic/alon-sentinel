use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::Response,
};
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        error::ApiError,
        extractors::AuthenticatedRequest,
        pagination::{json_with_next_cursor, paginate_vec_with_cursor},
        permissions::PermissionKey,
        state::AppState,
    },
    auth::AuthService,
    domain::{site_monitor_checks, site_monitor_incidents, site_monitors},
};

use super::monitors::{
    HeartbeatSiteMonitorResponse, HttpSiteMonitorResponse, SslSiteMonitorResponse,
};
use super::{
    SiteResponse, checks_limit, encode_cursor, ensure_site_exists, http_monitor_status,
    page_fetch_limit, parse_cursor,
};

#[derive(Debug, Deserialize)]
pub(super) struct HistoryQuery {
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CheckOutcomeFilter {
    Success,
    Failure,
}

#[derive(Debug, Deserialize)]
pub(super) struct SiteChecksQuery {
    limit: Option<usize>,
    cursor: Option<String>,
    outcome: Option<CheckOutcomeFilter>,
}

#[derive(Serialize)]
pub(super) struct SiteMonitorCheckResponse {
    id: i64,
    site_monitor_id: i64,
    checked_at: String,
    monitor_type: &'static str,
    url_checked: String,
    expected_status_code: Option<i32>,
    is_success: bool,
    status_code: Option<i32>,
    response_time_ms: Option<i32>,
    total_duration_ms: Option<i32>,
    attempt_count: i32,
    was_retried: bool,
    failure_reason: Option<String>,
    error_message: Option<String>,
    certificate_expires_at: Option<String>,
    certificate_days_remaining: Option<i32>,
    certificate_issuer: Option<String>,
    certificate_subject: Option<String>,
    certificate_domain: Option<String>,
}

impl From<site_monitor_checks::SiteMonitorCheck> for SiteMonitorCheckResponse {
    fn from(check: site_monitor_checks::SiteMonitorCheck) -> Self {
        Self {
            id: check.id,
            site_monitor_id: check.site_monitor_id,
            checked_at: check.checked_at.to_rfc3339(),
            monitor_type: check.monitor_type.as_str(),
            url_checked: check.url_checked,
            expected_status_code: check.expected_status_code,
            is_success: check.is_success,
            status_code: check.status_code,
            response_time_ms: check.response_time_ms,
            total_duration_ms: check.total_duration_ms,
            attempt_count: check.attempt_count,
            was_retried: check.was_retried,
            failure_reason: check.failure_reason,
            error_message: check.error_message,
            certificate_expires_at: check.certificate_expires_at.map(|value| value.to_rfc3339()),
            certificate_days_remaining: check.certificate_days_remaining,
            certificate_issuer: check.certificate_issuer,
            certificate_subject: check.certificate_subject,
            certificate_domain: check.certificate_domain,
        }
    }
}

impl From<&site_monitor_checks::SiteMonitorCheck> for SiteMonitorCheckResponse {
    fn from(check: &site_monitor_checks::SiteMonitorCheck) -> Self {
        Self {
            id: check.id,
            site_monitor_id: check.site_monitor_id,
            checked_at: check.checked_at.to_rfc3339(),
            monitor_type: check.monitor_type.as_str(),
            url_checked: check.url_checked.clone(),
            expected_status_code: check.expected_status_code,
            is_success: check.is_success,
            status_code: check.status_code,
            response_time_ms: check.response_time_ms,
            total_duration_ms: check.total_duration_ms,
            attempt_count: check.attempt_count,
            was_retried: check.was_retried,
            failure_reason: check.failure_reason.clone(),
            error_message: check.error_message.clone(),
            certificate_expires_at: check.certificate_expires_at.map(|value| value.to_rfc3339()),
            certificate_days_remaining: check.certificate_days_remaining,
            certificate_issuer: check.certificate_issuer.clone(),
            certificate_subject: check.certificate_subject.clone(),
            certificate_domain: check.certificate_domain.clone(),
        }
    }
}

#[derive(Serialize)]
pub(super) struct RecentCheckStatsResponse {
    window_size: usize,
    total_checks: usize,
    successful_checks: usize,
    failed_checks: usize,
    success_rate: Option<f64>,
}

#[derive(Serialize)]
pub(super) struct SiteSummaryResponse {
    site: SiteResponse,
    http_monitors: Vec<HttpSiteMonitorResponse>,
    ssl_monitors: Vec<SslSiteMonitorResponse>,
    heartbeat_monitors: Vec<HeartbeatSiteMonitorResponse>,
    current_state: &'static str,
    incident_open: bool,
    recent_checks: RecentCheckStatsResponse,
    latest_check: Option<SiteMonitorCheckResponse>,
    latest_failure: Option<SiteMonitorCheckResponse>,
}

pub(super) async fn get_site_summary(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<HistoryQuery>,
) -> Result<Json<SiteSummaryResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SiteChecksRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let site = ensure_site_exists(&state.pool, site_id).await?;
    let http_monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let ssl_monitors =
        site_monitors::repository::list_ssl_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let heartbeat_monitors =
        site_monitors::repository::list_heartbeat_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let window_size = summary_window(query.limit)?;
    let recent_checks = site_monitor_checks::repository::list_by_site_id(
        &state.pool,
        site_id,
        &site_monitor_checks::CheckCursorQuery {
            cursor_checked_at: None,
            cursor_id: None,
            is_success: None,
            limit: window_size as i64,
        },
    )
    .await
    .map_err(ApiError::internal_error)?;

    let current_state = summary_current_state(&http_monitors, &ssl_monitors, &heartbeat_monitors);
    let incident_open =
        site_monitor_incidents::repository::has_open_incident_for_site(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let recent_stats = build_recent_check_stats(&recent_checks, window_size);
    let latest_check = recent_checks.first().map(SiteMonitorCheckResponse::from);
    let latest_failure = recent_checks
        .iter()
        .find(|check| !check.is_success)
        .map(SiteMonitorCheckResponse::from);

    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteSummaryResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, current_state),
        http_monitors: http_monitors
            .into_iter()
            .map(HttpSiteMonitorResponse::from)
            .collect(),
        ssl_monitors: ssl_monitors
            .into_iter()
            .map(SslSiteMonitorResponse::from)
            .collect(),
        heartbeat_monitors: heartbeat_monitors
            .into_iter()
            .map(HeartbeatSiteMonitorResponse::from)
            .collect(),
        current_state,
        incident_open,
        recent_checks: recent_stats,
        latest_check,
        latest_failure,
    }))
}

pub(super) async fn list_site_checks(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<SiteChecksQuery>,
) -> Result<Response, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SiteChecksRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    ensure_site_exists(&state.pool, site_id).await?;
    let limit = checks_limit(query.limit)? as usize;
    let cursor = parse_cursor(query.cursor.as_deref())?;
    let checks = site_monitor_checks::repository::list_by_site_id(
        &state.pool,
        site_id,
        &site_monitor_checks::CheckCursorQuery {
            cursor_checked_at: cursor.as_ref().map(|cursor| cursor.timestamp),
            cursor_id: cursor.as_ref().map(|cursor| cursor.id),
            is_success: check_outcome_to_is_success(query.outcome),
            limit: page_fetch_limit(limit),
        },
    )
    .await
    .map_err(ApiError::internal_error)?;
    let (checks, next_cursor) = paginate_vec_with_cursor(checks, limit, |check| {
        encode_cursor(check.checked_at, check.id)
    });

    Ok(json_with_next_cursor(
        checks
            .into_iter()
            .map(SiteMonitorCheckResponse::from)
            .collect::<Vec<_>>(),
        next_cursor,
    ))
}

fn summary_window(limit: Option<usize>) -> Result<usize, ApiError> {
    const DEFAULT_WINDOW: usize = 20;
    const MAX_WINDOW: usize = 200;

    super::bounded_limit(limit, DEFAULT_WINDOW, MAX_WINDOW, "limit").map(|value| value as usize)
}

fn summary_current_state(
    http_monitors: &[site_monitors::SiteMonitor],
    ssl_monitors: &[site_monitors::SiteMonitor],
    heartbeat_monitors: &[site_monitors::SiteMonitor],
) -> &'static str {
    let all_monitors = http_monitors
        .iter()
        .chain(ssl_monitors.iter())
        .chain(heartbeat_monitors.iter())
        .collect::<Vec<_>>();
    let active_monitors = all_monitors
        .iter()
        .copied()
        .filter(|monitor| monitor.is_active)
        .collect::<Vec<_>>();

    if all_monitors.is_empty() {
        return "not_configured";
    }

    if active_monitors.is_empty() {
        return "disabled";
    }

    if active_monitors
        .iter()
        .any(|monitor| monitor.last_is_success == Some(false))
    {
        return "failing";
    }

    if active_monitors
        .iter()
        .any(|monitor| monitor.last_is_success == Some(true))
    {
        return "healthy";
    }

    "pending_first_check"
}

fn build_recent_check_stats(
    checks: &[site_monitor_checks::SiteMonitorCheck],
    window_size: usize,
) -> RecentCheckStatsResponse {
    let total_checks = checks.len();
    let successful_checks = checks.iter().filter(|check| check.is_success).count();
    let failed_checks = total_checks.saturating_sub(successful_checks);
    let success_rate = (total_checks > 0).then(|| {
        ((successful_checks as f64 / total_checks as f64) * 100.0 * 100.0).round() / 100.0
    });

    RecentCheckStatsResponse {
        window_size,
        total_checks,
        successful_checks,
        failed_checks,
        success_rate,
    }
}

fn check_outcome_to_is_success(outcome: Option<CheckOutcomeFilter>) -> Option<bool> {
    match outcome {
        Some(CheckOutcomeFilter::Success) => Some(true),
        Some(CheckOutcomeFilter::Failure) => Some(false),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::site_monitor_checks::SiteMonitorCheck;
    use crate::domain::site_monitors::{SiteMonitor, SiteMonitorType};
    use chrono::DateTime;

    #[test]
    fn build_recent_check_stats_computes_rate() {
        let checks = vec![
            build_check(1, "2026-04-24T10:00:00Z", true, Some(200), None),
            build_check(2, "2026-04-24T10:01:00Z", false, Some(500), Some("boom")),
            build_check(3, "2026-04-24T10:02:00Z", true, Some(200), None),
        ];

        let stats = build_recent_check_stats(&checks, 20);

        assert_eq!(stats.window_size, 20);
        assert_eq!(stats.total_checks, 3);
        assert_eq!(stats.successful_checks, 2);
        assert_eq!(stats.failed_checks, 1);
        assert_eq!(stats.success_rate, Some(66.67));
    }

    #[test]
    fn summary_current_state_aggregates_across_monitor_types() {
        let empty_monitors = Vec::new();
        assert_eq!(
            summary_current_state(&empty_monitors, &empty_monitors, &empty_monitors),
            "not_configured"
        );

        let disabled_monitors = vec![build_monitor(1, SiteMonitorType::Http, false, None)];
        assert_eq!(
            summary_current_state(&disabled_monitors, &empty_monitors, &empty_monitors),
            "disabled"
        );

        let pending_monitors = vec![
            build_monitor(1, SiteMonitorType::Http, true, None),
            build_monitor(2, SiteMonitorType::Ssl, true, None),
        ];
        assert_eq!(
            summary_current_state(&pending_monitors, &empty_monitors, &empty_monitors),
            "pending_first_check"
        );

        let healthy_monitors = vec![
            build_monitor(1, SiteMonitorType::Http, true, Some(true)),
            build_monitor(2, SiteMonitorType::Ssl, true, None),
        ];
        assert_eq!(
            summary_current_state(&healthy_monitors, &empty_monitors, &empty_monitors),
            "healthy"
        );

        let failing_monitors = vec![
            build_monitor(1, SiteMonitorType::Http, true, Some(true)),
            build_monitor(2, SiteMonitorType::Ssl, true, Some(false)),
        ];
        assert_eq!(
            summary_current_state(&failing_monitors, &empty_monitors, &empty_monitors),
            "failing"
        );
    }

    fn build_check(
        id: i64,
        checked_at: &str,
        is_success: bool,
        status_code: Option<i32>,
        error_message: Option<&str>,
    ) -> SiteMonitorCheck {
        SiteMonitorCheck {
            id,
            site_monitor_id: 10,
            checked_at: DateTime::parse_from_rfc3339(checked_at)
                .expect("valid timestamp")
                .with_timezone(&chrono::Utc),
            monitor_type: SiteMonitorType::Http,
            url_checked: "https://test.com/health".to_string(),
            expected_status_code: Some(200),
            is_success,
            status_code,
            response_time_ms: Some(123),
            total_duration_ms: Some(123),
            attempt_count: 1,
            was_retried: false,
            failure_reason: (!is_success).then(|| "status_code_mismatch".to_string()),
            error_message: error_message.map(ToOwned::to_owned),
            certificate_expires_at: None,
            certificate_days_remaining: None,
            certificate_issuer: None,
            certificate_subject: None,
            certificate_domain: None,
        }
    }

    fn build_monitor(
        id: i64,
        monitor_type: SiteMonitorType,
        is_active: bool,
        last_is_success: Option<bool>,
    ) -> SiteMonitor {
        let timestamp = DateTime::parse_from_rfc3339("2026-04-24T10:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&chrono::Utc);

        SiteMonitor {
            id,
            site_id: 10,
            monitor_type,
            target_url: format!("https://test.com/{id}"),
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
            is_active,
            check_claimed_at: None,
            check_lease_until: None,
            check_claimed_by: None,
            last_checked_at: last_is_success.map(|_| timestamp),
            last_successful_check_at: if last_is_success == Some(true) {
                Some(timestamp)
            } else {
                None
            },
            last_is_success,
            last_status_code: match last_is_success {
                Some(true) => Some(200),
                Some(false) => Some(500),
                None => None,
            },
            last_response_time_ms: last_is_success.map(|_| 123),
            last_failure_reason: (last_is_success == Some(false)).then(|| "timeout".to_string()),
            last_error_message: (last_is_success == Some(false)).then(|| "failed".to_string()),
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
