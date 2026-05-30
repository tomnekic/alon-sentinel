use anyhow::Result;
use sqlx::{PgPool, Postgres, Transaction};

use super::model::AdminUser;

pub struct NewAdminUser<'a> {
    pub email: &'a str,
    pub display_name: &'a str,
    pub password_hash: &'a str,
}

pub async fn create_admin_user(pool: &PgPool, new_user: NewAdminUser<'_>) -> Result<AdminUser> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        INSERT INTO admin_users (
            email,
            display_name,
            password_hash
        )
        VALUES ($1, $2, $3)
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(new_user.email)
    .bind(new_user.display_name)
    .bind(new_user.password_hash)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn create_admin_user_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    new_user: NewAdminUser<'_>,
) -> Result<AdminUser> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        INSERT INTO admin_users (
            email,
            display_name,
            password_hash
        )
        VALUES ($1, $2, $3)
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(new_user.email)
    .bind(new_user.display_name)
    .bind(new_user.password_hash)
    .fetch_one(&mut **tx)
    .await?;

    Ok(user)
}

pub async fn upsert_admin_user(pool: &PgPool, new_user: NewAdminUser<'_>) -> Result<AdminUser> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        INSERT INTO admin_users (
            email,
            display_name,
            password_hash
        )
        VALUES ($1, $2, $3)
        ON CONFLICT (email) DO UPDATE
        SET
            display_name = EXCLUDED.display_name,
            password_hash = EXCLUDED.password_hash,
            is_active = TRUE,
            updated_at = NOW()
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(new_user.email)
    .bind(new_user.display_name)
    .bind(new_user.password_hash)
    .fetch_one(pool)
    .await?;

    Ok(user)
}

pub async fn get_admin_user_by_email(pool: &PgPool, email: &str) -> Result<Option<AdminUser>> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        SELECT
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        FROM admin_users
        WHERE lower(email) = lower($1)
        "#,
    )
    .bind(email)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

pub async fn get_admin_user_by_id(pool: &PgPool, admin_user_id: i64) -> Result<Option<AdminUser>> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        SELECT
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        FROM admin_users
        WHERE id = $1
        "#,
    )
    .bind(admin_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

pub async fn list_admin_users(pool: &PgPool) -> Result<Vec<AdminUser>> {
    let users = sqlx::query_as::<_, AdminUser>(
        r#"
        SELECT
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        FROM admin_users
        ORDER BY lower(display_name), lower(email), id
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

pub async fn update_admin_user(
    pool: &PgPool,
    admin_user_id: i64,
    p: &super::model::AdminUserUpdateParams<'_>,
) -> Result<Option<AdminUser>> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        UPDATE admin_users
        SET
            email = $2,
            display_name = $3,
            password_hash = COALESCE($4, password_hash),
            is_active = $5,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(admin_user_id)
    .bind(p.email)
    .bind(p.display_name)
    .bind(p.password_hash)
    .bind(p.is_active)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

pub async fn update_admin_user_in_tx(
    tx: &mut Transaction<'_, Postgres>,
    admin_user_id: i64,
    p: &super::model::AdminUserUpdateParams<'_>,
) -> Result<Option<AdminUser>> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        UPDATE admin_users
        SET
            email = $2,
            display_name = $3,
            password_hash = COALESCE($4, password_hash),
            is_active = $5,
            updated_at = NOW()
        WHERE id = $1
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(admin_user_id)
    .bind(p.email)
    .bind(p.display_name)
    .bind(p.password_hash)
    .bind(p.is_active)
    .fetch_optional(&mut **tx)
    .await?;

    Ok(user)
}

pub async fn delete_admin_user(pool: &PgPool, admin_user_id: i64) -> Result<Option<AdminUser>> {
    let user = sqlx::query_as::<_, AdminUser>(
        r#"
        DELETE FROM admin_users
        WHERE id = $1
        RETURNING
            id,
            uuid,
            email,
            display_name,
            password_hash,
            is_active,
            last_login_at,
            created_at,
            updated_at
        "#,
    )
    .bind(admin_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}

pub async fn count_active_admin_users(pool: &PgPool) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM admin_users WHERE is_active = true
        "#,
    )
    .fetch_one(pool)
    .await?;

    Ok(count)
}

pub async fn update_admin_user_last_login(pool: &PgPool, admin_user_id: i64) -> Result<()> {
    sqlx::query(
        r#"
        UPDATE admin_users
        SET
            last_login_at = NOW(),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(admin_user_id)
    .execute(pool)
    .await?;

    Ok(())
}
