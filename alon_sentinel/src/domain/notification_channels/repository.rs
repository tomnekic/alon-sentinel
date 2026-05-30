use anyhow::Result;
use sqlx::{Executor, PgPool, Postgres};

use super::model::{EffectiveNotificationChannel, NotificationChannel, NotificationChannelParams};

pub async fn list_channels(pool: &PgPool) -> Result<Vec<NotificationChannel>> {
    let channels = sqlx::query_as::<_, NotificationChannel>(
        r#"
        SELECT
            id,
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        FROM notification_channels
        ORDER BY id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(channels)
}

pub async fn list_effective_by_site_id<'a, E>(
    executor: E,
    site_id: i64,
) -> Result<Vec<EffectiveNotificationChannel>>
where
    E: Executor<'a, Database = Postgres>,
{
    let channels = sqlx::query_as::<_, EffectiveNotificationChannel>(
        r#"
        SELECT
            nc.id,
            nc.channel_type,
            nc.name,
            nc.destination,
            nc.webhook_secret_ciphertext,
            nc.notify_on_failure AS default_notify_on_failure,
            nc.notify_on_recovery AS default_notify_on_recovery,
            nc.is_active AS default_is_active,
            COALESCE(snco.notify_on_failure, nc.notify_on_failure) AS notify_on_failure,
            COALESCE(snco.notify_on_recovery, nc.notify_on_recovery) AS notify_on_recovery,
            COALESCE(snco.is_active, nc.is_active) AS is_active,
            snco.id AS override_id,
            snco.notify_on_failure AS override_notify_on_failure,
            snco.notify_on_recovery AS override_notify_on_recovery,
            snco.is_active AS override_is_active
        FROM sites s
        INNER JOIN notification_channels nc ON TRUE
        LEFT JOIN site_notification_channel_overrides snco
            ON snco.site_id = s.id
           AND snco.notification_channel_id = nc.id
        WHERE s.id = $1
        ORDER BY nc.id
        "#,
    )
    .bind(site_id)
    .fetch_all(executor)
    .await?;

    Ok(channels)
}

pub async fn create_channel(
    pool: &PgPool,
    p: &NotificationChannelParams<'_>,
) -> Result<NotificationChannel> {
    let channel = sqlx::query_as::<_, NotificationChannel>(
        r#"
        INSERT INTO notification_channels (
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id,
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(p.channel_type)
    .bind(p.name)
    .bind(p.destination)
    .bind(p.webhook_secret_ciphertext)
    .bind(p.notify_on_failure)
    .bind(p.notify_on_recovery)
    .bind(p.is_active)
    .fetch_one(pool)
    .await?;

    Ok(channel)
}

pub async fn update_channel(
    pool: &PgPool,
    channel_id: i64,
    p: &NotificationChannelParams<'_>,
) -> Result<Option<NotificationChannel>> {
    let channel = sqlx::query_as::<_, NotificationChannel>(
        r#"
        UPDATE notification_channels
        SET
            channel_type = $2,
            name = $3,
            destination = $4,
            webhook_secret_ciphertext = $5,
            notify_on_failure = $6,
            notify_on_recovery = $7,
            is_active = $8,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(channel_id)
    .bind(p.channel_type)
    .bind(p.name)
    .bind(p.destination)
    .bind(p.webhook_secret_ciphertext)
    .bind(p.notify_on_failure)
    .bind(p.notify_on_recovery)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(channel)
}

pub async fn delete_channel(pool: &PgPool, channel_id: i64) -> Result<Option<NotificationChannel>> {
    let channel = sqlx::query_as::<_, NotificationChannel>(
        r#"
        DELETE FROM notification_channels
        WHERE id = $1
        RETURNING
            id,
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        "#,
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;

    Ok(channel)
}

pub async fn get_channel_by_id(
    pool: &PgPool,
    channel_id: i64,
) -> Result<Option<NotificationChannel>> {
    let channel = sqlx::query_as::<_, NotificationChannel>(
        r#"
        SELECT
            id,
            channel_type,
            name,
            destination,
            webhook_secret_ciphertext,
            notify_on_failure,
            notify_on_recovery,
            is_active,
            created_at,
            updated_at
        FROM notification_channels
        WHERE id = $1
        "#,
    )
    .bind(channel_id)
    .fetch_optional(pool)
    .await?;

    Ok(channel)
}
