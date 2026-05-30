use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{
    PgPool,
    types::{JsonValue, Uuid},
};

use super::model::{AccessToken, ApiClient, ApiClientAuditLog, ApiClientScope, ApiClientType};

pub struct NewApiClient<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub client_type: ApiClientType,
    pub client_id: &'a str,
    pub client_secret_hash: &'a str,
    pub secret_prefix: &'a str,
    pub created_by_user_id: Option<&'a str>,
}

pub struct NewAccessToken<'a> {
    pub api_client_id: i64,
    pub token_hash: &'a str,
    pub token_prefix: &'a str,
    pub expires_at: DateTime<Utc>,
}

pub struct NewApiClientAuditLog<'a> {
    pub api_client_id: Option<i64>,
    pub action: &'a str,
    pub ip_address: Option<&'a str>,
    pub user_agent: Option<&'a str>,
    pub meta_json: Option<&'a JsonValue>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct AccessTokenAuthContextRow {
    pub access_token_id: i64,
    pub access_token_api_client_id: i64,
    pub access_token_token_hash: String,
    pub access_token_token_prefix: String,
    pub access_token_expires_at: DateTime<Utc>,
    pub access_token_revoked_at: Option<DateTime<Utc>>,
    pub access_token_revoked_reason: Option<String>,
    pub access_token_last_used_at: Option<DateTime<Utc>>,
    pub access_token_created_at: DateTime<Utc>,
    pub client_id: i64,
    pub client_uuid: Uuid,
    pub client_name: String,
    pub client_description: Option<String>,
    pub client_type: ApiClientType,
    pub client_client_id: String,
    pub client_client_secret_hash: String,
    pub client_secret_prefix: String,
    pub client_is_active: bool,
    pub client_last_used_at: Option<DateTime<Utc>>,
    pub client_created_by_user_id: Option<String>,
    pub client_created_at: DateTime<Utc>,
    pub client_updated_at: DateTime<Utc>,
    pub scopes: Vec<String>,
}

pub async fn create_api_client(pool: &PgPool, new_client: NewApiClient<'_>) -> Result<ApiClient> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        INSERT INTO api_clients (
            name,
            description,
            type,
            client_id,
            client_secret_hash,
            secret_prefix,
            created_by_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        RETURNING
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        "#,
    )
    .bind(new_client.name)
    .bind(new_client.description)
    .bind(new_client.client_type)
    .bind(new_client.client_id)
    .bind(new_client.client_secret_hash)
    .bind(new_client.secret_prefix)
    .bind(new_client.created_by_user_id)
    .fetch_one(pool)
    .await?;

    Ok(client)
}

pub async fn upsert_api_client(pool: &PgPool, new_client: NewApiClient<'_>) -> Result<ApiClient> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        INSERT INTO api_clients (
            name,
            description,
            type,
            client_id,
            client_secret_hash,
            secret_prefix,
            created_by_user_id
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        ON CONFLICT (client_id) DO UPDATE
        SET
            name = EXCLUDED.name,
            description = EXCLUDED.description,
            type = EXCLUDED.type,
            client_secret_hash = EXCLUDED.client_secret_hash,
            secret_prefix = EXCLUDED.secret_prefix,
            is_active = TRUE,
            updated_at = NOW()
        RETURNING
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        "#,
    )
    .bind(new_client.name)
    .bind(new_client.description)
    .bind(new_client.client_type)
    .bind(new_client.client_id)
    .bind(new_client.client_secret_hash)
    .bind(new_client.secret_prefix)
    .bind(new_client.created_by_user_id)
    .fetch_one(pool)
    .await?;

    Ok(client)
}

pub async fn get_api_client_by_id(pool: &PgPool, api_client_id: i64) -> Result<Option<ApiClient>> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        SELECT
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        FROM api_clients
        WHERE id = $1
        "#,
    )
    .bind(api_client_id)
    .fetch_optional(pool)
    .await?;

    Ok(client)
}

pub async fn get_api_client_by_client_id(
    pool: &PgPool,
    client_id: &str,
) -> Result<Option<ApiClient>> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        SELECT
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        FROM api_clients
        WHERE client_id = $1
        "#,
    )
    .bind(client_id)
    .fetch_optional(pool)
    .await?;

    Ok(client)
}

pub async fn update_api_client_last_used(pool: &PgPool, api_client_id: i64) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE api_clients
        SET
            last_used_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(api_client_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn create_api_client_scope(
    pool: &PgPool,
    api_client_id: i64,
    scope: &str,
) -> Result<ApiClientScope> {
    let api_client_scope = sqlx::query_as::<_, ApiClientScope>(
        r#"
        INSERT INTO api_client_scopes (
            api_client_id,
            scope
        )
        VALUES ($1, $2)
        ON CONFLICT (api_client_id, scope) DO UPDATE
        SET scope = EXCLUDED.scope
        RETURNING
            id,
            api_client_id,
            scope,
            created_at
        "#,
    )
    .bind(api_client_id)
    .bind(scope)
    .fetch_one(pool)
    .await?;

    Ok(api_client_scope)
}

pub async fn list_api_client_scopes(
    pool: &PgPool,
    api_client_id: i64,
) -> Result<Vec<ApiClientScope>> {
    let api_client_scopes = sqlx::query_as::<_, ApiClientScope>(
        r#"
        SELECT
            id,
            api_client_id,
            scope,
            created_at
        FROM api_client_scopes
        WHERE api_client_id = $1
        ORDER BY scope
        "#,
    )
    .bind(api_client_id)
    .fetch_all(pool)
    .await?;

    Ok(api_client_scopes)
}

pub async fn create_access_token(
    pool: &PgPool,
    new_token: NewAccessToken<'_>,
) -> Result<AccessToken> {
    let access_token = sqlx::query_as::<_, AccessToken>(
        r#"
        INSERT INTO access_tokens (
            api_client_id,
            token_hash,
            token_prefix,
            expires_at
        )
        VALUES ($1, $2, $3, $4)
        RETURNING
            id,
            api_client_id,
            token_hash,
            token_prefix,
            expires_at,
            revoked_at,
            revoked_reason,
            last_used_at,
            created_at
        "#,
    )
    .bind(new_token.api_client_id)
    .bind(new_token.token_hash)
    .bind(new_token.token_prefix)
    .bind(new_token.expires_at)
    .fetch_one(pool)
    .await?;

    Ok(access_token)
}

pub async fn get_access_token_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<AccessToken>> {
    let access_token = sqlx::query_as::<_, AccessToken>(
        r#"
        SELECT
            id,
            api_client_id,
            token_hash,
            token_prefix,
            expires_at,
            revoked_at,
            revoked_reason,
            last_used_at,
            created_at
        FROM access_tokens
        WHERE token_hash = $1
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(access_token)
}

pub async fn get_access_token_auth_context_by_token_hash(
    pool: &PgPool,
    token_hash: &str,
) -> Result<Option<AccessTokenAuthContextRow>> {
    let context = sqlx::query_as::<_, AccessTokenAuthContextRow>(
        r#"
        SELECT
            at.id AS access_token_id,
            at.api_client_id AS access_token_api_client_id,
            at.token_hash AS access_token_token_hash,
            at.token_prefix AS access_token_token_prefix,
            at.expires_at AS access_token_expires_at,
            at.revoked_at AS access_token_revoked_at,
            at.revoked_reason AS access_token_revoked_reason,
            at.last_used_at AS access_token_last_used_at,
            at.created_at AS access_token_created_at,
            ac.id AS client_id,
            ac.uuid AS client_uuid,
            ac.name AS client_name,
            ac.description AS client_description,
            ac.type AS client_type,
            ac.client_id AS client_client_id,
            ac.client_secret_hash AS client_client_secret_hash,
            ac.secret_prefix AS client_secret_prefix,
            ac.is_active AS client_is_active,
            ac.last_used_at AS client_last_used_at,
            ac.created_by_user_id AS client_created_by_user_id,
            ac.created_at AS client_created_at,
            ac.updated_at AS client_updated_at,
            COALESCE(
                ARRAY_AGG(DISTINCT acs.scope ORDER BY acs.scope)
                    FILTER (WHERE acs.scope IS NOT NULL),
                ARRAY[]::TEXT[]
            ) AS scopes
        FROM access_tokens at
        INNER JOIN api_clients ac ON ac.id = at.api_client_id
        LEFT JOIN api_client_scopes acs ON acs.api_client_id = ac.id
        WHERE at.token_hash = $1
        GROUP BY
            at.id,
            at.api_client_id,
            at.token_hash,
            at.token_prefix,
            at.expires_at,
            at.revoked_at,
            at.revoked_reason,
            at.last_used_at,
            at.created_at,
            ac.id,
            ac.uuid,
            ac.name,
            ac.description,
            ac.type,
            ac.client_id,
            ac.client_secret_hash,
            ac.secret_prefix,
            ac.is_active,
            ac.last_used_at,
            ac.created_by_user_id,
            ac.created_at,
            ac.updated_at
        "#,
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    Ok(context)
}

pub async fn update_access_token_last_used(pool: &PgPool, access_token_id: i64) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE access_tokens
        SET last_used_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(access_token_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn revoke_access_token(
    pool: &PgPool,
    access_token_id: i64,
    revoked_reason: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE access_tokens
        SET
            revoked_at = NOW(),
            revoked_reason = $2
        WHERE id = $1
        "#,
    )
    .bind(access_token_id)
    .bind(revoked_reason)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn list_api_clients(pool: &PgPool) -> Result<Vec<ApiClient>> {
    let clients = sqlx::query_as::<_, ApiClient>(
        r#"
        SELECT
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        FROM api_clients
        ORDER BY created_at DESC
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(clients)
}

pub async fn update_api_client(
    pool: &PgPool,
    id: i64,
    name: &str,
    description: Option<&str>,
    is_active: bool,
) -> Result<Option<ApiClient>> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        UPDATE api_clients
        SET
            name = $2,
            description = $3,
            is_active = $4,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        "#,
    )
    .bind(id)
    .bind(name)
    .bind(description)
    .bind(is_active)
    .fetch_optional(pool)
    .await?;

    Ok(client)
}

pub async fn rotate_api_client_secret(
    pool: &PgPool,
    id: i64,
    new_secret_hash: &str,
    new_secret_prefix: &str,
) -> Result<Option<ApiClient>> {
    let client = sqlx::query_as::<_, ApiClient>(
        r#"
        UPDATE api_clients
        SET
            client_secret_hash = $2,
            secret_prefix = $3,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            uuid,
            name,
            description,
            type AS client_type,
            client_id,
            client_secret_hash,
            secret_prefix,
            is_active,
            last_used_at,
            created_by_user_id,
            created_at,
            updated_at
        "#,
    )
    .bind(id)
    .bind(new_secret_hash)
    .bind(new_secret_prefix)
    .fetch_optional(pool)
    .await?;

    Ok(client)
}

pub async fn delete_api_client(pool: &PgPool, id: i64) -> Result<bool> {
    let result = sqlx::query("DELETE FROM api_clients WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() == 1)
}

pub async fn replace_api_client_scopes(
    pool: &PgPool,
    api_client_id: i64,
    scopes: &[String],
) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM api_client_scopes WHERE api_client_id = $1")
        .bind(api_client_id)
        .execute(&mut *tx)
        .await?;

    for scope in scopes {
        sqlx::query("INSERT INTO api_client_scopes (api_client_id, scope) VALUES ($1, $2)")
            .bind(api_client_id)
            .bind(scope)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn create_api_client_audit_log(
    pool: &PgPool,
    new_log: NewApiClientAuditLog<'_>,
) -> Result<ApiClientAuditLog> {
    let audit_log = sqlx::query_as::<_, ApiClientAuditLog>(
        r#"
        INSERT INTO api_client_audit_logs (
            api_client_id,
            action,
            ip_address,
            user_agent,
            meta_json
        )
        VALUES ($1, $2, CAST($3 AS INET), $4, $5)
        RETURNING
            id,
            api_client_id,
            action,
            host(ip_address) AS ip_address,
            user_agent,
            meta_json,
            created_at
        "#,
    )
    .bind(new_log.api_client_id)
    .bind(new_log.action)
    .bind(new_log.ip_address)
    .bind(new_log.user_agent)
    .bind(new_log.meta_json)
    .fetch_one(pool)
    .await?;

    Ok(audit_log)
}
