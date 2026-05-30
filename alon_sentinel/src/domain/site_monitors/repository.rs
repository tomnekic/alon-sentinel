use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::model::SiteMonitor;

const SITE_MONITOR_COLUMNS: &str = r#"
    id,
    site_id,
    monitor_type,
    target_url,
    check_interval_seconds,
    expected_status_code,
    body_must_contain,
    body_must_not_contain,
    body_must_contain_texts,
    body_must_not_contain_texts,
    json_path_exists,
    json_path_equals,
    json_path_not_equals,
    max_response_time_ms,
    required_header_name,
    required_header_value,
    header_assertions,
    ssl_certificate_checks_enabled,
    ssl_expiry_warning_days,
    tcp_target_host,
    tcp_target_port,
    dns_hostname,
    dns_record_type,
    dns_expected_value,
    dns_nameserver,
    heartbeat_token,
    heartbeat_grace_seconds,
    http_check_timeout_seconds_override,
    http_check_max_attempts_override,
    http_check_retry_delays_ms_override,
    is_active,
    check_claimed_at,
    check_lease_until,
    check_claimed_by,
    last_checked_at,
    last_successful_check_at,
    last_is_success,
    last_status_code,
    last_response_time_ms,
    last_failure_reason,
    last_error_message,
    last_heartbeat_received_at,
    last_certificate_expires_at,
    last_certificate_days_remaining,
    last_certificate_issuer,
    last_certificate_subject,
    last_certificate_domain,
    created_at,
    updated_at
"#;

const SITE_MONITOR_COLUMNS_QUALIFIED: &str = r#"
    sm.id,
    sm.site_id,
    sm.monitor_type,
    sm.target_url,
    sm.check_interval_seconds,
    sm.expected_status_code,
    sm.body_must_contain,
    sm.body_must_not_contain,
    sm.body_must_contain_texts,
    sm.body_must_not_contain_texts,
    sm.json_path_exists,
    sm.json_path_equals,
    sm.json_path_not_equals,
    sm.max_response_time_ms,
    sm.required_header_name,
    sm.required_header_value,
    sm.header_assertions,
    sm.ssl_certificate_checks_enabled,
    sm.ssl_expiry_warning_days,
    sm.tcp_target_host,
    sm.tcp_target_port,
    sm.dns_hostname,
    sm.dns_record_type,
    sm.dns_expected_value,
    sm.dns_nameserver,
    sm.heartbeat_token,
    sm.heartbeat_grace_seconds,
    sm.http_check_timeout_seconds_override,
    sm.http_check_max_attempts_override,
    sm.http_check_retry_delays_ms_override,
    sm.is_active,
    sm.check_claimed_at,
    sm.check_lease_until,
    sm.check_claimed_by,
    sm.last_checked_at,
    sm.last_successful_check_at,
    sm.last_is_success,
    sm.last_status_code,
    sm.last_response_time_ms,
    sm.last_failure_reason,
    sm.last_error_message,
    sm.last_heartbeat_received_at,
    sm.last_certificate_expires_at,
    sm.last_certificate_days_remaining,
    sm.last_certificate_issuer,
    sm.last_certificate_subject,
    sm.last_certificate_domain,
    sm.created_at,
    sm.updated_at
"#;

#[derive(Debug, sqlx::FromRow)]
pub struct HttpMonitorState {
    pub site_id: i64,
    pub has_active_monitor: bool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SslMonitorState {
    pub site_id: i64,
    pub has_active_monitor: bool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct HeartbeatMonitorState {
    pub site_id: i64,
    pub has_active_monitor: bool,
}

pub async fn list_http_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND sm.monitor_type = 'http'
        ORDER BY sm.id
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn get_http_monitor_by_site_id_and_target_url(
    pool: &PgPool,
    site_id: i64,
    target_url: &str,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.site_id = $1
          AND sm.monitor_type = 'http'
          AND sm.target_url = $2
        LIMIT 1
        "#,
    )))
    .bind(site_id)
    .bind(target_url)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn list_ssl_monitors_by_site_id(pool: &PgPool, site_id: i64) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND sm.monitor_type = 'ssl'
        ORDER BY sm.id
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn get_ssl_monitor_by_site_id_and_target_url(
    pool: &PgPool,
    site_id: i64,
    target_url: &str,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.site_id = $1
          AND sm.monitor_type = 'ssl'
          AND sm.target_url = $2
        LIMIT 1
        "#,
    )))
    .bind(site_id)
    .bind(target_url)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn list_heartbeat_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND sm.monitor_type = 'heartbeat'
        ORDER BY sm.id
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn get_heartbeat_monitor_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.site_id = $1
          AND sm.monitor_type = 'heartbeat'
        ORDER BY sm.id
        LIMIT 1
        "#,
    )))
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn get_heartbeat_monitor_by_token(
    pool: &PgPool,
    heartbeat_token: &str,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.monitor_type = 'heartbeat'
          AND sm.heartbeat_token = $1
        LIMIT 1
        "#,
    )))
    .bind(heartbeat_token)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn list_http_monitor_states(pool: &PgPool) -> Result<Vec<HttpMonitorState>> {
    let states = sqlx::query_as::<_, HttpMonitorState>(
        r#"
        SELECT
            sm.site_id,
            BOOL_OR(sm.is_active) AS has_active_monitor
        FROM site_monitors sm
        WHERE sm.monitor_type = 'http'
        GROUP BY sm.site_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(states)
}

pub async fn list_ssl_monitor_states(pool: &PgPool) -> Result<Vec<SslMonitorState>> {
    let states = sqlx::query_as::<_, SslMonitorState>(
        r#"
        SELECT
            sm.site_id,
            BOOL_OR(sm.is_active) AS has_active_monitor
        FROM site_monitors sm
        WHERE sm.monitor_type = 'ssl'
        GROUP BY sm.site_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(states)
}

pub async fn list_heartbeat_monitor_states(pool: &PgPool) -> Result<Vec<HeartbeatMonitorState>> {
    let states = sqlx::query_as::<_, HeartbeatMonitorState>(
        r#"
        SELECT
            sm.site_id,
            BOOL_OR(sm.is_active) AS has_active_monitor
        FROM site_monitors sm
        WHERE sm.monitor_type = 'heartbeat'
        GROUP BY sm.site_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(states)
}

pub async fn update_http_site_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
    p: &super::model::HttpMonitorParams<'_>,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            target_url = $3,
            check_interval_seconds = $4,
            expected_status_code = $5,
            body_must_contain = $6,
            body_must_not_contain = $7,
            body_must_contain_texts = $8,
            body_must_not_contain_texts = $9,
            json_path_exists = $10,
            json_path_equals = $11,
            json_path_not_equals = $12,
            max_response_time_ms = $13,
            required_header_name = $14,
            required_header_value = $15,
            header_assertions = $16,
            ssl_certificate_checks_enabled = $17,
            ssl_expiry_warning_days = $18,
            http_check_timeout_seconds_override = $19,
            http_check_max_attempts_override = $20,
            http_check_retry_delays_ms_override = $21,
            is_active = $22,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'http'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.expected_status_code)
    .bind(p.body_must_contain)
    .bind(p.body_must_not_contain)
    .bind(p.body_must_contain_texts)
    .bind(p.body_must_not_contain_texts)
    .bind(p.json_path_exists)
    .bind(p.json_path_equals.clone())
    .bind(p.json_path_not_equals.clone())
    .bind(p.max_response_time_ms)
    .bind(p.required_header_name)
    .bind(p.required_header_value)
    .bind(p.header_assertions.clone())
    .bind(p.ssl_certificate_checks_enabled)
    .bind(p.ssl_expiry_warning_days)
    .bind(p.http_check_timeout_seconds_override)
    .bind(p.http_check_max_attempts_override)
    .bind(p.http_check_retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn update_ssl_site_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
    p: &super::model::SslMonitorParams<'_>,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            target_url = $3,
            check_interval_seconds = $4,
            body_must_contain = NULL,
            body_must_not_contain = NULL,
            body_must_contain_texts = NULL,
            body_must_not_contain_texts = NULL,
            json_path_exists = NULL,
            json_path_equals = NULL,
            json_path_not_equals = NULL,
            max_response_time_ms = NULL,
            required_header_name = NULL,
            required_header_value = NULL,
            header_assertions = NULL,
            ssl_certificate_checks_enabled = TRUE,
            ssl_expiry_warning_days = $5,
            http_check_timeout_seconds_override = $6,
            http_check_max_attempts_override = $7,
            http_check_retry_delays_ms_override = $8,
            is_active = $9,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'ssl'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.ssl_expiry_warning_days)
    .bind(p.http_check_timeout_seconds_override)
    .bind(p.http_check_max_attempts_override)
    .bind(p.http_check_retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn create_http_site_monitor(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::HttpMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            check_interval_seconds,
            expected_status_code,
            body_must_contain,
            body_must_not_contain,
            body_must_contain_texts,
            body_must_not_contain_texts,
            json_path_exists,
            json_path_equals,
            json_path_not_equals,
            max_response_time_ms,
            required_header_name,
            required_header_value,
            header_assertions,
            ssl_certificate_checks_enabled,
            ssl_expiry_warning_days,
            http_check_timeout_seconds_override,
            http_check_max_attempts_override,
            http_check_retry_delays_ms_override,
            is_active
        )
        VALUES ($1, 'http', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.expected_status_code)
    .bind(p.body_must_contain)
    .bind(p.body_must_not_contain)
    .bind(p.body_must_contain_texts)
    .bind(p.body_must_not_contain_texts)
    .bind(p.json_path_exists)
    .bind(p.json_path_equals.clone())
    .bind(p.json_path_not_equals.clone())
    .bind(p.max_response_time_ms)
    .bind(p.required_header_name)
    .bind(p.required_header_value)
    .bind(p.header_assertions.clone())
    .bind(p.ssl_certificate_checks_enabled)
    .bind(p.ssl_expiry_warning_days)
    .bind(p.http_check_timeout_seconds_override)
    .bind(p.http_check_max_attempts_override)
    .bind(p.http_check_retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn create_heartbeat_site_monitor(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::HeartbeatMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            check_interval_seconds,
            expected_status_code,
            heartbeat_token,
            heartbeat_grace_seconds,
            is_active,
            last_checked_at
        )
        VALUES ($1, 'heartbeat', $2, $3, 200, $4, $5, $6, NOW())
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.heartbeat_token)
    .bind(p.heartbeat_grace_seconds)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn upsert_http_monitor_by_site_id_and_target_url(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::HttpMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            check_interval_seconds,
            expected_status_code,
            body_must_contain,
            body_must_not_contain,
            body_must_contain_texts,
            body_must_not_contain_texts,
            json_path_exists,
            json_path_equals,
            json_path_not_equals,
            max_response_time_ms,
            required_header_name,
            required_header_value,
            header_assertions,
            ssl_certificate_checks_enabled,
            ssl_expiry_warning_days,
            http_check_timeout_seconds_override,
            http_check_max_attempts_override,
            http_check_retry_delays_ms_override,
            is_active
        )
        VALUES ($1, 'http', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21)
        ON CONFLICT (site_id, monitor_type, target_url) DO UPDATE
        SET
            check_interval_seconds = EXCLUDED.check_interval_seconds,
            expected_status_code = EXCLUDED.expected_status_code,
            body_must_contain = EXCLUDED.body_must_contain,
            body_must_not_contain = EXCLUDED.body_must_not_contain,
            body_must_contain_texts = EXCLUDED.body_must_contain_texts,
            body_must_not_contain_texts = EXCLUDED.body_must_not_contain_texts,
            json_path_exists = EXCLUDED.json_path_exists,
            json_path_equals = EXCLUDED.json_path_equals,
            json_path_not_equals = EXCLUDED.json_path_not_equals,
            max_response_time_ms = EXCLUDED.max_response_time_ms,
            required_header_name = EXCLUDED.required_header_name,
            required_header_value = EXCLUDED.required_header_value,
            header_assertions = EXCLUDED.header_assertions,
            ssl_certificate_checks_enabled = EXCLUDED.ssl_certificate_checks_enabled,
            ssl_expiry_warning_days = EXCLUDED.ssl_expiry_warning_days,
            http_check_timeout_seconds_override = EXCLUDED.http_check_timeout_seconds_override,
            http_check_max_attempts_override = EXCLUDED.http_check_max_attempts_override,
            http_check_retry_delays_ms_override = EXCLUDED.http_check_retry_delays_ms_override,
            is_active = EXCLUDED.is_active,
            updated_at = NOW()
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.expected_status_code)
    .bind(p.body_must_contain)
    .bind(p.body_must_not_contain)
    .bind(p.body_must_contain_texts)
    .bind(p.body_must_not_contain_texts)
    .bind(p.json_path_exists)
    .bind(p.json_path_equals.clone())
    .bind(p.json_path_not_equals.clone())
    .bind(p.max_response_time_ms)
    .bind(p.required_header_name)
    .bind(p.required_header_value)
    .bind(p.header_assertions.clone())
    .bind(p.ssl_certificate_checks_enabled)
    .bind(p.ssl_expiry_warning_days)
    .bind(p.http_check_timeout_seconds_override)
    .bind(p.http_check_max_attempts_override)
    .bind(p.http_check_retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn update_heartbeat_site_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
    p: &super::model::HeartbeatMonitorUpdateParams,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            check_interval_seconds = $3,
            heartbeat_grace_seconds = $4,
            is_active = $5,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'heartbeat'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .bind(p.check_interval_seconds)
    .bind(p.heartbeat_grace_seconds)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn upsert_ssl_monitor_by_site_id_and_target_url(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::SslMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            check_interval_seconds,
            expected_status_code,
            ssl_certificate_checks_enabled,
            ssl_expiry_warning_days,
            http_check_timeout_seconds_override,
            http_check_max_attempts_override,
            http_check_retry_delays_ms_override,
            is_active
        )
        VALUES ($1, 'ssl', $2, $3, 200, TRUE, $4, $5, $6, $7, $8)
        ON CONFLICT (site_id, monitor_type, target_url) DO UPDATE
        SET
            check_interval_seconds = EXCLUDED.check_interval_seconds,
            ssl_certificate_checks_enabled = TRUE,
            ssl_expiry_warning_days = EXCLUDED.ssl_expiry_warning_days,
            http_check_timeout_seconds_override = EXCLUDED.http_check_timeout_seconds_override,
            http_check_max_attempts_override = EXCLUDED.http_check_max_attempts_override,
            http_check_retry_delays_ms_override = EXCLUDED.http_check_retry_delays_ms_override,
            is_active = EXCLUDED.is_active,
            updated_at = NOW()
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(p.target_url)
    .bind(p.check_interval_seconds)
    .bind(p.ssl_expiry_warning_days)
    .bind(p.http_check_timeout_seconds_override)
    .bind(p.http_check_max_attempts_override)
    .bind(p.http_check_retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn disable_http_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE site_id = $1
          AND monitor_type = 'http'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn disable_ssl_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE site_id = $1
          AND monitor_type = 'ssl'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn disable_heartbeat_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE site_id = $1
          AND monitor_type = 'heartbeat'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn disable_http_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'http'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn disable_ssl_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'ssl'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn disable_heartbeat_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'heartbeat'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn pause_http_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'http'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn resume_http_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = TRUE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'http'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn pause_ssl_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'ssl'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn resume_ssl_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = TRUE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'ssl'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn pause_heartbeat_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'heartbeat'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn resume_heartbeat_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = TRUE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'heartbeat'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn record_heartbeat_ping(
    pool: &PgPool,
    heartbeat_token: &str,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            last_heartbeat_received_at = NOW(),
            updated_at = NOW()
        WHERE monitor_type = 'heartbeat'
          AND heartbeat_token = $1
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(heartbeat_token)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn claim_site_monitors_due_for_check(
    pool: &PgPool,
    worker_id: &str,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        WITH due AS (
            SELECT sm.id
            FROM site_monitors sm
            INNER JOIN sites s ON s.id = sm.site_id
            WHERE sm.is_active = TRUE
              AND s.is_active = TRUE
              AND (
                    sm.last_checked_at IS NULL
                    OR sm.last_checked_at <= NOW() - (sm.check_interval_seconds * INTERVAL '1 second')
                  )
              AND (
                    sm.check_lease_until IS NULL
                    OR sm.check_lease_until < NOW()
                  )
            ORDER BY sm.last_checked_at NULLS FIRST, sm.id
            LIMIT $1
            FOR UPDATE OF sm SKIP LOCKED
        )
        UPDATE site_monitors sm
        SET
            check_claimed_at = NOW(),
            check_lease_until = NOW() + ($2 * INTERVAL '1 second'),
            check_claimed_by = $3,
            updated_at = NOW()
        FROM due
        WHERE sm.id = due.id
        RETURNING
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        "#,
    )))
    .bind(limit)
    .bind(lease_seconds)
    .bind(worker_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn next_claimable_check_at(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let next_due = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        r#"
        SELECT MIN(
            GREATEST(
                GREATEST(
                    COALESCE(
                        sm.last_checked_at + (sm.check_interval_seconds * INTERVAL '1 second'),
                        NOW()
                    ),
                    NOW()
                ),
                CASE
                    WHEN sm.check_lease_until IS NOT NULL AND sm.check_lease_until > NOW()
                        THEN sm.check_lease_until
                    ELSE NOW()
                END
            )
        )
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE sm.is_active = TRUE
          AND s.is_active = TRUE
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(next_due)
}

pub async fn update_site_monitor_last_check(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    site_monitor_id: i64,
    claimed_by: &str,
    p: &super::model::MonitorLastCheckParams<'_>,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitors
        SET
            last_checked_at = NOW(),
            last_successful_check_at = CASE WHEN $3 THEN NOW() ELSE last_successful_check_at END,
            last_is_success = $3,
            last_status_code = $4,
            last_response_time_ms = $5,
            last_failure_reason = $6,
            last_error_message = $7,
            last_certificate_expires_at = $8,
            last_certificate_days_remaining = $9,
            last_certificate_issuer = $10,
            last_certificate_subject = $11,
            last_certificate_domain = $12,
            check_claimed_at = NULL,
            check_lease_until = NULL,
            check_claimed_by = NULL,
            updated_at = NOW()
        WHERE id = $1
          AND check_claimed_by = $2
        "#,
    )
    .bind(site_monitor_id)
    .bind(claimed_by)
    .bind(p.is_success)
    .bind(p.status_code)
    .bind(p.response_time_ms)
    .bind(p.failure_reason)
    .bind(p.error_message)
    .bind(p.certificate_expires_at)
    .bind(p.certificate_days_remaining)
    .bind(p.certificate_issuer)
    .bind(p.certificate_subject)
    .bind(p.certificate_domain)
    .execute(&mut **transact)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn release_site_monitor_claim(
    pool: &PgPool,
    site_monitor_id: i64,
    claimed_by: &str,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitors
        SET
            check_claimed_at = NULL,
            check_lease_until = NULL,
            check_claimed_by = NULL,
            updated_at = NOW()
        WHERE id = $1
          AND check_claimed_by = $2
        "#,
    )
    .bind(site_monitor_id)
    .bind(claimed_by)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn extend_site_monitor_claim(
    pool: &PgPool,
    site_monitor_id: i64,
    claimed_by: &str,
    lease_seconds: i64,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitors
        SET
            check_lease_until = NOW() + ($3 * INTERVAL '1 second'),
            updated_at = NOW()
        WHERE id = $1
          AND check_claimed_by = $2
        "#,
    )
    .bind(site_monitor_id)
    .bind(claimed_by)
    .bind(lease_seconds)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn list_tcp_monitors_by_site_id(pool: &PgPool, site_id: i64) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND sm.monitor_type = 'tcp'
        ORDER BY sm.id
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn get_tcp_monitor_by_site_id_and_host_port(
    pool: &PgPool,
    site_id: i64,
    tcp_target_host: &str,
    tcp_target_port: i32,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.site_id = $1
          AND sm.monitor_type = 'tcp'
          AND sm.tcp_target_host = $2
          AND sm.tcp_target_port = $3
        LIMIT 1
        "#,
    )))
    .bind(site_id)
    .bind(tcp_target_host)
    .bind(tcp_target_port)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn upsert_tcp_monitor_by_site_id_and_host_port(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::TcpMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let target_url = format!("tcp://{}:{}", p.target_host, p.target_port);
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            tcp_target_host,
            tcp_target_port,
            check_interval_seconds,
            expected_status_code,
            max_response_time_ms,
            http_check_timeout_seconds_override,
            http_check_max_attempts_override,
            http_check_retry_delays_ms_override,
            is_active
        )
        VALUES ($1, 'tcp', $2, $3, $4, $5, 200, $6, $7, $8, $9, $10)
        ON CONFLICT (site_id, monitor_type, target_url) DO UPDATE
        SET
            check_interval_seconds = EXCLUDED.check_interval_seconds,
            max_response_time_ms = EXCLUDED.max_response_time_ms,
            http_check_timeout_seconds_override = EXCLUDED.http_check_timeout_seconds_override,
            http_check_max_attempts_override = EXCLUDED.http_check_max_attempts_override,
            http_check_retry_delays_ms_override = EXCLUDED.http_check_retry_delays_ms_override,
            is_active = EXCLUDED.is_active,
            updated_at = NOW()
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(&target_url)
    .bind(p.target_host)
    .bind(p.target_port)
    .bind(p.check_interval_seconds)
    .bind(p.max_connect_time_ms)
    .bind(p.timeout_seconds_override)
    .bind(p.max_attempts_override)
    .bind(p.retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn update_tcp_site_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
    p: &super::model::TcpMonitorParams<'_>,
) -> Result<Option<SiteMonitor>> {
    let target_url = format!("tcp://{}:{}", p.target_host, p.target_port);
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            target_url = $3,
            tcp_target_host = $4,
            tcp_target_port = $5,
            check_interval_seconds = $6,
            max_response_time_ms = $7,
            http_check_timeout_seconds_override = $8,
            http_check_max_attempts_override = $9,
            http_check_retry_delays_ms_override = $10,
            is_active = $11,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'tcp'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .bind(&target_url)
    .bind(p.target_host)
    .bind(p.target_port)
    .bind(p.check_interval_seconds)
    .bind(p.max_connect_time_ms)
    .bind(p.timeout_seconds_override)
    .bind(p.max_attempts_override)
    .bind(p.retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn disable_tcp_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE site_id = $1
          AND monitor_type = 'tcp'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn disable_tcp_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'tcp'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn pause_tcp_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'tcp'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn resume_tcp_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = TRUE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'tcp'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn list_dns_monitors_by_site_id(pool: &PgPool, site_id: i64) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND sm.monitor_type = 'dns'
        ORDER BY sm.id
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn get_dns_monitor_by_site_id_and_hostname_record_type(
    pool: &PgPool,
    site_id: i64,
    dns_hostname: &str,
    dns_record_type: &str,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {SITE_MONITOR_COLUMNS_QUALIFIED}
        FROM site_monitors sm
        WHERE sm.site_id = $1
          AND sm.monitor_type = 'dns'
          AND sm.dns_hostname = $2
          AND sm.dns_record_type = $3
        LIMIT 1
        "#,
    )))
    .bind(site_id)
    .bind(dns_hostname)
    .bind(dns_record_type)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn upsert_dns_monitor_by_site_id_and_hostname_record_type(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::DnsMonitorParams<'_>,
) -> Result<SiteMonitor> {
    let target_url = format!("dns://{}/{}", p.hostname, p.record_type);
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitors (
            site_id,
            monitor_type,
            target_url,
            dns_hostname,
            dns_record_type,
            dns_expected_value,
            dns_nameserver,
            check_interval_seconds,
            expected_status_code,
            http_check_timeout_seconds_override,
            http_check_max_attempts_override,
            http_check_retry_delays_ms_override,
            is_active
        )
        VALUES ($1, 'dns', $2, $3, $4, $5, $6, $7, 200, $8, $9, $10, $11)
        ON CONFLICT (site_id, monitor_type, target_url) DO UPDATE
        SET
            dns_expected_value = EXCLUDED.dns_expected_value,
            dns_nameserver = EXCLUDED.dns_nameserver,
            check_interval_seconds = EXCLUDED.check_interval_seconds,
            http_check_timeout_seconds_override = EXCLUDED.http_check_timeout_seconds_override,
            http_check_max_attempts_override = EXCLUDED.http_check_max_attempts_override,
            http_check_retry_delays_ms_override = EXCLUDED.http_check_retry_delays_ms_override,
            is_active = EXCLUDED.is_active,
            updated_at = NOW()
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(&target_url)
    .bind(p.hostname)
    .bind(p.record_type)
    .bind(p.expected_value)
    .bind(p.nameserver)
    .bind(p.check_interval_seconds)
    .bind(p.timeout_seconds_override)
    .bind(p.max_attempts_override)
    .bind(p.retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn update_dns_site_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
    p: &super::model::DnsMonitorParams<'_>,
) -> Result<Option<SiteMonitor>> {
    let target_url = format!("dns://{}/{}", p.hostname, p.record_type);
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            target_url = $3,
            dns_hostname = $4,
            dns_record_type = $5,
            dns_expected_value = $6,
            dns_nameserver = $7,
            check_interval_seconds = $8,
            http_check_timeout_seconds_override = $9,
            http_check_max_attempts_override = $10,
            http_check_retry_delays_ms_override = $11,
            is_active = $12,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'dns'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .bind(&target_url)
    .bind(p.hostname)
    .bind(p.record_type)
    .bind(p.expected_value)
    .bind(p.nameserver)
    .bind(p.check_interval_seconds)
    .bind(p.timeout_seconds_override)
    .bind(p.max_attempts_override)
    .bind(p.retry_delays_ms_override)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn disable_dns_monitors_by_site_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Vec<SiteMonitor>> {
    let site_monitors = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE site_id = $1
          AND monitor_type = 'dns'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .fetch_all(pool)
    .await?;

    Ok(site_monitors)
}

pub async fn disable_dns_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'dns'
          AND is_active = TRUE
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn pause_dns_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = FALSE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'dns'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

pub async fn resume_dns_monitor_by_id(
    pool: &PgPool,
    site_id: i64,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitor>> {
    let site_monitor = sqlx::query_as::<_, SiteMonitor>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitors
        SET
            is_active = TRUE,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
          AND monitor_type = 'dns'
        RETURNING
            {SITE_MONITOR_COLUMNS}
        "#,
    )))
    .bind(site_monitor_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site_monitor)
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteHealthState {
    pub site_id: i64,
    pub has_active_monitor: bool,
    pub any_failing: Option<bool>,
    pub any_succeeding: Option<bool>,
}

pub async fn list_site_health_states(pool: &PgPool) -> Result<Vec<SiteHealthState>> {
    let states = sqlx::query_as::<_, SiteHealthState>(
        r#"
        SELECT
            sm.site_id,
            BOOL_OR(sm.is_active)                                               AS has_active_monitor,
            BOOL_OR(sm.last_is_success = FALSE) FILTER (WHERE sm.is_active)     AS any_failing,
            BOOL_OR(sm.last_is_success = TRUE)  FILTER (WHERE sm.is_active)     AS any_succeeding
        FROM site_monitors sm
        GROUP BY sm.site_id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(states)
}

pub async fn get_site_health_state_by_id(
    pool: &PgPool,
    site_id: i64,
) -> Result<Option<SiteHealthState>> {
    let state = sqlx::query_as::<_, SiteHealthState>(
        r#"
        SELECT
            sm.site_id,
            BOOL_OR(sm.is_active)                                               AS has_active_monitor,
            BOOL_OR(sm.last_is_success = FALSE) FILTER (WHERE sm.is_active)     AS any_failing,
            BOOL_OR(sm.last_is_success = TRUE)  FILTER (WHERE sm.is_active)     AS any_succeeding
        FROM site_monitors sm
        WHERE sm.site_id = $1
        GROUP BY sm.site_id
        "#,
    )
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(state)
}
