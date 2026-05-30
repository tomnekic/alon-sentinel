use anyhow::{Result, anyhow, bail};
use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, types::JsonValue};

use crate::api::permissions::{PermissionKey, PermissionSet};
use crate::domain::{
    admin_auth::{self, AdminAccessToken, AdminAuthAuditLog},
    admin_users::{self, AdminUser},
    api_auth::{self, AccessToken, ApiClient, ApiClientAuditLog},
    permissions, roles,
};

const DEFAULT_ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;
pub(crate) const TOKEN_BYTES: usize = 32;
pub(crate) const TOKEN_PREFIX_LEN: usize = 12;

#[derive(Debug, Clone, Copy)]
pub struct AuthConfig {
    pub access_token_ttl_seconds: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            access_token_ttl_seconds: DEFAULT_ACCESS_TOKEN_TTL_SECONDS,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    pub client: ApiClient,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct IssuedAccessToken {
    pub token: String,
    pub token_prefix: String,
    pub expires_at: DateTime<Utc>,
    pub client: ApiClient,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AccessTokenContext {
    pub token: AccessToken,
    pub client: ApiClient,
    pub scopes: Vec<String>,
    pub permissions: PermissionSet,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedAdminUser {
    pub user: AdminUser,
    pub roles: Vec<String>,
    pub permissions: PermissionSet,
}

#[derive(Debug, Clone)]
pub struct IssuedAdminAccessToken {
    pub token: String,
    pub token_prefix: String,
    pub expires_at: DateTime<Utc>,
    pub user: AdminUser,
    pub roles: Vec<String>,
    pub permissions: PermissionSet,
}

#[derive(Debug, Clone)]
pub struct AdminAccessTokenContext {
    pub token: AdminAccessToken,
    pub user: AdminUser,
    pub roles: Vec<String>,
    pub permissions: PermissionSet,
}

#[derive(Debug, Clone)]
pub enum BearerTokenContext {
    ApiClient(AccessTokenContext),
    AdminUser(AdminAccessTokenContext),
}

#[derive(Debug)]
pub enum BearerAuthError {
    Unauthorized(anyhow::Error),
    Internal(anyhow::Error),
}

impl BearerAuthError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(anyhow!(message.into()))
    }

    fn internal(error: anyhow::Error) -> Self {
        Self::Internal(error)
    }
}

struct AdminAuthAuditEvent<'a> {
    admin_user_id: Option<i64>,
    action: &'a str,
    ip_address: Option<&'a str>,
    user_agent: Option<&'a str>,
    meta_json: Option<&'a JsonValue>,
}

pub struct AuthService<'a> {
    pool: &'a PgPool,
    config: AuthConfig,
}

impl<'a> AuthService<'a> {
    pub fn new(pool: &'a PgPool, config: AuthConfig) -> Self {
        Self { pool, config }
    }

    pub fn hash_client_secret(client_secret: &str) -> Result<String> {
        Self::hash_password(client_secret)
    }

    pub fn verify_client_secret(client_secret_hash: &str, client_secret: &str) -> Result<bool> {
        Self::verify_password_hash(client_secret_hash, client_secret)
    }

    pub fn hash_password(password: &str) -> Result<String> {
        if password.is_empty() {
            bail!("password can not be empty");
        }

        let salt = SaltString::generate(&mut OsRng);
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|err| anyhow!("failed to hash password: {err}"))?;

        Ok(password_hash.to_string())
    }

    pub fn verify_password_hash(password_hash: &str, password: &str) -> Result<bool> {
        let parsed_hash = PasswordHash::new(password_hash)
            .map_err(|err| anyhow!("invalid password hash format: {err}"))?;

        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok())
    }

    pub async fn authenticate_client_credentials(
        &self,
        client_id: &str,
        client_secret: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<AuthenticatedClient> {
        let Some(client) =
            api_auth::repository::get_api_client_by_client_id(self.pool, client_id).await?
        else {
            self.write_auth_failed_audit(None, ip_address, user_agent)
                .await?;
            bail!("invalid client credentials");
        };

        if !client.is_active {
            self.write_auth_failed_audit(Some(client.id), ip_address, user_agent)
                .await?;
            bail!("api client is inactive");
        }

        if !Self::verify_client_secret(&client.client_secret_hash, client_secret)? {
            self.write_auth_failed_audit(Some(client.id), ip_address, user_agent)
                .await?;
            bail!("invalid client credentials");
        }

        let scopes = self.load_client_scopes(client.id).await?;

        Ok(AuthenticatedClient { client, scopes })
    }

    pub async fn issue_access_token(
        &self,
        client_id: &str,
        client_secret: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<IssuedAccessToken> {
        let authenticated = self
            .authenticate_client_credentials(client_id, client_secret, ip_address, user_agent)
            .await?;

        let raw_token = generate_raw_token();
        let token_hash = hash_token(&raw_token);
        let token_prefix = raw_token.chars().take(TOKEN_PREFIX_LEN).collect::<String>();
        let expires_at = Utc::now() + Duration::seconds(self.config.access_token_ttl_seconds);

        let _stored_token = api_auth::repository::create_access_token(
            self.pool,
            api_auth::repository::NewAccessToken {
                api_client_id: authenticated.client.id,
                token_hash: &token_hash,
                token_prefix: &token_prefix,
                expires_at,
            },
        )
        .await?;

        api_auth::repository::update_api_client_last_used(self.pool, authenticated.client.id)
            .await?;
        self.write_audit_log(
            Some(authenticated.client.id),
            "token_issued",
            ip_address,
            user_agent,
        )
        .await?;

        Ok(IssuedAccessToken {
            token: raw_token,
            token_prefix,
            expires_at,
            client: authenticated.client,
            scopes: authenticated.scopes,
        })
    }

    pub async fn authenticate_admin_credentials(
        &self,
        email: &str,
        password: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<AuthenticatedAdminUser> {
        let email = email.trim();
        let Some(user) = admin_users::repository::get_admin_user_by_email(self.pool, email).await?
        else {
            self.write_admin_auth_failed_audit(None, email, ip_address, user_agent)
                .await?;
            bail!("invalid admin credentials");
        };

        if !user.is_active {
            self.write_admin_auth_failed_audit(Some(user.id), email, ip_address, user_agent)
                .await?;
            bail!("admin user is inactive");
        }

        if !Self::verify_password_hash(&user.password_hash, password)? {
            self.write_admin_auth_failed_audit(Some(user.id), email, ip_address, user_agent)
                .await?;
            bail!("invalid admin credentials");
        }

        let roles = roles::repository::list_role_keys_for_admin_user(self.pool, user.id).await?;
        let permissions = PermissionSet::from_strs(
            permissions::repository::list_permission_keys_for_admin_user(self.pool, user.id)
                .await?,
        )?;

        Ok(AuthenticatedAdminUser {
            user,
            roles,
            permissions,
        })
    }

    pub async fn issue_admin_access_token(
        &self,
        email: &str,
        password: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<IssuedAdminAccessToken> {
        let authenticated = self
            .authenticate_admin_credentials(email, password, ip_address, user_agent)
            .await?;

        let raw_token = generate_raw_token();
        let token_hash = hash_token(&raw_token);
        let token_prefix = raw_token.chars().take(TOKEN_PREFIX_LEN).collect::<String>();
        let expires_at = Utc::now() + Duration::seconds(self.config.access_token_ttl_seconds);

        let _stored_token = admin_auth::repository::create_admin_access_token(
            self.pool,
            admin_auth::repository::NewAdminAccessToken {
                admin_user_id: authenticated.user.id,
                token_hash: &token_hash,
                token_prefix: &token_prefix,
                expires_at,
            },
        )
        .await?;

        admin_users::repository::update_admin_user_last_login(self.pool, authenticated.user.id)
            .await?;
        self.write_admin_auth_audit_log(AdminAuthAuditEvent {
            admin_user_id: Some(authenticated.user.id),
            action: "token_issued",
            ip_address,
            user_agent,
            meta_json: None,
        })
        .await?;

        Ok(IssuedAdminAccessToken {
            token: raw_token,
            token_prefix,
            expires_at,
            user: authenticated.user,
            roles: authenticated.roles,
            permissions: authenticated.permissions,
        })
    }

    pub async fn authenticate_bearer_token(
        &self,
        raw_token: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> std::result::Result<AccessTokenContext, BearerAuthError> {
        let token_hash = hash_token(raw_token);
        let Some(context_row) = api_auth::repository::get_access_token_auth_context_by_token_hash(
            self.pool,
            &token_hash,
        )
        .await
        .map_err(BearerAuthError::internal)?
        else {
            self.write_auth_failed_audit(None, ip_address, user_agent)
                .await
                .map_err(BearerAuthError::internal)?;
            return Err(BearerAuthError::unauthorized("invalid access token"));
        };
        let token = AccessToken {
            id: context_row.access_token_id,
            api_client_id: context_row.access_token_api_client_id,
            token_hash: context_row.access_token_token_hash,
            token_prefix: context_row.access_token_token_prefix,
            expires_at: context_row.access_token_expires_at,
            revoked_at: context_row.access_token_revoked_at,
            revoked_reason: context_row.access_token_revoked_reason,
            last_used_at: context_row.access_token_last_used_at,
            created_at: context_row.access_token_created_at,
        };
        let client = ApiClient {
            id: context_row.client_id,
            uuid: context_row.client_uuid,
            name: context_row.client_name,
            description: context_row.client_description,
            client_type: context_row.client_type,
            client_id: context_row.client_client_id,
            client_secret_hash: context_row.client_client_secret_hash,
            secret_prefix: context_row.client_secret_prefix,
            is_active: context_row.client_is_active,
            last_used_at: context_row.client_last_used_at,
            created_by_user_id: context_row.client_created_by_user_id,
            created_at: context_row.client_created_at,
            updated_at: context_row.client_updated_at,
        };

        if token.revoked_at.is_some() {
            self.write_auth_failed_audit(None, ip_address, user_agent)
                .await
                .map_err(BearerAuthError::internal)?;
            return Err(BearerAuthError::unauthorized(
                "access token has been revoked",
            ));
        }

        if token.expires_at <= Utc::now() {
            self.write_auth_failed_audit(None, ip_address, user_agent)
                .await
                .map_err(BearerAuthError::internal)?;
            return Err(BearerAuthError::unauthorized("access token has expired"));
        }

        if !client.is_active {
            self.write_auth_failed_audit(Some(client.id), ip_address, user_agent)
                .await
                .map_err(BearerAuthError::internal)?;
            return Err(BearerAuthError::unauthorized("api client is inactive"));
        }

        let scopes = context_row.scopes;
        let permissions = permissions_from_api_client_scopes(&scopes);

        tokio::try_join!(
            api_auth::repository::update_access_token_last_used(self.pool, token.id),
            api_auth::repository::update_api_client_last_used(self.pool, client.id),
        )
        .map_err(BearerAuthError::internal)?;

        Ok(AccessTokenContext {
            token,
            client,
            scopes,
            permissions,
        })
    }

    pub async fn authenticate_admin_bearer_token(
        &self,
        raw_token: &str,
    ) -> std::result::Result<AdminAccessTokenContext, BearerAuthError> {
        let token_hash = hash_token(raw_token);
        let Some(context_row) =
            admin_auth::repository::get_admin_access_token_auth_context_by_token_hash(
                self.pool,
                &token_hash,
            )
            .await
            .map_err(BearerAuthError::internal)?
        else {
            return Err(BearerAuthError::unauthorized("invalid admin access token"));
        };
        let token = AdminAccessToken {
            id: context_row.access_token_id,
            admin_user_id: context_row.access_token_admin_user_id,
            token_hash: context_row.access_token_token_hash,
            token_prefix: context_row.access_token_token_prefix,
            expires_at: context_row.access_token_expires_at,
            revoked_at: context_row.access_token_revoked_at,
            revoked_reason: context_row.access_token_revoked_reason,
            last_used_at: context_row.access_token_last_used_at,
            created_at: context_row.access_token_created_at,
        };

        if token.revoked_at.is_some() {
            return Err(BearerAuthError::unauthorized(
                "admin access token has been revoked",
            ));
        }

        if token.expires_at <= Utc::now() {
            return Err(BearerAuthError::unauthorized(
                "admin access token has expired",
            ));
        }

        let user = AdminUser {
            id: context_row.user_id,
            uuid: context_row.user_uuid,
            email: context_row.user_email,
            display_name: context_row.user_display_name,
            password_hash: context_row.user_password_hash,
            is_active: context_row.user_is_active,
            last_login_at: context_row.user_last_login_at,
            created_at: context_row.user_created_at,
            updated_at: context_row.user_updated_at,
        };

        if !user.is_active {
            return Err(BearerAuthError::unauthorized("admin user is inactive"));
        }

        let roles = context_row.roles;
        let permissions = PermissionSet::from_strs(context_row.permission_keys)
            .map_err(BearerAuthError::internal)?;

        admin_auth::repository::update_admin_access_token_last_used(self.pool, token.id)
            .await
            .map_err(BearerAuthError::internal)?;

        Ok(AdminAccessTokenContext {
            token,
            user,
            roles,
            permissions,
        })
    }

    pub async fn authenticate_any_bearer_token(
        &self,
        raw_token: &str,
    ) -> std::result::Result<BearerTokenContext, BearerAuthError> {
        let token_hash = hash_token(raw_token);
        let Some(token_kind) = self
            .lookup_bearer_token_kind(&token_hash)
            .await
            .map_err(BearerAuthError::internal)?
        else {
            return Err(BearerAuthError::unauthorized("invalid bearer token"));
        };

        match token_kind.as_str() {
            "admin" => match self.authenticate_admin_bearer_token(raw_token).await {
                Ok(admin_context) => Ok(BearerTokenContext::AdminUser(admin_context)),
                Err(BearerAuthError::Internal(error)) => Err(BearerAuthError::Internal(error)),
                Err(BearerAuthError::Unauthorized(_)) => {
                    Err(BearerAuthError::unauthorized("invalid bearer token"))
                }
            },
            "api" => match self.authenticate_bearer_token(raw_token, None, None).await {
                Ok(api_context) => Ok(BearerTokenContext::ApiClient(api_context)),
                Err(BearerAuthError::Internal(error)) => Err(BearerAuthError::Internal(error)),
                Err(BearerAuthError::Unauthorized(_)) => {
                    Err(BearerAuthError::unauthorized("invalid bearer token"))
                }
            },
            _ => Err(BearerAuthError::unauthorized("invalid bearer token")),
        }
    }

    pub async fn revoke_admin_access_token(
        &self,
        admin_access_token_id: i64,
        revoked_reason: Option<&str>,
    ) -> Result<()> {
        admin_auth::repository::revoke_admin_access_token(
            self.pool,
            admin_access_token_id,
            revoked_reason,
        )
        .await
    }

    pub async fn revoke_access_token(
        &self,
        raw_token: &str,
        revoked_reason: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        let token_hash = hash_token(raw_token);
        let Some(token) =
            api_auth::repository::get_access_token_by_token_hash(self.pool, &token_hash).await?
        else {
            bail!("access token not found");
        };

        api_auth::repository::revoke_access_token(self.pool, token.id, revoked_reason).await?;

        let client =
            api_auth::repository::get_api_client_by_id(self.pool, token.api_client_id).await?;
        let client_id = client.as_ref().map(|item| item.id);

        self.write_audit_log(client_id, "token_revoked", ip_address, user_agent)
            .await?;

        Ok(())
    }

    pub async fn revoke_access_token_by_id(
        &self,
        access_token_id: i64,
        api_client_id: i64,
        revoked_reason: Option<&str>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<()> {
        api_auth::repository::revoke_access_token(self.pool, access_token_id, revoked_reason)
            .await?;
        self.write_audit_log(Some(api_client_id), "token_revoked", ip_address, user_agent)
            .await?;
        Ok(())
    }

    pub fn require_scope(scopes: &[String], required_scope: &str) -> Result<()> {
        if has_scope(scopes, required_scope) {
            return Ok(());
        }

        bail!("missing required scope: {required_scope}");
    }

    pub fn require_permission(
        permissions: &PermissionSet,
        required_permission: PermissionKey,
    ) -> Result<()> {
        if permissions.contains(required_permission) {
            return Ok(());
        }

        bail!(
            "missing required permission: {}",
            required_permission.as_str()
        );
    }

    async fn lookup_bearer_token_kind(&self, token_hash: &str) -> Result<Option<String>> {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT token_kind
            FROM (
                SELECT 'admin'::TEXT AS token_kind, 0 AS priority
                FROM admin_access_tokens
                WHERE token_hash = $1

                UNION ALL

                SELECT 'api'::TEXT AS token_kind, 1 AS priority
                FROM access_tokens
                WHERE token_hash = $1
            ) bearer_tokens
            ORDER BY priority
            LIMIT 1
            "#,
        )
        .bind(token_hash)
        .fetch_optional(self.pool)
        .await
        .map_err(Into::into)
    }

    async fn load_client_scopes(&self, api_client_id: i64) -> Result<Vec<String>> {
        let scopes = api_auth::repository::list_api_client_scopes(self.pool, api_client_id)
            .await?
            .into_iter()
            .map(|item| item.scope)
            .collect::<Vec<_>>();

        Ok(scopes)
    }

    async fn write_auth_failed_audit(
        &self,
        api_client_id: Option<i64>,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<ApiClientAuditLog> {
        self.write_audit_log(api_client_id, "auth_failed", ip_address, user_agent)
            .await
    }

    async fn write_audit_log(
        &self,
        api_client_id: Option<i64>,
        action: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<ApiClientAuditLog> {
        api_auth::repository::create_api_client_audit_log(
            self.pool,
            api_auth::repository::NewApiClientAuditLog {
                api_client_id,
                action,
                ip_address,
                user_agent,
                meta_json: None,
            },
        )
        .await
    }

    async fn write_admin_auth_failed_audit(
        &self,
        admin_user_id: Option<i64>,
        email: &str,
        ip_address: Option<&str>,
        user_agent: Option<&str>,
    ) -> Result<AdminAuthAuditLog> {
        let meta_json = serde_json::json!({ "email": email });
        self.write_admin_auth_audit_log(AdminAuthAuditEvent {
            admin_user_id,
            action: "auth_failed",
            ip_address,
            user_agent,
            meta_json: Some(&meta_json),
        })
        .await
    }

    async fn write_admin_auth_audit_log(
        &self,
        event: AdminAuthAuditEvent<'_>,
    ) -> Result<AdminAuthAuditLog> {
        admin_auth::repository::create_admin_auth_audit_log(
            self.pool,
            admin_auth::repository::NewAdminAuthAuditLog {
                admin_user_id: event.admin_user_id,
                action: event.action,
                ip_address: event.ip_address,
                user_agent: event.user_agent,
                meta_json: event.meta_json,
            },
        )
        .await
    }
}

pub(crate) fn generate_raw_token() -> String {
    let mut bytes = [0_u8; TOKEN_BYTES];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub(crate) fn hash_token(raw_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw_token.as_bytes());
    hex::encode(hasher.finalize())
}

fn has_scope(scopes: &[String], required_scope: &str) -> bool {
    scopes.iter().any(|scope| scope == required_scope)
}

fn permissions_from_api_client_scopes(scopes: &[String]) -> PermissionSet {
    let mut permissions = Vec::new();

    for scope in scopes {
        match scope.as_str() {
            "sites:read" => permissions.extend([
                PermissionKey::SitesRead,
                PermissionKey::SiteMonitorsRead,
                PermissionKey::SiteChecksRead,
                PermissionKey::SiteIncidentsRead,
                PermissionKey::NotificationChannelsRead,
                PermissionKey::SiteNotificationChannelOverridesRead,
                PermissionKey::NotificationDeliveriesRead,
            ]),
            "sites:write" => permissions.extend([
                PermissionKey::SitesCreate,
                PermissionKey::SitesUpdate,
                PermissionKey::SitesDelete,
                PermissionKey::SiteMonitorsCreate,
                PermissionKey::SiteMonitorsUpdate,
                PermissionKey::SiteMonitorsDelete,
                PermissionKey::NotificationChannelsCreate,
                PermissionKey::NotificationChannelsUpdate,
                PermissionKey::NotificationChannelsDelete,
                PermissionKey::SiteNotificationChannelOverridesCreate,
                PermissionKey::SiteNotificationChannelOverridesUpdate,
                PermissionKey::SiteNotificationChannelOverridesDelete,
            ]),
            _ => {}
        }
    }

    PermissionSet::from_keys(permissions)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::api_auth::{AccessToken, ApiClient, ApiClientType};
    use sqlx::types::Uuid;

    #[test]
    fn hash_client_secret_round_trips() {
        let secret = "super-secret";

        let secret_hash =
            AuthService::hash_client_secret(secret).expect("client secret should hash");

        let verified = AuthService::verify_client_secret(&secret_hash, secret)
            .expect("client secret should verify");

        assert!(verified);
    }

    #[test]
    fn verify_client_secret_returns_false_for_wrong_secret() {
        let secret_hash =
            AuthService::hash_client_secret("correct-secret").expect("client secret should hash");

        let verified = AuthService::verify_client_secret(&secret_hash, "wrong-secret")
            .expect("verification should complete");

        assert!(!verified);
    }

    #[test]
    fn generated_tokens_are_hex_and_prefixed_consistently() {
        let token = generate_raw_token();
        let prefix = token.chars().take(TOKEN_PREFIX_LEN).collect::<String>();

        assert_eq!(token.len(), TOKEN_BYTES * 2);
        assert_eq!(prefix.len(), TOKEN_PREFIX_LEN);
        assert!(token.chars().all(|char| char.is_ascii_hexdigit()));
    }

    #[test]
    fn token_hash_is_stable() {
        let token = "same-token";

        let left = hash_token(token);
        let right = hash_token(token);

        assert_eq!(left, right);
        assert_eq!(left.len(), 64);
    }

    #[test]
    fn require_scope_accepts_present_scope() {
        let scopes = vec!["sites:read".to_string(), "checks:write".to_string()];

        let result = AuthService::require_scope(&scopes, "checks:write");

        assert!(result.is_ok());
    }

    #[test]
    fn require_scope_rejects_missing_scope() {
        let scopes = vec!["sites:read".to_string()];

        let result = AuthService::require_scope(&scopes, "checks:write");

        assert!(result.is_err());
    }

    #[test]
    fn hash_password_round_trips() {
        let password_hash = AuthService::hash_password("correct-horse-battery-staple")
            .expect("password should hash");

        let verified =
            AuthService::verify_password_hash(&password_hash, "correct-horse-battery-staple")
                .expect("password verification should complete");

        assert!(verified);
    }

    #[test]
    fn permissions_are_derived_from_api_client_scopes() {
        let permissions = permissions_from_api_client_scopes(&[
            "sites:read".to_string(),
            "sites:write".to_string(),
        ]);

        assert!(permissions.contains(PermissionKey::SitesRead));
        assert!(permissions.contains(PermissionKey::SitesUpdate));
        assert!(permissions.contains(PermissionKey::NotificationDeliveriesRead));
        assert!(permissions.contains(PermissionKey::NotificationChannelsUpdate));
        assert!(permissions.contains(PermissionKey::SiteNotificationChannelOverridesDelete));
    }

    #[test]
    fn authenticate_any_bearer_token_prioritizes_internal_admin_errors() {
        let result = combine_any_bearer_token_results(
            Err(BearerAuthError::internal(anyhow!("database unavailable"))),
            Ok(build_api_context()),
        );

        let Err(BearerAuthError::Internal(error)) = result else {
            panic!("expected internal admin error to short-circuit");
        };

        assert_eq!(error.to_string(), "database unavailable");
    }

    #[test]
    fn authenticate_any_bearer_token_returns_generic_unauthorized_when_both_paths_reject() {
        let result = combine_any_bearer_token_results(
            Err(BearerAuthError::unauthorized("invalid admin access token")),
            Err(BearerAuthError::unauthorized("invalid access token")),
        );

        let Err(BearerAuthError::Unauthorized(error)) = result else {
            panic!("expected unauthorized error");
        };

        assert_eq!(error.to_string(), "invalid bearer token");
    }

    fn combine_any_bearer_token_results(
        admin_result: std::result::Result<AdminAccessTokenContext, BearerAuthError>,
        api_result: std::result::Result<AccessTokenContext, BearerAuthError>,
    ) -> std::result::Result<BearerTokenContext, BearerAuthError> {
        match admin_result {
            Ok(admin_context) => Ok(BearerTokenContext::AdminUser(admin_context)),
            Err(BearerAuthError::Internal(error)) => Err(BearerAuthError::Internal(error)),
            Err(BearerAuthError::Unauthorized(_)) => match api_result {
                Ok(api_context) => Ok(BearerTokenContext::ApiClient(api_context)),
                Err(BearerAuthError::Internal(error)) => Err(BearerAuthError::Internal(error)),
                Err(BearerAuthError::Unauthorized(_)) => {
                    Err(BearerAuthError::unauthorized("invalid bearer token"))
                }
            },
        }
    }

    fn build_api_context() -> AccessTokenContext {
        let timestamp = Utc::now();

        AccessTokenContext {
            token: AccessToken {
                id: 1,
                api_client_id: 1,
                token_hash: "hash".to_string(),
                token_prefix: "prefix".to_string(),
                expires_at: timestamp,
                revoked_at: None,
                revoked_reason: None,
                last_used_at: None,
                created_at: timestamp,
            },
            client: ApiClient {
                id: 1,
                uuid: Uuid::nil(),
                name: "client".to_string(),
                description: None,
                client_type: ApiClientType::InstallationClient,
                client_id: "client-id".to_string(),
                client_secret_hash: "secret-hash".to_string(),
                secret_prefix: "secret-prefix".to_string(),
                is_active: true,
                last_used_at: None,
                created_by_user_id: None,
                created_at: timestamp,
                updated_at: timestamp,
            },
            scopes: vec!["sites:read".to_string()],
            permissions: PermissionSet::from_keys([PermissionKey::SitesRead]),
        }
    }
}
