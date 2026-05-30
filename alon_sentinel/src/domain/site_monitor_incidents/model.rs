use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::domain::site_monitors::SiteMonitorType;

pub struct OpenIncidentParams<'a> {
    pub site_monitor_id: i64,
    pub monitor_type: SiteMonitorType,
    pub target_url: &'a str,
    pub expected_status_code: i32,
    pub check_id: i64,
    pub checked_at: DateTime<Utc>,
    pub status_code: Option<i32>,
    pub failure_reason: Option<&'a str>,
    pub error_message: Option<&'a str>,
}

pub struct IncidentFailureParams<'a> {
    pub check_id: i64,
    pub checked_at: DateTime<Utc>,
    pub status_code: Option<i32>,
    pub failure_reason: Option<&'a str>,
    pub error_message: Option<&'a str>,
}

pub struct ResolveIncidentParams {
    pub check_id: i64,
    pub checked_at: DateTime<Utc>,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
}

pub struct IncidentCursorQuery {
    pub cursor_opened_at: Option<DateTime<Utc>>,
    pub cursor_id: Option<i64>,
    pub status: Option<SiteMonitorIncidentStatus>,
    pub limit: i64,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteMonitorIncidentWithSite {
    pub id: i64,
    pub site_id: i64,
    pub site_name: String,
    pub site_base_url: String,
    pub site_monitor_id: i64,
    pub status: SiteMonitorIncidentStatus,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub opened_check_id: Option<i64>,
    pub resolved_check_id: Option<i64>,
    pub monitor_type: SiteMonitorType,
    pub target_url: String,
    pub expected_status_code: i32,
    pub opened_status_code: Option<i32>,
    pub opened_failure_reason: Option<String>,
    pub opened_error_message: Option<String>,
    pub failure_count: i32,
    pub last_status_code: Option<i32>,
    pub last_failure_reason: Option<String>,
    pub last_error_message: Option<String>,
    pub resolved_reason: Option<SiteMonitorIncidentResolvedReason>,
    pub resolved_status_code: Option<i32>,
    pub resolved_response_time_ms: Option<i32>,
    pub downtime_seconds: Option<i32>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "site_monitor_incident_status", rename_all = "snake_case")]
pub enum SiteMonitorIncidentStatus {
    Open,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(
    type_name = "site_monitor_incident_resolved_reason",
    rename_all = "snake_case"
)]
pub enum SiteMonitorIncidentResolvedReason {
    Recovered,
    MonitoringDisabled,
    SiteDeactivated,
}

impl SiteMonitorIncidentResolvedReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Recovered => "recovered",
            Self::MonitoringDisabled => "monitoring_disabled",
            Self::SiteDeactivated => "site_deactivated",
        }
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteMonitorIncident {
    pub id: i64,
    pub site_id: i64,
    pub site_monitor_id: i64,
    pub status: SiteMonitorIncidentStatus,
    pub opened_at: DateTime<Utc>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub opened_check_id: Option<i64>,
    pub resolved_check_id: Option<i64>,
    pub last_check_id: Option<i64>,
    pub monitor_type: SiteMonitorType,
    pub target_url: String,
    pub expected_status_code: i32,
    pub opened_status_code: Option<i32>,
    pub opened_failure_reason: Option<String>,
    pub opened_error_message: Option<String>,
    pub failure_count: i32,
    pub last_checked_at: DateTime<Utc>,
    pub last_status_code: Option<i32>,
    pub last_failure_reason: Option<String>,
    pub last_error_message: Option<String>,
    pub resolved_reason: Option<SiteMonitorIncidentResolvedReason>,
    pub resolved_status_code: Option<i32>,
    pub resolved_response_time_ms: Option<i32>,
    pub downtime_seconds: Option<i32>,
    pub acknowledged_at: Option<DateTime<Utc>>,
    pub acknowledged_by: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
