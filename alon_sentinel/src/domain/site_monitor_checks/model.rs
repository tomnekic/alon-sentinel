use chrono::{DateTime, Utc};

use crate::domain::site_monitors::SiteMonitorType;

pub struct CheckCursorQuery {
    pub cursor_checked_at: Option<DateTime<Utc>>,
    pub cursor_id: Option<i64>,
    pub is_success: Option<bool>,
    pub limit: i64,
}

pub struct CreateMonitorCheckParams<'a> {
    pub monitor_type: SiteMonitorType,
    pub url_checked: &'a str,
    pub expected_status_code: Option<i32>,
    pub is_success: bool,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub total_duration_ms: Option<i32>,
    pub attempt_count: i32,
    pub was_retried: bool,
    pub failure_reason: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub certificate_expires_at: Option<DateTime<Utc>>,
    pub certificate_days_remaining: Option<i32>,
    pub certificate_issuer: Option<&'a str>,
    pub certificate_subject: Option<&'a str>,
    pub certificate_domain: Option<&'a str>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteMonitorCheck {
    pub id: i64,
    pub site_monitor_id: i64,
    pub checked_at: DateTime<Utc>,
    pub monitor_type: SiteMonitorType,
    pub url_checked: String,
    pub expected_status_code: Option<i32>,
    pub is_success: bool,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub total_duration_ms: Option<i32>,
    pub attempt_count: i32,
    pub was_retried: bool,
    pub failure_reason: Option<String>,
    pub error_message: Option<String>,
    pub certificate_expires_at: Option<DateTime<Utc>>,
    pub certificate_days_remaining: Option<i32>,
    pub certificate_issuer: Option<String>,
    pub certificate_subject: Option<String>,
    pub certificate_domain: Option<String>,
}
