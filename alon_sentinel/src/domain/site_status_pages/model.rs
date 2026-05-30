use chrono::{DateTime, Utc};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct SiteStatusPage {
    pub id: i64,
    pub site_id: i64,
    pub is_enabled: bool,
    pub slug: String,
    pub page_title: Option<String>,
    pub show_monitor_details: bool,
    pub show_uptime_percentages: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
