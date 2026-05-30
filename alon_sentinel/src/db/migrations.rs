use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use sqlx::PgPool;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AppliedMigration {
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
}

pub async fn run_pending_migrations(pool: &PgPool) -> Result<MigrationReport> {
    ensure_migrations_table(pool).await?;
    import_legacy_sqlx_migrations(pool).await?;

    let applied = list_applied_migrations(pool).await?;
    let applied_set = applied
        .into_iter()
        .map(|migration| migration.filename)
        .collect::<std::collections::BTreeSet<_>>();

    let mut report = MigrationReport {
        applied: Vec::new(),
        skipped: Vec::new(),
    };

    for path in migration_paths()? {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| anyhow::anyhow!("invalid migration filename: {}", path.display()))?
            .to_string();

        if applied_set.contains(&filename) {
            report.skipped.push(filename);
            continue;
        }

        let sql = fs::read_to_string(&path)
            .with_context(|| format!("failed to read migration {}", path.display()))?;

        let mut transaction = pool.begin().await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("failed to execute migration {}", path.display()))?;

        sqlx::query(
            r#"
            INSERT INTO schema_migrations (filename)
            VALUES ($1)
            "#,
        )
        .bind(&filename)
        .execute(&mut *transaction)
        .await
        .with_context(|| format!("failed to record migration {}", filename))?;

        transaction.commit().await?;
        report.applied.push(filename);
    }

    Ok(report)
}

async fn import_legacy_sqlx_migrations(pool: &PgPool) -> Result<()> {
    let applied_count: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM schema_migrations
        "#,
    )
    .fetch_one(pool)
    .await?;

    if applied_count > 0 {
        return Ok(());
    }

    let has_legacy_table: Option<String> = sqlx::query_scalar(
        r#"
        SELECT to_regclass('public._sqlx_migrations')::text
        "#,
    )
    .fetch_one(pool)
    .await?;

    if has_legacy_table.is_none() {
        return Ok(());
    }

    let legacy_versions: Vec<i64> = sqlx::query_scalar(
        r#"
        SELECT version
        FROM _sqlx_migrations
        WHERE success = TRUE
        ORDER BY version
        "#,
    )
    .fetch_all(pool)
    .await?;

    if legacy_versions.is_empty() {
        return Ok(());
    }

    let paths = migration_paths()?;

    for version in legacy_versions {
        let version_prefix = version.to_string();
        let Some(filename) = paths
            .iter()
            .filter_map(|path| path.file_name().and_then(|value| value.to_str()))
            .find(|filename| filename.starts_with(&version_prefix))
        else {
            continue;
        };

        sqlx::query(
            r#"
            INSERT INTO schema_migrations (filename)
            VALUES ($1)
            ON CONFLICT (filename) DO NOTHING
            "#,
        )
        .bind(filename)
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn ensure_migrations_table(pool: &PgPool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS schema_migrations (
            id BIGSERIAL PRIMARY KEY,
            filename TEXT NOT NULL,
            applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            CONSTRAINT uq_schema_migrations_filename UNIQUE (filename)
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn list_applied_migrations(pool: &PgPool) -> Result<Vec<AppliedMigration>> {
    let rows = sqlx::query_as::<_, AppliedMigration>(
        r#"
        SELECT filename
        FROM schema_migrations
        ORDER BY filename
        "#,
    )
    .fetch_all(pool)
    .await?;

    Ok(rows)
}

fn migration_paths() -> Result<Vec<PathBuf>> {
    let migrations_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("migrations");
    if !migrations_dir.exists() {
        bail!(
            "migrations directory not found: {}",
            migrations_dir.display()
        );
    }

    let mut paths = fs::read_dir(&migrations_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()?;

    paths.retain(|path| {
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.eq_ignore_ascii_case("sql"))
            .unwrap_or(false)
    });
    paths.sort();

    Ok(paths)
}
