use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

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

use super::super::{SiteResponse, http_monitor_status};
use super::generate_heartbeat_token;

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct HeartbeatSiteMonitorResponse {
    id: i64,
    site_id: i64,
    monitor_type: &'static str,
    ping_path: String,
    check_interval_seconds: i32,
    heartbeat_grace_seconds: Option<i32>,
    is_active: bool,
    last_heartbeat_received_at: Option<String>,
    last_checked_at: Option<String>,
    last_successful_check_at: Option<String>,
    last_is_success: Option<bool>,
    last_failure_reason: Option<String>,
    last_error_message: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<site_monitors::SiteMonitor> for HeartbeatSiteMonitorResponse {
    fn from(monitor: site_monitors::SiteMonitor) -> Self {
        Self {
            id: monitor.id,
            site_id: monitor.site_id,
            monitor_type: "heartbeat",
            ping_path: monitor.target_url,
            check_interval_seconds: monitor.check_interval_seconds,
            heartbeat_grace_seconds: monitor.heartbeat_grace_seconds,
            is_active: monitor.is_active,
            last_heartbeat_received_at: monitor
                .last_heartbeat_received_at
                .map(|value| value.to_rfc3339()),
            last_checked_at: monitor.last_checked_at.map(|value| value.to_rfc3339()),
            last_successful_check_at: monitor
                .last_successful_check_at
                .map(|value| value.to_rfc3339()),
            last_is_success: monitor.last_is_success,
            last_failure_reason: monitor.last_failure_reason,
            last_error_message: monitor.last_error_message,
            created_at: monitor.created_at.to_rfc3339(),
            updated_at: monitor.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct SiteHeartbeatMonitoringResponse {
    site: SiteResponse,
    heartbeat_monitors: Vec<HeartbeatSiteMonitorResponse>,
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DisableHeartbeatSiteMonitorResponse {
    disabled: bool,
}

#[derive(Deserialize)]
pub(in crate::api::routes::sites) struct UpsertHeartbeatSiteMonitorRequest {
    check_interval_seconds: i32,
    heartbeat_grace_seconds: Option<usize>,
    is_active: bool,
}

// --- Heartbeat monitor handlers ---

pub(in crate::api::routes::sites) async fn get_heartbeat_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<SiteHeartbeatMonitoringResponse>, ApiError> {
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
    let heartbeat_monitors =
        site_monitors::repository::list_heartbeat_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteHeartbeatMonitoringResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, "not_configured"),
        heartbeat_monitors: heartbeat_monitors
            .into_iter()
            .map(HeartbeatSiteMonitorResponse::from)
            .collect(),
    }))
}

pub(in crate::api::routes::sites) async fn upsert_heartbeat_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpsertHeartbeatSiteMonitorRequest>,
) -> Result<Json<HeartbeatSiteMonitorResponse>, ApiError> {
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

    let heartbeat_grace_seconds = validate_and_normalize_heartbeat_monitor_payload(&payload)?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let monitor = if let Some(existing_monitor) =
        site_monitors::repository::get_heartbeat_monitor_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?
    {
        site_monitors::repository::update_heartbeat_site_monitor_by_id(
            &state.pool,
            site_id,
            existing_monitor.id,
            &site_monitors::HeartbeatMonitorUpdateParams {
                check_interval_seconds: payload.check_interval_seconds,
                heartbeat_grace_seconds,
                is_active: payload.is_active,
            },
        )
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("heartbeat monitor not found"))?
    } else {
        let heartbeat_token = generate_heartbeat_token();
        let ping_path = heartbeat_ping_path(&heartbeat_token);
        site_monitors::repository::create_heartbeat_site_monitor(
            &state.pool,
            site_id,
            &site_monitors::HeartbeatMonitorParams {
                target_url: &ping_path,
                heartbeat_token: &heartbeat_token,
                check_interval_seconds: payload.check_interval_seconds,
                heartbeat_grace_seconds,
                is_active: payload.is_active,
            },
        )
        .await
        .map_err(ApiError::internal_error)?
    };

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

pub(in crate::api::routes::sites) async fn update_heartbeat_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertHeartbeatSiteMonitorRequest>,
) -> Result<Json<HeartbeatSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let heartbeat_grace_seconds = validate_and_normalize_heartbeat_monitor_payload(&payload)?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let Some(monitor) = site_monitors::repository::update_heartbeat_site_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
        &site_monitors::HeartbeatMonitorUpdateParams {
            check_interval_seconds: payload.check_interval_seconds,
            heartbeat_grace_seconds,
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("heartbeat monitor not found"));
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

pub(in crate::api::routes::sites) async fn disable_heartbeat_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DisableHeartbeatSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let monitors =
        site_monitors::repository::disable_heartbeat_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;

    if monitors.is_empty() {
        return Err(ApiError::not_found("heartbeat monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_site(
        &state.pool,
        site_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableHeartbeatSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn disable_heartbeat_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DisableHeartbeatSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let disabled = site_monitors::repository::disable_heartbeat_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
    )
    .await
    .map_err(ApiError::internal_error)?
    .is_some();

    if !disabled {
        return Err(ApiError::not_found("heartbeat monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableHeartbeatSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn pause_heartbeat_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<HeartbeatSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::pause_heartbeat_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("heartbeat monitor not found"));
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

pub(in crate::api::routes::sites) async fn resume_heartbeat_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<HeartbeatSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::resume_heartbeat_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("heartbeat monitor not found"));
    };

    Ok(Json(monitor.into()))
}

pub(in crate::api::routes::sites) async fn record_heartbeat_ping(
    State(state): State<Arc<AppState>>,
    Path(heartbeat_token): Path<String>,
) -> Result<StatusCode, ApiError> {
    if heartbeat_token.trim().is_empty() {
        return Err(ApiError::not_found("heartbeat monitor not found"));
    }

    let updated = site_monitors::repository::record_heartbeat_ping(&state.pool, &heartbeat_token)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !updated {
        return Err(ApiError::not_found("heartbeat monitor not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

fn validate_and_normalize_heartbeat_monitor_payload(
    payload: &UpsertHeartbeatSiteMonitorRequest,
) -> Result<Option<i32>, ApiError> {
    if payload.check_interval_seconds < 30 {
        return Err(ApiError::bad_request(
            "check_interval_seconds must be at least 30",
        ));
    }

    let heartbeat_grace_seconds = payload
        .heartbeat_grace_seconds
        .map(|value| {
            i32::try_from(value)
                .map_err(|_| ApiError::bad_request("heartbeat_grace_seconds is too large"))
        })
        .transpose()?;

    Ok(heartbeat_grace_seconds)
}

fn heartbeat_ping_path(heartbeat_token: &str) -> String {
    format!("/v1/heartbeat/{heartbeat_token}")
}
