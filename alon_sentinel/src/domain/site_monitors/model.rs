use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpHeaderAssertion {
    pub name: String,
    pub equals: Option<String>,
    pub contains: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JsonPathValueAssertion {
    pub path: String,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "site_monitor_type", rename_all = "snake_case")]
pub enum SiteMonitorType {
    Http,
    Ssl,
    Heartbeat,
    Tcp,
    Dns,
}

impl SiteMonitorType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Ssl => "ssl",
            Self::Heartbeat => "heartbeat",
            Self::Tcp => "tcp",
            Self::Dns => "dns",
        }
    }
}

pub struct HttpMonitorParams<'a> {
    pub target_url: &'a str,
    pub check_interval_seconds: i32,
    pub expected_status_code: i32,
    pub body_must_contain: Option<&'a str>,
    pub body_must_not_contain: Option<&'a str>,
    pub body_must_contain_texts: Option<&'a [String]>,
    pub body_must_not_contain_texts: Option<&'a [String]>,
    pub json_path_exists: Option<&'a [String]>,
    pub json_path_equals: Option<sqlx::types::Json<Vec<JsonPathValueAssertion>>>,
    pub json_path_not_equals: Option<sqlx::types::Json<Vec<JsonPathValueAssertion>>>,
    pub max_response_time_ms: Option<i32>,
    pub required_header_name: Option<&'a str>,
    pub required_header_value: Option<&'a str>,
    pub header_assertions: Option<sqlx::types::Json<Vec<HttpHeaderAssertion>>>,
    pub ssl_certificate_checks_enabled: bool,
    pub ssl_expiry_warning_days: Option<i32>,
    pub http_check_timeout_seconds_override: Option<i32>,
    pub http_check_max_attempts_override: Option<i32>,
    pub http_check_retry_delays_ms_override: Option<&'a [i64]>,
    pub is_active: bool,
}

pub struct SslMonitorParams<'a> {
    pub target_url: &'a str,
    pub check_interval_seconds: i32,
    pub ssl_expiry_warning_days: Option<i32>,
    pub http_check_timeout_seconds_override: Option<i32>,
    pub http_check_max_attempts_override: Option<i32>,
    pub http_check_retry_delays_ms_override: Option<&'a [i64]>,
    pub is_active: bool,
}

pub struct TcpMonitorParams<'a> {
    pub target_host: &'a str,
    pub target_port: i32,
    pub check_interval_seconds: i32,
    pub max_connect_time_ms: Option<i32>,
    pub timeout_seconds_override: Option<i32>,
    pub max_attempts_override: Option<i32>,
    pub retry_delays_ms_override: Option<&'a [i64]>,
    pub is_active: bool,
}

pub struct HeartbeatMonitorParams<'a> {
    pub target_url: &'a str,
    pub heartbeat_token: &'a str,
    pub check_interval_seconds: i32,
    pub heartbeat_grace_seconds: Option<i32>,
    pub is_active: bool,
}

pub struct HeartbeatMonitorUpdateParams {
    pub check_interval_seconds: i32,
    pub heartbeat_grace_seconds: Option<i32>,
    pub is_active: bool,
}

pub struct MonitorLastCheckParams<'a> {
    pub is_success: bool,
    pub status_code: Option<i32>,
    pub response_time_ms: Option<i32>,
    pub failure_reason: Option<&'a str>,
    pub error_message: Option<&'a str>,
    pub certificate_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub certificate_days_remaining: Option<i32>,
    pub certificate_issuer: Option<&'a str>,
    pub certificate_subject: Option<&'a str>,
    pub certificate_domain: Option<&'a str>,
}

pub struct DnsMonitorParams<'a> {
    pub hostname: &'a str,
    pub record_type: &'a str,
    pub expected_value: Option<&'a str>,
    pub nameserver: Option<&'a str>,
    pub check_interval_seconds: i32,
    pub timeout_seconds_override: Option<i32>,
    pub max_attempts_override: Option<i32>,
    pub retry_delays_ms_override: Option<&'a [i64]>,
    pub is_active: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_monitor_type_as_str_covers_all_variants() {
        assert_eq!(SiteMonitorType::Http.as_str(), "http");
        assert_eq!(SiteMonitorType::Ssl.as_str(), "ssl");
        assert_eq!(SiteMonitorType::Heartbeat.as_str(), "heartbeat");
        assert_eq!(SiteMonitorType::Tcp.as_str(), "tcp");
        assert_eq!(SiteMonitorType::Dns.as_str(), "dns");
    }
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteMonitor {
    pub id: i64,
    pub site_id: i64,
    pub monitor_type: SiteMonitorType,
    pub target_url: String,
    pub check_interval_seconds: i32,
    pub expected_status_code: i32,
    pub body_must_contain: Option<String>,
    pub body_must_not_contain: Option<String>,
    pub body_must_contain_texts: Option<Vec<String>>,
    pub body_must_not_contain_texts: Option<Vec<String>>,
    pub json_path_exists: Option<Vec<String>>,
    pub json_path_equals: Option<Json<Vec<JsonPathValueAssertion>>>,
    pub json_path_not_equals: Option<Json<Vec<JsonPathValueAssertion>>>,
    pub max_response_time_ms: Option<i32>,
    pub required_header_name: Option<String>,
    pub required_header_value: Option<String>,
    pub header_assertions: Option<Json<Vec<HttpHeaderAssertion>>>,
    pub ssl_certificate_checks_enabled: bool,
    pub ssl_expiry_warning_days: Option<i32>,
    pub tcp_target_host: Option<String>,
    pub tcp_target_port: Option<i32>,
    pub dns_hostname: Option<String>,
    pub dns_record_type: Option<String>,
    pub dns_expected_value: Option<String>,
    pub dns_nameserver: Option<String>,
    pub heartbeat_token: Option<String>,
    pub heartbeat_grace_seconds: Option<i32>,
    pub http_check_timeout_seconds_override: Option<i32>,
    pub http_check_max_attempts_override: Option<i32>,
    pub http_check_retry_delays_ms_override: Option<Vec<i64>>,
    pub is_active: bool,
    pub check_claimed_at: Option<DateTime<Utc>>,
    pub check_lease_until: Option<DateTime<Utc>>,
    pub check_claimed_by: Option<String>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_successful_check_at: Option<DateTime<Utc>>,
    pub last_is_success: Option<bool>,
    pub last_status_code: Option<i32>,
    pub last_response_time_ms: Option<i32>,
    pub last_failure_reason: Option<String>,
    pub last_error_message: Option<String>,
    pub last_heartbeat_received_at: Option<DateTime<Utc>>,
    pub last_certificate_expires_at: Option<DateTime<Utc>>,
    pub last_certificate_days_remaining: Option<i32>,
    pub last_certificate_issuer: Option<String>,
    pub last_certificate_subject: Option<String>,
    pub last_certificate_domain: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
