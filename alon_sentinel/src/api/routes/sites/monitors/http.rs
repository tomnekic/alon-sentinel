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
pub(in crate::api::routes::sites) struct HttpSiteMonitorResponse {
    id: i64,
    site_id: i64,
    monitor_type: &'static str,
    target_url: String,
    check_interval_seconds: i32,
    expected_status_code: i32,
    body_must_contain: Option<String>,
    body_must_not_contain: Option<String>,
    body_must_contain_texts: Option<Vec<String>>,
    body_must_not_contain_texts: Option<Vec<String>>,
    json_path_exists: Option<Vec<String>>,
    json_path_equals: Option<Vec<site_monitors::JsonPathValueAssertion>>,
    json_path_not_equals: Option<Vec<site_monitors::JsonPathValueAssertion>>,
    max_response_time_ms: Option<i32>,
    required_header_name: Option<String>,
    required_header_value: Option<String>,
    header_assertions: Option<Vec<site_monitors::HttpHeaderAssertion>>,
    ssl_certificate_checks_enabled: bool,
    ssl_expiry_warning_days: Option<i32>,
    http_check_timeout_seconds_override: Option<i32>,
    http_check_max_attempts_override: Option<i32>,
    http_check_retry_delays_ms_override: Option<Vec<i64>>,
    is_active: bool,
    last_checked_at: Option<String>,
    last_successful_check_at: Option<String>,
    last_is_success: Option<bool>,
    last_status_code: Option<i32>,
    last_response_time_ms: Option<i32>,
    last_failure_reason: Option<String>,
    last_error_message: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<site_monitors::SiteMonitor> for HttpSiteMonitorResponse {
    fn from(monitor: site_monitors::SiteMonitor) -> Self {
        Self {
            id: monitor.id,
            site_id: monitor.site_id,
            monitor_type: "http",
            target_url: monitor.target_url,
            check_interval_seconds: monitor.check_interval_seconds,
            expected_status_code: monitor.expected_status_code,
            body_must_contain: monitor.body_must_contain,
            body_must_not_contain: monitor.body_must_not_contain,
            body_must_contain_texts: monitor.body_must_contain_texts,
            body_must_not_contain_texts: monitor.body_must_not_contain_texts,
            json_path_exists: monitor.json_path_exists,
            json_path_equals: monitor.json_path_equals.map(|assertions| assertions.0),
            json_path_not_equals: monitor.json_path_not_equals.map(|assertions| assertions.0),
            max_response_time_ms: monitor.max_response_time_ms,
            required_header_name: monitor.required_header_name,
            required_header_value: monitor.required_header_value,
            header_assertions: monitor.header_assertions.map(|assertions| assertions.0),
            ssl_certificate_checks_enabled: monitor.ssl_certificate_checks_enabled,
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
            last_status_code: monitor.last_status_code,
            last_response_time_ms: monitor.last_response_time_ms,
            last_failure_reason: monitor.last_failure_reason,
            last_error_message: monitor.last_error_message,
            created_at: monitor.created_at.to_rfc3339(),
            updated_at: monitor.updated_at.to_rfc3339(),
        }
    }
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct SiteMonitoringResponse {
    site: SiteResponse,
    http_monitors: Vec<HttpSiteMonitorResponse>,
}

#[derive(Serialize)]
pub(in crate::api::routes::sites) struct DisableHttpSiteMonitorResponse {
    disabled: bool,
}

#[derive(Deserialize)]
pub(in crate::api::routes::sites) struct UpsertHttpSiteMonitorRequest {
    target_url: String,
    check_interval_seconds: i32,
    expected_status_code: i32,
    body_must_contain: Option<String>,
    body_must_not_contain: Option<String>,
    body_must_contain_texts: Option<Vec<String>>,
    body_must_not_contain_texts: Option<Vec<String>>,
    json_path_exists: Option<Vec<String>>,
    json_path_equals: Option<Vec<site_monitors::JsonPathValueAssertion>>,
    json_path_not_equals: Option<Vec<site_monitors::JsonPathValueAssertion>>,
    max_response_time_ms: Option<usize>,
    required_header_name: Option<String>,
    required_header_value: Option<String>,
    header_assertions: Option<Vec<site_monitors::HttpHeaderAssertion>>,
    ssl_certificate_checks_enabled: Option<bool>,
    ssl_expiry_warning_days: Option<usize>,
    http_check_timeout_seconds_override: Option<usize>,
    http_check_max_attempts_override: Option<usize>,
    http_check_retry_delays_ms_override: Option<Vec<u64>>,
    is_active: bool,
}

struct NormalizedHttpMonitorPayload {
    target_url: String,
    body_must_contain: Option<String>,
    body_must_not_contain: Option<String>,
    body_must_contain_texts: Option<Vec<String>>,
    body_must_not_contain_texts: Option<Vec<String>>,
    json_path_exists: Option<Vec<String>>,
    json_path_equals: Option<sqlx::types::Json<Vec<site_monitors::JsonPathValueAssertion>>>,
    json_path_not_equals: Option<sqlx::types::Json<Vec<site_monitors::JsonPathValueAssertion>>>,
    required_header_name: Option<String>,
    required_header_value: Option<String>,
    header_assertions: Option<sqlx::types::Json<Vec<site_monitors::HttpHeaderAssertion>>>,
    ssl_certificate_checks_enabled: bool,
    ssl_expiry_warning_days: Option<i32>,
    retry_delays_override: Option<Vec<i64>>,
}

// --- HTTP monitor handlers ---

pub(in crate::api::routes::sites) async fn get_http_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<SiteMonitoringResponse>, ApiError> {
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
    let http_monitor_status = http_monitor_status(&http_monitors);

    Ok(Json(SiteMonitoringResponse {
        site: SiteResponse::from_site(site, http_monitor_status, false, "not_configured"),
        http_monitors: http_monitors
            .into_iter()
            .map(HttpSiteMonitorResponse::from)
            .collect(),
    }))
}

pub(in crate::api::routes::sites) async fn upsert_http_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpsertHttpSiteMonitorRequest>,
) -> Result<Json<HttpSiteMonitorResponse>, ApiError> {
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

    let n = validate_and_normalize_http_monitor_payload(
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

    let monitor = site_monitors::repository::upsert_http_monitor_by_site_id_and_target_url(
        &state.pool,
        site_id,
        &site_monitors::HttpMonitorParams {
            target_url: &n.target_url,
            check_interval_seconds: payload.check_interval_seconds,
            expected_status_code: payload.expected_status_code,
            body_must_contain: n.body_must_contain.as_deref(),
            body_must_not_contain: n.body_must_not_contain.as_deref(),
            body_must_contain_texts: n.body_must_contain_texts.as_deref(),
            body_must_not_contain_texts: n.body_must_not_contain_texts.as_deref(),
            json_path_exists: n.json_path_exists.as_deref(),
            json_path_equals: n.json_path_equals,
            json_path_not_equals: n.json_path_not_equals,
            max_response_time_ms: payload.max_response_time_ms.map(|v| v as i32),
            required_header_name: n.required_header_name.as_deref(),
            required_header_value: n.required_header_value.as_deref(),
            header_assertions: n.header_assertions,
            ssl_certificate_checks_enabled: n.ssl_certificate_checks_enabled,
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

pub(in crate::api::routes::sites) async fn update_http_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertHttpSiteMonitorRequest>,
) -> Result<Json<HttpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let n = validate_and_normalize_http_monitor_payload(
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
        site_monitors::repository::get_http_monitor_by_site_id_and_target_url(
            &state.pool,
            site_id,
            &n.target_url,
        )
        .await
        .map_err(ApiError::internal_error)?
        && existing_monitor.id != monitor_id
    {
        return Err(ApiError::bad_request(
            "target_url already exists for another http monitor on this site",
        ));
    }

    let Some(monitor) = site_monitors::repository::update_http_site_monitor_by_id(
        &state.pool,
        site_id,
        monitor_id,
        &site_monitors::HttpMonitorParams {
            target_url: &n.target_url,
            check_interval_seconds: payload.check_interval_seconds,
            expected_status_code: payload.expected_status_code,
            body_must_contain: n.body_must_contain.as_deref(),
            body_must_not_contain: n.body_must_not_contain.as_deref(),
            body_must_contain_texts: n.body_must_contain_texts.as_deref(),
            body_must_not_contain_texts: n.body_must_not_contain_texts.as_deref(),
            json_path_exists: n.json_path_exists.as_deref(),
            json_path_equals: n.json_path_equals,
            json_path_not_equals: n.json_path_not_equals,
            max_response_time_ms: payload.max_response_time_ms.map(|v| v as i32),
            required_header_name: n.required_header_name.as_deref(),
            required_header_value: n.required_header_value.as_deref(),
            header_assertions: n.header_assertions,
            ssl_certificate_checks_enabled: n.ssl_certificate_checks_enabled,
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
        return Err(ApiError::not_found("http monitor not found"));
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

pub(in crate::api::routes::sites) async fn disable_http_site_monitor(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DisableHttpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let monitors =
        site_monitors::repository::disable_http_monitors_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;

    if monitors.is_empty() {
        return Err(ApiError::not_found("http monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_site(
        &state.pool,
        site_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableHttpSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn disable_http_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<DisableHttpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let disabled =
        site_monitors::repository::disable_http_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
            .is_some();

    if !disabled {
        return Err(ApiError::not_found("http monitor not found"));
    }

    site_monitor_incidents::repository::resolve_open_incidents_for_monitor(
        &state.pool,
        monitor_id,
        SiteMonitorIncidentResolvedReason::MonitoringDisabled,
    )
    .await
    .map_err(ApiError::internal_error)?;

    Ok(Json(DisableHttpSiteMonitorResponse { disabled: true }))
}

pub(in crate::api::routes::sites) async fn pause_http_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<HttpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::pause_http_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("http monitor not found"));
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

pub(in crate::api::routes::sites) async fn resume_http_site_monitor_by_id(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, monitor_id)): Path<(i64, i64)>,
) -> Result<Json<HttpSiteMonitorResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteMonitorsUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(monitor) =
        site_monitors::repository::resume_http_monitor_by_id(&state.pool, site_id, monitor_id)
            .await
            .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("http monitor not found"));
    };

    Ok(Json(monitor.into()))
}

async fn validate_and_normalize_http_monitor_payload(
    payload: &UpsertHttpSiteMonitorRequest,
    allow_private: bool,
) -> Result<NormalizedHttpMonitorPayload, ApiError> {
    if payload.target_url.trim().is_empty() {
        return Err(ApiError::bad_request("target_url is required"));
    }

    validate_monitor_target_url(payload.target_url.trim(), allow_private).await?;

    if payload.check_interval_seconds < 30 {
        return Err(ApiError::bad_request(
            "check_interval_seconds must be at least 30",
        ));
    }

    if !(100..=599).contains(&payload.expected_status_code) {
        return Err(ApiError::bad_request(
            "expected_status_code must be between 100 and 599",
        ));
    }

    validate_http_monitor_policy_overrides(payload)?;

    let ssl_certificate_checks_enabled = payload.ssl_certificate_checks_enabled.unwrap_or(false);
    validate_ssl_certificate_check_target_url(
        payload.target_url.trim(),
        ssl_certificate_checks_enabled,
    )?;
    let ssl_expiry_warning_days = normalize_ssl_expiry_warning_days(
        ssl_certificate_checks_enabled,
        payload.ssl_expiry_warning_days,
    )?;

    let body_must_contain =
        normalize_http_assertion_text(payload.body_must_contain.as_deref(), "body_must_contain")?
            .map(str::to_string);
    let body_must_not_contain = normalize_http_assertion_text(
        payload.body_must_not_contain.as_deref(),
        "body_must_not_contain",
    )?
    .map(str::to_string);
    let body_must_contain_texts = normalize_http_assertion_texts(
        payload.body_must_contain_texts.as_ref(),
        "body_must_contain_texts",
    )?;
    let body_must_not_contain_texts = normalize_http_assertion_texts(
        payload.body_must_not_contain_texts.as_ref(),
        "body_must_not_contain_texts",
    )?;
    let json_path_exists =
        normalize_json_path_exists(payload.json_path_exists.as_ref(), "json_path_exists")?;
    let json_path_equals = normalize_json_path_value_assertions(
        payload.json_path_equals.as_ref(),
        "json_path_equals",
    )?;
    let json_path_not_equals = normalize_json_path_value_assertions(
        payload.json_path_not_equals.as_ref(),
        "json_path_not_equals",
    )?;
    let required_header_name =
        normalize_http_assertion_header_name(payload.required_header_name.as_deref())?
            .map(str::to_string);
    let required_header_value = normalize_http_assertion_text(
        payload.required_header_value.as_deref(),
        "required_header_value",
    )?
    .map(str::to_string);
    let header_assertions = normalize_http_header_assertions(payload.header_assertions.as_ref())?;
    let retry_delays_override = payload
        .http_check_retry_delays_ms_override
        .as_ref()
        .map(|values| values.iter().map(|value| *value as i64).collect::<Vec<_>>());

    Ok(NormalizedHttpMonitorPayload {
        target_url: payload.target_url.trim().to_string(),
        body_must_contain,
        body_must_not_contain,
        body_must_contain_texts,
        body_must_not_contain_texts,
        json_path_exists,
        json_path_equals,
        json_path_not_equals,
        required_header_name,
        required_header_value,
        header_assertions,
        ssl_certificate_checks_enabled,
        ssl_expiry_warning_days,
        retry_delays_override,
    })
}

fn validate_http_monitor_policy_overrides(
    payload: &UpsertHttpSiteMonitorRequest,
) -> Result<(), ApiError> {
    if matches!(payload.body_must_contain.as_deref(), Some(value) if value.trim().is_empty()) {
        return Err(ApiError::bad_request("body_must_contain must not be blank"));
    }

    if matches!(payload.body_must_not_contain.as_deref(), Some(value) if value.trim().is_empty()) {
        return Err(ApiError::bad_request(
            "body_must_not_contain must not be blank",
        ));
    }

    if let Some(values) = &payload.body_must_contain_texts {
        if values.is_empty() {
            return Err(ApiError::bad_request(
                "body_must_contain_texts must not be empty",
            ));
        }

        if values.iter().any(|value| value.trim().is_empty()) {
            return Err(ApiError::bad_request(
                "body_must_contain_texts must not contain blank values",
            ));
        }
    }

    if let Some(values) = &payload.body_must_not_contain_texts {
        if values.is_empty() {
            return Err(ApiError::bad_request(
                "body_must_not_contain_texts must not be empty",
            ));
        }

        if values.iter().any(|value| value.trim().is_empty()) {
            return Err(ApiError::bad_request(
                "body_must_not_contain_texts must not contain blank values",
            ));
        }
    }

    if let Some(values) = &payload.json_path_exists {
        if values.is_empty() {
            return Err(ApiError::bad_request("json_path_exists must not be empty"));
        }

        if values.iter().any(|value| value.trim().is_empty()) {
            return Err(ApiError::bad_request(
                "json_path_exists must not contain blank values",
            ));
        }
    }

    if let Some(assertions) = &payload.json_path_equals {
        if assertions.is_empty() {
            return Err(ApiError::bad_request("json_path_equals must not be empty"));
        }

        if assertions
            .iter()
            .any(|assertion| assertion.path.trim().is_empty())
        {
            return Err(ApiError::bad_request(
                "json_path_equals paths must not be blank",
            ));
        }
    }

    if let Some(assertions) = &payload.json_path_not_equals {
        if assertions.is_empty() {
            return Err(ApiError::bad_request(
                "json_path_not_equals must not be empty",
            ));
        }

        if assertions
            .iter()
            .any(|assertion| assertion.path.trim().is_empty())
        {
            return Err(ApiError::bad_request(
                "json_path_not_equals paths must not be blank",
            ));
        }
    }

    if payload.max_response_time_ms == Some(0) {
        return Err(ApiError::bad_request(
            "max_response_time_ms must be greater than 0",
        ));
    }

    if matches!(payload.required_header_name.as_deref(), Some(value) if value.trim().is_empty()) {
        return Err(ApiError::bad_request(
            "required_header_name must not be blank",
        ));
    }

    if matches!(payload.required_header_value.as_deref(), Some(value) if value.trim().is_empty()) {
        return Err(ApiError::bad_request(
            "required_header_value must not be blank",
        ));
    }

    if payload.required_header_name.is_none() && payload.required_header_value.is_some() {
        return Err(ApiError::bad_request(
            "required_header_name is required when required_header_value is provided",
        ));
    }

    if let Some(assertions) = &payload.header_assertions {
        if assertions.is_empty() {
            return Err(ApiError::bad_request("header_assertions must not be empty"));
        }

        for assertion in assertions {
            if assertion.name.trim().is_empty() {
                return Err(ApiError::bad_request(
                    "header_assertions names must not be blank",
                ));
            }

            reqwest::header::HeaderName::from_bytes(assertion.name.trim().as_bytes()).map_err(
                |_| ApiError::bad_request("header_assertions contain an invalid HTTP header name"),
            )?;

            if matches!(assertion.equals.as_deref(), Some(value) if value.trim().is_empty()) {
                return Err(ApiError::bad_request(
                    "header_assertions equals values must not be blank",
                ));
            }

            if matches!(assertion.contains.as_deref(), Some(value) if value.trim().is_empty()) {
                return Err(ApiError::bad_request(
                    "header_assertions contains values must not be blank",
                ));
            }

            if assertion.equals.is_some() && assertion.contains.is_some() {
                return Err(ApiError::bad_request(
                    "header_assertions may specify only one of equals or contains",
                ));
            }
        }
    }

    if matches!(payload.max_response_time_ms, Some(value) if value > i32::MAX as usize) {
        return Err(ApiError::bad_request("max_response_time_ms is too large"));
    }

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

fn normalize_http_assertion_header_name(value: Option<&str>) -> Result<Option<&str>, ApiError> {
    let Some(value) = value.map(str::trim) else {
        return Ok(None);
    };

    if value.is_empty() {
        return Err(ApiError::bad_request(
            "required_header_name must not be blank",
        ));
    }

    reqwest::header::HeaderName::from_bytes(value.as_bytes()).map_err(|_| {
        ApiError::bad_request("required_header_name is not a valid HTTP header name")
    })?;

    Ok(Some(value))
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

fn normalize_http_assertion_texts(
    values: Option<&Vec<String>>,
    field_name: &str,
) -> Result<Option<Vec<String>>, ApiError> {
    let Some(values) = values else {
        return Ok(None);
    };

    if values.is_empty() {
        return Err(ApiError::bad_request(format!(
            "{field_name} must not be empty"
        )));
    }

    let normalized = values
        .iter()
        .map(|value| value.trim())
        .map(|value| {
            if value.is_empty() {
                Err(ApiError::bad_request(format!(
                    "{field_name} must not contain blank values"
                )))
            } else {
                Ok(value.to_string())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(normalized))
}

fn normalize_http_assertion_text<'a>(
    value: Option<&'a str>,
    field_name: &str,
) -> Result<Option<&'a str>, ApiError> {
    match value.map(str::trim) {
        Some("") => Err(ApiError::bad_request(format!(
            "{field_name} must not be blank"
        ))),
        Some(value) => Ok(Some(value)),
        None => Ok(None),
    }
}

fn normalize_json_path_exists(
    values: Option<&Vec<String>>,
    field_name: &str,
) -> Result<Option<Vec<String>>, ApiError> {
    let Some(values) = values else {
        return Ok(None);
    };

    if values.is_empty() {
        return Err(ApiError::bad_request(format!(
            "{field_name} must not be empty"
        )));
    }

    let normalized = values
        .iter()
        .map(|value| value.trim())
        .map(|value| {
            if value.is_empty() {
                Err(ApiError::bad_request(format!(
                    "{field_name} must not contain blank values"
                )))
            } else {
                Ok(value.to_string())
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(normalized))
}

fn normalize_json_path_value_assertions(
    assertions: Option<&Vec<site_monitors::JsonPathValueAssertion>>,
    field_name: &str,
) -> Result<Option<sqlx::types::Json<Vec<site_monitors::JsonPathValueAssertion>>>, ApiError> {
    let Some(assertions) = assertions else {
        return Ok(None);
    };

    if assertions.is_empty() {
        return Err(ApiError::bad_request(format!(
            "{field_name} must not be empty"
        )));
    }

    let normalized = assertions
        .iter()
        .map(|assertion| {
            let path = assertion.path.trim();
            if path.is_empty() {
                return Err(ApiError::bad_request(format!(
                    "{field_name} paths must not be blank"
                )));
            }

            Ok(site_monitors::JsonPathValueAssertion {
                path: path.to_string(),
                value: assertion.value.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(sqlx::types::Json(normalized)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_request() -> UpsertHttpSiteMonitorRequest {
        UpsertHttpSiteMonitorRequest {
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
            ssl_certificate_checks_enabled: None,
            ssl_expiry_warning_days: None,
            http_check_timeout_seconds_override: None,
            http_check_max_attempts_override: None,
            http_check_retry_delays_ms_override: None,
            is_active: true,
        }
    }

    #[test]
    fn policy_overrides_valid_request_passes() {
        assert!(validate_http_monitor_policy_overrides(&minimal_request()).is_ok());
    }

    #[test]
    fn policy_overrides_blank_body_must_contain_fails() {
        let mut req = minimal_request();
        req.body_must_contain = Some("   ".to_string());
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_blank_body_must_not_contain_fails() {
        let mut req = minimal_request();
        req.body_must_not_contain = Some("".to_string());
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_empty_body_must_contain_texts_fails() {
        let mut req = minimal_request();
        req.body_must_contain_texts = Some(vec![]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_blank_item_in_body_must_contain_texts_fails() {
        let mut req = minimal_request();
        req.body_must_contain_texts = Some(vec!["ok".to_string(), "  ".to_string()]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_max_response_time_zero_fails() {
        let mut req = minimal_request();
        req.max_response_time_ms = Some(0);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_header_value_without_name_fails() {
        let mut req = minimal_request();
        req.required_header_name = None;
        req.required_header_value = Some("healthy".to_string());
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_empty_header_assertions_fails() {
        let mut req = minimal_request();
        req.header_assertions = Some(vec![]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_header_assertion_with_both_equals_and_contains_fails() {
        let mut req = minimal_request();
        req.header_assertions = Some(vec![site_monitors::HttpHeaderAssertion {
            name: "x-health".to_string(),
            equals: Some("ok".to_string()),
            contains: Some("ok".to_string()),
        }]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_timeout_zero_fails() {
        let mut req = minimal_request();
        req.http_check_timeout_seconds_override = Some(0);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_max_attempts_zero_fails() {
        let mut req = minimal_request();
        req.http_check_max_attempts_override = Some(0);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_retry_delays_empty_fails() {
        let mut req = minimal_request();
        req.http_check_retry_delays_ms_override = Some(vec![]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_retry_delays_with_zero_value_fails() {
        let mut req = minimal_request();
        req.http_check_retry_delays_ms_override = Some(vec![500, 0, 1000]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn policy_overrides_delays_fewer_than_max_attempts_minus_one_fails() {
        let mut req = minimal_request();
        req.http_check_max_attempts_override = Some(3);
        req.http_check_retry_delays_ms_override = Some(vec![500]);
        assert!(validate_http_monitor_policy_overrides(&req).is_err());
    }

    #[test]
    fn normalize_ssl_expiry_disabled_no_warning_days_ok() {
        let result = normalize_ssl_expiry_warning_days(false, None);
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn normalize_ssl_expiry_disabled_with_warning_days_fails() {
        let result = normalize_ssl_expiry_warning_days(false, Some(30));
        assert!(result.is_err());
    }

    #[test]
    fn normalize_ssl_expiry_enabled_defaults_to_14() {
        let result = normalize_ssl_expiry_warning_days(true, None);
        assert_eq!(result.unwrap(), Some(14));
    }

    #[test]
    fn normalize_ssl_expiry_enabled_too_low_fails() {
        let result = normalize_ssl_expiry_warning_days(true, Some(7));
        assert!(result.is_err());
    }

    #[test]
    fn normalize_ssl_expiry_enabled_exactly_8_ok() {
        let result = normalize_ssl_expiry_warning_days(true, Some(8));
        assert_eq!(result.unwrap(), Some(8));
    }

    #[test]
    fn validate_ssl_url_disabled_any_scheme_ok() {
        assert!(validate_ssl_certificate_check_target_url("http://example.com", false).is_ok());
        assert!(validate_ssl_certificate_check_target_url("https://example.com", false).is_ok());
    }

    #[test]
    fn validate_ssl_url_enabled_https_ok() {
        assert!(validate_ssl_certificate_check_target_url("https://example.com", true).is_ok());
    }

    #[test]
    fn validate_ssl_url_enabled_http_fails() {
        assert!(validate_ssl_certificate_check_target_url("http://example.com", true).is_err());
    }

    #[test]
    fn normalize_assertion_text_none_returns_none() {
        assert_eq!(normalize_http_assertion_text(None, "field").unwrap(), None);
    }

    #[test]
    fn normalize_assertion_text_blank_fails() {
        assert!(normalize_http_assertion_text(Some("   "), "field").is_err());
        assert!(normalize_http_assertion_text(Some(""), "field").is_err());
    }

    #[test]
    fn normalize_assertion_text_trims_and_returns() {
        assert_eq!(
            normalize_http_assertion_text(Some("  hello  "), "field").unwrap(),
            Some("hello")
        );
    }

    #[test]
    fn normalize_assertion_texts_none_returns_none() {
        assert_eq!(normalize_http_assertion_texts(None, "field").unwrap(), None);
    }

    #[test]
    fn normalize_assertion_texts_empty_fails() {
        assert!(normalize_http_assertion_texts(Some(&vec![]), "field").is_err());
    }

    #[test]
    fn normalize_assertion_texts_blank_item_fails() {
        assert!(
            normalize_http_assertion_texts(
                Some(&vec!["ok".to_string(), "  ".to_string()]),
                "field"
            )
            .is_err()
        );
    }

    #[test]
    fn normalize_assertion_texts_trims_values() {
        let result = normalize_http_assertion_texts(
            Some(&vec!["  foo  ".to_string(), " bar ".to_string()]),
            "field",
        )
        .unwrap();
        assert_eq!(result, Some(vec!["foo".to_string(), "bar".to_string()]));
    }
}

fn normalize_http_header_assertions(
    assertions: Option<&Vec<site_monitors::HttpHeaderAssertion>>,
) -> Result<Option<sqlx::types::Json<Vec<site_monitors::HttpHeaderAssertion>>>, ApiError> {
    let Some(assertions) = assertions else {
        return Ok(None);
    };

    if assertions.is_empty() {
        return Err(ApiError::bad_request("header_assertions must not be empty"));
    }

    let normalized = assertions
        .iter()
        .map(|assertion| {
            let name = assertion.name.trim();
            if name.is_empty() {
                return Err(ApiError::bad_request(
                    "header_assertions names must not be blank",
                ));
            }

            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ApiError::bad_request("header_assertions contain an invalid HTTP header name")
            })?;

            let equals = normalize_http_assertion_text(assertion.equals.as_deref(), "equals")?
                .map(ToOwned::to_owned);
            let contains =
                normalize_http_assertion_text(assertion.contains.as_deref(), "contains")?
                    .map(ToOwned::to_owned);

            if equals.is_some() && contains.is_some() {
                return Err(ApiError::bad_request(
                    "header_assertions may specify only one of equals or contains",
                ));
            }

            Ok(site_monitors::HttpHeaderAssertion {
                name: name.to_string(),
                equals,
                contains,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Some(sqlx::types::Json(normalized)))
}
