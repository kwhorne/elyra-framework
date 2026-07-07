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

use sqlx::any::AnyPoolOptions;
use sqlx::AnyPool;

mod error;
mod migrate;
pub mod model;

pub use error::{Error, Result};
pub use migrate::{Migration, MigrationState, MigrationStatus, Migrator};
pub use model::{Model, Query, Value};

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

impl Database {
    /// Connect eagerly (opens a connection now). Use from async contexts / the CLI.
    pub async fn connect(url: &str) -> Result<Self> {
        // Idempotent: safe to call on every connect.
        sqlx::any::install_default_drivers();
        let driver = Driver::from_url(url).ok_or_else(|| Error::UnknownDriver(url.to_owned()))?;
        let pool = AnyPoolOptions::new().connect(url).await?;
        Ok(Self { pool, driver })
    }

    /// Build the pool without connecting; connections open on first use. Safe to
    /// call during app setup (no query issued yet).
    pub fn connect_lazy(url: &str) -> Result<Self> {
        sqlx::any::install_default_drivers();
        let driver = Driver::from_url(url).ok_or_else(|| Error::UnknownDriver(url.to_owned()))?;
        let pool = AnyPoolOptions::new().connect_lazy(url)?;
        Ok(Self { pool, driver })
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
