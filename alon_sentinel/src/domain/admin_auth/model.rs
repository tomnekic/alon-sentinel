use chrono::{DateTime, Utc};
use sqlx::types::JsonValue;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminAccessToken {
    pub id: i64,
    pub admin_user_id: i64,
    pub token_hash: String,
    pub token_prefix: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminAuthAuditLog {
    pub id: i64,
    pub admin_user_id: Option<i64>,
    pub action: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub meta_json: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
}
