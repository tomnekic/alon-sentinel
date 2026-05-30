use anyhow::Result;
use sqlx::PgPool;

use super::model::SiteStatusPage;

pub async fn get_by_site_id(pool: &PgPool, site_id: i64) -> Result<Option<SiteStatusPage>> {
    let row = sqlx::query_as::<_, SiteStatusPage>(
        r#"
        SELECT
            id, site_id, is_enabled, slug, page_title,
            show_monitor_details, show_uptime_percentages,
            created_at, updated_at
        FROM site_status_pages
        WHERE site_id = $1
        "#,
    )
    .bind(site_id)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub async fn get_by_slug(pool: &PgPool, slug: &str) -> Result<Option<SiteStatusPage>> {
    let row = sqlx::query_as::<_, SiteStatusPage>(
        r#"
        SELECT
            id, site_id, is_enabled, slug, page_title,
            show_monitor_details, show_uptime_percentages,
            created_at, updated_at
        FROM site_status_pages
        WHERE slug = $1
        "#,
    )
    .bind(slug)
    .fetch_optional(pool)
    .await?;

    Ok(row)
}

pub struct UpsertPayload<'a> {
    pub site_id: i64,
    pub is_enabled: bool,
    pub slug: &'a str,
    pub page_title: Option<&'a str>,
    pub show_monitor_details: bool,
    pub show_uptime_percentages: bool,
}

pub async fn upsert(pool: &PgPool, payload: UpsertPayload<'_>) -> Result<SiteStatusPage> {
    let row = sqlx::query_as::<_, SiteStatusPage>(
        r#"
        INSERT INTO site_status_pages (
            site_id, is_enabled, slug, page_title,
            show_monitor_details, show_uptime_percentages
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT (site_id) DO UPDATE SET
            is_enabled              = EXCLUDED.is_enabled,
            slug                    = EXCLUDED.slug,
            page_title              = EXCLUDED.page_title,
            show_monitor_details    = EXCLUDED.show_monitor_details,
            show_uptime_percentages = EXCLUDED.show_uptime_percentages,
            updated_at              = NOW()
        RETURNING
            id, site_id, is_enabled, slug, page_title,
            show_monitor_details, show_uptime_percentages,
            created_at, updated_at
        "#,
    )
    .bind(payload.site_id)
    .bind(payload.is_enabled)
    .bind(payload.slug)
    .bind(payload.page_title)
    .bind(payload.show_monitor_details)
    .bind(payload.show_uptime_percentages)
    .fetch_one(pool)
    .await?;

    Ok(row)
}
