use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub struct NotificationChannelParams<'a> {
    pub channel_type: NotificationChannelType,
    pub name: &'a str,
    pub destination: &'a str,
    pub webhook_secret_ciphertext: Option<&'a str>,
    pub notify_on_failure: bool,
    pub notify_on_recovery: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "notification_channel_type", rename_all = "snake_case")]
pub enum NotificationChannelType {
    Webhook,
    Email,
    Slack,
    Discord,
}

#[derive(Debug, sqlx::FromRow)]
pub struct NotificationChannel {
    pub id: i64,
    pub channel_type: NotificationChannelType,
    pub name: String,
    pub destination: String,
    pub webhook_secret_ciphertext: Option<String>,
    pub notify_on_failure: bool,
    pub notify_on_recovery: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct EffectiveNotificationChannel {
    pub id: i64,
    pub channel_type: NotificationChannelType,
    pub name: String,
    pub destination: String,
    pub webhook_secret_ciphertext: Option<String>,
    pub default_notify_on_failure: bool,
    pub default_notify_on_recovery: bool,
    pub default_is_active: bool,
    pub notify_on_failure: bool,
    pub notify_on_recovery: bool,
    pub is_active: bool,
    pub override_id: Option<i64>,
    pub override_notify_on_failure: Option<bool>,
    pub override_notify_on_recovery: Option<bool>,
    pub override_is_active: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_channel_type_serializes_to_snake_case() {
        assert_eq!(
            serde_json::to_string(&NotificationChannelType::Webhook).unwrap(),
            "\"webhook\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationChannelType::Email).unwrap(),
            "\"email\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationChannelType::Slack).unwrap(),
            "\"slack\""
        );
        assert_eq!(
            serde_json::to_string(&NotificationChannelType::Discord).unwrap(),
            "\"discord\""
        );
    }

    #[test]
    fn notification_channel_type_deserializes_from_snake_case() {
        assert_eq!(
            serde_json::from_str::<NotificationChannelType>("\"webhook\"").unwrap(),
            NotificationChannelType::Webhook
        );
        assert_eq!(
            serde_json::from_str::<NotificationChannelType>("\"email\"").unwrap(),
            NotificationChannelType::Email
        );
        assert_eq!(
            serde_json::from_str::<NotificationChannelType>("\"slack\"").unwrap(),
            NotificationChannelType::Slack
        );
        assert_eq!(
            serde_json::from_str::<NotificationChannelType>("\"discord\"").unwrap(),
            NotificationChannelType::Discord
        );
    }

    #[test]
    fn notification_channel_type_rejects_unknown_variant() {
        assert!(serde_json::from_str::<NotificationChannelType>("\"teams\"").is_err());
        assert!(serde_json::from_str::<NotificationChannelType>("\"Webhook\"").is_err());
    }

    #[test]
    fn notification_channel_type_serde_roundtrip() {
        for variant in [
            NotificationChannelType::Webhook,
            NotificationChannelType::Email,
            NotificationChannelType::Slack,
            NotificationChannelType::Discord,
        ] {
            let serialized = serde_json::to_string(&variant).unwrap();
            let deserialized: NotificationChannelType = serde_json::from_str(&serialized).unwrap();
            assert_eq!(deserialized, variant);
        }
    }
}
