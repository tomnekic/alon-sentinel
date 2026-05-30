use std::collections::HashMap;

use anyhow::Result;
use sqlx::PgPool;

use super::model::Permission;

#[derive(sqlx::FromRow)]
struct AdminUserPermissionKeyRow {
    admin_user_id: i64,
    permission_key: String,
}

#[derive(sqlx::FromRow)]
struct RolePermissionKeyRow {
    role_id: i64,
    permission_key: String,
}

pub async fn list_permissions(pool: &PgPool) -> Result<Vec<Permission>> {
    let permissions = sqlx::query_as::<_, Permission>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            created_at
        FROM permissions
        ORDER BY key, id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(permissions)
}

pub async fn list_permission_keys_for_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
) -> Result<Vec<String>> {
    let permission_keys = sqlx::query_scalar::<_, String>(
        r#"
        SELECT DISTINCT p.key
        FROM admin_user_roles aur
        INNER JOIN role_permissions rp ON rp.role_id = aur.role_id
        INNER JOIN permissions p ON p.id = rp.permission_id
        WHERE aur.admin_user_id = $1
        ORDER BY p.key
        "#,
    )
    .bind(admin_user_id)
    .fetch_all(pool)
    .await?;

    Ok(permission_keys)
}

pub async fn list_permission_keys_for_admin_users(
    pool: &PgPool,
    admin_user_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>> {
    if admin_user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, AdminUserPermissionKeyRow>(
        r#"
        SELECT DISTINCT
            aur.admin_user_id,
            p.key AS permission_key
        FROM admin_user_roles aur
        INNER JOIN role_permissions rp ON rp.role_id = aur.role_id
        INNER JOIN permissions p ON p.id = rp.permission_id
        WHERE aur.admin_user_id = ANY($1)
        ORDER BY aur.admin_user_id, p.key
        "#,
    )
    .bind(admin_user_ids)
    .fetch_all(pool)
    .await?;

    let mut permission_keys_by_user_id = HashMap::new();
    for row in rows {
        permission_keys_by_user_id
            .entry(row.admin_user_id)
            .or_insert_with(Vec::new)
            .push(row.permission_key);
    }

    Ok(permission_keys_by_user_id)
}

pub async fn list_permission_keys_for_role(pool: &PgPool, role_id: i64) -> Result<Vec<String>> {
    let permission_keys = sqlx::query_scalar::<_, String>(
        r#"
        SELECT p.key
        FROM role_permissions rp
        INNER JOIN permissions p ON p.id = rp.permission_id
        WHERE rp.role_id = $1
        ORDER BY p.key
        "#,
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;

    Ok(permission_keys)
}

pub async fn list_permission_keys_for_roles(
    pool: &PgPool,
    role_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>> {
    if role_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, RolePermissionKeyRow>(
        r#"
        SELECT
            rp.role_id,
            p.key AS permission_key
        FROM role_permissions rp
        INNER JOIN permissions p ON p.id = rp.permission_id
        WHERE rp.role_id = ANY($1)
        ORDER BY rp.role_id, p.key
        "#,
    )
    .bind(role_ids)
    .fetch_all(pool)
    .await?;

    let mut permission_keys_by_role_id = HashMap::new();
    for row in rows {
        permission_keys_by_role_id
            .entry(row.role_id)
            .or_insert_with(Vec::new)
            .push(row.permission_key);
    }

    Ok(permission_keys_by_role_id)
}

pub async fn list_permissions_by_keys(
    pool: &PgPool,
    permission_keys: &[String],
) -> Result<Vec<Permission>> {
    let permissions = sqlx::query_as::<_, Permission>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            created_at
        FROM permissions
        WHERE key = ANY($1)
        ORDER BY key, id
        "#,
    )
    .bind(permission_keys)
    .fetch_all(pool)
    .await?;

    Ok(permissions)
}

pub async fn replace_permissions_for_role(
    pool: &PgPool,
    role_id: i64,
    permission_ids: &[i64],
) -> Result<()> {
    let mut transaction = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM role_permissions
        WHERE role_id = $1
        "#,
    )
    .bind(role_id)
    .execute(&mut *transaction)
    .await?;

    for permission_id in permission_ids {
        sqlx::query(
            r#"
            INSERT INTO role_permissions (
                role_id,
                permission_id
            )
            VALUES ($1, $2)
            ON CONFLICT (role_id, permission_id) DO NOTHING
            "#,
        )
        .bind(role_id)
        .bind(permission_id)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;

    Ok(())
}
