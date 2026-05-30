use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use sqlx::types::{JsonValue, Uuid};

use super::model::{AdminAccessToken, AdminAuthAuditLog};

pub struct NewAdminAccessToken<'a> {
    pub admin_user_id: i64,
    pub token_hash: &'a str,
    pub token_prefix: &'a str,
    pub expires_at: DateTime<Utc>,
}

pub struct NewAdminAuthAuditLog<'a> {
    pub admin_user_id: Option<i64>,
    pub action: &'a str,
    pub ip_address: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub meta_json: Option<&'a JsonValue>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AdminAccessTokenAuthContextRow {
    pub access_token_id: i64,
    pub access_token_admin_user_id: i64,
    pub access_token_token_hash: String,
    pub access_token_token_prefix: String,
    pub access_token_expires_at: DateTime<Utc>,
    pub access_token_revoked_at: Option<DateTime<Utc>>,
    pub access_token_revoked_reason: Option<String>,
    pub access_token_last_used_at: Option<DateTime<Utc>>,
    pub access_token_created_at: DateTime<Utc>,
    pub user_id: i64,
    pub user_uuid: Uuid,
    pub user_email: String,
    pub user_display_name: String,
    pub user_password_hash: String,
    pub user_is_active: bool,
    pub user_last_login_at: Option<DateTime<Utc>>,
    pub user_created_at: DateTime<Utc>,
    pub user_updated_at: DateTime<Utc>,
    pub roles: Vec<String>,
    pub permission_keys: Vec<String>,
}

pub async fn create_admin_access_token(
    pool: &PgPool,
    new_token: NewAdminAccessToken<'_>,
) -> Result<AdminAccessToken> {
    let token = sqlx::query_as::<_, AdminAccessToken>(
        r#"
        INSERT INTO admin_access_tokens (
            admin_user_id,
            token_hash,
            token_prefix,
            expires_at
        )
        VALUES ($1, $2, $3, $4)
        RETURNING
            id,
            admin_user_id,
            token_hash,
            token_prefix,
            expires_at,
            revoked_at,
            revoked_reason,
            last_used_at,
            created_at
        "#,
    )
    .bind(new_token.admin_user_id)
    .bind(new_token.token_hash)
    .bind(new_token.token_prefix)
    .bind(new_token.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(token)
}

pub async fn get_admin_access_token_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<AdminAccessToken>> {
    let token = sqlx::query_as::<_, AdminAccessToken>(
        r#"
        SELECT
            id,
            admin_user_id,
            token_hash,
            token_prefix,
            expires_at,
            revoked_at,
            revoked_reason,
            last_used_at,
            created_at
        FROM admin_access_tokens
        WHERE token_hash = $1
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(token)
}

pub async fn get_admin_access_token_auth_context_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<AdminAccessTokenAuthContextRow>> {
    let context = sqlx::query_as::<_, AdminAccessTokenAuthContextRow>(
        r#"
        SELECT
            aat.id AS access_token_id,
            aat.admin_user_id AS access_token_admin_user_id,
            aat.token_hash AS access_token_token_hash,
            aat.token_prefix AS access_token_token_prefix,
            aat.expires_at AS access_token_expires_at,
            aat.revoked_at AS access_token_revoked_at,
            aat.revoked_reason AS access_token_revoked_reason,
            aat.last_used_at AS access_token_last_used_at,
            aat.created_at AS access_token_created_at,
            au.id AS user_id,
            au.uuid AS user_uuid,
            au.email AS user_email,
            au.display_name AS user_display_name,
            au.password_hash AS user_password_hash,
            au.is_active AS user_is_active,
            au.last_login_at AS user_last_login_at,
            au.created_at AS user_created_at,
            au.updated_at AS user_updated_at,
            COALESCE(
                ARRAY_AGG(DISTINCT r.key ORDER BY r.key)
                    FILTER (WHERE r.key IS NOT NULL),
                ARRAY[]::TEXT[]
            ) AS roles,
            COALESCE(
                ARRAY_AGG(DISTINCT p.key ORDER BY p.key)
                    FILTER (WHERE p.key IS NOT NULL),
                ARRAY[]::TEXT[]
            ) AS permission_keys
        FROM admin_access_tokens aat
        INNER JOIN admin_users au ON au.id = aat.admin_user_id
        LEFT JOIN admin_user_roles aur ON aur.admin_user_id = au.id
        LEFT JOIN roles r ON r.id = aur.role_id
        LEFT JOIN role_permissions rp ON rp.role_id = r.id
        LEFT JOIN permissions p ON p.id = rp.permission_id
        WHERE aat.token_hash = $1
        GROUP BY
            aat.id,
            aat.admin_user_id,
            aat.token_hash,
            aat.token_prefix,
            aat.expires_at,
            aat.revoked_at,
            aat.revoked_reason,
            aat.last_used_at,
            aat.created_at,
            au.id,
            au.uuid,
            au.email,
            au.display_name,
            au.password_hash,
            au.is_active,
            au.last_login_at,
            au.created_at,
            au.updated_at
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(context)
}

pub async fn update_admin_access_token_last_used(
    pool: &PgPool,
    admin_access_token_id: i64,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE admin_access_tokens
        SET last_used_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(admin_access_token_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_active_tokens_for_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
) -> Result<Vec<AdminAccessToken>> {
    let tokens = sqlx::query_as::<_, AdminAccessToken>(
        r#"
        SELECT
            id,
            admin_user_id,
            token_hash,
            token_prefix,
            expires_at,
            revoked_at,
            revoked_reason,
            last_used_at,
            created_at
        FROM admin_access_tokens
        WHERE admin_user_id = $1
          AND revoked_at IS NULL
          AND expires_at > NOW()
        "#,
    )
    .bind(admin_user_id)
    .fetch_all(pool)
    .await?;

    Ok(tokens)
}

pub async fn revoke_admin_access_token(
    pool: &PgPool,
    admin_access_token_id: i64,
    revoked_reason: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE admin_access_tokens
        SET
            revoked_at = NOW(),
            revoked_reason = $2
        WHERE id = $1
        "#,
    )
    .bind(admin_access_token_id)
    .bind(revoked_reason)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_admin_auth_audit_log(
    pool: &PgPool,
    new_log: NewAdminAuthAuditLog<'_>,
) -> Result<AdminAuthAuditLog> {
    let audit_log = sqlx::query_as::<_, AdminAuthAuditLog>(
        r#"
        INSERT INTO admin_auth_audit_logs (
            admin_user_id,
            action,
            ip_address,
            user_agent,
            meta_json
        )
        VALUES ($1, $2, CAST($3 AS INET), $4, $5)
        RETURNING
            id,
            admin_user_id,
            action,
            host(ip_address) AS ip_address,
            user_agent,
            meta_json,
            created_at
        "#,
    )
    .bind(new_log.admin_user_id)
    .bind(new_log.action)
    .bind(new_log.ip_address)
    .bind(new_log.user_agent)
    .bind(new_log.meta_json)
    .fetch_one(pool)
    .await?;

    Ok(audit_log)
}
