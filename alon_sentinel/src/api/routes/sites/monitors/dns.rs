use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use super::super::{SiteResponse, http_monitor_status};
use crate::monitoring::dns_checker;
use crate::{
    api::{
        error::ApiError, extractors::AuthenticatedRequest, permissions::PermissionKey,
        state::AppState,
    },
    auth::AuthService,
    domain::{
        site_monitor_incidents::{self, SiteMonitorIncidentResolvedReason},
        site_monitors, sites,
    },
};

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DnsSiteMonitorResponse {
    id: i64,
    site_id: i64,
    monitor_type: &'static str,
    hostname: String,
    record_type: String,
    expected_value: Option<String>,
    nameserver: Option<String>,
    check_interval_seconds: i32,
    timeout_seconds_override: Option<i32>,
    max_attempts_override: Option<i32>,
    retry_delays_ms_override: Option<Vec<i64>>,
    is_active: bool,
    last_checked_at: Option<String>,
    last_successful_check_at: Option<String>,
    last_is_success: Option<bool>,
    last_response_time_ms: Option<i32>,
    last_failure_reason: Option<String>,
    last_error_message: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<site_monitors::SiteMonitor> for DnsSiteMonitorResponse {
    fn from(monitor: site_monitors::SiteMonitor) -> Self {
        Self {
            id: monitor.id,
            site_id: monitor.site_id,
            monitor_type: "dns",
            hostname: monitor.dns_hostname.unwrap_or_default(),
            record_type: monitor.dns_record_type.unwrap_or_default(),
            expected_value: monitor.dns_expected_value,
            nameserver: monitor.dns_nameserver,
            check_interval_seconds: monitor.check_interval_seconds,
            timeout_seconds_override: monitor.http_check_timeout_seconds_override,
            max_attempts_override: monitor.http_check_max_attempts_override,
            retry_delays_ms_override: monitor.http_check_retry_delays_ms_override,
            is_active: monitor.is_active,
            last_checked_at: monitor.last_checked_at.map(|v| v.to_rfc3339()),
            last_successful_check_at: monitor.last_successful_check_at.map(|v| v.to_rfc3339()),
            last_is_success: monitor.last_is_success,
            last_response_time_ms: monitor.last_response_time_ms,
            last_failure_reason: monitor.last_failure_reason,
            last_error_message: monitor.last_error_message,
            created_at: monitor.created_at.to_rfc3339(),
            updated_at: monitor.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct SiteDnsMonitoringResponse {
    site: SiteResponse,
    dns_monitors: Vec<DnsSiteMonitorResponse>,
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DisableDnsSiteMonitorResponse {
    disabled: bool,
}

#[derive(Deserialize)]
pub(in crate::api::routes::sites) struct UpsertDnsSiteMonitorRequest {
    hostname: String,
    record_type: String,
    expected_value: Option<String>,
    nameserver: Option<String>,
    check_interval_seconds: i32,
    timeout_seconds_override: Option<usize>,
    max_attempts_override: Option<usize>,
    retry_delays_ms_override: Option<Vec<u64>>,
    is_active: bool,
}

// --- DNS monitor handlers ---

pub(in crate::api::routes::sites) async fn get_dns_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<SiteDnsMonitoringResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SiteMonitorsRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(site) = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("site not found"));
    };

    let http_monitors =
        site_monitors::repository::list_http_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let dns_monitors =
        site_monitors::repository::list_dns_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteDnsMonitoringResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, "not_configured"),
        dns_monitors: dns_monitors
            .into_iter()
            .map(DnsSiteMonitorResponse::from)
            .collect(),
    }))
}

pub(in crate::api::routes::sites) async fn upsert_dns_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpsertDnsSiteMonitorRequest>,
) -> Result<Json<DnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsCreate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let retry_delays = validate_and_normalize_dns_monitor_payload(&payload)?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let record_type = payload.record_type.trim().to_uppercase();
    let monitor =
        site_monitors::repository::upsert_dns_monitor_by_site_id_and_hostname_record_type(
            &state.pool,
            site_id,
            &site_monitors::DnsMonitorParams {
                hostname: payload.hostname.trim(),
                record_type: &record_type,
                expected_value: payload.expected_value.as_deref(),
                nameserver: payload.nameserver.as_deref(),
                check_interval_seconds: payload.check_interval_seconds,
                timeout_seconds_override: payload.timeout_seconds_override.map(|v| v as i32),
                max_attempts_override: payload.max_attempts_override.map(|v| v as i32),
                retry_delays_ms_override: retry_delays.as_deref(),
                is_active: payload.is_active,
            },
        )
        .await
        .map_err(ApiError::internal_error)?;

    if !payload.is_active {
        site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
            &state.pool,
            monitor.id,
            SiteMonitorIncidentResolvedReason::MonitoringDisabled,
        )
        .await
        .map_err(ApiError::internal_error)?;
    }

    Ok(Json(monitor.into()))
}

pub(in crate::api::routes::sites) async fn update_dns_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertDnsSiteMonitorRequest>,
) -> Result<Json<DnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let retry_delays = validate_and_normalize_dns_monitor_payload(&payload)?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let record_type = payload.record_type.trim().to_uppercase();

    if let Some(existing) =
        site_monitors::repository::get_dns_monitor_by_site_id_and_hostname_record_type(
            &state.pool,
            site_id,
            payload.hostname.trim(),
            &record_type,
        )
        .await
        .map_err(ApiError::internal_error)?
        && existing.id != monitor_id
    {
        return Err(ApiError::bad_request(
            "hostname+record_type already exists for another dns monitor on this site",
        ));
    }

    let Some(monitor) = site_monitors::repository::update_dns_site_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
        &site_monitors::DnsMonitorParams {
            hostname: payload.hostname.trim(),
            record_type: &record_type,
            expected_value: payload.expected_value.as_deref(),
            nameserver: payload.nameserver.as_deref(),
            check_interval_seconds: payload.check_interval_seconds,
            timeout_seconds_override: payload.timeout_seconds_override.map(|v| v as i32),
            max_attempts_override: payload.max_attempts_override.map(|v| v as i32),
            retry_delays_ms_override: retry_delays.as_deref(),
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("dns monitor not found"));
    };

    if !payload.is_active {
        site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
            &state.pool,
            monitor_id,
            SiteMonitorIncidentResolvedReason::MonitoringDisabled,
        )
        .await
        .map_err(ApiError::internal_error)?;
    }

    Ok(Json(monitor.into()))
}

pub(in crate::api::routes::sites) async fn disable_dns_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DisableDnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let monitors = site_monitors::repository::disable_dns_monitors_by_site_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?;

    if monitors.is_empty() {
        return Err(ApiError::not_found("dns monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_site(
        &state.pool,
        site_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableDnsSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn disable_dns_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DisableDnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let disabled =
        site_monitors::repository::disable_dns_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
            .is_some();

    if !disabled {
        return Err(ApiError::not_found("dns monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableDnsSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn pause_dns_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::pause_dns_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("dns monitor not found"));
    };

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(monitor.into()))
}

pub(in crate::api::routes::sites) async fn resume_dns_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DnsSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::resume_dns_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("dns monitor not found"));
    };

    Ok(Json(monitor.into()))
}

fn validate_and_normalize_dns_monitor_payload(
    payload: &UpsertDnsSiteMonitorRequest,
) -> Result<Option<Vec<i64>>, ApiError> {
    if payload.hostname.trim().is_empty() {
        return Err(ApiError::bad_request("hostname is required"));
    }
    if !dns_checker::is_valid_record_type(payload.record_type.trim()) {
        return Err(ApiError::bad_request(
            "record_type must be one of: A, AAAA, CNAME, MX, TXT, NS",
        ));
    }
    if payload.check_interval_seconds < 30 {
        return Err(ApiError::bad_request(
            "check_interval_seconds must be at least 30",
        ));
    }
    if let Some(v) = payload.timeout_seconds_override
        && v == 0
    {
        return Err(ApiError::bad_request(
            "timeout_seconds_override must be greater than 0",
        ));
    }
    if let Some(v) = payload.max_attempts_override
        && v == 0
    {
        return Err(ApiError::bad_request(
            "max_attempts_override must be greater than 0",
        ));
    }
    if let Some(ns) = payload.nameserver.as_deref()
        && ns.trim().parse::<std::net::IpAddr>().is_err()
    {
        return Err(ApiError::bad_request(
            "nameserver must be an IP address (e.g. 8.8.8.8)",
        ));
    }
    let retry_delays = payload
        .retry_delays_ms_override
        .as_ref()
        .map(|values| values.iter().map(|v| *v as i64).collect::<Vec<_>>());

    Ok(retry_delays)
}
