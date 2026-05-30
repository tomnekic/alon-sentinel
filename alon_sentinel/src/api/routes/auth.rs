use std::sync::Arc;

use anyhow::Result;
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, HeaderValue, header::SET_COOKIE},
    response::IntoResponse,
    routing::post,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::{
    api::{
        error::ApiError,
        extractors::{AuthenticatedAdminRequest, AuthenticatedApiClientRequest, RequestContext},
        state::AppState,
    },
    auth::{AuthService, IssuedAdminAccessToken},
};

const ADMIN_SESSION_COOKIE: &str = "admin_session";
const API_TOKEN_RATE_LIMIT_BUCKET: &str = "api_token";
const ADMIN_LOGIN_RATE_LIMIT_BUCKET: &str = "admin_login";

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/auth/token", post(issue_token))
        .route("/v1/auth/revoke", post(revoke_token))
        .route("/v1/admin/auth/login", post(issue_admin_token))
        .route("/v1/admin/auth/logout", post(logout_admin))
        .route("/v1/admin/auth/me", axum::routing::get(get_admin_me))
}

#[derive(Deserialize)]
struct IssueTokenRequest {
    client_id: String,
    client_secret: String,
}

#[derive(Serialize)]
struct IssueTokenResponse {
    access_token: String,
    token_type: &'static str,
    expires_at: String,
    expires_in: i64,
    scope: Vec<String>,
}

async fn issue_token(
    State(state): State<Arc<AppState>>,
    request_context: RequestContext,
    Json(payload): Json<IssueTokenRequest>,
) -> Result<Json<IssueTokenResponse>, ApiError> {
    enforce_auth_rate_limit(
        &state,
        API_TOKEN_RATE_LIMIT_BUCKET,
        &request_context.ip_address,
    )?;

    if payload.client_id.trim().is_empty() || payload.client_secret.is_empty() {
        return Err(ApiError::bad_request(
            "client_id and client_secret are required",
        ));
    }

    let auth_service = AuthService::new(&state.pool, state.auth_config);
    let issued_token = auth_service
        .issue_access_token(
            &payload.client_id,
            &payload.client_secret,
            Some(request_context.ip_address.as_str()),
            request_context.user_agent.as_deref(),
        )
        .await
        .map_err(ApiError::unauthorized_error)?;

    let expires_in = (issued_token.expires_at - Utc::now()).num_seconds().max(0);

    Ok(Json(IssueTokenResponse {
        access_token: issued_token.token,
        token_type: "Bearer",
        expires_at: issued_token.expires_at.to_rfc3339(),
        expires_in,
        scope: issued_token.scopes,
    }))
}

#[derive(Deserialize)]
struct RevokeTokenRequest {
    token: String,
    revoked_reason: Option<String>,
}

#[derive(Serialize)]
struct RevokeTokenResponse {
    revoked: bool,
}

async fn revoke_token(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedApiClientRequest,
    request_context: RequestContext,
    Json(payload): Json<RevokeTokenRequest>,
) -> Result<Json<RevokeTokenResponse>, ApiError> {
    if payload.token.trim().is_empty() {
        return Err(ApiError::bad_request("token is required"));
    }
    if payload.token != authenticated.raw_token {
        return Err(ApiError::forbidden(
            "can only revoke the current access token",
        ));
    }

    let auth_service = AuthService::new(&state.pool, state.auth_config);
    auth_service
        .revoke_access_token_by_id(
            authenticated.auth.token.id,
            authenticated.auth.client.id,
            payload.revoked_reason.as_deref(),
            Some(request_context.ip_address.as_str()),
            request_context.user_agent.as_deref(),
        )
        .await
        .map_err(ApiError::bad_request_error)?;
    state
        .auth_token_cache
        .invalidate_raw_token(&authenticated.raw_token);

    Ok(Json(RevokeTokenResponse { revoked: true }))
}

#[derive(Deserialize)]
struct IssueAdminTokenRequest {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct AdminUserResponse {
    id: i64,
    email: String,
    display_name: String,
    is_active: bool,
    last_login_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct IssueAdminTokenResponse {
    access_token: String,
    expires_at: String,
    expires_in: i64,
    roles: Vec<String>,
    permissions: Vec<String>,
    user: AdminUserResponse,
}

#[derive(Serialize)]
struct AdminSessionResponse {
    roles: Vec<String>,
    permissions: Vec<String>,
    user: AdminUserResponse,
}

impl From<crate::domain::admin_users::AdminUser> for AdminUserResponse {
    fn from(user: crate::domain::admin_users::AdminUser) -> Self {
        Self {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
            is_active: user.is_active,
            last_login_at: user.last_login_at.map(|value| value.to_rfc3339()),
            created_at: user.created_at.to_rfc3339(),
            updated_at: user.updated_at.to_rfc3339(),
        }
    }
}

impl From<IssuedAdminAccessToken> for IssueAdminTokenResponse {
    fn from(issued_token: IssuedAdminAccessToken) -> Self {
        let expires_in = (issued_token.expires_at - Utc::now()).num_seconds().max(0);
        Self {
            access_token: issued_token.token,
            expires_at: issued_token.expires_at.to_rfc3339(),
            expires_in,
            roles: issued_token.roles,
            permissions: issued_token.permissions.to_strings(),
            user: AdminUserResponse::from(issued_token.user),
        }
    }
}

async fn issue_admin_token(
    State(state): State<Arc<AppState>>,
    request_context: RequestContext,
    Json(payload): Json<IssueAdminTokenRequest>,
) -> Result<impl IntoResponse, ApiError> {
    enforce_auth_rate_limit(
        &state,
        ADMIN_LOGIN_RATE_LIMIT_BUCKET,
        &request_context.ip_address,
    )?;

    if payload.email.trim().is_empty() || payload.password.is_empty() {
        return Err(ApiError::bad_request("email and password are required"));
    }

    let auth_service = AuthService::new(&state.pool, state.auth_config);
    let issued_token = auth_service
        .issue_admin_access_token(
            payload.email.trim(),
            &payload.password,
            Some(request_context.ip_address.as_str()),
            request_context.user_agent.as_deref(),
        )
        .await
        .map_err(ApiError::unauthorized_error)?;

    let cookie = build_session_cookie(
        &issued_token.token,
        issued_token
            .expires_at
            .signed_duration_since(Utc::now())
            .num_seconds()
            .max(0),
        state.cookie_secure,
    );

    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::internal("failed to build session cookie"))?,
    );

    Ok((headers, Json(IssueAdminTokenResponse::from(issued_token))))
}

fn enforce_auth_rate_limit(
    state: &AppState,
    bucket: &'static str,
    ip_address: &str,
) -> Result<(), ApiError> {
    if state.auth_rate_limiter.check(bucket, ip_address) {
        return Ok(());
    }

    Err(ApiError::too_many_requests(
        "too many authentication attempts from this IP address",
    ))
}

async fn logout_admin(
    State(state): State<Arc<AppState>>,
    authenticated: AuthenticatedAdminRequest,
) -> Result<impl IntoResponse, ApiError> {
    let auth_service = AuthService::new(&state.pool, state.auth_config);
    auth_service
        .revoke_admin_access_token(authenticated.auth.token.id, Some("admin_logout"))
        .await
        .map_err(ApiError::internal_error)?;
    state.auth_token_cache.invalidate_hashed_token(
        &authenticated.auth.token.token_prefix,
        &authenticated.auth.token.token_hash,
    );

    let cookie = clear_session_cookie(state.cookie_secure);
    let mut headers = HeaderMap::new();
    headers.insert(
        SET_COOKIE,
        HeaderValue::from_str(&cookie)
            .map_err(|_| ApiError::internal("failed to build session cookie"))?,
    );

    Ok((headers, Json(RevokeTokenResponse { revoked: true })))
}

async fn get_admin_me(
    authenticated: AuthenticatedAdminRequest,
) -> Result<Json<AdminSessionResponse>, ApiError> {
    Ok(Json(AdminSessionResponse {
        roles: authenticated.auth.roles,
        permissions: authenticated.auth.permissions.to_strings(),
        user: AdminUserResponse::from(authenticated.auth.user),
    }))
}

fn build_session_cookie(token: &str, max_age_seconds: i64, secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!(
        "{ADMIN_SESSION_COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age_seconds}{secure_attr}"
    )
}

fn clear_session_cookie(secure: bool) -> String {
    let secure_attr = if secure { "; Secure" } else { "" };
    format!("{ADMIN_SESSION_COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{secure_attr}")
}

#[cfg(test)]
mod tests {
    use super::{ADMIN_SESSION_COOKIE, build_session_cookie, clear_session_cookie};

    #[test]
    fn build_session_cookie_contains_required_attributes() {
        let cookie = build_session_cookie("tok123", 3600, false);
        assert!(cookie.starts_with(&format!("{ADMIN_SESSION_COOKIE}=tok123")));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
        assert!(cookie.contains("Max-Age=3600"));
    }

    #[test]
    fn build_session_cookie_omits_secure_flag_when_not_requested() {
        let cookie = build_session_cookie("tok123", 3600, false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn build_session_cookie_includes_secure_flag_when_requested() {
        let cookie = build_session_cookie("tok123", 3600, true);
        assert!(cookie.contains("; Secure"));
    }

    #[test]
    fn clear_session_cookie_sets_max_age_to_zero_and_empties_value() {
        let cookie = clear_session_cookie(false);
        assert!(cookie.contains(&format!("{ADMIN_SESSION_COOKIE}=")));
        assert!(cookie.contains("Max-Age=0"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Path=/"));
    }

    #[test]
    fn clear_session_cookie_omits_secure_flag_when_not_requested() {
        let cookie = clear_session_cookie(false);
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn clear_session_cookie_includes_secure_flag_when_requested() {
        let cookie = clear_session_cookie(true);
        assert!(cookie.contains("; Secure"));
    }
}
