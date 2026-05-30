use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::notification_channels::NotificationChannelType;

pub struct DeliveryCursorQuery {
    pub cursor_created_at: Option<DateTime<Utc>>,
    pub cursor_id: Option<i64>,
    pub status: Option<NotificationDeliveryStatus>,
    pub event_type: Option<NotificationEventType>,
    pub limit: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "notification_event_type", rename_all = "snake_case")]
pub enum NotificationEventType {
    Failure,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "notification_delivery_status", rename_all = "snake_case")]
pub enum NotificationDeliveryStatus {
    Pending,
    Delivered,
    Failed,
}

#[derive(Debug, sqlx::FromRow)]
pub struct NotificationDelivery {
    pub id: i64,
    pub notification_channel_id: i64,
    pub site_monitor_id: i64,
    pub site_monitor_check_id: i64,
    pub incident_id: Option<i64>,
    pub event_type: NotificationEventType,
    pub payload: Value,
    pub status: NotificationDeliveryStatus,
    pub attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub lease_until: Option<DateTime<Utc>>,
    pub claimed_by: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct ClaimedNotificationDelivery {
    pub id: i64,
    pub notification_channel_id: i64,
    pub site_monitor_id: i64,
    pub site_monitor_check_id: i64,
    pub incident_id: Option<i64>,
    pub event_type: NotificationEventType,
    pub payload: Value,
    pub status: NotificationDeliveryStatus,
    pub attempts: i32,
    pub channel_type: NotificationChannelType,
    pub channel_name: String,
    pub destination: String,
    pub webhook_secret_ciphertext: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteNotificationDelivery {
    pub id: i64,
    pub notification_channel_id: i64,
    pub site_monitor_id: i64,
    pub site_monitor_check_id: i64,
    pub incident_id: Option<i64>,
    pub event_type: NotificationEventType,
    pub payload: Value,
    pub status: NotificationDeliveryStatus,
    pub attempts: i32,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub lease_until: Option<DateTime<Utc>>,
    pub claimed_by: Option<String>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub channel_type: NotificationChannelType,
    pub channel_name: String,
    pub destination: String,
}
