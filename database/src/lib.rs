//! # elyra-db
//!
//! Database drivers and the migration engine for Elyra, kept in a GUI-free
//! crate so the CLL (`rata migrate`) and the framework can both use it.
//!
//! One [`Database`] abstraction spans SQLite, MySQL, and Postgres via sqlx's
//! `Any` driver — the backend is chosen by the URL scheme (`sqlite:`, `mysql:`,
//! `postgres:`). Bind it into the container and resolve it in commands:
//!
//! ```ignore
//! #[command]
//! async fn todos(ctx: Ctx) -> Vec<Todo> {
//!     let db = ctx.get::<Database>();
//!     sqlx::query_as::<_, Todo>("SELECT * FROM todos").fetch_all(db.pool()).await.unwrap()
//! }
//! ```

use std::path::PathBuf;
use std::time::Duration;

use sqlx::any::AnyPoolOptions;
use sqlx::AnyPool;

mod error;
mod migrate;
pub mod model;
pub mod schema;

pub use error::{Error, Result};
pub use migrate::{Migration, MigrationState, MigrationStatus, Migrator, RustMigration};
pub use model::Page;
pub use model::{Model, Query, Value};
pub use schema::{Schema, Table};

// Re-export sqlx so app crates can write queries without a direct dependency.
pub use sqlx;

/// Which backend a connection targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    Sqlite,
    MySql,
    Postgres,
}

impl Driver {
    /// Detect the driver from a connection URL's scheme.
    pub fn from_url(url: &str) -> Option<Driver> {
        let scheme = url.split(':').next().unwrap_or("");
        match scheme {
            "sqlite" => Some(Driver::Sqlite),
            "mysql" | "mariadb" => Some(Driver::MySql),
            "postgres" | "postgresql" => Some(Driver::Postgres),
            _ => None,
        }
    }
}

/// A database handle: a connection pool plus the detected driver. Cheap to clone
/// (the pool is `Arc`-backed) and `Send + Sync`, so it lives in the container.
#[derive(Clone)]
pub struct Database {
    pool: AnyPool,
    driver: Driver,
}

/// Pool tuning. Defaults are explicit rather than sqlx's, because a desktop app
/// wants a small pool and a *bounded* wait instead of hanging on acquire.
#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Option<Duration>,
    pub max_lifetime: Option<Duration>,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            max_connections: 5,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(10),
            idle_timeout: Some(Duration::from_secs(600)),
            max_lifetime: Some(Duration::from_secs(1800)),
        }
    }
}

impl DatabaseOptions {
    pub fn max_connections(mut self, n: u32) -> Self {
        self.max_connections = n.max(1);
        self
    }

    pub fn acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    fn apply(&self) -> AnyPoolOptions {
        let mut options = AnyPoolOptions::new()
            .max_connections(self.max_connections)
            .min_connections(self.min_connections)
            .acquire_timeout(self.acquire_timeout);
        options = options.idle_timeout(self.idle_timeout);
        options = options.max_lifetime(self.max_lifetime);
        options
    }
}

impl Database {
    /// Connect eagerly (opens a connection now). Use from async contexts / the CLI.
    pub async fn connect(url: &str) -> Result<Self> {
        Self::connect_with(url, DatabaseOptions::default()).await
    }

    /// Connect eagerly with explicit pool options.
    pub async fn connect_with(url: &str, options: DatabaseOptions) -> Result<Self> {
        // Idempotent: safe to call on every connect.
        sqlx::any::install_default_drivers();
        let driver = Driver::from_url(url).ok_or_else(|| Error::UnknownDriver(url.to_owned()))?;
        let pool = options.apply().connect(url).await?;
        let db = Self { pool, driver };
        db.tune().await;
        Ok(db)
    }

    /// Build the pool without connecting; connections open on first use. Safe to
    /// call during app setup (no query issued yet).
    pub fn connect_lazy(url: &str) -> Result<Self> {
        Self::connect_lazy_with(url, DatabaseOptions::default())
    }

    /// Lazy connect with explicit pool options.
    pub fn connect_lazy_with(url: &str, options: DatabaseOptions) -> Result<Self> {
        sqlx::any::install_default_drivers();
        let driver = Driver::from_url(url).ok_or_else(|| Error::UnknownDriver(url.to_owned()))?;
        let pool = options.apply().connect_lazy(url)?;
        Ok(Self { pool, driver })
    }

    /// SQLite needs WAL + a busy timeout to survive concurrent access from the
    /// UI thread and background jobs; the other backends need nothing here.
    async fn tune(&self) {
        if self.driver != Driver::Sqlite {
            return;
        }
        for pragma in [
            "PRAGMA journal_mode = WAL",
            "PRAGMA busy_timeout = 5000",
            "PRAGMA foreign_keys = ON",
        ] {
            let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(pragma))
                .execute(&self.pool)
                .await;
        }
    }

    /// Run `work` inside a transaction, committing on `Ok` and rolling back on
    /// `Err` — Laravel's `DB::transaction`.
    ///
    /// ```ignore
    /// db.transaction(|tx| Box::pin(async move {
    ///     sqlx::query("UPDATE accounts SET balance = balance - 100 WHERE id = 1")
    ///         .execute(&mut **tx).await?;
    ///     Ok(())
    /// })).await?;
    /// ```
    pub async fn transaction<F, T>(&self, work: F) -> Result<T>
    where
        F: for<'c> FnOnce(
            &'c mut sqlx::Transaction<'static, sqlx::Any>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<T>> + Send + 'c>,
        >,
    {
        let mut tx = self.pool.begin().await?;
        match work(&mut tx).await {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(e) => {
                // Best effort: the original error is what the caller cares about.
                let _ = tx.rollback().await;
                Err(e)
            }
        }
    }

    /// Start a transaction manually (remember to `commit`).
    pub async fn begin(&self) -> Result<sqlx::Transaction<'static, sqlx::Any>> {
        Ok(self.pool.begin().await?)
    }

    /// The underlying sqlx pool, for running queries.
    pub fn pool(&self) -> &AnyPool {
        &self.pool
    }

    /// The detected backend.
    pub fn driver(&self) -> Driver {
        self.driver
    }

    /// A [`Migrator`] for the given migrations directory, sharing this pool.
    pub fn migrator(&self, dir: impl Into<PathBuf>) -> Migrator {
        Migrator::new(dir.into(), self.pool.clone())
    }
}
