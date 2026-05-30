use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use super::model::Site;

pub async fn create_site(pool: &PgPool, name: &str, base_url: &str) -> Result<Site> {
    let site = sqlx::query_as::<_, Site>(
        r#"
        INSERT INTO sites (
            name,
            base_url
        )
        VALUES ($1, $2)
        RETURNING
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(name)
    .bind(base_url)
    .fetch_one(pool)
    .await?;

    Ok(site)
}

pub async fn get_all_sites(pool: &PgPool) -> Result<Vec<Site>> {
    let sites = sqlx::query_as::<_, Site>(
        r#"
        SELECT
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        FROM sites
        ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(sites)
}

pub async fn list_sites(
    pool: &PgPool,
    query: Option<&str>,
    limit: i64,
    cursor_id: Option<i64>,
) -> Result<Vec<Site>> {
    let sites = sqlx::query_as::<_, Site>(
        r#"
        SELECT
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        FROM sites
        WHERE (
            $1::TEXT IS NULL
            OR name ILIKE '%' || $1 || '%'
            OR base_url ILIKE '%' || $1 || '%'
            OR CAST(id AS TEXT) ILIKE '%' || $1 || '%'
        )
          AND ($2::BIGINT IS NULL OR id > $2)
        ORDER BY id
        LIMIT $3
        "#,
    )
    .bind(query)
    .bind(cursor_id)
    .bind(limit)
    .fetch_all(pool)
    .await?;

    Ok(sites)
}

pub async fn get_site_by_id(pool: &PgPool, site_id: i64) -> Result<Option<Site>> {
    let site = sqlx::query_as::<_, Site>(
        r#"
        SELECT
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        FROM sites
        WHERE id = $1
        "#,
    )
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site)
}

pub async fn get_first_site(pool: &PgPool) -> Result<Option<Site>> {
    let site = sqlx::query_as::<_, Site>(
        r#"
        SELECT
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        FROM sites
        ORDER BY id
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    Ok(site)
}

pub async fn update_site(
    pool: &PgPool,
    site_id: i64,
    name: &str,
    base_url: &str,
    is_active: bool,
) -> Result<Option<Site>> {
    let site = sqlx::query_as::<_, Site>(
        r#"
        UPDATE sites
        SET
            name = $2,
            base_url = $3,
            is_active = $4,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(site_id)
    .bind(name)
    .bind(base_url)
    .bind(is_active)
    .fetch_optional(pool)
    .await?;

    Ok(site)
}

pub async fn get_active_sites(pool: &PgPool) -> Result<Vec<Site>> {
    let sites = sqlx::query_as::<_, Site>(
        r#"
        SELECT
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        FROM sites
        WHERE is_active = TRUE
        ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(sites)
}

pub struct SiteDashboardCounts {
    pub total: i64,
    pub active: i64,
    pub with_any_monitor: i64,
    pub without_any_monitor: i64,
}

pub async fn get_dashboard_counts(pool: &PgPool) -> Result<SiteDashboardCounts> {
    let row = sqlx::query!(
        r#"
        SELECT
            COUNT(*) AS "total!",
            COUNT(*) FILTER (WHERE sites.is_active) AS "active!",
            COUNT(DISTINCT sm.site_id) AS "with_any_monitor!",
            COUNT(*) - COUNT(DISTINCT sm.site_id) AS "without_any_monitor!"
        FROM sites
        LEFT JOIN site_monitors sm ON sm.site_id = sites.id AND sm.is_active = TRUE
        "#
    )
    .fetch_one(pool)
    .await?;

    Ok(SiteDashboardCounts {
        total: row.total,
        active: row.active,
        with_any_monitor: row.with_any_monitor,
        without_any_monitor: row.without_any_monitor,
    })
}

#[derive(Debug, sqlx::FromRow)]
pub struct DashboardSiteRow {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub is_active: bool,
    pub monitors_total: i64,
    pub monitors_active_count: i64,
    pub any_failing: Option<bool>,
    pub any_succeeding: Option<bool>,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_response_time_ms: Option<i32>,
    pub has_open_incident: bool,
}

pub async fn get_dashboard_sites(pool: &PgPool) -> Result<Vec<DashboardSiteRow>> {
    let rows = sqlx::query_as::<_, DashboardSiteRow>(
        r#"
        SELECT
            s.id,
            s.name,
            s.base_url,
            s.is_active,
            COUNT(sm.id)                                                        AS monitors_total,
            COUNT(sm.id) FILTER (WHERE sm.is_active)                            AS monitors_active_count,
            BOOL_OR(sm.last_is_success = FALSE) FILTER (WHERE sm.is_active)     AS any_failing,
            BOOL_OR(sm.last_is_success = TRUE)  FILTER (WHERE sm.is_active)     AS any_succeeding,
            MAX(sm.last_checked_at)                                             AS last_checked_at,
            MAX(sm.last_response_time_ms) FILTER (
                WHERE sm.monitor_type = 'http' AND sm.is_active
            )                                                                   AS last_response_time_ms,
            EXISTS(
                SELECT 1 FROM site_monitor_incidents smi
                WHERE smi.site_id = s.id AND smi.status = 'open'
            )                                                                   AS has_open_incident
        FROM sites s
        LEFT JOIN site_monitors sm ON sm.site_id = s.id
        GROUP BY s.id
        ORDER BY s.id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

pub async fn delete_site(pool: &PgPool, site_id: i64) -> Result<Option<Site>> {
    let site = sqlx::query_as::<_, Site>(
        r#"
        DELETE FROM sites
        WHERE id = $1
        RETURNING
            id,
            name,
            base_url,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(site)
}
