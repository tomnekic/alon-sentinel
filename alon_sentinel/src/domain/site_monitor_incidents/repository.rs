use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::model::{
    IncidentCursorQuery, IncidentFailureParams, OpenIncidentParams, ResolveIncidentParams,
    SiteMonitorIncident, SiteMonitorIncidentResolvedReason, SiteMonitorIncidentStatus,
    SiteMonitorIncidentWithSite,
};

const INCIDENT_COLUMNS: &str = r#"
    id,
    site_id,
    site_monitor_id,
    status,
    opened_at,
    resolved_at,
    opened_check_id,
    resolved_check_id,
    last_check_id,
    monitor_type,
    target_url,
    expected_status_code,
    opened_status_code,
    opened_failure_reason,
    opened_error_message,
    failure_count,
    last_checked_at,
    last_status_code,
    last_failure_reason,
    last_error_message,
    resolved_reason,
    resolved_status_code,
    resolved_response_time_ms,
    downtime_seconds,
    acknowledged_at,
    acknowledged_by,
    created_at,
    updated_at
"#;

pub async fn open_incident(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    site_id: i64,
    p: &OpenIncidentParams<'_>,
) -> Result<SiteMonitorIncident> {
    let incident = sqlx::query_as::<_, SiteMonitorIncident>(sqlx::AssertSqlSafe(format!(
        r#"
        INSERT INTO site_monitor_incidents (
            site_id,
            site_monitor_id,
            status,
            opened_at,
            opened_check_id,
            last_check_id,
            monitor_type,
            target_url,
            expected_status_code,
            opened_status_code,
            opened_failure_reason,
            opened_error_message,
            failure_count,
            last_checked_at,
            last_status_code,
            last_failure_reason,
            last_error_message
        )
        VALUES ($1, $2, 'open', $3, $4, $4, $5, $6, $7, $8, $9, $10, 1, $3, $8, $9, $10)
        RETURNING
            {INCIDENT_COLUMNS}
        "#,
    )))
    .bind(site_id)
    .bind(p.site_monitor_id)
    .bind(p.checked_at)
    .bind(p.check_id)
    .bind(p.monitor_type)
    .bind(p.target_url)
    .bind(p.expected_status_code)
    .bind(p.status_code)
    .bind(p.failure_reason)
    .bind(p.error_message)
    .fetch_one(&mut **transact)
    .await?;

    Ok(incident)
}

pub async fn get_open_incident_for_monitor(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    site_monitor_id: i64,
) -> Result<Option<SiteMonitorIncident>> {
    let incident = sqlx::query_as::<_, SiteMonitorIncident>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {INCIDENT_COLUMNS}
        FROM site_monitor_incidents
        WHERE site_monitor_id = $1
          AND status = 'open'
        LIMIT 1
        "#,
    )))
    .bind(site_monitor_id)
    .fetch_optional(&mut **transact)
    .await?;

    Ok(incident)
}

pub async fn update_incident_failure(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    incident_id: i64,
    p: &IncidentFailureParams<'_>,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitor_incidents
        SET
            failure_count = failure_count + 1,
            last_check_id = $2,
            last_checked_at = $3,
            last_status_code = $4,
            last_failure_reason = $5,
            last_error_message = $6,
            updated_at = NOW()
        WHERE id = $1
          AND status = 'open'
        "#,
    )
    .bind(incident_id)
    .bind(p.check_id)
    .bind(p.checked_at)
    .bind(p.status_code)
    .bind(p.failure_reason)
    .bind(p.error_message)
    .execute(&mut **transact)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn resolve_incident(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    incident_id: i64,
    p: &ResolveIncidentParams,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitor_incidents
        SET
            status = 'resolved',
            resolved_at = $2,
            resolved_check_id = $3,
            last_check_id = $3,
            last_checked_at = $2,
            last_status_code = $4,
            last_failure_reason = NULL,
            last_error_message = NULL,
            resolved_reason = 'recovered',
            resolved_status_code = $4,
            resolved_response_time_ms = $5,
            downtime_seconds = EXTRACT(EPOCH FROM ($2 - opened_at))::INTEGER,
            updated_at = NOW()
        WHERE id = $1
          AND status = 'open'
        "#,
    )
    .bind(incident_id)
    .bind(p.checked_at)
    .bind(p.check_id)
    .bind(p.status_code)
    .bind(p.response_time_ms)
    .execute(&mut **transact)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn resolve_open_incidents_for_monitor(
    pool: &PgPool,
    site_monitor_id: i64,
    reason: SiteMonitorIncidentResolvedReason,
) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitor_incidents
        SET
            status = 'resolved',
            resolved_at = NOW(),
            last_checked_at = NOW(),
            resolved_reason = $2,
            downtime_seconds = EXTRACT(EPOCH FROM (NOW() - opened_at))::INTEGER,
            updated_at = NOW()
        WHERE site_monitor_id = $1
          AND status = 'open'
        "#,
    )
    .bind(site_monitor_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn resolve_open_incidents_for_site(
    pool: &PgPool,
    site_id: i64,
    reason: SiteMonitorIncidentResolvedReason,
) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE site_monitor_incidents
        SET
            status = 'resolved',
            resolved_at = NOW(),
            last_checked_at = NOW(),
            resolved_reason = $2,
            downtime_seconds = EXTRACT(EPOCH FROM (NOW() - opened_at))::INTEGER,
            updated_at = NOW()
        WHERE site_id = $1
          AND status = 'open'
        "#,
    )
    .bind(site_id)
    .bind(reason)
    .execute(pool)
    .await?;

    Ok(result.rows_affected())
}

pub async fn acknowledge_incident(
    pool: &PgPool,
    incident_id: i64,
    site_id: i64,
    admin_user_id: i64,
) -> Result<Option<SiteMonitorIncident>> {
    let incident = sqlx::query_as::<_, SiteMonitorIncident>(sqlx::AssertSqlSafe(format!(
        r#"
        UPDATE site_monitor_incidents
        SET
            acknowledged_at = NOW(),
            acknowledged_by = $3,
            updated_at = NOW()
        WHERE id = $1
          AND site_id = $2
        RETURNING
            {INCIDENT_COLUMNS}
        "#,
    )))
    .bind(incident_id)
    .bind(site_id)
    .bind(admin_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(incident)
}

pub async fn list_by_site_id(
    pool: &PgPool,
    site_id: i64,
    q: &IncidentCursorQuery,
) -> Result<Vec<SiteMonitorIncident>> {
    let incidents = sqlx::query_as::<_, SiteMonitorIncident>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {INCIDENT_COLUMNS}
        FROM site_monitor_incidents
        WHERE site_id = $1
          AND (
                $2::TIMESTAMPTZ IS NULL
                OR opened_at < $2
                OR (opened_at = $2 AND id < $3)
              )
          AND ($4::site_monitor_incident_status IS NULL OR status = $4)
        ORDER BY opened_at DESC, id DESC
        LIMIT $5
        "#,
    )))
    .bind(site_id)
    .bind(q.cursor_opened_at)
    .bind(q.cursor_id)
    .bind(q.status)
    .bind(q.limit)
    .fetch_all(pool)
    .await?;

    Ok(incidents)
}

pub async fn has_open_incident_for_site(pool: &PgPool, site_id: i64) -> Result<bool> {
    let exists = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1
            FROM site_monitor_incidents
            WHERE site_id = $1
              AND status = 'open'
        )
        "#,
    )
    .bind(site_id)
    .fetch_one(pool)
    .await?;

    Ok(exists)
}

pub async fn get_site_ids_with_open_incidents(pool: &PgPool) -> Result<Vec<i64>> {
    let site_ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT DISTINCT site_id
        FROM site_monitor_incidents
        WHERE status = 'open'
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(site_ids)
}

pub async fn count_open_incidents(pool: &PgPool) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM site_monitor_incidents WHERE status = 'open'"#,
    )
    .fetch_one(pool)
    .await?;

    Ok(count)
}

pub async fn list_global_incidents(
    pool: &PgPool,
    cursor_opened_at: Option<DateTime<Utc>>,
    cursor_id: Option<i64>,
    status: Option<SiteMonitorIncidentStatus>,
    limit: i64,
) -> Result<Vec<SiteMonitorIncidentWithSite>> {
    let incidents = sqlx::query_as::<_, SiteMonitorIncidentWithSite>(
        r#"
        SELECT
            smi.id,
            smi.site_id,
            s.name AS site_name,
            s.base_url AS site_base_url,
            smi.site_monitor_id,
            smi.status,
            smi.opened_at,
            smi.resolved_at,
            smi.opened_check_id,
            smi.resolved_check_id,
            smi.monitor_type,
            smi.target_url,
            smi.expected_status_code,
            smi.opened_status_code,
            smi.opened_failure_reason,
            smi.opened_error_message,
            smi.failure_count,
            smi.last_status_code,
            smi.last_failure_reason,
            smi.last_error_message,
            smi.resolved_reason,
            smi.resolved_status_code,
            smi.resolved_response_time_ms,
            smi.downtime_seconds,
            smi.acknowledged_at,
            smi.acknowledged_by,
            smi.created_at,
            smi.updated_at
        FROM site_monitor_incidents smi
        INNER JOIN sites s ON s.id = smi.site_id
        WHERE (
            $1::TIMESTAMPTZ IS NULL
            OR smi.opened_at < $1
            OR (smi.opened_at = $1 AND smi.id < $2)
        )
        AND ($3::site_monitor_incident_status IS NULL OR smi.status = $3)
        ORDER BY smi.opened_at DESC, smi.id DESC
        LIMIT $4
        "#,
    )
    .bind(cursor_opened_at)
    .bind(cursor_id)
    .bind(status)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(incidents)
}

pub async fn get_incident_by_id_and_site_id(
    pool: &PgPool,
    incident_id: i64,
    site_id: i64,
) -> Result<Option<SiteMonitorIncident>> {
    let incident = sqlx::query_as::<_, SiteMonitorIncident>(sqlx::AssertSqlSafe(format!(
        r#"
        SELECT
            {INCIDENT_COLUMNS}
        FROM site_monitor_incidents
        WHERE id = $1
          AND site_id = $2
        "#,
    )))
    .bind(incident_id)
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(incident)
}
