use chrono::{DateTime, Utc};
use sqlx::types::{JsonValue, Uuid};

#[derive(Debug, Clone, Copy, PartialEq, Eq, sqlx::Type)]
#[sqlx(type_name = "api_client_type", rename_all = "snake_case")]
pub enum ApiClientType {
    InternalService,
    InstallationClient,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiClient {
    pub id: i64,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub client_type: ApiClientType,
    pub client_id: String,
    pub client_secret_hash: String,
    pub secret_prefix: String,
    pub is_active: bool,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_by_user_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiClientScope {
    pub id: i64,
    pub api_client_id: i64,
    pub scope: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AccessToken {
    pub id: i64,
    pub api_client_id: i64,
    pub token_hash: String,
    pub token_prefix: String,
    pub expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ApiClientAuditLog {
    pub id: i64,
    pub api_client_id: Option<i64>,
    pub action: String,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub meta_json: Option<JsonValue>,
    pub created_at: DateTime<Utc>,
}
