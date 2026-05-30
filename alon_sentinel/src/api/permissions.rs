use std::{
    collections::BTreeSet,
    fmt::{self, Display},
    str::FromStr,
};

use anyhow::{Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PermissionKey {
    SitesCreate,
    SitesRead,
    SitesUpdate,
    SitesDelete,
    SiteMonitorsCreate,
    SiteMonitorsRead,
    SiteMonitorsUpdate,
    SiteMonitorsDelete,
    SiteChecksRead,
    SiteIncidentsRead,
    NotificationDeliveriesRead,
    NotificationChannelsCreate,
    NotificationChannelsRead,
    NotificationChannelsUpdate,
    NotificationChannelsDelete,
    SiteNotificationChannelOverridesCreate,
    SiteNotificationChannelOverridesRead,
    SiteNotificationChannelOverridesUpdate,
    SiteNotificationChannelOverridesDelete,
    IncidentsWrite,
    UsersRead,
    UsersWrite,
    RolesRead,
    RolesWrite,
    ApiClientsRead,
    ApiClientsWrite,
}

impl PermissionKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SitesCreate => "sites.create",
            Self::SitesRead => "sites.read",
            Self::SitesUpdate => "sites.update",
            Self::SitesDelete => "sites.delete",
            Self::SiteMonitorsCreate => "site_monitors.create",
            Self::SiteMonitorsRead => "site_monitors.read",
            Self::SiteMonitorsUpdate => "site_monitors.update",
            Self::SiteMonitorsDelete => "site_monitors.delete",
            Self::SiteChecksRead => "site_checks.read",
            Self::SiteIncidentsRead => "site_incidents.read",
            Self::NotificationDeliveriesRead => "notification_deliveries.read",
            Self::NotificationChannelsCreate => "notification_channels.create",
            Self::NotificationChannelsRead => "notification_channels.read",
            Self::NotificationChannelsUpdate => "notification_channels.update",
            Self::NotificationChannelsDelete => "notification_channels.delete",
            Self::SiteNotificationChannelOverridesCreate => {
                "site_notification_channel_overrides.create"
            }
            Self::SiteNotificationChannelOverridesRead => {
                "site_notification_channel_overrides.read"
            }
            Self::SiteNotificationChannelOverridesUpdate => {
                "site_notification_channel_overrides.update"
            }
            Self::SiteNotificationChannelOverridesDelete => {
                "site_notification_channel_overrides.delete"
            }
            Self::IncidentsWrite => "incidents.write",
            Self::UsersRead => "users.read",
            Self::UsersWrite => "users.write",
            Self::RolesRead => "roles.read",
            Self::RolesWrite => "roles.write",
            Self::ApiClientsRead => "api_clients.read",
            Self::ApiClientsWrite => "api_clients.write",
        }
    }
}

impl Display for PermissionKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PermissionKey {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "sites.create" => Ok(Self::SitesCreate),
            "sites.read" => Ok(Self::SitesRead),
            "sites.update" => Ok(Self::SitesUpdate),
            "sites.delete" => Ok(Self::SitesDelete),
            "site_monitors.create" => Ok(Self::SiteMonitorsCreate),
            "site_monitors.read" => Ok(Self::SiteMonitorsRead),
            "site_monitors.update" => Ok(Self::SiteMonitorsUpdate),
            "site_monitors.delete" => Ok(Self::SiteMonitorsDelete),
            "site_checks.read" => Ok(Self::SiteChecksRead),
            "site_incidents.read" => Ok(Self::SiteIncidentsRead),
            "notification_deliveries.read" => Ok(Self::NotificationDeliveriesRead),
            "notification_channels.create" => Ok(Self::NotificationChannelsCreate),
            "notification_channels.read" => Ok(Self::NotificationChannelsRead),
            "notification_channels.update" => Ok(Self::NotificationChannelsUpdate),
            "notification_channels.delete" => Ok(Self::NotificationChannelsDelete),
            "site_notification_channel_overrides.create" => {
                Ok(Self::SiteNotificationChannelOverridesCreate)
            }
            "site_notification_channel_overrides.read" => {
                Ok(Self::SiteNotificationChannelOverridesRead)
            }
            "site_notification_channel_overrides.update" => {
                Ok(Self::SiteNotificationChannelOverridesUpdate)
            }
            "site_notification_channel_overrides.delete" => {
                Ok(Self::SiteNotificationChannelOverridesDelete)
            }
            "incidents.write" => Ok(Self::IncidentsWrite),
            "users.read" => Ok(Self::UsersRead),
            "users.write" => Ok(Self::UsersWrite),
            "roles.read" => Ok(Self::RolesRead),
            "roles.write" => Ok(Self::RolesWrite),
            "api_clients.read" => Ok(Self::ApiClientsRead),
            "api_clients.write" => Ok(Self::ApiClientsWrite),
            _ => Err(anyhow!("unknown permission key: {value}")),
        }
    }
}

impl TryFrom<&str> for PermissionKey {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        Self::from_str(value)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    inner: BTreeSet<PermissionKey>,
}

impl PermissionSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_keys(keys: impl IntoIterator<Item = PermissionKey>) -> Self {
        Self {
            inner: keys.into_iter().collect(),
        }
    }

    pub fn from_strs(keys: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut permissions = BTreeSet::new();
        for key in keys {
            permissions.insert(PermissionKey::from_str(&key)?);
        }
        Ok(Self { inner: permissions })
    }

    pub fn contains(&self, permission: PermissionKey) -> bool {
        self.inner.contains(&permission)
    }

    pub fn to_strings(&self) -> Vec<String> {
        self.inner.iter().map(|key| key.to_string()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{PermissionKey, PermissionSet};

    #[test]
    fn permission_key_round_trips_through_string() {
        let key = PermissionKey::SiteMonitorsUpdate;

        assert_eq!(key.as_str(), "site_monitors.update");
        assert_eq!(
            "site_monitors.update"
                .parse::<PermissionKey>()
                .expect("permission key should parse"),
            key
        );
    }

    #[test]
    fn permission_set_deduplicates_and_sorts_strings() {
        let set = PermissionSet::from_keys([
            PermissionKey::UsersWrite,
            PermissionKey::SitesRead,
            PermissionKey::UsersWrite,
        ]);

        assert_eq!(
            set.to_strings(),
            vec!["sites.read".to_string(), "users.write".to_string()]
        );
    }
}
