use std::collections::HashMap;

use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};

use super::model::{AdminUserRole, Role};

#[derive(sqlx::FromRow)]
struct AdminUserRoleKeyRow {
    admin_user_id: i64,
    role_key: String,
}

#[derive(sqlx::FromRow)]
struct PermissionRoleKeyRow {
    permission_id: i64,
    role_key: String,
}

pub async fn list_roles(pool: &PgPool) -> Result<Vec<Role>> {
    let roles = sqlx::query_as::<_, Role>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        FROM roles
        ORDER BY is_system DESC, key, id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(roles)
}

pub async fn get_role_by_key(pool: &PgPool, role_key: &str) -> Result<Option<Role>> {
    let role = sqlx::query_as::<_, Role>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        FROM roles
        WHERE key = $1
        "#,
    )
    .bind(role_key)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn get_role_by_id(pool: &PgPool, role_id: i64) -> Result<Option<Role>> {
    let role = sqlx::query_as::<_, Role>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        FROM roles
        WHERE id = $1
        "#,
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn list_roles_by_keys(pool: &PgPool, role_keys: &[String]) -> Result<Vec<Role>> {
    let roles = sqlx::query_as::<_, Role>(
        r#"
        SELECT
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        FROM roles
        WHERE key = ANY($1)
        ORDER BY key, id
        "#,
    )
    .bind(role_keys)
    .fetch_all(pool)
    .await?;

    Ok(roles)
}

pub async fn create_role(
    pool: &PgPool,
    key: &str,
    name: &str,
    description: Option<&str>,
) -> Result<Role> {
    let role = sqlx::query_as::<_, Role>(
        r#"
        INSERT INTO roles (
            key,
            name,
            description,
            is_system
        )
        VALUES ($1, $2, $3, FALSE)
        RETURNING
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        "#,
    )
    .bind(key)
    .bind(name)
    .bind(description)
    .fetch_one(pool)
    .await?;

    Ok(role)
}

pub async fn update_role(
    pool: &PgPool,
    role_id: i64,
    name: &str,
    description: Option<&str>,
) -> Result<Option<Role>> {
    let role = sqlx::query_as::<_, Role>(
        r#"
        UPDATE roles
        SET
            name = $2,
            description = $3,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        "#,
    )
    .bind(role_id)
    .bind(name)
    .bind(description)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn delete_role(pool: &PgPool, role_id: i64) -> Result<Option<Role>> {
    let role = sqlx::query_as::<_, Role>(
        r#"
        DELETE FROM roles
        WHERE id = $1
        RETURNING
            id,
            key,
            name,
            description,
            is_system,
            created_at,
            updated_at
        "#,
    )
    .bind(role_id)
    .fetch_optional(pool)
    .await?;

    Ok(role)
}

pub async fn assign_role_to_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
    role_id: i64,
) -> Result<AdminUserRole> {
    let admin_user_role = sqlx::query_as::<_, AdminUserRole>(
        r#"
        INSERT INTO admin_user_roles (
            admin_user_id,
            role_id
        )
        VALUES ($1, $2)
        ON CONFLICT (admin_user_id, role_id) DO UPDATE
        SET role_id = EXCLUDED.role_id
        RETURNING
            id,
            admin_user_id,
            role_id,
            created_at
        "#,
    )
    .bind(admin_user_id)
    .bind(role_id)
    .fetch_one(pool)
    .await?;

    Ok(admin_user_role)
}

pub async fn list_role_keys_for_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
) -> Result<Vec<String>> {
    let role_keys = sqlx::query_scalar::<_, String>(
        r#"
        SELECT r.key
        FROM admin_user_roles aur
        INNER JOIN roles r ON r.id = aur.role_id
        WHERE aur.admin_user_id = $1
        ORDER BY r.key
        "#,
    )
    .bind(admin_user_id)
    .fetch_all(pool)
    .await?;

    Ok(role_keys)
}

pub async fn list_role_keys_for_admin_users(
    pool: &PgPool,
    admin_user_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>> {
    if admin_user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, AdminUserRoleKeyRow>(
        r#"
        SELECT
            aur.admin_user_id,
            r.key AS role_key
        FROM admin_user_roles aur
        INNER JOIN roles r ON r.id = aur.role_id
        WHERE aur.admin_user_id = ANY($1)
        ORDER BY aur.admin_user_id, r.key
        "#,
    )
    .bind(admin_user_ids)
    .fetch_all(pool)
    .await?;

    let mut role_keys_by_user_id = HashMap::new();
    for row in rows {
        role_keys_by_user_id
            .entry(row.admin_user_id)
            .or_insert_with(Vec::new)
            .push(row.role_key);
    }

    Ok(role_keys_by_user_id)
}

pub async fn replace_roles_for_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
    role_ids: &[i64],
) -> Result<()> {
    let mut transaction = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM admin_user_roles
        WHERE admin_user_id = $1
        "#,
    )
    .bind(admin_user_id)
    .execute(&mut *transaction)
    .await?;

    for role_id in role_ids {
        sqlx::query(
            r#"
            INSERT INTO admin_user_roles (
                admin_user_id,
                role_id
            )
            VALUES ($1, $2)
            ON CONFLICT (admin_user_id, role_id) DO NOTHING
            "#,
        )
        .bind(admin_user_id)
        .bind(role_id)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;

    Ok(())
}

pub async fn replace_roles_for_admin_user_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    admin_user_id: i64,
    role_ids: &[i64],
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM admin_user_roles
        WHERE admin_user_id = $1
        "#,
    )
    .bind(admin_user_id)
    .execute(&mut **tx)
    .await?;

    for role_id in role_ids {
        sqlx::query(
            r#"
            INSERT INTO admin_user_roles (
                admin_user_id,
                role_id
            )
            VALUES ($1, $2)
            ON CONFLICT (admin_user_id, role_id) DO NOTHING
            "#,
        )
        .bind(admin_user_id)
        .bind(role_id)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

pub async fn list_admin_user_ids_for_role(pool: &PgPool, role_id: i64) -> Result<Vec<i64>> {
    let user_ids = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT admin_user_id
        FROM admin_user_roles
        WHERE role_id = $1
        "#,
    )
    .bind(role_id)
    .fetch_all(pool)
    .await?;

    Ok(user_ids)
}

pub async fn list_role_keys_for_permission(
    pool: &PgPool,
    permission_id: i64,
) -> Result<Vec<String>> {
    let role_keys = sqlx::query_scalar::<_, String>(
        r#"
        SELECT r.key
        FROM role_permissions rp
        INNER JOIN roles r ON r.id = rp.role_id
        WHERE rp.permission_id = $1
        ORDER BY r.key
        "#,
    )
    .bind(permission_id)
    .fetch_all(pool)
    .await?;

    Ok(role_keys)
}

pub async fn list_role_keys_for_permissions(
    pool: &PgPool,
    permission_ids: &[i64],
) -> Result<HashMap<i64, Vec<String>>> {
    if permission_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query_as::<_, PermissionRoleKeyRow>(
        r#"
        SELECT
            rp.permission_id,
            r.key AS role_key
        FROM role_permissions rp
        INNER JOIN roles r ON r.id = rp.role_id
        WHERE rp.permission_id = ANY($1)
        ORDER BY rp.permission_id, r.key
        "#,
    )
    .bind(permission_ids)
    .fetch_all(pool)
    .await?;

    let mut role_keys_by_permission_id = HashMap::new();
    for row in rows {
        role_keys_by_permission_id
            .entry(row.permission_id)
            .or_insert_with(Vec::new)
            .push(row.role_key);
    }

    Ok(role_keys_by_permission_id)
}
