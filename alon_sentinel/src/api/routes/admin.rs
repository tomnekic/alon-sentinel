use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::{
    api::{
        error::ApiError,
        extractors::AuthenticatedAdminRequest,
        pagination::{json_with_next_cursor, paginate_vec_with_cursor},
        permissions::PermissionKey,
        routes::sites::{
            SiteIncidentResponse, encode_cursor, history_limit, page_fetch_limit, parse_cursor,
        },
        state::AppState,
    },
    auth::{AuthService, TOKEN_PREFIX_LEN, generate_raw_token},
    domain::{
        admin_auth, admin_users, api_auth, permissions, roles, site_monitor_incidents, sites,
    },
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/dashboard", get(get_dashboard))
        .route("/v1/incidents", get(list_global_incidents))
        .route(
            "/v1/admin/users",
            get(list_admin_users).post(create_admin_user),
        )
        .route(
            "/v1/admin/users/{user_id}",
            patch(update_admin_user).delete(delete_admin_user),
        )
        .route("/v1/admin/roles", get(list_roles).post(create_role))
        .route(
            "/v1/admin/roles/{role_id}",
            patch(update_role).delete(delete_role),
        )
        .route("/v1/admin/permissions", get(list_permissions))
        .route(
            "/v1/admin/sites/{site_id}/incidents/{incident_id}/acknowledge",
            post(acknowledge_incident),
        )
        .route(
            "/v1/admin/api-clients",
            get(list_api_clients).post(create_api_client),
        )
        .route(
            "/v1/admin/api-clients/{client_id}",
            patch(update_api_client).delete(delete_api_client),
        )
        .route(
            "/v1/admin/api-clients/{client_id}/rotate-secret",
            post(rotate_api_client_secret),
        )
}

#[derive(Serialize)]
struct ManagedAdminUserResponse {
    id: i64,
    email: String,
    display_name: String,
    is_active: bool,
    last_login_at: Option<String>,
    created_at: String,
    updated_at: String,
    roles: Vec<String>,
    permissions: Vec<String>,
}

#[derive(Serialize)]
struct ManagedRoleResponse {
    id: i64,
    key: String,
    name: String,
    description: Option<String>,
    is_system: bool,
    created_at: String,
    updated_at: String,
    permissions: Vec<String>,
}

#[derive(Serialize)]
struct ManagedPermissionResponse {
    id: i64,
    key: String,
    name: String,
    description: Option<String>,
    created_at: String,
    roles: Vec<String>,
}

#[derive(Deserialize)]
struct CreateAdminUserRequest {
    email: String,
    display_name: String,
    password: String,
    is_active: Option<bool>,
    role_keys: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateAdminUserRequest {
    email: String,
    display_name: String,
    password: Option<String>,
    is_active: bool,
    role_keys: Vec<String>,
}

#[derive(Deserialize)]
struct CreateRoleRequest {
    key: String,
    name: String,
    description: Option<String>,
    permission_keys: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateRoleRequest {
    name: String,
    description: Option<String>,
    permission_keys: Vec<String>,
}

async fn list_admin_users(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<Vec<ManagedAdminUserResponse>>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::UsersRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let users = admin_users::repository::list_admin_users(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;
    let user_ids = users.iter().map(|user| user.id).collect::<Vec<_>>();
    let role_keys_by_user_id =
        roles::repository::list_role_keys_for_admin_users(&state.pool, &user_ids)
            .await
            .map_err(ApiError::internal_error)?;
    let permission_keys_by_user_id =
        permissions::repository::list_permission_keys_for_admin_users(&state.pool, &user_ids)
            .await
            .map_err(ApiError::internal_error)?;

    let payload = users
        .into_iter()
        .map(|user| ManagedAdminUserResponse {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            is_active: user.is_active,
            last_login_at: user.last_login_at.map(|value| value.to_rfc3339()),
            created_at: user.created_at.to_rfc3339(),
            updated_at: user.updated_at.to_rfc3339(),
            roles: role_keys_by_user_id
                .get(&user.id)
                .cloned()
                .unwrap_or_default(),
            permissions: permission_keys_by_user_id
                .get(&user.id)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    Ok(Json(payload))
}

async fn create_admin_user(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Json(payload): Json<CreateAdminUserRequest>,
) -> Result<(StatusCode, Json<ManagedAdminUserResponse>), ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::UsersWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    validate_admin_user_payload(
        &payload.email,
        &payload.display_name,
        Some(&payload.password),
    )?;
    let role_ids = resolve_role_ids(&state.pool, &payload.role_keys).await?;
    let password_hash =
        AuthService::hash_password(&payload.password).map_err(ApiError::bad_request_error)?;

    let mut tx = state
        .pool
        .begin()
        .await
        .map_err(|e| ApiError::internal_error(anyhow::Error::from(e)))?;

    let user = admin_users::repository::create_admin_user_in_tx(
        &mut tx,
        admin_users::repository::NewAdminUser {
            email: payload.email.trim(),
            display_name: payload.display_name.trim(),
            password_hash: &password_hash,
        },
    )
    .await
    .map_err(ApiError::internal_error)?;

    roles::repository::replace_roles_for_admin_user_in_tx(&mut tx, user.id, &role_ids)
        .await
        .map_err(ApiError::internal_error)?;

    let user = if payload.is_active == Some(false) {
        let Some(updated) = admin_users::repository::update_admin_user_in_tx(
            &mut tx,
            user.id,
            &admin_users::AdminUserUpdateParams {
                email: payload.email.trim(),
                display_name: payload.display_name.trim(),
                password_hash: None,
                is_active: false,
            },
        )
        .await
        .map_err(ApiError::internal_error)?
        else {
            return Err(ApiError::not_found("admin user not found"));
        };
        updated
    } else {
        user
    };

    tx.commit()
        .await
        .map_err(|e| ApiError::internal_error(anyhow::Error::from(e)))?;

    Ok((
        StatusCode::CREATED,
        Json(build_managed_admin_user_response(&state.pool, user).await?),
    ))
}

async fn update_admin_user(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(user_id): Path<i64>,
    Json(payload): Json<UpdateAdminUserRequest>,
) -> Result<Json<ManagedAdminUserResponse>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::UsersWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    validate_admin_user_payload(
        &payload.email,
        &payload.display_name,
        payload.password.as_deref(),
    )?;
    let role_ids = resolve_role_ids(&state.pool, &payload.role_keys).await?;
    let password_hash = match payload
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(password) => {
            Some(AuthService::hash_password(password).map_err(ApiError::bad_request_error)?)
        }
        None => None,
    };

    let Some(user) = admin_users::repository::update_admin_user(
        &state.pool,
        user_id,
        &admin_users::AdminUserUpdateParams {
            email: payload.email.trim(),
            display_name: payload.display_name.trim(),
            password_hash: password_hash.as_deref(),
            is_active: payload.is_active,
        },
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("admin user not found"));
    };

    roles::repository::replace_roles_for_admin_user(&state.pool, user.id, &role_ids)
        .await
        .map_err(ApiError::internal_error)?;
    invalidate_tokens_for_user(&state, user.id).await?;

    Ok(Json(
        build_managed_admin_user_response(&state.pool, user).await?,
    ))
}

#[derive(Serialize)]
struct DeleteManagedAdminUserResponse {
    deleted: bool,
}

async fn delete_admin_user(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(user_id): Path<i64>,
) -> Result<Json<DeleteManagedAdminUserResponse>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::UsersWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    if user_id == authenticated.auth.user.id {
        return Err(ApiError::bad_request("cannot delete your own account"));
    }

    let active_count = admin_users::repository::count_active_admin_users(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;

    if active_count <= 1 {
        return Err(ApiError::bad_request(
            "cannot delete the last active admin user",
        ));
    }

    invalidate_tokens_for_user(&state, user_id).await?;

    let deleted = admin_users::repository::delete_admin_user(&state.pool, user_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    if !deleted {
        return Err(ApiError::not_found("admin user not found"));
    }

    Ok(Json(DeleteManagedAdminUserResponse { deleted }))
}

async fn list_roles(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<Vec<ManagedRoleResponse>>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::RolesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let roles_list = roles::repository::list_roles(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;
    let role_ids = roles_list.iter().map(|role| role.id).collect::<Vec<_>>();
    let permission_keys_by_role_id =
        permissions::repository::list_permission_keys_for_roles(&state.pool, &role_ids)
            .await
            .map_err(ApiError::internal_error)?;

    let payload = roles_list
        .into_iter()
        .map(|role| ManagedRoleResponse {
            id: role.id,
            key: role.key,
            name: role.name,
            description: role.description,
            is_system: role.is_system,
            created_at: role.created_at.to_rfc3339(),
            updated_at: role.updated_at.to_rfc3339(),
            permissions: permission_keys_by_role_id
                .get(&role.id)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    Ok(Json(payload))
}

async fn create_role(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Json(payload): Json<CreateRoleRequest>,
) -> Result<(StatusCode, Json<ManagedRoleResponse>), ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::RolesWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    validate_role_payload(&payload.key, &payload.name)?;
    let permission_ids = resolve_permission_ids(&state.pool, &payload.permission_keys).await?;
    let role = roles::repository::create_role(
        &state.pool,
        payload.key.trim(),
        payload.name.trim(),
        payload
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
    .await
    .map_err(ApiError::internal_error)?;

    permissions::repository::replace_permissions_for_role(&state.pool, role.id, &permission_ids)
        .await
        .map_err(ApiError::internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(build_managed_role_response(&state.pool, role).await?),
    ))
}

async fn update_role(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(role_id): Path<i64>,
    Json(payload): Json<UpdateRoleRequest>,
) -> Result<Json<ManagedRoleResponse>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::RolesWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    if payload.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    let permission_ids = resolve_permission_ids(&state.pool, &payload.permission_keys).await?;
    let Some(role) = roles::repository::update_role(
        &state.pool,
        role_id,
        payload.name.trim(),
        payload
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("role not found"));
    };

    let affected_user_ids = roles::repository::list_admin_user_ids_for_role(&state.pool, role.id)
        .await
        .map_err(ApiError::internal_error)?;
    permissions::repository::replace_permissions_for_role(&state.pool, role.id, &permission_ids)
        .await
        .map_err(ApiError::internal_error)?;
    for user_id in affected_user_ids {
        invalidate_tokens_for_user(&state, user_id).await?;
    }

    Ok(Json(build_managed_role_response(&state.pool, role).await?))
}

#[derive(Serialize)]
struct DeleteManagedRoleResponse {
    deleted: bool,
}

async fn delete_role(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(role_id): Path<i64>,
) -> Result<Json<DeleteManagedRoleResponse>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::RolesWrite)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let role = roles::repository::get_role_by_id(&state.pool, role_id)
        .await
        .map_err(ApiError::internal_error)?
        .ok_or_else(|| ApiError::not_found("role not found"))?;

    if role.is_system {
        return Err(ApiError::bad_request("system roles can not be deleted"));
    }

    let affected_user_ids = roles::repository::list_admin_user_ids_for_role(&state.pool, role_id)
        .await
        .map_err(ApiError::internal_error)?;

    let deleted = roles::repository::delete_role(&state.pool, role_id)
        .await
        .map_err(ApiError::internal_error)?
        .is_some();

    for user_id in affected_user_ids {
        invalidate_tokens_for_user(&state, user_id).await?;
    }

    Ok(Json(DeleteManagedRoleResponse { deleted }))
}

async fn list_permissions(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<Vec<ManagedPermissionResponse>>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::RolesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let permission_rows = permissions::repository::list_permissions(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;
    let permission_ids = permission_rows
        .iter()
        .map(|permission| permission.id)
        .collect::<Vec<_>>();
    let role_keys_by_permission_id =
        roles::repository::list_role_keys_for_permissions(&state.pool, &permission_ids)
            .await
            .map_err(ApiError::internal_error)?;

    let payload = permission_rows
        .into_iter()
        .map(|permission| ManagedPermissionResponse {
            id: permission.id,
            key: permission.key,
            name: permission.name,
            description: permission.description,
            created_at: permission.created_at.to_rfc3339(),
            roles: role_keys_by_permission_id
                .get(&permission.id)
                .cloned()
                .unwrap_or_default(),
        })
        .collect();

    Ok(Json(payload))
}

async fn invalidate_tokens_for_user(
    state: &Arc<AppState>,
    admin_user_id: i64,
) -> Result<(), ApiError> {
    let tokens =
        admin_auth::repository::list_active_tokens_for_admin_user(&state.pool, admin_user_id)
            .await
            .map_err(ApiError::internal_error)?;
    for token in tokens {
        state
            .auth_token_cache
            .invalidate_hashed_token(&token.token_prefix, &token.token_hash);
    }
    Ok(())
}

async fn build_managed_admin_user_response(
    pool: &PgPool,
    user: admin_users::AdminUser,
) -> Result<ManagedAdminUserResponse, ApiError> {
    let role_keys = roles::repository::list_role_keys_for_admin_user(pool, user.id)
        .await
        .map_err(ApiError::internal_error)?;
    let permission_keys =
        permissions::repository::list_permission_keys_for_admin_user(pool, user.id)
            .await
            .map_err(ApiError::internal_error)?;

    Ok(ManagedAdminUserResponse {
        id: user.id,
        email: user.email,
        display_name: user.display_name,
        is_active: user.is_active,
        last_login_at: user.last_login_at.map(|value| value.to_rfc3339()),
        created_at: user.created_at.to_rfc3339(),
        updated_at: user.updated_at.to_rfc3339(),
        roles: role_keys,
        permissions: permission_keys,
    })
}

async fn build_managed_role_response(
    pool: &PgPool,
    role: roles::Role,
) -> Result<ManagedRoleResponse, ApiError> {
    let permission_keys = permissions::repository::list_permission_keys_for_role(pool, role.id)
        .await
        .map_err(ApiError::internal_error)?;

    Ok(ManagedRoleResponse {
        id: role.id,
        key: role.key,
        name: role.name,
        description: role.description,
        is_system: role.is_system,
        created_at: role.created_at.to_rfc3339(),
        updated_at: role.updated_at.to_rfc3339(),
        permissions: permission_keys,
    })
}

fn validate_admin_user_payload(
    email: &str,
    display_name: &str,
    password: Option<&str>,
) -> Result<(), ApiError> {
    if email.trim().is_empty() {
        return Err(ApiError::bad_request("email is required"));
    }

    if display_name.trim().is_empty() {
        return Err(ApiError::bad_request("display_name is required"));
    }

    if matches!(password, Some(value) if value.trim().is_empty()) {
        return Err(ApiError::bad_request("password can not be empty"));
    }

    Ok(())
}

fn validate_role_payload(key: &str, name: &str) -> Result<(), ApiError> {
    if key.trim().is_empty() {
        return Err(ApiError::bad_request("key is required"));
    }

    if name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    Ok(())
}

async fn resolve_role_ids(pool: &PgPool, role_keys: &[String]) -> Result<Vec<i64>, ApiError> {
    if role_keys.is_empty() {
        return Ok(Vec::new());
    }

    let role_rows = roles::repository::list_roles_by_keys(pool, role_keys)
        .await
        .map_err(ApiError::internal_error)?;
    if role_rows.len() != role_keys.len() {
        return Err(ApiError::bad_request("one or more role_keys are invalid"));
    }

    Ok(role_rows.into_iter().map(|role| role.id).collect())
}

async fn resolve_permission_ids(
    pool: &PgPool,
    permission_keys: &[String],
) -> Result<Vec<i64>, ApiError> {
    if permission_keys.is_empty() {
        return Ok(Vec::new());
    }

    let canonical_permission_keys = permission_keys
        .iter()
        .map(|key| {
            key.parse::<PermissionKey>()
                .map(|permission| permission.to_string())
                .map_err(|_| ApiError::bad_request("one or more permission_keys are invalid"))
        })
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let permission_rows =
        permissions::repository::list_permissions_by_keys(pool, &canonical_permission_keys)
            .await
            .map_err(ApiError::internal_error)?;
    if permission_rows.len() != canonical_permission_keys.len() {
        return Err(ApiError::internal(
            "one or more built-in permission records are missing",
        ));
    }

    Ok(permission_rows
        .into_iter()
        .map(|permission| permission.id)
        .collect())
}

#[derive(Serialize)]
struct DashboardSummary {
    sites_total: i64,
    sites_active: i64,
    sites_up: i64,
    sites_down: i64,
    sites_paused: i64,
    sites_unchecked: i64,
    sites_with_monitor: i64,
    sites_without_monitor: i64,
    open_incidents: i64,
    monitors_active: i64,
}

#[derive(Serialize)]
struct DashboardSiteEntry {
    id: i64,
    name: String,
    base_url: String,
    is_active: bool,
    status: &'static str,
    has_open_incident: bool,
    last_checked_at: Option<String>,
    last_response_time_ms: Option<i32>,
    monitors_active: i64,
    monitors_total: i64,
}

#[derive(Serialize)]
struct DashboardIncidentEntry {
    id: i64,
    site_id: i64,
    site_name: String,
    monitor_type: &'static str,
    target_url: String,
    opened_at: String,
    failure_count: i32,
    last_failure_reason: Option<String>,
    acknowledged_at: Option<String>,
}

#[derive(Serialize)]
struct DashboardResponse {
    summary: DashboardSummary,
    sites: Vec<DashboardSiteEntry>,
    recent_incidents: Vec<DashboardIncidentEntry>,
}

fn dashboard_site_status(row: &sites::repository::DashboardSiteRow) -> &'static str {
    if !row.is_active {
        return "inactive";
    }
    if row.any_failing == Some(true) {
        return "down";
    }
    if row.any_succeeding == Some(true) {
        return "up";
    }
    if row.monitors_active_count == 0 && row.monitors_total > 0 {
        return "paused";
    }
    "unchecked"
}

#[derive(Serialize)]
struct GlobalIncidentResponse {
    id: i64,
    site_id: i64,
    site_name: String,
    site_base_url: String,
    monitor_id: i64,
    monitor_type: &'static str,
    target_url: String,
    expected_status_code: i32,
    status: &'static str,
    opened_at: String,
    resolved_at: Option<String>,
    started_check_id: Option<i64>,
    resolved_check_id: Option<i64>,
    opened_status_code: Option<i32>,
    opened_failure_reason: Option<String>,
    opened_error_message: Option<String>,
    failure_count: i32,
    last_status_code: Option<i32>,
    last_failure_reason: Option<String>,
    last_error_message: Option<String>,
    resolved_reason: Option<String>,
    resolved_status_code: Option<i32>,
    resolved_response_time_ms: Option<i32>,
    downtime_seconds: Option<i32>,
    acknowledged_at: Option<String>,
    acknowledged_by: Option<i64>,
}

impl From<site_monitor_incidents::SiteMonitorIncidentWithSite> for GlobalIncidentResponse {
    fn from(i: site_monitor_incidents::SiteMonitorIncidentWithSite) -> Self {
        Self {
            id: i.id,
            site_id: i.site_id,
            site_name: i.site_name,
            site_base_url: i.site_base_url,
            monitor_id: i.site_monitor_id,
            monitor_type: i.monitor_type.as_str(),
            target_url: i.target_url,
            expected_status_code: i.expected_status_code,
            status: match i.status {
                site_monitor_incidents::SiteMonitorIncidentStatus::Open => "open",
                site_monitor_incidents::SiteMonitorIncidentStatus::Resolved => "resolved",
            },
            opened_at: i.opened_at.to_rfc3339(),
            resolved_at: i.resolved_at.map(|ts| ts.to_rfc3339()),
            started_check_id: i.opened_check_id,
            resolved_check_id: i.resolved_check_id,
            opened_status_code: i.opened_status_code,
            opened_failure_reason: i.opened_failure_reason,
            opened_error_message: i.opened_error_message,
            failure_count: i.failure_count,
            last_status_code: i.last_status_code,
            last_failure_reason: i.last_failure_reason,
            last_error_message: i.last_error_message,
            resolved_reason: i.resolved_reason.map(|r| r.as_str().to_owned()),
            resolved_status_code: i.resolved_status_code,
            resolved_response_time_ms: i.resolved_response_time_ms,
            downtime_seconds: i.downtime_seconds,
            acknowledged_at: i.acknowledged_at.map(|ts| ts.to_rfc3339()),
            acknowledged_by: i.acknowledged_by,
        }
    }
}

#[derive(Deserialize)]
struct GlobalIncidentsQuery {
    cursor: Option<String>,
    limit: Option<usize>,
    status: Option<site_monitor_incidents::SiteMonitorIncidentStatus>,
}

async fn get_dashboard(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<DashboardResponse>, ApiError> {
    AuthService::require_permission(&authenticated.auth.permissions, PermissionKey::SitesRead)
        .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let (site_rows, open_incident_count, recent_raw) = tokio::try_join!(
        sites::repository::get_dashboard_sites(&state.pool),
        site_monitor_incidents::repository::count_open_incidents(&state.pool),
        site_monitor_incidents::repository::list_global_incidents(
            &state.pool,
            None,
            None,
            Some(site_monitor_incidents::SiteMonitorIncidentStatus::Open),
            5,
        ),
    )
    .map_err(ApiError::internal_error)?;

    let sites_total = site_rows.len() as i64;
    let sites_with_monitor = site_rows.iter().filter(|r| r.monitors_total > 0).count() as i64;
    let sites_without_monitor = site_rows.iter().filter(|r| r.monitors_total == 0).count() as i64;
    let mut sites_active: i64 = 0;
    let mut sites_up: i64 = 0;
    let mut sites_down: i64 = 0;
    let mut sites_paused: i64 = 0;
    let mut sites_unchecked: i64 = 0;
    let mut monitors_active_total: i64 = 0;

    let mut site_entries: Vec<DashboardSiteEntry> = site_rows
        .iter()
        .map(|row| {
            let status = dashboard_site_status(row);
            if row.is_active {
                sites_active += 1;
                match status {
                    "up" => sites_up += 1,
                    "down" => sites_down += 1,
                    "paused" => sites_paused += 1,
                    _ => sites_unchecked += 1,
                }
            }
            monitors_active_total += row.monitors_active_count;
            DashboardSiteEntry {
                id: row.id,
                name: row.name.clone(),
                base_url: row.base_url.clone(),
                is_active: row.is_active,
                status,
                has_open_incident: row.has_open_incident,
                last_checked_at: row.last_checked_at.map(|ts| ts.to_rfc3339()),
                last_response_time_ms: row.last_response_time_ms,
                monitors_active: row.monitors_active_count,
                monitors_total: row.monitors_total,
            }
        })
        .collect();

    site_entries.sort_by_key(|s| match s.status {
        "down" => 0i8,
        "up" => 1,
        "paused" => 2,
        "unchecked" => 3,
        _ => 4,
    });

    let recent_incidents = recent_raw
        .into_iter()
        .map(|i| DashboardIncidentEntry {
            id: i.id,
            site_id: i.site_id,
            site_name: i.site_name,
            monitor_type: i.monitor_type.as_str(),
            target_url: i.target_url,
            opened_at: i.opened_at.to_rfc3339(),
            failure_count: i.failure_count,
            last_failure_reason: i.last_failure_reason,
            acknowledged_at: i.acknowledged_at.map(|ts| ts.to_rfc3339()),
        })
        .collect();

    Ok(Json(DashboardResponse {
        summary: DashboardSummary {
            sites_total,
            sites_active,
            sites_up,
            sites_down,
            sites_paused,
            sites_unchecked,
            sites_with_monitor,
            sites_without_monitor,
            open_incidents: open_incident_count,
            monitors_active: monitors_active_total,
        },
        sites: site_entries,
        recent_incidents,
    }))
}

async fn list_global_incidents(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Query(query): Query<GlobalIncidentsQuery>,
) -> Result<Response, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::SiteIncidentsRead,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let limit = history_limit(query.limit)?;
    let cursor = parse_cursor(query.cursor.as_deref())?;

    let incidents = site_monitor_incidents::repository::list_global_incidents(
        &state.pool,
        cursor.as_ref().map(|c| c.timestamp),
        cursor.as_ref().map(|c| c.id),
        query.status,
        page_fetch_limit(limit as usize),
    )
    .await
    .map_err(ApiError::internal_error)?;

    let (incidents, next_cursor) =
        paginate_vec_with_cursor(incidents, limit as usize, |incident| {
            encode_cursor(incident.opened_at, incident.id)
        });

    Ok(json_with_next_cursor(
        incidents
            .into_iter()
            .map(GlobalIncidentResponse::from)
            .collect::<Vec<_>>(),
        next_cursor,
    ))
}

async fn acknowledge_incident(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path((site_id, incident_id)): Path<(i64, i64)>,
) -> Result<Json<SiteIncidentResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::IncidentsWrite,
    )
    .map_err(|error| ApiError::forbidden(error.to_string()))?;

    let admin_user_id = authenticated.auth.user.id;

    let Some(incident) = site_monitor_incidents::repository::acknowledge_incident(
        &state.pool,
        incident_id,
        site_id,
        admin_user_id,
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("incident not found"));
    };

    Ok(Json(SiteIncidentResponse::from(incident)))
}

#[derive(Serialize)]
struct ApiClientResponse {
    id: i64,
    name: String,
    description: Option<String>,
    client_type: &'static str,
    client_id: String,
    secret_prefix: String,
    scopes: Vec<String>,
    is_active: bool,
    last_used_at: Option<String>,
    created_by_user_id: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct CreatedApiClientResponse {
    #[serde(flatten)]
    client: ApiClientResponse,
    client_secret: String,
}

#[derive(Deserialize)]
struct CreateApiClientRequest {
    name: String,
    description: Option<String>,
    client_type: String,
    scopes: Vec<String>,
}

#[derive(Deserialize)]
struct UpdateApiClientRequest {
    name: String,
    description: Option<String>,
    is_active: bool,
}

fn build_api_client_response(
    client: api_auth::ApiClient,
    scopes: Vec<String>,
) -> ApiClientResponse {
    ApiClientResponse {
        id: client.id,
        name: client.name,
        description: client.description,
        client_type: match client.client_type {
            api_auth::ApiClientType::InternalService => "internal_service",
            api_auth::ApiClientType::InstallationClient => "installation_client",
        },
        client_id: client.client_id,
        secret_prefix: client.secret_prefix,
        scopes,
        is_active: client.is_active,
        last_used_at: client.last_used_at.map(|ts| ts.to_rfc3339()),
        created_by_user_id: client.created_by_user_id,
        created_at: client.created_at.to_rfc3339(),
        updated_at: client.updated_at.to_rfc3339(),
    }
}

fn parse_api_client_type(s: &str) -> Result<api_auth::ApiClientType, ApiError> {
    match s {
        "internal_service" => Ok(api_auth::ApiClientType::InternalService),
        "installation_client" => Ok(api_auth::ApiClientType::InstallationClient),
        _ => Err(ApiError::bad_request(
            "client_type must be internal_service or installation_client",
        )),
    }
}

fn validate_scopes(scopes: &[String]) -> Result<(), ApiError> {
    for scope in scopes {
        if !matches!(scope.as_str(), "sites:read" | "sites:write") {
            return Err(ApiError::bad_request(
                "scopes must be one of: sites:read, sites:write",
            ));
        }
    }
    Ok(())
}

async fn list_api_clients(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<Vec<ApiClientResponse>>, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::ApiClientsRead,
    )
    .map_err(|e| ApiError::forbidden(e.to_string()))?;

    let clients = api_auth::repository::list_api_clients(&state.pool)
        .await
        .map_err(ApiError::internal_error)?;

    let client_ids: Vec<i64> = clients.iter().map(|c| c.id).collect();
    let mut scopes_by_client_id: std::collections::HashMap<i64, Vec<String>> =
        std::collections::HashMap::new();
    for id in &client_ids {
        let scopes = api_auth::repository::list_api_client_scopes(&state.pool, *id)
            .await
            .map_err(ApiError::internal_error)?;
        scopes_by_client_id.insert(*id, scopes.into_iter().map(|s| s.scope).collect());
    }

    let payload = clients
        .into_iter()
        .map(|c| {
            let scopes = scopes_by_client_id.remove(&c.id).unwrap_or_default();
            build_api_client_response(c, scopes)
        })
        .collect();

    Ok(Json(payload))
}

async fn create_api_client(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Json(payload): Json<CreateApiClientRequest>,
) -> Result<(StatusCode, Json<CreatedApiClientResponse>), ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::ApiClientsWrite,
    )
    .map_err(|e| ApiError::forbidden(e.to_string()))?;

    if payload.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }
    let client_type = parse_api_client_type(&payload.client_type)?;
    validate_scopes(&payload.scopes)?;

    let raw_id_bytes = generate_raw_token();
    let client_id = format!("ac_{}", &raw_id_bytes[..16]);

    let raw_secret = generate_raw_token();
    let secret_prefix: String = raw_secret.chars().take(TOKEN_PREFIX_LEN).collect();
    let secret_hash =
        AuthService::hash_client_secret(&raw_secret).map_err(ApiError::internal_error)?;

    let created_by = authenticated.auth.user.id.to_string();
    let client = api_auth::repository::create_api_client(
        &state.pool,
        api_auth::repository::NewApiClient {
            name: payload.name.trim(),
            description: payload.description.as_deref(),
            client_type,
            client_id: &client_id,
            client_secret_hash: &secret_hash,
            secret_prefix: &secret_prefix,
            created_by_user_id: Some(&created_by),
        },
    )
    .await
    .map_err(ApiError::internal_error)?;

    api_auth::repository::replace_api_client_scopes(&state.pool, client.id, &payload.scopes)
        .await
        .map_err(ApiError::internal_error)?;

    Ok((
        StatusCode::CREATED,
        Json(CreatedApiClientResponse {
            client: build_api_client_response(client, payload.scopes),
            client_secret: raw_secret,
        }),
    ))
}

async fn update_api_client(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(client_id): Path<i64>,
    Json(payload): Json<UpdateApiClientRequest>,
) -> Result<Json<ApiClientResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::ApiClientsWrite,
    )
    .map_err(|e| ApiError::forbidden(e.to_string()))?;

    if payload.name.trim().is_empty() {
        return Err(ApiError::bad_request("name is required"));
    }

    let Some(client) = api_auth::repository::update_api_client(
        &state.pool,
        client_id,
        payload.name.trim(),
        payload.description.as_deref(),
        payload.is_active,
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("api client not found"));
    };

    let scopes = api_auth::repository::list_api_client_scopes(&state.pool, client.id)
        .await
        .map_err(ApiError::internal_error)?
        .into_iter()
        .map(|s| s.scope)
        .collect();

    Ok(Json(build_api_client_response(client, scopes)))
}

async fn delete_api_client(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(client_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::ApiClientsWrite,
    )
    .map_err(|e| ApiError::forbidden(e.to_string()))?;

    let deleted = api_auth::repository::delete_api_client(&state.pool, client_id)
        .await
        .map_err(ApiError::internal_error)?;

    if !deleted {
        return Err(ApiError::not_found("api client not found"));
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn rotate_api_client_secret(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
    Path(client_id): Path<i64>,
) -> Result<Json<CreatedApiClientResponse>, ApiError> {
    AuthService::require_permission(
        &authenticated.auth.permissions,
        PermissionKey::ApiClientsWrite,
    )
    .map_err(|e| ApiError::forbidden(e.to_string()))?;

    let raw_secret = generate_raw_token();
    let secret_prefix: String = raw_secret.chars().take(TOKEN_PREFIX_LEN).collect();
    let secret_hash =
        AuthService::hash_client_secret(&raw_secret).map_err(ApiError::internal_error)?;

    let Some(client) = api_auth::repository::rotate_api_client_secret(
        &state.pool,
        client_id,
        &secret_hash,
        &secret_prefix,
    )
    .await
    .map_err(ApiError::internal_error)?
    else {
        return Err(ApiError::not_found("api client not found"));
    };

    let scopes = api_auth::repository::list_api_client_scopes(&state.pool, client.id)
        .await
        .map_err(ApiError::internal_error)?
        .into_iter()
        .map(|s| s.scope)
        .collect();

    Ok(Json(CreatedApiClientResponse {
        client: build_api_client_response(client, scopes),
        client_secret: raw_secret,
    }))
}
