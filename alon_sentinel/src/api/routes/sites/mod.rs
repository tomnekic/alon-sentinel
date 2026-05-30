use std::{collections::HashMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    routing::{get, patch, post},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    api::{
        error::ApiError,
        extractors::AuthenticatedRequest,
        pagination::{json_with_next_cursor, paginate_vec_with_cursor},
        permissions::PermissionKey,
        state::AppState,
    },
    auth::AuthService,
    domain::{
        notification_channels::{self, NotificationChannelType},
        notification_deliveries,
        site_monitor_incidents::{self, SiteMonitorIncidentResolvedReason},
        site_monitors, site_notification_channel_overrides, site_status_pages, sites,
    },
};

mod checks;
mod incidents;
mod monitors;
mod uptime;

pub(crate) use incidents::SiteIncidentResponse;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/sites", get(list_sites).post(create_site))
        .route(
            "/v1/sites/{site_id}",
            patch(update_site).delete(delete_site),
        )
        .route("/v1/sites/{site_id}/summary", get(checks::get_site_summary))
        .route("/v1/sites/{site_id}/uptime", get(uptime::get_site_uptime))
        .route(
            "/v1/sites/{site_id}/uptime/daily",
            get(uptime::get_site_uptime_daily),
        )
        .route("/v1/sites/{site_id}/checks", get(checks::list_site_checks))
        .route(
            "/v1/sites/{site_id}/incidents",
            get(incidents::list_site_incidents),
        )
        .route(
            "/v1/sites/{site_id}/notifications/deliveries",
            get(list_site_notification_deliveries),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/http",
            get(monitors::http::get_http_site_monitor)
                .put(monitors::http::upsert_http_site_monitor)
                .delete(monitors::http::disable_http_site_monitor),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/http/{monitor_id}",
            patch(monitors::http::update_http_site_monitor)
                .delete(monitors::http::disable_http_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/http/{monitor_id}/pause",
            post(monitors::http::pause_http_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/http/{monitor_id}/resume",
            post(monitors::http::resume_http_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/ssl",
            get(monitors::ssl::get_ssl_site_monitor)
                .put(monitors::ssl::upsert_ssl_site_monitor)
                .delete(monitors::ssl::disable_ssl_site_monitor),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/ssl/{monitor_id}",
            patch(monitors::ssl::update_ssl_site_monitor)
                .delete(monitors::ssl::disable_ssl_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/ssl/{monitor_id}/pause",
            post(monitors::ssl::pause_ssl_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/ssl/{monitor_id}/resume",
            post(monitors::ssl::resume_ssl_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/heartbeat",
            get(monitors::heartbeat::get_heartbeat_site_monitor)
                .put(monitors::heartbeat::upsert_heartbeat_site_monitor)
                .delete(monitors::heartbeat::disable_heartbeat_site_monitor),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/heartbeat/{monitor_id}",
            patch(monitors::heartbeat::update_heartbeat_site_monitor)
                .delete(monitors::heartbeat::disable_heartbeat_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/heartbeat/{monitor_id}/pause",
            post(monitors::heartbeat::pause_heartbeat_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/heartbeat/{monitor_id}/resume",
            post(monitors::heartbeat::resume_heartbeat_site_monitor_by_id),
        )
        .route(
            "/v1/heartbeat/{heartbeat_token}",
            post(monitors::heartbeat::record_heartbeat_ping),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/tcp",
            get(monitors::tcp::get_tcp_site_monitor)
                .put(monitors::tcp::upsert_tcp_site_monitor)
                .delete(monitors::tcp::disable_tcp_site_monitor),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/tcp/{monitor_id}",
            patch(monitors::tcp::update_tcp_site_monitor)
                .delete(monitors::tcp::disable_tcp_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/tcp/{monitor_id}/pause",
            post(monitors::tcp::pause_tcp_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/tcp/{monitor_id}/resume",
            post(monitors::tcp::resume_tcp_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/dns",
            get(monitors::dns::get_dns_site_monitor)
                .put(monitors::dns::upsert_dns_site_monitor)
                .delete(monitors::dns::disable_dns_site_monitor),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/dns/{monitor_id}",
            patch(monitors::dns::update_dns_site_monitor)
                .delete(monitors::dns::disable_dns_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/dns/{monitor_id}/pause",
            post(monitors::dns::pause_dns_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/monitoring/dns/{monitor_id}/resume",
            post(monitors::dns::resume_dns_site_monitor_by_id),
        )
        .route(
            "/v1/sites/{site_id}/notifications/channels",
            get(list_site_notification_channels),
        )
        .route(
            "/v1/sites/{site_id}/notifications/channels/{channel_id}",
            patch(upsert_site_notification_channel_override)
                .delete(delete_site_notification_channel_override),
        )
        .route(
            "/v1/sites/{site_id}/status-page",
            get(get_status_page_config).put(upsert_status_page_config),
        )
}

#[derive(Serialize)]
pub(super) struct SiteResponse {
    id: i64,
    name: String,
    base_url: String,
    is_active: bool,
    has_http_monitor: bool,
    http_monitor_status: &'static str,
    has_open_incident: bool,
    current_state: &'static str,
    created_at: String,
    updated_at: String,
}

impl SiteResponse {
    pub(super) fn from_site(
        site: sites::Site,
        http_monitor_status: &'static str,
        has_open_incident: bool,
        current_state: &'static str,
    ) -> Self {
        Self {
            id: site.id,
            name: site.name,
            base_url: site.base_url,
            is_active: site.is_active,
            has_http_monitor: http_monitor_status != "not_configured",
            http_monitor_status,
            has_open_incident,
            current_state,
            created_at: site.created_at.to_rfc3339(),
            updated_at: site.updated_at.to_rfc3339(),
        }
    }
}

fn site_current_state(health: Option<&site_monitors::SiteHealthState>) -> &'static str {
    match health {
        None => "not_configured",
        Some(h) => {
            if !h.has_active_monitor {
                "disabled"
            } else if h.any_failing == Some(true) {
                "failing"
            } else if h.any_succeeding == Some(true) {
                "healthy"
            } else {
                "pending_first_check"
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct SitesQuery {
    q: Option<String>,
    limit: Option<usize>,
    cursor: Option<String>,
}

async fn list_sites(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Query(query): Query<SitesQuery>,
) -> Result<Response, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let limit = sites_limit(query.limit)?;
    let cursor_id = parse_sites_cursor(query.cursor.as_deref())?;
    let search_query = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let (sites, monitor_states, health_states, incident_site_ids) = tokio::try_join!(
        sites::repository::list_sites(&state.pool, search_query, limit + 1, cursor_id),
        site_monitors::repository::list_http_monitor_states(&state.pool),
        site_monitors::repository::list_site_health_states(&state.pool),
        site_monitor_incidents::repository::get_site_ids_with_open_incidents(&state.pool),
    )
    .map_err(ApiError::internal_error)?;

    let monitor_states: HashMap<i64, bool> = monitor_states
        .into_iter()
        .map(|state| (state.site_id, state.has_active_monitor))
        .collect();
    let health_states: HashMap<i64, site_monitors::SiteHealthState> =
        health_states.into_iter().map(|h| (h.site_id, h)).collect();
    let incident_site_ids: std::collections::HashSet<i64> = incident_site_ids.into_iter().collect();

    let payload: Vec<SiteResponse> = sites
        .into_iter()
        .map(|site| {
            let site_id = site.id;
            let http_monitor_status = match monitor_states.get(&site_id).copied() {
                Some(true) => "active",
                Some(false) => "disabled",
                None => "not_configured",
            };
            let has_open_incident = incident_site_ids.contains(&site_id);
            let current_state = site_current_state(health_states.get(&site_id));

            SiteResponse::from_site(site, http_monitor_status, has_open_incident, current_state)
        })
        .collect();
    let (payload, next_cursor) =
        paginate_vec_with_cursor(payload, limit as usize, |site| site.id.to_string());

    Ok(json_with_next_cursor(payload, next_cursor))
}

#[derive(Deserialize)]
struct CreateSiteRequest {
    name: String,
    base_url: String,
}

async fn create_site(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Json(payload): Json<CreateSiteRequest>,
) -> Result<(StatusCode, Json<SiteResponse>), ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesCreate)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    if payload.name.trim().is_empty() || payload.base_url.trim().is_empty() {
        return Err(ApiError::bad_request("name and base_url are required"));
    }

    let site =
        sites::repository::create_site(&state.pool, payload.name.trim(), payload.base_url.trim())
            .await
            .map_err(|err| match err.downcast_ref::<sqlx::Error>() {
                Some(sqlx::Error::Database(db_err))
                    if db_err.code().as_deref() == Some("23505") =>
                {
                    ApiError::conflict("a site with this base_url already exists")
                }
                _ => ApiError::internal_error(err),
            })?;

    Ok((
        StatusCode::CREATED,
        Json(SiteResponse::from_site(
            site,
            "not_configured",
            false,
            "not_configured",
        )),
    ))
}

#[derive(Deserialize)]
struct UpdateSiteRequest {
    name: String,
    base_url: String,
    is_active: bool,
}

async fn update_site(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(payload): Json<UpdateSiteRequest>,
) -> Result<Json<SiteResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesUpdate)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    if payload.name.trim().is_empty() || payload.base_url.trim().is_empty() {
        return Err(ApiError::bad_request("name and base_url are required"));
    }

    let Some(site) = sites::repository::update_site(
        &state.pool,
        site_id,
        payload.name.trim(),
        payload.base_url.trim(),
        payload.is_active,
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("site not found"));
    };

    if !payload.is_active {
        site_monitor_incidents::repository::resolve_open_incidents_for_site(
            &state.pool,
            site_id,
            SiteMonitorIncidentResolvedReason::SiteDeactivated,
        )
        .await
        .map_err(ApiError::internal_error)?;
    }

    let (http_monitors, health_state) = tokio::try_join!(
        site_monitors::repository::list_http_monitors_by_site_id(&state.pool, site_id),
        site_monitors::repository::get_site_health_state_by_id(&state.pool, site_id),
    )
    .map_err(ApiError::internal_error)?;
    let http_monitor_status = http_monitor_status(&http_monitors);
    let current_state = site_current_state(health_state.as_ref());

    Ok(Json(SiteResponse::from_site(
        site,
        http_monitor_status,
        false,
        current_state,
    )))
}

#[derive(Serialize)]
struct DeleteSiteResponse {
    deleted: bool,
}

async fn delete_site(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<DeleteSiteResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesDelete)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let deleted = sites::repository::delete_site(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !deleted {
        return Err(ApiError::not_found("site not found"));
    }

    Ok(Json(DeleteSiteResponse { deleted }))
}

#[derive(Debug, Deserialize)]
struct SiteNotificationDeliveriesQuery {
    limit: Option<usize>,
    cursor: Option<String>,
    status: Option<notification_deliveries::NotificationDeliveryStatus>,
    event_type: Option<notification_deliveries::NotificationEventType>,
}

#[derive(Serialize)]
struct SiteNotificationDeliveryResponse {
    id: i64,
    notification_channel_id: i64,
    site_monitor_id: i64,
    site_monitor_check_id: i64,
    incident_id: Option<i64>,
    event_type: notification_deliveries::NotificationEventType,
    payload: serde_json::Value,
    status: notification_deliveries::NotificationDeliveryStatus,
    attempts: i32,
    next_attempt_at: Option<String>,
    claimed_at: Option<String>,
    lease_until: Option<String>,
    claimed_by: Option<String>,
    delivered_at: Option<String>,
    last_error: Option<String>,
    created_at: String,
    updated_at: String,
    channel_type: NotificationChannelType,
    channel_name: String,
    destination: String,
}

impl From<notification_deliveries::SiteNotificationDelivery> for SiteNotificationDeliveryResponse {
    fn from(delivery: notification_deliveries::SiteNotificationDelivery) -> Self {
        Self {
            id: delivery.id,
            notification_channel_id: delivery.notification_channel_id,
            site_monitor_id: delivery.site_monitor_id,
            site_monitor_check_id: delivery.site_monitor_check_id,
            incident_id: delivery.incident_id,
            event_type: delivery.event_type,
            payload: delivery.payload,
            status: delivery.status,
            attempts: delivery.attempts,
            next_attempt_at: delivery.next_attempt_at.map(|value| value.to_rfc3339()),
            claimed_at: delivery.claimed_at.map(|value| value.to_rfc3339()),
            lease_until: delivery.lease_until.map(|value| value.to_rfc3339()),
            claimed_by: delivery.claimed_by,
            delivered_at: delivery.delivered_at.map(|value| value.to_rfc3339()),
            last_error: delivery.last_error,
            created_at: delivery.created_at.to_rfc3339(),
            updated_at: delivery.updated_at.to_rfc3339(),
            channel_type: delivery.channel_type,
            channel_name: delivery.channel_name,
            destination: delivery.destination,
        }
    }
}

async fn list_site_notification_deliveries(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Query(query): Query<SiteNotificationDeliveriesQuery>,
) -> Result<Response, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::NotificationDeliveriesRead,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    ensure_site_exists(&state.pool, site_id).await?;
    let limit = history_limit(query.limit)? as usize;
    let cursor = parse_cursor(query.cursor.as_deref())?;
    let deliveries = notification_deliveries::repository::list_by_site_id(
        &state.pool,
        site_id,
        &notification_deliveries::DeliveryCursorQuery {
            cursor_created_at: cursor.as_ref().map(|cursor| cursor.timestamp),
            cursor_id: cursor.as_ref().map(|cursor| cursor.id),
            status: query.status,
            event_type: query.event_type,
            limit: page_fetch_limit(limit),
        },
    )
    .await
    .map_err(ApiError::internal_error)?;
    let (deliveries, next_cursor) = paginate_vec_with_cursor(deliveries, limit, |delivery| {
        encode_cursor(delivery.created_at, delivery.id)
    });

    Ok(json_with_next_cursor(
        deliveries
            .into_iter()
            .map(SiteNotificationDeliveryResponse::from)
            .collect::<Vec<_>>(),
        next_cursor,
    ))
}

#[derive(Serialize)]
struct SiteNotificationChannelResponse {
    id: i64,
    channel_type: NotificationChannelType,
    name: String,
    destination: String,
    default_notify_on_failure: bool,
    default_notify_on_recovery: bool,
    default_is_active: bool,
    effective_notify_on_failure: bool,
    effective_notify_on_recovery: bool,
    effective_is_active: bool,
    override_id: Option<i64>,
    override_notify_on_failure: Option<bool>,
    override_notify_on_recovery: Option<bool>,
    override_is_active: Option<bool>,
}

impl From<notification_channels::EffectiveNotificationChannel> for SiteNotificationChannelResponse {
    fn from(channel: notification_channels::EffectiveNotificationChannel) -> Self {
        Self {
            id: channel.id,
            channel_type: channel.channel_type,
            name: channel.name,
            destination: channel.destination,
            default_notify_on_failure: channel.default_notify_on_failure,
            default_notify_on_recovery: channel.default_notify_on_recovery,
            default_is_active: channel.default_is_active,
            effective_notify_on_failure: channel.notify_on_failure,
            effective_notify_on_recovery: channel.notify_on_recovery,
            effective_is_active: channel.is_active,
            override_id: channel.override_id,
            override_notify_on_failure: channel.override_notify_on_failure,
            override_notify_on_recovery: channel.override_notify_on_recovery,
            override_is_active: channel.override_is_active,
        }
    }
}

async fn list_site_notification_channels(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<Vec<SiteNotificationChannelResponse>>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteNotificationChannelOverridesRead,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let site_exists = sites::repository::get_site_by_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !site_exists {
        return Err(ApiError::not_found("site not found"));
    }

    let channels =
        notification_channels::repository::list_effective_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?;

    Ok(Json(
        channels
            .into_iter()
            .map(SiteNotificationChannelResponse::from)
            .collect(),
    ))
}

#[derive(Deserialize)]
struct UpsertSiteNotificationChannelOverrideRequest {
    notify_on_failure: Option<bool>,
    notify_on_recovery: Option<bool>,
    is_active: Option<bool>,
}

async fn upsert_site_notification_channel_override(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, channel_id)): Path<(i64, i64)>,
    Json(payload): Json<UpsertSiteNotificationChannelOverrideRequest>,
) -> Result<Json<SiteNotificationChannelResponse>, ApiError> {
    validate_site_notification_channel_override_payload(&payload)?;

    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteNotificationChannelOverridesCreate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteNotificationChannelOverridesUpdate,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let Some(_) = site_notification_channel_overrides::repository::upsert_for_site(
        &state.pool,
        site_id,
        &site_notification_channel_overrides::ChannelOverrideParams {
            notification_channel_id: channel_id,
            notify_on_failure: payload.notify_on_failure,
            notify_on_recovery: payload.notify_on_recovery,
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found(
            "site or notification channel not found",
        ));
    };

    let channel =
        notification_channels::repository::list_effective_by_site_id(&state.pool, site_id)
            .await
            .map_err(ApiError::internal_error)?
            .into_iter()
            .find(|channel| channel.id == channel_id)
            .ok_or_else(|| ApiError::not_found("site or notification channel not found"))?;

    Ok(Json(SiteNotificationChannelResponse::from(channel)))
}

#[derive(Serialize)]
struct DeleteSiteNotificationChannelOverrideResponse {
    deleted: bool,
}

async fn delete_site_notification_channel_override(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path((site_id, channel_id)): Path<(i64, i64)>,
) -> Result<Json<DeleteSiteNotificationChannelOverrideResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.permissions,
        PermissionKey::SiteNotificationChannelOverridesDelete,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let deleted = site_notification_channel_overrides::repository::delete_for_site(
        &state.pool,
        site_id,
        channel_id,
    )
    .await
    .map_err(ApiError::internal_error)?
    .is_some();

    if !deleted {
        return Err(ApiError::not_found(
            "notification channel override not found",
        ));
    }

    Ok(Json(DeleteSiteNotificationChannelOverrideResponse {
        deleted,
    }))
}

fn validate_site_notification_channel_override_payload(
    payload: &UpsertSiteNotificationChannelOverrideRequest,
) -> Result<(), ApiError> {
    if payload.notify_on_failure.is_none()
        && payload.notify_on_recovery.is_none()
        && payload.is_active.is_none()
    {
        return Err(ApiError::bad_request(
            "at least one override field must be provided",
        ));
    }

    Ok(())
}

#[derive(Serialize)]
struct StatusPageConfigResponse {
    site_id: i64,
    is_enabled: bool,
    slug: String,
    page_title: Option<String>,
    show_monitor_details: bool,
    show_uptime_percentages: bool,
}

impl From<site_status_pages::SiteStatusPage> for StatusPageConfigResponse {
    fn from(p: site_status_pages::SiteStatusPage) -> Self {
        Self {
            site_id: p.site_id,
            is_enabled: p.is_enabled,
            slug: p.slug,
            page_title: p.page_title,
            show_monitor_details: p.show_monitor_details,
            show_uptime_percentages: p.show_uptime_percentages,
        }
    }
}

async fn get_status_page_config(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
) -> Result<Json<StatusPageConfigResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let site = ensure_site_exists(&state.pool, site_id).await?;

    let config = site_status_pages::repository::get_by_site_id(&state.pool, site_id)
        .await
        .map_err(ApiError::internal_error)?
        .unwrap_or_else(|| site_status_pages::SiteStatusPage {
            id: 0,
            site_id,
            is_enabled: false,
            slug: slugify(&site.name),
            page_title: None,
            show_monitor_details: true,
            show_uptime_percentages: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        });

    Ok(Json(StatusPageConfigResponse::from(config)))
}

#[derive(Deserialize)]
struct UpsertStatusPageConfigBody {
    is_enabled: bool,
    slug: String,
    page_title: Option<String>,
    show_monitor_details: bool,
    show_uptime_percentages: bool,
}

async fn upsert_status_page_config(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedRequest,
    Path(site_id): Path<i64>,
    Json(body): Json<UpsertStatusPageConfigBody>,
) -> Result<Json<StatusPageConfigResponse>, ApiError> {
    AuthService::require_permission(&authenticated.permissions, PermissionKey::SitesUpdate)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    ensure_site_exists(&state.pool, site_id).await?;

    let slug = body.slug.trim().to_string();
    validate_slug(&slug)?;

    let page_title = body
        .page_title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let config = site_status_pages::repository::upsert(
        &state.pool,
        site_status_pages::repository::UpsertPayload {
            site_id,
            is_enabled: body.is_enabled,
            slug: &slug,
            page_title,
            show_monitor_details: body.show_monitor_details,
            show_uptime_percentages: body.show_uptime_percentages,
        },
    )
    .await
    .map_err(|err| {
        let msg = err.to_string();
        if msg.contains("idx_site_status_pages_slug") || msg.contains("unique") {
            ApiError::conflict("slug already in use")
        } else {
            ApiError::internal_error(err)
        }
    })?;

    Ok(Json(StatusPageConfigResponse::from(config)))
}

fn validate_slug(slug: &str) -> Result<(), ApiError> {
    if slug.len() < 3 || slug.len() > 60 {
        return Err(ApiError::bad_request(
            "slug must be between 3 and 60 characters",
        ));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ApiError::bad_request(
            "slug may only contain lowercase letters, digits, and hyphens",
        ));
    }
    Ok(())
}

pub(crate) fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(super) async fn ensure_site_exists(
    pool: &PgPool,
    site_id: i64,
) -> Result<sites::Site, ApiError> {
    let site = sites::repository::get_site_by_id(pool, site_id)
        .await
        .map_err(ApiError::internal_error)?;

    site.ok_or_else(|| ApiError::not_found("site not found"))
}

pub(crate) fn history_limit(limit: Option<usize>) -> Result<i64, ApiError> {
    const DEFAULT_LIMIT: usize = 50;
    const MAX_LIMIT: usize = 200;

    bounded_limit(limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")
}

pub(crate) fn checks_limit(limit: Option<usize>) -> Result<i64, ApiError> {
    const DEFAULT_LIMIT: usize = 500;
    const MAX_LIMIT: usize = 2000;

    bounded_limit(limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")
}

fn sites_limit(limit: Option<usize>) -> Result<i64, ApiError> {
    const DEFAULT_LIMIT: usize = 10;
    const MAX_LIMIT: usize = 100;

    bounded_limit(limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")
}

fn parse_sites_cursor(cursor: Option<&str>) -> Result<Option<i64>, ApiError> {
    let Some(cursor) = cursor.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    cursor
        .parse::<i64>()
        .map(Some)
        .map_err(|_| ApiError::bad_request("cursor must be a valid site id"))
}

pub(super) fn bounded_limit(
    value: Option<usize>,
    default: usize,
    max: usize,
    field_name: &str,
) -> Result<i64, ApiError> {
    let limit = value.unwrap_or(default);

    if limit == 0 {
        return Err(ApiError::bad_request(format!(
            "{field_name} must be greater than 0"
        )));
    }

    if limit > max {
        return Err(ApiError::bad_request(format!(
            "{field_name} must be less than or equal to {max}"
        )));
    }

    Ok(limit as i64)
}

pub(crate) fn page_fetch_limit(limit: usize) -> i64 {
    (limit.saturating_add(1)) as i64
}

pub(crate) fn parse_cursor(cursor: Option<&str>) -> Result<Option<CursorToken>, ApiError> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };

    let (timestamp, id) = cursor
        .split_once(',')
        .ok_or_else(|| ApiError::bad_request("cursor must be in '<timestamp>,<id>' format"))?;
    let timestamp = DateTime::parse_from_rfc3339(timestamp)
        .map_err(|_| ApiError::bad_request("cursor timestamp must be RFC 3339"))?
        .with_timezone(&Utc);
    let id = id
        .parse::<i64>()
        .map_err(|_| ApiError::bad_request("cursor id must be an integer"))?;

    Ok(Some(CursorToken { timestamp, id }))
}

pub(crate) fn encode_cursor(timestamp: DateTime<Utc>, id: i64) -> String {
    format!("{},{}", timestamp.to_rfc3339(), id)
}

#[derive(Debug, Clone)]
pub(crate) struct CursorToken {
    pub(crate) timestamp: DateTime<Utc>,
    pub(crate) id: i64,
}

pub(super) fn http_monitor_status(http_monitors: &[site_monitors::SiteMonitor]) -> &'static str {
    if http_monitors.iter().any(|monitor| monitor.is_active) {
        "active"
    } else if http_monitors.is_empty() {
        "not_configured"
    } else {
        "disabled"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::site_monitors::{SiteMonitor, SiteMonitorType};

    #[test]
    fn http_monitor_status_aggregates_multiple_http_monitors() {
        let empty_monitors = Vec::new();
        assert_eq!(http_monitor_status(&empty_monitors), "not_configured");

        let disabled_monitors = vec![
            build_monitor(1, SiteMonitorType::Http, false, None),
            build_monitor(2, SiteMonitorType::Http, false, Some(false)),
        ];
        assert_eq!(http_monitor_status(&disabled_monitors), "disabled");

        let active_monitors = vec![
            build_monitor(1, SiteMonitorType::Http, false, None),
            build_monitor(2, SiteMonitorType::Http, true, None),
        ];
        assert_eq!(http_monitor_status(&active_monitors), "active");
    }

    fn build_monitor(
        id: i64,
        monitor_type: SiteMonitorType,
        is_active: bool,
        last_is_success: Option<bool>,
    ) -> SiteMonitor {
        let timestamp = DateTime::parse_from_rfc3339("2026-04-24T10:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        SiteMonitor {
            id,
            site_id: 10,
            monitor_type,
            target_url: format!("https://test.com/{id}"),
            check_interval_seconds: 60,
            expected_status_code: 200,
            body_must_contain: None,
            body_must_not_contain: None,
            body_must_contain_texts: None,
            body_must_not_contain_texts: None,
            json_path_exists: None,
            json_path_equals: None,
            json_path_not_equals: None,
            max_response_time_ms: None,
            required_header_name: None,
            required_header_value: None,
            header_assertions: None,
            ssl_certificate_checks_enabled: false,
            ssl_expiry_warning_days: None,
            tcp_target_host: None,
            tcp_target_port: None,
            dns_hostname: None,
            dns_record_type: None,
            dns_expected_value: None,
            dns_nameserver: None,
            heartbeat_token: None,
            heartbeat_grace_seconds: None,
            http_check_timeout_seconds_override: None,
            http_check_max_attempts_override: None,
            http_check_retry_delays_ms_override: None,
            is_active,
            check_claimed_at: None,
            check_lease_until: None,
            check_claimed_by: None,
            last_checked_at: last_is_success.map(|_| timestamp),
            last_successful_check_at: if last_is_success == Some(true) {
                Some(timestamp)
            } else {
                None
            },
            last_is_success,
            last_status_code: match last_is_success {
                Some(true) => Some(200),
                Some(false) => Some(500),
                None => None,
            },
            last_response_time_ms: last_is_success.map(|_| 123),
            last_failure_reason: (last_is_success == Some(false)).then(|| "timeout".to_string()),
            last_error_message: (last_is_success == Some(false)).then(|| "failed".to_string()),
            last_heartbeat_received_at: None,
            last_certificate_expires_at: None,
            last_certificate_days_remaining: None,
            last_certificate_issuer: None,
            last_certificate_subject: None,
            last_certificate_domain: None,
            created_at: timestamp,
            updated_at: timestamp,
        }
    }
}
