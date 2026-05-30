use chrono::{DateTime, Utc};

#[derive(Debug, sqlx::FromRow)]
pub struct Site {
    pub id: i64,
    pub name: String,
    pub base_url: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
