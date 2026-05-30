use anyhow::Result;
use sqlx::PgPool;

use super::model::SiteNotificationChannelOverride;

pub async fn get_for_site(
    pool: &PgPool,
    site_id: i64,
    notification_channel_id: i64,
) -> Result<Option<SiteNotificationChannelOverride>> {
    let override_row = sqlx::query_as::<_, SiteNotificationChannelOverride>(
        r#"
        SELECT
            id,
            site_id,
            notification_channel_id,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        FROM site_notification_channel_overrides
        WHERE site_id = $1
          AND notification_channel_id = $2
        "#,
    )
    .bind(site_id)
    .bind(notification_channel_id)
    .fetch_optional(pool)
    .await?;

    Ok(override_row)
}

pub async fn upsert_for_site(
    pool: &PgPool,
    site_id: i64,
    p: &super::model::ChannelOverrideParams,
) -> Result<Option<SiteNotificationChannelOverride>> {
    let override_row = sqlx::query_as::<_, SiteNotificationChannelOverride>(
        r#"
        INSERT INTO site_notification_channel_overrides (
            site_id,
            notification_channel_id,
            notify_on_failure,
            notify_on_recovery,
            is_active
        )
        SELECT
            s.id,
            nc.id,
            $3,
            $4,
            $5
        FROM sites s
        INNER JOIN notification_channels nc ON TRUE
        WHERE s.id = $1
          AND nc.id = $2
        ON CONFLICT (site_id, notification_channel_id) DO UPDATE
        SET
            notify_on_failure = EXCLUDED.notify_on_failure,
            notify_on_recovery = EXCLUDED.notify_on_recovery,
            is_active = EXCLUDED.is_active,
            updated_at = NOW()
        RETURNING
            id,
            site_id,
            notification_channel_id,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(site_id)
    .bind(p.notification_channel_id)
    .bind(p.notify_on_failure)
    .bind(p.notify_on_recovery)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(override_row)
}

pub async fn delete_for_site(
    pool: &PgPool,
    site_id: i64,
    notification_channel_id: i64,
) -> Result<Option<SiteNotificationChannelOverride>> {
    let override_row = sqlx::query_as::<_, SiteNotificationChannelOverride>(
        r#"
        DELETE FROM site_notification_channel_overrides snco
        USING sites s, notification_channels nc
        WHERE snco.site_id = s.id
          AND snco.notification_channel_id = nc.id
          AND s.id = $1
          AND nc.id = $2
        RETURNING
            snco.id,
            snco.site_id,
            snco.notification_channel_id,
            snco.notify_on_failure,
            snco.notify_on_recovery,
            snco.is_active,
            snco.created_at,
            snco.updated_at
        "#,
    )
    .bind(site_id)
    .bind(notification_channel_id)
    .fetch_optional(pool)
    .await?;

    Ok(override_row)
}
