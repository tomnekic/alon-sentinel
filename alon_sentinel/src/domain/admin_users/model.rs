use chrono::{DateTime, Utc};
use sqlx::types::Uuid;

pub struct AdminUserUpdateParams<'a> {
    pub email: &'a str,
    pub display_name: &'a str,
    pub password_hash: Option<&'a str>,
    pub is_active: bool,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminUser {
    pub id: i64,
    pub uuid: Uuid,
    pub email: String,
    pub display_name: String,
    pub password_hash: String,
    pub is_active: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
