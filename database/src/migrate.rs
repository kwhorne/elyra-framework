//! The migration engine — Elyra's `php artisan migrate`.
//!
//! Migrations are SQL files in a directory, named `<version>_<name>.sql` (the
//! "up"), with an optional `<version>_<name>.down.sql` (the "down", for
//! rollback). `version` is a numeric, sortable prefix. Applied migrations are
//! tracked in a `_elyra_migrations` table, grouped into **batches** so
//! `rollback` can undo the most recent `migrate` as a unit (like Laravel).
//!
//! Migrations can also be **Rust** ([`RustMigration`]), returning statements from
//! the [`Schema`](crate::Schema) builder instead of hand-written per-driver SQL.
//! Both kinds share the `_elyra_migrations` table and the same batch semantics, so
//! an app can mix them.
//!
//! Portability note: values written to the tracking table (version, name,
//! batch, timestamp) are validated to `[A-Za-z0-9_]` / integers and inlined, so
//! we avoid the per-backend placeholder differences of the `Any` driver.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::{AnyPool, Row};

use crate::error::{Error, Result};
use crate::Driver;

const TABLE: &str = "_elyra_migrations";

/// A discovered migration on disk.
pub struct Migration {
    pub version: String,
    pub name: String,
    up: PathBuf,
    down: Option<PathBuf>,
}

/// Whether a migration has been applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    Applied { batch: i64 },
    Pending,
}

/// A migration paired with its state, for `status`.
#[derive(Debug)]
pub struct MigrationStatus {
    pub version: String,
    pub name: String,
    pub state: MigrationState,
}

/// A migration written in Rust, typically built with [`Schema`](crate::Schema).
///
/// ```
/// use elyra_db::{RustMigration, Schema, Driver};
///
/// struct CreateUsers;
/// impl RustMigration for CreateUsers {
///     fn version(&self) -> &str { "20260801120000" }
///     fn name(&self) -> &str { "create_users_table" }
///     fn up(&self, driver: Driver) -> Vec<String> {
///         Schema::create("users", |t| { t.id(); t.string("email").unique(); }).to_sql(driver)
///     }
///     fn down(&self, driver: Driver) -> Vec<String> {
///         Schema::drop_if_exists("users").to_sql(driver)
///     }
/// }
///
/// assert!(CreateUsers.up(Driver::Sqlite)[0].contains("CREATE TABLE"));
/// ```
pub trait RustMigration: Send + Sync {
    /// A numeric, sortable version (e.g. `"20260801120000"`), unique per migration.
    fn version(&self) -> &str;

    /// A short snake_case name, recorded in the tracking table.
    fn name(&self) -> &str;

    /// Statements to apply, in order.
    fn up(&self, driver: Driver) -> Vec<String>;

    /// Statements to roll back, in order. Empty means "irreversible".
    fn down(&self, _driver: Driver) -> Vec<String> {
        Vec::new()
    }
}

/// Runs migrations against a pool.
pub struct Migrator {
    dir: PathBuf,
    pool: AnyPool,
}

impl Migrator {
    pub fn new(dir: PathBuf, pool: AnyPool) -> Self {
        Self { dir, pool }
    }

    /// Apply every pending migration as one new batch. Returns applied versions.
    pub async fn run(&self) -> Result<Vec<String>> {
        self.ensure_table().await?;
        let applied = self.applied().await?;
        let batch = applied.values().copied().max().unwrap_or(0) + 1;

        let mut done = Vec::new();
        for migration in self.discover()? {
            if applied.contains_key(&migration.version) {
                continue;
            }
            let sql = read(&migration.up)?;
            let insert = format!(
                "INSERT INTO {TABLE} (version, name, batch, applied_at) VALUES ('{}', '{}', {}, {})",
                migration.version,
                migration.name,
                batch,
                now(),
            );
            // Run the migration and record it atomically. On SQLite/Postgres a
            // failing script rolls back cleanly (transactional DDL); MySQL
            // auto-commits DDL, so a mid-file failure there may leave partial state.
            let mut tx = self.pool.begin().await?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(&mut *tx)
                .await?;
            sqlx::raw_sql(sqlx::AssertSqlSafe(insert))
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            done.push(migration.version);
        }
        Ok(done)
    }

    /// Apply every pending [`RustMigration`] as one new batch. Mixes freely with
    /// SQL-file migrations: both are tracked in the same table.
    pub async fn run_rust(
        &self,
        migrations: &[Box<dyn RustMigration>],
        driver: Driver,
    ) -> Result<Vec<String>> {
        self.ensure_table().await?;
        let applied = self.applied().await?;
        let batch = applied.values().copied().max().unwrap_or(0) + 1;

        let mut ordered: Vec<&Box<dyn RustMigration>> = migrations.iter().collect();
        ordered.sort_by_key(|m| m.version().to_string());

        let mut done = Vec::new();
        for migration in ordered {
            let version = migration.version().to_string();
            let name = migration.name().to_string();
            if applied.contains_key(&version) {
                continue;
            }
            validate_identifier(&version)?;
            validate_identifier(&name)?;

            let mut tx = self.pool.begin().await?;
            for statement in migration.up(driver) {
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "INSERT INTO {TABLE} (version, name, batch, applied_at) VALUES ('{version}', '{name}', {batch}, {})",
                now()
            )))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            done.push(version);
        }
        Ok(done)
    }

    /// Roll back the most recent batch of [`RustMigration`]s.
    pub async fn rollback_rust(
        &self,
        migrations: &[Box<dyn RustMigration>],
        driver: Driver,
    ) -> Result<Vec<String>> {
        self.ensure_table().await?;
        let Some(batch) = self.max_batch().await? else {
            return Ok(Vec::new());
        };
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT version FROM {TABLE} WHERE batch = {batch} ORDER BY version DESC"
        )))
        .fetch_all(&self.pool)
        .await?;
        let versions: Vec<String> = rows.iter().map(|r| r.get::<String, _>("version")).collect();

        let mut done = Vec::new();
        for version in versions {
            validate_identifier(&version)?;
            let statements = migrations
                .iter()
                .find(|m| m.version() == version)
                .map(|m| m.down(driver))
                .unwrap_or_default();

            let mut tx = self.pool.begin().await?;
            for statement in statements {
                sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {TABLE} WHERE version = '{version}'"
            )))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            done.push(version);
        }
        Ok(done)
    }

    /// Roll back the most recent batch (runs each migration's `.down.sql`).
    pub async fn rollback(&self) -> Result<Vec<String>> {
        self.ensure_table().await?;
        let Some(batch) = self.max_batch().await? else {
            return Ok(Vec::new());
        };

        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT version FROM {TABLE} WHERE batch = {batch} ORDER BY version DESC"
        )))
        .fetch_all(&self.pool)
        .await?;
        let versions: Vec<String> = rows.iter().map(|r| r.get::<String, _>("version")).collect();

        let migrations = self.discover()?;
        let mut done = Vec::new();
        for version in versions {
            let down_sql = match migrations
                .iter()
                .find(|m| m.version == version)
                .and_then(|m| m.down.as_ref())
            {
                Some(down) => Some(read(down)?),
                None => None,
            };
            // Run the down script (if any) and drop the history row atomically.
            let mut tx = self.pool.begin().await?;
            if let Some(sql) = down_sql {
                sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                    .execute(&mut *tx)
                    .await?;
            }
            sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {TABLE} WHERE version = '{version}'"
            )))
            .execute(&mut *tx)
            .await?;
            tx.commit().await?;
            done.push(version);
        }
        Ok(done)
    }

    /// Every discovered migration paired with its applied/pending state.
    pub async fn status(&self) -> Result<Vec<MigrationStatus>> {
        self.ensure_table().await?;
        let applied = self.applied().await?;
        Ok(self
            .discover()?
            .into_iter()
            .map(|m| {
                let state = match applied.get(&m.version) {
                    Some(&batch) => MigrationState::Applied { batch },
                    None => MigrationState::Pending,
                };
                MigrationStatus {
                    version: m.version,
                    name: m.name,
                    state,
                }
            })
            .collect())
    }

    async fn ensure_table(&self) -> Result<()> {
        let sql = format!(
            "CREATE TABLE IF NOT EXISTS {TABLE} (\
                version VARCHAR(64) PRIMARY KEY, \
                name VARCHAR(255) NOT NULL, \
                batch INTEGER NOT NULL, \
                applied_at BIGINT NOT NULL)"
        );
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn applied(&self) -> Result<BTreeMap<String, i64>> {
        let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT version, batch FROM {TABLE}"
        )))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("version"), r.get::<i64, _>("batch")))
            .collect())
    }

    async fn max_batch(&self) -> Result<Option<i64>> {
        let row = sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT MAX(batch) AS m FROM {TABLE}"
        )))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get::<i64, _>("m").ok())
    }

    fn discover(&self) -> Result<Vec<Migration>> {
        let mut migrations = Vec::new();
        if !self.dir.exists() {
            return Ok(migrations);
        }

        let mut paths: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map_err(io)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        paths.sort();

        for path in paths {
            let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            // Only "up" files; skip ".down.sql" companions and non-SQL.
            if !fname.ends_with(".sql") || fname.ends_with(".down.sql") {
                continue;
            }
            let stem = &fname[..fname.len() - ".sql".len()];
            if !stem.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(Error::InvalidMigration(fname.to_owned()));
            }
            let (version, name) = split_version(stem)?;

            let down = self.dir.join(format!("{stem}.down.sql"));
            let down = down.exists().then_some(down);

            migrations.push(Migration {
                version,
                name,
                up: path,
                down,
            });
        }

        migrations.sort_by(|a, b| a.version.cmp(&b.version));
        Ok(migrations)
    }
}

/// Values are inlined into the tracking-table statements (the `Any` driver's
/// placeholders differ per backend), so they must be strictly `[A-Za-z0-9_]`.
fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty() || !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(Error::InvalidMigration(value.to_owned()));
    }
    Ok(())
}

fn split_version(stem: &str) -> Result<(String, String)> {
    match stem.split_once('_') {
        Some((v, n)) if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) && !n.is_empty() => {
            Ok((v.to_owned(), n.to_owned()))
        }
        _ => Err(Error::InvalidMigration(format!("{stem}.sql"))),
    }
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(io)
}

fn io(e: std::io::Error) -> Error {
    Error::Io(e.to_string())
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
