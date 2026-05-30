use chrono::{DateTime, Utc};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Permission {
    pub id: i64,
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RolePermission {
    pub id: i64,
    pub role_id: i64,
    pub permission_id: i64,
    pub created_at: DateTime<Utc>,
}
