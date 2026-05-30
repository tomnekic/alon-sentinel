use std::sync::Arc;

use anyhow::anyhow;
use std::net::{IpAddr, SocketAddr};

use anyhow::Result;
use axum::extract::ConnectInfo;
use axum::{
    extract::{FromRef, FromRequestParts},
    http::{
        header::{AUTHORIZATION, COOKIE, USER_AGENT},
        request::Parts,
    },
};

use crate::{
    api::permissions::PermissionSet,
    api::{error::ApiError, state::AppState},
    auth::{
        AccessTokenContext, AdminAccessTokenContext, AuthService, BearerAuthError,
        BearerTokenContext,
    },
};

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub(crate) ip_address: String,
    pub(crate) user_agent: Option<String>,
}

impl<S> FromRequestParts<S> for RequestContext
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_agent = parts
            .headers
            .get(USER_AGENT)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);

        let ip_address = extract_request_ip_address(parts, _state).await?;

        Ok(Self {
            ip_address,
            user_agent,
        })
    }
}

pub struct AuthenticatedRequest {
    pub(crate) permissions: PermissionSet,
}

pub struct AuthenticatedApiClientRequest {
    pub(crate) auth: AccessTokenContext,
    pub(crate) raw_token: String,
}

pub struct AuthenticatedAdminRequest {
    pub(crate) auth: AdminAccessTokenContext,
}

impl<S> FromRequestParts<S> for AuthenticatedRequest
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = Arc::<AppState>::from_ref(state);

        let permissions = if let Ok(bearer_token) = extract_bearer_token(parts) {
            let auth = authenticate_cached_bearer_token(&app_state, bearer_token).await?;
            match auth {
                BearerTokenContext::ApiClient(context) => context.permissions,
                BearerTokenContext::AdminUser(context) => context.permissions,
            }
        } else {
            let cookie_token = extract_admin_session_cookie(parts)?;
            let auth = authenticate_cached_admin_bearer_token(&app_state, cookie_token).await?;
            auth.permissions
        };

        Ok(Self { permissions })
    }
}

impl<S> FromRequestParts<S> for AuthenticatedAdminRequest
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = Arc::<AppState>::from_ref(state);
        let raw_token = extract_admin_session_cookie(parts)
            .or_else(|_| extract_bearer_token(parts))
            .map(str::to_string)?;
        let auth = authenticate_cached_admin_bearer_token(&app_state, &raw_token).await?;
        Ok(Self { auth })
    }
}

impl<S> FromRequestParts<S> for AuthenticatedApiClientRequest
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = Arc::<AppState>::from_ref(state);
        let bearer_token = extract_bearer_token(parts)?.to_string();
        let auth = authenticate_cached_bearer_token(&app_state, &bearer_token).await?;

        match auth {
            BearerTokenContext::ApiClient(auth) => Ok(Self {
                auth,
                raw_token: bearer_token,
            }),
            BearerTokenContext::AdminUser(_) => Err(ApiError::unauthorized(
                "admin bearer token is not valid for this endpoint",
            )),
        }
    }
}

fn extract_admin_session_cookie(parts: &Parts) -> Result<&str, ApiError> {
    let cookie_header = parts
        .headers
        .get(COOKIE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("admin session cookie required"))?;

    cookie_header
        .split(';')
        .map(str::trim)
        .find_map(|segment| segment.strip_prefix("admin_session="))
        .filter(|token| !token.is_empty())
        .ok_or_else(|| ApiError::unauthorized("admin session cookie required"))
}

fn extract_bearer_token(parts: &Parts) -> Result<&str, ApiError> {
    let authorization = parts
        .headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::unauthorized("missing authorization header"))?;

    authorization
        .strip_prefix("Bearer ")
        .ok_or_else(|| ApiError::unauthorized("invalid authorization scheme"))
}

async fn authenticate_cached_bearer_token(
    app_state: &Arc<AppState>,
    bearer_token: &str,
) -> Result<BearerTokenContext, ApiError> {
    if let Some(context) = app_state.auth_token_cache.get_bearer_context(bearer_token) {
        return Ok(context);
    }

    let auth_service = AuthService::new(&app_state.pool, app_state.auth_config);
    let context = auth_service
        .authenticate_any_bearer_token(bearer_token)
        .await
        .map_err(map_bearer_auth_error)?;
    app_state
        .auth_token_cache
        .insert_bearer_context(bearer_token, context.clone());

    Ok(context)
}

async fn authenticate_cached_admin_bearer_token(
    app_state: &Arc<AppState>,
    bearer_token: &str,
) -> Result<AdminAccessTokenContext, ApiError> {
    if let Some(context) = app_state.auth_token_cache.get_admin_context(bearer_token) {
        return Ok(context);
    }

    let auth_service = AuthService::new(&app_state.pool, app_state.auth_config);
    let context = auth_service
        .authenticate_admin_bearer_token(bearer_token)
        .await
        .map_err(map_bearer_auth_error)?;
    app_state
        .auth_token_cache
        .insert_bearer_context(bearer_token, BearerTokenContext::AdminUser(context.clone()));

    Ok(context)
}

fn map_bearer_auth_error(error: BearerAuthError) -> ApiError {
    match error {
        BearerAuthError::Unauthorized(error) => ApiError::unauthorized_error(error),
        BearerAuthError::Internal(error) => ApiError::internal_error(error),
    }
}

async fn extract_request_ip_address<S>(parts: &mut Parts, state: &S) -> Result<String, ApiError>
where
    Arc<AppState>: FromRef<S>,
    S: Send + Sync,
{
    let app_state = Arc::<AppState>::from_ref(state);
    let peer_ip = ConnectInfo::<SocketAddr>::from_request_parts(parts, state)
        .await
        .map(|ConnectInfo(address)| address.ip())
        .map_err(|_| {
            ApiError::internal_error(anyhow!(
                "missing peer socket address in request extensions; ensure axum is served with into_make_service_with_connect_info::<SocketAddr>()"
            ))
        })?;

    if app_state.trust_proxy_headers && app_state.trusted_proxy_ips.contains(&peer_ip) {
        return extract_forwarded_ip_address(parts);
    }

    Ok(peer_ip.to_string())
}

fn extract_forwarded_ip_address(parts: &Parts) -> Result<String, ApiError> {
    let forwarded_for = parts
        .headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::bad_request("missing x-forwarded-for header"))?;

    let forwarded_ip = forwarded_for
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| ApiError::bad_request("x-forwarded-for header is empty"))?;

    forwarded_ip
        .parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .map_err(|_| {
            ApiError::bad_request("x-forwarded-for header must end with a valid IP address")
        })
}

#[cfg(test)]
mod tests {
    use super::RequestContext;
    use crate::api::error::ApiError;
    use crate::{
        api::{
            rate_limit::{AuthRateLimiter, StatusPageCache},
            state::AppState,
        },
        auth::{AuthConfig, AuthTokenCache},
        crypto::WebhookSecretEncryptionKey,
    };
    use axum::extract::ConnectInfo;
    use axum::{extract::FromRequestParts, http::Request};
    use sqlx::postgres::PgPoolOptions;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::{sync::Arc, time::Duration};

    #[derive(Clone)]
    struct TestState;

    fn test_webhook_secret_encryption_key() -> WebhookSecretEncryptionKey {
        WebhookSecretEncryptionKey::from_hex(
            "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff",
        )
        .expect("test webhook secret encryption key should parse")
    }

    impl axum::extract::FromRef<TestState> for Arc<AppState> {
        fn from_ref(_input: &TestState) -> Self {
            Arc::new(AppState {
                pool: PgPoolOptions::new()
                    .connect_lazy("postgresql://postgres:postgres@localhost/alon_sentinel_test")
                    .expect("lazy pool should construct"),
                auth_config: AuthConfig::default(),
                auth_rate_limiter: AuthRateLimiter::new(10, Duration::from_secs(60)),
                auth_token_cache: AuthTokenCache::new(),
                trust_proxy_headers: false,
                trusted_proxy_ips: Vec::new(),
                cookie_secure: false,
                http_monitor_allow_private_targets: false,
                webhook_secret_encryption_key: test_webhook_secret_encryption_key(),
                db_max_connections: 0,
                public_rate_limiter: AuthRateLimiter::new(60, Duration::from_secs(60)),
                status_page_cache: StatusPageCache::new(16, Duration::from_secs(30)),
            })
        }
    }

    #[tokio::test]
    async fn request_context_uses_peer_socket_ip_when_proxy_headers_are_disabled() {
        let mut request = Request::builder()
            .uri("/v1/auth/token")
            .header("x-forwarded-for", "198.51.100.8")
            .body(())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            ))));
        let (mut parts, _) = request.into_parts();

        let context = RequestContext::from_request_parts(&mut parts, &TestState)
            .await
            .expect("request context should extract");

        assert_eq!(context.ip_address, "127.0.0.1");
    }

    #[tokio::test]
    async fn request_context_rejects_missing_connect_info() {
        let request = Request::builder()
            .uri("/v1/auth/token")
            .body(())
            .expect("request should build");
        let (mut parts, _) = request.into_parts();

        let error = RequestContext::from_request_parts(&mut parts, &TestState)
            .await
            .expect_err("missing connect info should fail");

        assert_internal(error);
    }

    #[tokio::test]
    async fn request_context_uses_forwarded_ip_when_peer_is_trusted_proxy() {
        let mut request = Request::builder()
            .uri("/v1/auth/token")
            .header("x-forwarded-for", "198.51.100.8, 203.0.113.10")
            .body(())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            ))));
        let (mut parts, _) = request.into_parts();
        let state = test_state_with_trusted_proxy(true, &["127.0.0.1"]);

        let context = RequestContext::from_request_parts(&mut parts, &state)
            .await
            .expect("request context should extract");

        assert_eq!(context.ip_address, "198.51.100.8");
    }

    #[tokio::test]
    async fn request_context_ignores_forwarded_ip_when_peer_is_not_trusted_proxy() {
        let mut request = Request::builder()
            .uri("/v1/auth/token")
            .header("x-forwarded-for", "198.51.100.8")
            .body(())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
                8080,
            ))));
        let (mut parts, _) = request.into_parts();
        let state = test_state_with_trusted_proxy(true, &["127.0.0.1"]);

        let context = RequestContext::from_request_parts(&mut parts, &state)
            .await
            .expect("request context should extract");

        assert_eq!(context.ip_address, "203.0.113.10");
    }

    #[tokio::test]
    async fn request_context_rejects_missing_forwarded_for_for_trusted_proxy() {
        let mut request = Request::builder()
            .uri("/v1/auth/token")
            .body(())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            ))));
        let (mut parts, _) = request.into_parts();
        let state = test_state_with_trusted_proxy(true, &["127.0.0.1"]);

        let error = RequestContext::from_request_parts(&mut parts, &state)
            .await
            .expect_err("missing forwarded header should fail");

        assert_bad_request(error);
    }

    #[tokio::test]
    async fn request_context_rejects_invalid_forwarded_for_for_trusted_proxy() {
        let mut request = Request::builder()
            .uri("/v1/auth/token")
            .header("x-forwarded-for", "not-an-ip")
            .body(())
            .expect("request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from((
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                8080,
            ))));
        let (mut parts, _) = request.into_parts();
        let state = test_state_with_trusted_proxy(true, &["127.0.0.1"]);

        let error = RequestContext::from_request_parts(&mut parts, &state)
            .await
            .expect_err("invalid forwarded header should fail");

        assert_bad_request(error);
    }

    fn assert_bad_request(error: ApiError) {
        let response = axum::response::IntoResponse::into_response(error);
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    fn assert_internal(error: ApiError) {
        let response = axum::response::IntoResponse::into_response(error);
        assert_eq!(
            response.status(),
            axum::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    fn test_state_with_trusted_proxy(
        trust_proxy_headers: bool,
        trusted_proxy_ips: &[&str],
    ) -> Arc<AppState> {
        Arc::new(AppState {
            pool: PgPoolOptions::new()
                .connect_lazy("postgresql://postgres:postgres@localhost/alon_sentinel_test")
                .expect("lazy pool should construct"),
            auth_config: AuthConfig::default(),
            auth_rate_limiter: AuthRateLimiter::new(10, Duration::from_secs(60)),
            auth_token_cache: AuthTokenCache::new(),
            trust_proxy_headers,
            trusted_proxy_ips: trusted_proxy_ips
                .iter()
                .map(|ip| ip.parse().expect("trusted proxy IP should parse"))
                .collect(),
            cookie_secure: false,
            http_monitor_allow_private_targets: false,
            webhook_secret_encryption_key: test_webhook_secret_encryption_key(),
            db_max_connections: 0,
            public_rate_limiter: AuthRateLimiter::new(100, Duration::from_secs(60)),
            status_page_cache: StatusPageCache::new(10, Duration::from_secs(60)),
        })
    }
}
