use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::model::SiteMonitorCheck;

const SITE_MONITOR_CHECK_RETENTION_LOCK_KEY: i64 = 0x534d4352;

pub async fn create_site_monitor_check(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    site_monitor_id: i64,
    p: &super::model::CreateMonitorCheckParams<'_>,
) -> Result<SiteMonitorCheck> {
    let site_monitor_check = sqlx::query_as::<_, SiteMonitorCheck>(
        r#"
        INSERT INTO site_monitor_checks (
            site_monitor_id,
            monitor_type,
            url_checked,
            expected_status_code,
            is_success,
            status_code,
            response_time_ms,
            total_duration_ms,
            attempt_count,
            was_retried,
            failure_reason,
            error_message,
            certificate_expires_at,
            certificate_days_remaining,
            certificate_issuer,
            certificate_subject,
            certificate_domain
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
        RETURNING
            id,
            site_monitor_id,
            checked_at,
            monitor_type,
            url_checked,
            expected_status_code,
            is_success,
            status_code,
            response_time_ms,
            total_duration_ms,
            attempt_count,
            was_retried,
            failure_reason,
            error_message,
            certificate_expires_at,
            certificate_days_remaining,
            certificate_issuer,
            certificate_subject,
            certificate_domain
        "#,
    )
    .bind(site_monitor_id)
    .bind(p.monitor_type)
    .bind(p.url_checked)
    .bind(p.expected_status_code)
    .bind(p.is_success)
    .bind(p.status_code)
    .bind(p.response_time_ms)
    .bind(p.total_duration_ms)
    .bind(p.attempt_count)
    .bind(p.was_retried)
    .bind(p.failure_reason)
    .bind(p.error_message)
    .bind(p.certificate_expires_at)
    .bind(p.certificate_days_remaining)
    .bind(p.certificate_issuer)
    .bind(p.certificate_subject)
    .bind(p.certificate_domain)
    .fetch_one(&mut **transact)
    .await?;

    Ok(site_monitor_check)
}

pub async fn list_by_site_id(
    pool: &PgPool,
    site_id: i64,
    q: &super::model::CheckCursorQuery,
) -> Result<Vec<SiteMonitorCheck>> {
    let checks = sqlx::query_as::<_, SiteMonitorCheck>(
        r#"
        SELECT
            smc.id,
            smc.site_monitor_id,
            smc.checked_at,
            smc.monitor_type,
            smc.url_checked,
            smc.expected_status_code,
            smc.is_success,
            smc.status_code,
            smc.response_time_ms,
            smc.total_duration_ms,
            smc.attempt_count,
            smc.was_retried,
            smc.failure_reason,
            smc.error_message,
            smc.certificate_expires_at,
            smc.certificate_days_remaining,
            smc.certificate_issuer,
            smc.certificate_subject,
            smc.certificate_domain
        FROM site_monitor_checks smc
        INNER JOIN site_monitors sm ON sm.id = smc.site_monitor_id
        INNER JOIN sites s ON s.id = sm.site_id
        WHERE s.id = $1
          AND (
                $2 IS NULL
                OR smc.checked_at < $2
                OR (smc.checked_at = $2 AND smc.id < $3)
              )
          AND ($4 IS NULL OR smc.is_success = $4)
        ORDER BY smc.checked_at DESC, smc.id DESC
        LIMIT $5
        "#,
    )
    .bind(site_id)
    .bind(q.cursor_checked_at)
    .bind(q.cursor_id)
    .bind(q.is_success)
    .bind(q.limit)
    .fetch_all(pool)
    .await?;

    Ok(checks)
}

pub async fn prune_checks_older_than(
    pool: &PgPool,
    cutoff: DateTime<Utc>,
    limit: i64,
) -> Result<Option<u64>> {
    let mut transaction = pool.begin().await?;
    let lock_acquired = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
        .bind(SITE_MONITOR_CHECK_RETENTION_LOCK_KEY)
        .fetch_one(&mut *transaction)
        .await?;

    if !lock_acquired {
        transaction.rollback().await?;
        return Ok(None);
    }

    let deleted_count = sqlx::query_scalar::<_, i64>(
        r#"
        WITH expired_rows AS (
            SELECT ctid
            FROM site_monitor_checks
            WHERE checked_at < $1
            ORDER BY checked_at ASC, id ASC
            LIMIT $2
        ),
        deleted AS (
            DELETE FROM site_monitor_checks
            WHERE ctid IN (SELECT ctid FROM expired_rows)
            RETURNING 1
        )
        SELECT COUNT(*)
        FROM deleted
        "#,
    )
    .bind(cutoff)
    .bind(limit)
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(Some(deleted_count.max(0) as u64))
}

pub struct SiteUptimeStats {
    pub total_checks: i64,
    pub successful_checks: i64,
}

pub struct DailyUptimeBucket {
    pub date: String,
    pub total_checks: i64,
    pub successful_checks: i64,
}

pub async fn get_daily_uptime_stats(
    pool: &PgPool,
    site_id: i64,
    days: i64,
) -> Result<Vec<DailyUptimeBucket>> {
    let days_i32 = days as i32;
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT
            to_char(date_series.day, 'YYYY-MM-DD'),
            COUNT(smc.id)::bigint,
            COUNT(smc.id) FILTER (WHERE smc.is_success)::bigint
        FROM generate_series(
            (NOW() AT TIME ZONE 'UTC')::date - ($2::int - 1) * INTERVAL '1 day',
            (NOW() AT TIME ZONE 'UTC')::date,
            '1 day'::interval
        ) AS date_series(day)
        LEFT JOIN site_monitor_checks smc
            ON smc.checked_at >= date_series.day
           AND smc.checked_at <  date_series.day + INTERVAL '1 day'
           AND smc.site_monitor_id IN (
               SELECT id FROM site_monitors WHERE site_id = $1
           )
        GROUP BY date_series.day
        ORDER BY date_series.day ASC
        "#,
    )
    .bind(site_id)
    .bind(days_i32)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(date, total_checks, successful_checks)| DailyUptimeBucket {
                date,
                total_checks,
                successful_checks,
            },
        )
        .collect())
}

pub struct MonitorUptimeStats {
    pub total_7d: i64,
    pub success_7d: i64,
    pub total_30d: i64,
    pub success_30d: i64,
}

pub async fn get_monitor_uptime_stats(
    pool: &PgPool,
    monitor_id: i64,
) -> Result<MonitorUptimeStats> {
    let since_7d = Utc::now() - chrono::Duration::days(7);
    let since_30d = Utc::now() - chrono::Duration::days(30);
    let (total_7d, success_7d, total_30d, success_30d) = sqlx::query_as::<_, (i64, i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*) FILTER (WHERE checked_at >= $2)::bigint,
            COUNT(*) FILTER (WHERE is_success AND checked_at >= $2)::bigint,
            COUNT(*)::bigint,
            COUNT(*) FILTER (WHERE is_success)::bigint
        FROM site_monitor_checks
        WHERE site_monitor_id = $1
          AND checked_at >= $3
        "#,
    )
    .bind(monitor_id)
    .bind(since_7d)
    .bind(since_30d)
    .fetch_one(pool)
    .await?;
    Ok(MonitorUptimeStats {
        total_7d,
        success_7d,
        total_30d,
        success_30d,
    })
}

pub async fn get_monitor_daily_uptime_stats(
    pool: &PgPool,
    monitor_id: i64,
    days: i64,
) -> Result<Vec<DailyUptimeBucket>> {
    let days_i32 = days as i32;
    let rows = sqlx::query_as::<_, (String, i64, i64)>(
        r#"
        SELECT
            to_char(date_series.day, 'YYYY-MM-DD'),
            COUNT(smc.id)::bigint,
            COUNT(smc.id) FILTER (WHERE smc.is_success)::bigint
        FROM generate_series(
            (NOW() AT TIME ZONE 'UTC')::date - ($2::int - 1) * INTERVAL '1 day',
            (NOW() AT TIME ZONE 'UTC')::date,
            '1 day'::interval
        ) AS date_series(day)
        LEFT JOIN site_monitor_checks smc
            ON smc.checked_at >= date_series.day
           AND smc.checked_at <  date_series.day + INTERVAL '1 day'
           AND smc.site_monitor_id = $1
        GROUP BY date_series.day
        ORDER BY date_series.day ASC
        "#,
    )
    .bind(monitor_id)
    .bind(days_i32)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(date, total_checks, successful_checks)| DailyUptimeBucket {
                date,
                total_checks,
                successful_checks,
            },
        )
        .collect())
}

pub async fn get_site_uptime_stats(
    pool: &PgPool,
    site_id: i64,
    since: DateTime<Utc>,
) -> Result<SiteUptimeStats> {
    let (total_checks, successful_checks) = sqlx::query_as::<_, (i64, i64)>(
        r#"
        SELECT
            COUNT(*)::bigint,
            COUNT(*) FILTER (WHERE smc.is_success)::bigint
        FROM site_monitor_checks smc
        INNER JOIN site_monitors sm ON sm.id = smc.site_monitor_id
        WHERE sm.site_id = $1
          AND smc.checked_at >= $2
        "#,
    )
    .bind(site_id)
    .bind(since)
    .fetch_one(pool)
    .await?;

    Ok(SiteUptimeStats {
        total_checks,
        successful_checks,
    })
}
