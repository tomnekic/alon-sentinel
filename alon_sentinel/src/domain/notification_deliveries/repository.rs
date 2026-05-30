use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::PgPool;

use super::model::{
    ClaimedNotificationDelivery, DeliveryCursorQuery, NotificationEventType,
    SiteNotificationDelivery,
};

pub struct NewNotificationDelivery<'a> {
    pub notification_channel_id: i64,
    pub site_monitor_id: i64,
    pub site_monitor_check_id: i64,
    pub incident_id: Option<i64>,
    pub event_type: NotificationEventType,
    pub payload: &'a Value,
}

pub async fn enqueue_deliveries(
    transact: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    deliveries: &[NewNotificationDelivery<'_>],
) -> Result<()> {
    for delivery in deliveries {
        sqlx::query(
            r#"
            INSERT INTO notification_deliveries (
                notification_channel_id,
                site_monitor_id,
                site_monitor_check_id,
                incident_id,
                event_type,
                payload
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (
                notification_channel_id,
                site_monitor_check_id,
                event_type
            ) DO NOTHING
            "#,
        )
        .bind(delivery.notification_channel_id)
        .bind(delivery.site_monitor_id)
        .bind(delivery.site_monitor_check_id)
        .bind(delivery.incident_id)
        .bind(delivery.event_type)
        .bind(delivery.payload)
        .execute(&mut **transact)
        .await?;
    }

    Ok(())
}

pub async fn claim_due_deliveries(
    pool: &PgPool,
    worker_id: &str,
    limit: i64,
    lease_seconds: i64,
) -> Result<Vec<ClaimedNotificationDelivery>> {
    let deliveries = sqlx::query_as::<_, ClaimedNotificationDelivery>(
        r#"
        WITH due AS (
            SELECT nd.id
            FROM notification_deliveries nd
            INNER JOIN notification_channels nc ON nc.id = nd.notification_channel_id
            WHERE nc.is_active = TRUE
              AND (
                    nd.status = 'pending'
                    OR (
                        nd.status = 'failed'
                        AND nd.next_attempt_at IS NOT NULL
                    )
                  )
              AND nd.next_attempt_at <= NOW()
              AND (
                    nd.lease_until IS NULL
                    OR nd.lease_until < NOW()
                  )
            ORDER BY nd.next_attempt_at, nd.id
            LIMIT $1
            FOR UPDATE SKIP LOCKED
        )
        UPDATE notification_deliveries nd
        SET
            claimed_at = NOW(),
            lease_until = NOW() + ($2 * INTERVAL '1 second'),
            claimed_by = $3,
            updated_at = NOW()
        FROM due, notification_channels nc
        WHERE nd.id = due.id
          AND nc.id = nd.notification_channel_id
        RETURNING
            nd.id,
            nd.notification_channel_id,
            nd.site_monitor_id,
            nd.site_monitor_check_id,
            nd.incident_id,
            nd.event_type,
            nd.payload,
            nd.status,
            nd.attempts,
            nc.channel_type,
            nc.name AS channel_name,
            nc.destination,
            nc.webhook_secret_ciphertext
        "#,
    )
    .bind(limit)
    .bind(lease_seconds)
    .bind(worker_id)
    .fetch_all(pool)
    .await?;

    Ok(deliveries)
}

pub async fn next_claimable_delivery_at(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    let next_due = sqlx::query_scalar::<_, Option<DateTime<Utc>>>(
        r#"
        SELECT MIN(
            GREATEST(
                GREATEST(nd.next_attempt_at, NOW()),
                CASE
                    WHEN nd.lease_until IS NOT NULL AND nd.lease_until > NOW()
                        THEN nd.lease_until
                    ELSE NOW()
                END
            )
        )
        FROM notification_deliveries nd
        INNER JOIN notification_channels nc ON nc.id = nd.notification_channel_id
        WHERE nc.is_active = TRUE
          AND (
                nd.status = 'pending'
                OR (
                    nd.status = 'failed'
                    AND nd.next_attempt_at IS NOT NULL
                )
              )
          AND nd.next_attempt_at IS NOT NULL
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(next_due)
}

pub async fn mark_delivered(pool: &PgPool, delivery_id: i64, claimed_by: &str) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET
            status = 'delivered',
            attempts = attempts + 1,
            delivered_at = NOW(),
            claimed_at = NULL,
            lease_until = NULL,
            claimed_by = NULL,
            last_error = NULL,
            updated_at = NOW()
        WHERE id = $1
          AND claimed_by = $2
        "#,
    )
    .bind(delivery_id)
    .bind(claimed_by)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn mark_failed(
    pool: &PgPool,
    delivery_id: i64,
    claimed_by: &str,
    last_error: &str,
    next_attempt_at: Option<DateTime<Utc>>,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET
            status = 'failed',
            attempts = attempts + 1,
            next_attempt_at = $3,
            claimed_at = NULL,
            lease_until = NULL,
            claimed_by = NULL,
            last_error = $4,
            updated_at = NOW()
        WHERE id = $1
          AND claimed_by = $2
        "#,
    )
    .bind(delivery_id)
    .bind(claimed_by)
    .bind(next_attempt_at)
    .bind(last_error)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn extend_delivery_claim(
    pool: &PgPool,
    delivery_id: i64,
    claimed_by: &str,
    lease_seconds: i64,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET
            lease_until = NOW() + ($3 * INTERVAL '1 second'),
            updated_at = NOW()
        WHERE id = $1
          AND claimed_by = $2
        "#,
    )
    .bind(delivery_id)
    .bind(claimed_by)
    .bind(lease_seconds)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn release_delivery_claim(
    pool: &PgPool,
    delivery_id: i64,
    claimed_by: &str,
) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE notification_deliveries
        SET
            claimed_at = NULL,
            lease_until = NULL,
            claimed_by = NULL,
            updated_at = NOW()
        WHERE id = $1
          AND claimed_by = $2
        "#,
    )
    .bind(delivery_id)
    .bind(claimed_by)
    .execute(pool)
    .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn list_by_site_id(
    pool: &PgPool,
    site_id: i64,
    q: &DeliveryCursorQuery,
) -> Result<Vec<SiteNotificationDelivery>> {
    let deliveries = sqlx::query_as::<_, SiteNotificationDelivery>(
        r#"
        SELECT
            nd.id,
            nd.notification_channel_id,
            nd.site_monitor_id,
            nd.site_monitor_check_id,
            nd.incident_id,
            nd.event_type,
            nd.payload,
            nd.status,
            nd.attempts,
            nd.next_attempt_at,
            nd.claimed_at,
            nd.lease_until,
            nd.claimed_by,
            nd.delivered_at,
            nd.last_error,
            nd.created_at,
            nd.updated_at,
            nc.channel_type,
            nc.name AS channel_name,
            nc.destination
        FROM notification_deliveries nd
        INNER JOIN site_monitors sm ON sm.id = nd.site_monitor_id
        INNER JOIN sites s ON s.id = sm.site_id
        INNER JOIN notification_channels nc
            ON nc.id = nd.notification_channel_id
        WHERE s.id = $1
          AND (
                $2 IS NULL
                OR nd.created_at < $2
                OR (nd.created_at = $2 AND nd.id < $3)
              )
          AND ($4 IS NULL OR nd.status = $4)
          AND ($5 IS NULL OR nd.event_type = $5)
        ORDER BY nd.created_at DESC, nd.id DESC
        LIMIT $6
        "#,
    )
    .bind(site_id)
    .bind(q.cursor_created_at)
    .bind(q.cursor_id)
    .bind(q.status)
    .bind(q.event_type)
    .bind(q.limit)
    .fetch_all(pool)
    .await?;

    Ok(deliveries)
}
