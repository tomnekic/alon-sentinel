use chrono::{DateTime, Utc};

pub struct ChannelOverrideParams {
    pub notification_channel_id: i64,
    pub notify_on_failure: Option<bool>,
    pub notify_on_recovery: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct SiteNotificationChannelOverride {
    pub id: i64,
    pub site_id: i64,
    pub notification_channel_id: i64,
    pub notify_on_failure: Option<bool>,
    pub notify_on_recovery: Option<bool>,
    pub is_active: Option<bool>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
