use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        error::ApiError, extractors::AuthenticatedRequest, permissions::PermissionKey,
        state::AppState,
    },
    auth::AuthService,
    domain::{site_monitor_checks, sites},
};

#[derive(Serialize)]
pub(super) struct DailyUptimeBucketResponse {
    date: String,
    total_checks: i64,
    successful_checks: i64,
    uptime_percent: Option<f64>,
}

#[derive(Serialize)]
pub(super) struct SiteUptimeDailyResponse {
    days: i64,
    buckets: Vec<DailyUptimeBucketResponse>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SiteUptimeDailyQuery {
    days: Option<i64>,
}

pub(super) async fn get_site_uptime_daily(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<SiteUptimeDailyQuery>,
) -> Result<Json<SiteUptimeDailyResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("site not found"))?;

    let days = query.days.unwrap_or(90).clamp(7, 90);

    let buckets =
        site_monitor_checks::repository::get_daily_uptime_stats(&state.pool, site_id, days)
            .await
            .map_err(ApiError::internal_error)?;

    let response_buckets = buckets
        .into_iter()
        .map(|b| {
            let uptime_percent = if b.total_checks > 0 {
                Some((b.successful_checks as f64 / b.total_checks as f64) * 100.0)
            } else {
                None
            };
            DailyUptimeBucketResponse {
                date: b.date,
                total_checks: b.total_checks,
                successful_checks: b.successful_checks,
                uptime_percent,
            }
        })
        .collect();

    Ok(Json(SiteUptimeDailyResponse {
        days,
        buckets: response_buckets,
    }))
}

#[derive(Debug, Deserialize)]
pub(super) struct SiteUptimeQuery {
    window: Option<String>,
}

#[derive(Serialize)]
pub(super) struct SiteUptimeResponse {
    window: &'static str,
    window_days: i64,
    uptime_percent: Option<f64>,
    total_checks: i64,
    successful_checks: i64,
    failed_checks: i64,
}

pub(super) async fn get_site_uptime(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<SiteUptimeQuery>,
) -> Result<Json<SiteUptimeResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("site not found"))?;

    let (window_label, window_days): (&'static str, i64) = match query.window.as_deref() {
        Some("7d") => ("7d", 7),
        _ => ("30d", 30),
    };

    let since = Utc::now() - chrono::Duration::days(window_days);
    let stats = site_monitor_checks::repository::get_site_uptime_stats(&state.pool, site_id, since)
        .await
        .map_err(ApiError::internal_error)?;

    let uptime_percent = if stats.total_checks > 0 {
        Some((stats.successful_checks as f64 / stats.total_checks as f64) * 100.0)
    } else {
        None
    };

    Ok(Json(SiteUptimeResponse {
        window: window_label,
        window_days,
        uptime_percent,
        total_checks: stats.total_checks,
        successful_checks: stats.successful_checks,
        failed_checks: stats.total_checks - stats.successful_checks,
    }))
}
