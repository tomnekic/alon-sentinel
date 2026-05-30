use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
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
use super::validate_monitor_target_url;

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct SslSiteMonitorResponse {
    id: i64,
    site_id: i64,
    monitor_type: &'static str,
    target_url: String,
    check_interval_seconds: i32,
    ssl_expiry_warning_days: Option<i32>,
    http_check_timeout_seconds_override: Option<i32>,
    http_check_max_attempts_override: Option<i32>,
    http_check_retry_delays_ms_override: Option<Vec<i64>>,
    is_active: bool,
    last_checked_at: Option<String>,
    last_successful_check_at: Option<String>,
    last_is_success: Option<bool>,
    last_failure_reason: Option<String>,
    last_error_message: Option<String>,
    last_certificate_expires_at: Option<String>,
    last_certificate_days_remaining: Option<i32>,
    last_certificate_issuer: Option<String>,
    last_certificate_subject: Option<String>,
    last_certificate_domain: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<site_monitors::SiteMonitor> for SslSiteMonitorResponse {
    fn from(monitor: site_monitors::SiteMonitor) -> Self {
        Self {
            id: monitor.id,
            site_id: monitor.site_id,
            monitor_type: "ssl",
            target_url: monitor.target_url,
            check_interval_seconds: monitor.check_interval_seconds,
            ssl_expiry_warning_days: monitor.ssl_expiry_warning_days,
            http_check_timeout_seconds_override: monitor.http_check_timeout_seconds_override,
            http_check_max_attempts_override: monitor.http_check_max_attempts_override,
            http_check_retry_delays_ms_override: monitor.http_check_retry_delays_ms_override,
            is_active: monitor.is_active,
            last_checked_at: monitor.last_checked_at.map(|value| value.to_rfc3339()),
            last_successful_check_at: monitor
                .last_successful_check_at
                .map(|value| value.to_rfc3339()),
            last_is_success: monitor.last_is_success,
            last_failure_reason: monitor.last_failure_reason,
            last_error_message: monitor.last_error_message,
            last_certificate_expires_at: monitor
                .last_certificate_expires_at
                .map(|value| value.to_rfc3339()),
            last_certificate_days_remaining: monitor.last_certificate_days_remaining,
            last_certificate_issuer: monitor.last_certificate_issuer,
            last_certificate_subject: monitor.last_certificate_subject,
            last_certificate_domain: monitor.last_certificate_domain,
            created_at: monitor.created_at.to_rfc3339(),
            updated_at: monitor.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct SiteSslMonitoringResponse {
    site: SiteResponse,
    ssl_monitors: Vec<SslSiteMonitorResponse>,
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DisableSslSiteMonitorResponse {
    disabled: bool,
}

#[derive(Deserialize)]
pub(in crate::api::routes::sites) struct UpsertSslSiteMonitorRequest {
    target_url: String,
    check_interval_seconds: i32,
    ssl_expiry_warning_days: Option<usize>,
    http_check_timeout_seconds_override: Option<usize>,
    http_check_max_attempts_override: Option<usize>,
    http_check_retry_delays_ms_override: Option<Vec<u64>>,
    is_active: bool,
}

struct NormalizedSslMonitorPayload {
    target_url: String,
    ssl_expiry_warning_days: Option<i32>,
    retry_delays_override: Option<Vec<i64>>,
}

// --- SSL monitor handlers ---

pub(in crate::api::routes::sites) async fn get_ssl_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<SiteSslMonitoringResponse>, ApiError> {
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
    let ssl_monitors =
        site_monitors::repository::list_ssl_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;
    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteSslMonitoringResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, "not_configured"),
        ssl_monitors: ssl_monitors
            .into_iter()
            .map(SslSiteMonitorResponse::from)
            .collect(),
    }))
}

pub(in crate::api::routes::sites) async fn upsert_ssl_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpsertSslSiteMonitorRequest>,
) -> Result<Json<SslSiteMonitorResponse>, ApiError> {
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

    let n = validate_and_normalize_ssl_monitor_payload(
        &payload,
        state.http_monitor_allow_private_targets,
    )
    .await?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let monitor = site_monitors::repository::upsert_ssl_monitor_by_site_id_and_target_url(
        &state.pool,
        site_id,
        &site_monitors::SslMonitorParams {
            target_url: &n.target_url,
            check_interval_seconds: payload.check_interval_seconds,
            ssl_expiry_warning_days: n.ssl_expiry_warning_days,
            http_check_timeout_seconds_override: payload
                .http_check_timeout_seconds_override
                .map(|v| v as i32),
            http_check_max_attempts_override: payload
                .http_check_max_attempts_override
                .map(|v| v as i32),
            http_check_retry_delays_ms_override: n.retry_delays_override.as_deref(),
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

pub(in crate::api::routes::sites) async fn update_ssl_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertSslSiteMonitorRequest>,
) -> Result<Json<SslSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let n = validate_and_normalize_ssl_monitor_payload(
        &payload,
        state.http_monitor_allow_private_targets,
    )
    .await?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    if let Some(existing_monitor) =
        site_monitors::repository::get_ssl_monitor_by_site_id_and_target_url(
            &state.pool,
            site_id,
            &n.target_url,
        )
        .await
        .map_err(ApiError::internal_error)?
        && existing_monitor.id != monitor_id
    {
        return Err(ApiError::bad_request(
            "target_url already exists for another ssl monitor on this site",
        ));
    }

    let Some(monitor) = site_monitors::repository::update_ssl_site_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
        &site_monitors::SslMonitorParams {
            target_url: &n.target_url,
            check_interval_seconds: payload.check_interval_seconds,
            ssl_expiry_warning_days: n.ssl_expiry_warning_days,
            http_check_timeout_seconds_override: payload
                .http_check_timeout_seconds_override
                .map(|v| v as i32),
            http_check_max_attempts_override: payload
                .http_check_max_attempts_override
                .map(|v| v as i32),
            http_check_retry_delays_ms_override: n.retry_delays_override.as_deref(),
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("ssl monitor not found"));
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

pub(in crate::api::routes::sites) async fn disable_ssl_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DisableSslSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let monitors = site_monitors::repository::disable_ssl_monitors_by_site_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?;

    if monitors.is_empty() {
        return Err(ApiError::not_found("ssl monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_site(
        &state.pool,
        site_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableSslSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn disable_ssl_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DisableSslSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let disabled =
        site_monitors::repository::disable_ssl_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
            .is_some();

    if !disabled {
        return Err(ApiError::not_found("ssl monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableSslSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn pause_ssl_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<SslSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::pause_ssl_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("ssl monitor not found"));
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

pub(in crate::api::routes::sites) async fn resume_ssl_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<SslSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::resume_ssl_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("ssl monitor not found"));
    };

    Ok(Json(monitor.into()))
}

async fn validate_and_normalize_ssl_monitor_payload(
    payload: &UpsertSslSiteMonitorRequest,
    allow_private: bool,
) -> Result<NormalizedSslMonitorPayload, ApiError> {
    if payload.target_url.trim().is_empty() {
        return Err(ApiError::bad_request("target_url is required"));
    }

    validate_monitor_target_url(payload.target_url.trim(), allow_private).await?;
    validate_ssl_certificate_check_target_url(payload.target_url.trim(), true)?;

    if payload.check_interval_seconds < 30 {
        return Err(ApiError::bad_request(
            "check_interval_seconds must be at least 30",
        ));
    }

    validate_ssl_monitor_policy_overrides(payload)?;

    let ssl_expiry_warning_days =
        normalize_ssl_expiry_warning_days(true, payload.ssl_expiry_warning_days)?;
    let retry_delays_override = payload
        .http_check_retry_delays_ms_override
        .as_ref()
        .map(|values| values.iter().map(|value| *value as i64).collect::<Vec<_>>());

    Ok(NormalizedSslMonitorPayload {
        target_url: payload.target_url.trim().to_string(),
        ssl_expiry_warning_days,
        retry_delays_override,
    })
}

fn validate_ssl_monitor_policy_overrides(
    payload: &UpsertSslSiteMonitorRequest,
) -> Result<(), ApiError> {
    if payload.http_check_timeout_seconds_override == Some(0) {
        return Err(ApiError::bad_request(
            "http_check_timeout_seconds_override must be greater than 0",
        ));
    }

    if payload.http_check_max_attempts_override == Some(0) {
        return Err(ApiError::bad_request(
            "http_check_max_attempts_override must be greater than 0",
        ));
    }

    if let Some(delays) = &payload.http_check_retry_delays_ms_override {
        if delays.is_empty() {
            return Err(ApiError::bad_request(
                "http_check_retry_delays_ms_override must not be empty",
            ));
        }

        if delays.contains(&0) {
            return Err(ApiError::bad_request(
                "http_check_retry_delays_ms_override values must be greater than 0",
            ));
        }

        if delays.iter().any(|value| *value > i64::MAX as u64) {
            return Err(ApiError::bad_request(
                "http_check_retry_delays_ms_override values are too large",
            ));
        }
    }

    if let (Some(max_attempts), Some(delays)) = (
        payload.http_check_max_attempts_override,
        payload.http_check_retry_delays_ms_override.as_ref(),
    ) {
        let required_delay_count = max_attempts.saturating_sub(1);

        if delays.len() < required_delay_count {
            return Err(ApiError::bad_request(
                "http_check_retry_delays_ms_override must provide at least max_attempts - 1 values",
            ));
        }
    }

    Ok(())
}

fn normalize_ssl_expiry_warning_days(
    ssl_certificate_checks_enabled: bool,
    ssl_expiry_warning_days: Option<usize>,
) -> Result<Option<i32>, ApiError> {
    const DEFAULT_SSL_EXPIRY_WARNING_DAYS: usize = 14;
    const SSL_EXPIRY_CRITICAL_DAYS: usize = 7;

    if !ssl_certificate_checks_enabled {
        if ssl_expiry_warning_days.is_some() {
            return Err(ApiError::bad_request(
                "ssl_expiry_warning_days requires ssl_certificate_checks_enabled to be true",
            ));
        }

        return Ok(None);
    }

    let warning_days = ssl_expiry_warning_days.unwrap_or(DEFAULT_SSL_EXPIRY_WARNING_DAYS);
    if warning_days <= SSL_EXPIRY_CRITICAL_DAYS {
        return Err(ApiError::bad_request(
            "ssl_expiry_warning_days must be greater than 7",
        ));
    }

    if warning_days > i32::MAX as usize {
        return Err(ApiError::bad_request(
            "ssl_expiry_warning_days is too large",
        ));
    }

    Ok(Some(warning_days as i32))
}

fn validate_ssl_certificate_check_target_url(
    target_url: &str,
    ssl_certificate_checks_enabled: bool,
) -> Result<(), ApiError> {
    if !ssl_certificate_checks_enabled {
        return Ok(());
    }

    let parsed = reqwest::Url::parse(target_url)
        .map_err(|_| ApiError::bad_request("target_url is not a valid URL"))?;
    if parsed.scheme() != "https" {
        return Err(ApiError::bad_request(
            "ssl_certificate_checks_enabled requires an https target_url",
        ));
    }

    Ok(())
}
