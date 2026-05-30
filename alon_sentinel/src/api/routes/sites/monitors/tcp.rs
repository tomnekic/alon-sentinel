use std::{sync::Arc, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
};
use serde::{Deserialize, Serialize};

use super::super::{SiteResponse, http_monitor_status};
use crate::net;
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
pub(in crate::api::routes::sites) struct TcpSiteMonitorResponse {
    id: i64,
    site_id: i64,
    monitor_type: &'static str,
    target_host: String,
    target_port: i32,
    check_interval_seconds: i32,
    max_connect_time_ms: Option<i32>,
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

impl From<site_monitors::SiteMonitor> for TcpSiteMonitorResponse {
    fn from(monitor: site_monitors::SiteMonitor) -> Self {
        Self {
            id: monitor.id,
            site_id: monitor.site_id,
            monitor_type: "tcp",
            target_host: monitor.tcp_target_host.unwrap_or_default(),
            target_port: monitor.tcp_target_port.unwrap_or(0),
            check_interval_seconds: monitor.check_interval_seconds,
            max_connect_time_ms: monitor.max_response_time_ms,
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
pub(in crate::api::routes::sites) struct SiteTcpMonitoringResponse {
    site: SiteResponse,
    tcp_monitors: Vec<TcpSiteMonitorResponse>,
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DisableTcpSiteMonitorResponse {
    disabled: bool,
}

#[derive(Deserialize)]
pub(in crate::api::routes::sites) struct UpsertTcpSiteMonitorRequest {
    target_host: String,
    target_port: u16,
    check_interval_seconds: i32,
    max_connect_time_ms: Option<usize>,
    timeout_seconds_override: Option<usize>,
    max_attempts_override: Option<usize>,
    retry_delays_ms_override: Option<Vec<u64>>,
    is_active: bool,
}

// --- TCP monitor handlers ---

pub(in crate::api::routes::sites) async fn get_tcp_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<SiteTcpMonitoringResponse>, ApiError> {
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
    let tcp_monitors =
        site_monitors::repository::list_tcp_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteTcpMonitoringResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, "not_configured"),
        tcp_monitors: tcp_monitors
            .into_iter()
            .map(TcpSiteMonitorResponse::from)
            .collect(),
    }))
}

pub(in crate::api::routes::sites) async fn upsert_tcp_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpsertTcpSiteMonitorRequest>,
) -> Result<Json<TcpSiteMonitorResponse>, ApiError> {
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

    let retry_delays = validate_and_normalize_tcp_monitor_payload(&payload)?;
    validate_tcp_target(payload.target_host.trim(), payload.target_port).await?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let monitor = site_monitors::repository::upsert_tcp_monitor_by_site_id_and_host_port(
        &state.pool,
        site_id,
        &site_monitors::TcpMonitorParams {
            target_host: payload.target_host.trim(),
            target_port: payload.target_port as i32,
            check_interval_seconds: payload.check_interval_seconds,
            max_connect_time_ms: payload.max_connect_time_ms.map(|v| v as i32),
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

pub(in crate::api::routes::sites) async fn update_tcp_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertTcpSiteMonitorRequest>,
) -> Result<Json<TcpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let retry_delays = validate_and_normalize_tcp_monitor_payload(&payload)?;
    validate_tcp_target(payload.target_host.trim(), payload.target_port).await?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    if let Some(existing) = site_monitors::repository::get_tcp_monitor_by_site_id_and_host_port(
        &state.pool,
        site_id,
        payload.target_host.trim(),
        payload.target_port as i32,
    )
    .await
    .map_err(ApiError::internal_error)?
        && existing.id != monitor_id
    {
        return Err(ApiError::bad_request(
            "target_host:target_port already exists for another tcp monitor on this site",
        ));
    }

    let Some(monitor) = site_monitors::repository::update_tcp_site_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
        &site_monitors::TcpMonitorParams {
            target_host: payload.target_host.trim(),
            target_port: payload.target_port as i32,
            check_interval_seconds: payload.check_interval_seconds,
            max_connect_time_ms: payload.max_connect_time_ms.map(|v| v as i32),
            timeout_seconds_override: payload.timeout_seconds_override.map(|v| v as i32),
            max_attempts_override: payload.max_attempts_override.map(|v| v as i32),
            retry_delays_ms_override: retry_delays.as_deref(),
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("tcp monitor not found"));
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

pub(in crate::api::routes::sites) async fn disable_tcp_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DisableTcpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let monitors = site_monitors::repository::disable_tcp_monitors_by_site_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?;

    if monitors.is_empty() {
        return Err(ApiError::not_found("tcp monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_site(
        &state.pool,
        site_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableTcpSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn disable_tcp_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DisableTcpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let disabled =
        site_monitors::repository::disable_tcp_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
            .is_some();

    if !disabled {
        return Err(ApiError::not_found("tcp monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableTcpSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn pause_tcp_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<TcpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::pause_tcp_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("tcp monitor not found"));
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

pub(in crate::api::routes::sites) async fn resume_tcp_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<TcpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::resume_tcp_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("tcp monitor not found"));
    };

    Ok(Json(monitor.into()))
}

async fn validate_tcp_target(host: &str, port: u16) -> Result<(), ApiError> {
    let host = host.trim();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !net::check_ip_is_public(ip) {
            return Err(ApiError::bad_request(
                "target_host must be a public IP address",
            ));
        }
        return Ok(());
    }

    let resolved = tokio::time::timeout(
        Duration::from_secs(5),
        tokio::net::lookup_host(format!("{host}:{port}")),
    )
    .await
    .map_err(|_| ApiError::bad_request("target_host could not be resolved"))?
    .map_err(|_| ApiError::bad_request("target_host could not be resolved"))?;

    for addr in resolved {
        if !net::check_ip_is_public(addr.ip()) {
            return Err(ApiError::bad_request(
                "target_host resolves to a non-public IP address",
            ));
        }
    }

    Ok(())
}

fn validate_and_normalize_tcp_monitor_payload(
    payload: &UpsertTcpSiteMonitorRequest,
) -> Result<Option<Vec<i64>>, ApiError> {
    let host = payload.target_host.trim();
    if host.is_empty() {
        return Err(ApiError::bad_request("target_host is required"));
    }
    if payload.target_port == 0 {
        return Err(ApiError::bad_request(
            "target_port must be between 1 and 65535",
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
    let retry_delays = payload
        .retry_delays_ms_override
        .as_ref()
        .map(|values| values.iter().map(|v| *v as i64).collect::<Vec<_>>());

    Ok(retry_delays)
}
