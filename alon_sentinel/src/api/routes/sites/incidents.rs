use std::sync::Arc;

use axum::{
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
    domain::site_monitor_incidents,
};

use super::{encode_cursor, ensure_site_exists, history_limit, page_fetch_limit, parse_cursor};

#[derive(Debug, Deserialize)]
pub(super) struct SiteIncidentsQuery {
    limit: Option<usize>,
    cursor: Option<String>,
    status: Option<site_monitor_incidents::SiteMonitorIncidentStatus>,
}

#[derive(Serialize)]
pub(crate) struct SiteIncidentResponse {
    pub id: i64,
    pub monitor_id: i64,
    pub monitor_type: &'static str,
    pub target_url: String,
    pub expected_status_code: i32,
    pub status: &'static str,
    pub opened_at: String,
    pub resolved_at: Option<String>,
    pub started_check_id: Option<i64>,
    pub resolved_check_id: Option<i64>,
    pub opened_status_code: Option<i32>,
    pub opened_failure_reason: Option<String>,
    pub opened_error_message: Option<String>,
    pub failure_count: i32,
    pub last_status_code: Option<i32>,
    pub last_failure_reason: Option<String>,
    pub last_error_message: Option<String>,
    pub resolved_reason: Option<String>,
    pub resolved_status_code: Option<i32>,
    pub resolved_response_time_ms: Option<i32>,
    pub downtime_seconds: Option<i32>,
    pub acknowledged_at: Option<String>,
    pub acknowledged_by: Option<i64>,
}

impl From<site_monitor_incidents::SiteMonitorIncident> for SiteIncidentResponse {
    fn from(incident: site_monitor_incidents::SiteMonitorIncident) -> Self {
        Self {
            id: incident.id,
            monitor_id: incident.site_monitor_id,
            monitor_type: incident.monitor_type.as_str(),
            target_url: incident.target_url,
            expected_status_code: incident.expected_status_code,
            status: match incident.status {
                site_monitor_incidents::SiteMonitorIncidentStatus::Open => "open",
                site_monitor_incidents::SiteMonitorIncidentStatus::Resolved => "resolved",
            },
            opened_at: incident.opened_at.to_rfc3339(),
            resolved_at: incident.resolved_at.map(|ts| ts.to_rfc3339()),
            started_check_id: incident.opened_check_id,
            resolved_check_id: incident.resolved_check_id,
            opened_status_code: incident.opened_status_code,
            opened_failure_reason: incident.opened_failure_reason,
            opened_error_message: incident.opened_error_message,
            failure_count: incident.failure_count,
            last_status_code: incident.last_status_code,
            last_failure_reason: incident.last_failure_reason,
            last_error_message: incident.last_error_message,
            resolved_reason: incident.resolved_reason.map(|r| r.as_str().to_string()),
            resolved_status_code: incident.resolved_status_code,
            resolved_response_time_ms: incident.resolved_response_time_ms,
            downtime_seconds: incident.downtime_seconds,
            acknowledged_at: incident.acknowledged_at.map(|ts| ts.to_rfc3339()),
            acknowledged_by: incident.acknowledged_by,
        }
    }
}

pub(super) async fn list_site_incidents(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<SiteIncidentsQuery>,
) -> Result<Response, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SiteIncidentsRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    ensure_site_exists(&state.pool, site_id).await?;
    let limit = history_limit(query.limit)? as usize;
    let cursor = parse_cursor(query.cursor.as_deref())?;
    let incidents = site_monitor_incidents::repository::list_by_site_id(
        &state.pool,
        site_id,
        &site_monitor_incidents::IncidentCursorQuery {
            cursor_opened_at: cursor.as_ref().map(|c| c.timestamp),
            cursor_id: cursor.as_ref().map(|c| c.id),
            status: query.status,
            limit: page_fetch_limit(limit),
        },
    )
    .await
    .map_err(ApiError::internal_error)?;
    let (incidents, next_cursor) = paginate_vec_with_cursor(incidents, limit, |incident| {
        encode_cursor(incident.opened_at, incident.id)
    });

    Ok(json_with_next_cursor(
        incidents
            .into_iter()
            .map(SiteIncidentResponse::from)
            .collect::<Vec<_>>(),
        next_cursor,
    ))
}
