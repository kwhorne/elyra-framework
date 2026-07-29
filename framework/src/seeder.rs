//! Database seeders — Elyra's `db:seed`.
//!
//! A seeder fills a database with baseline or demo data. Register them on the
//! app and run with `ELYRA_SEED=1`:
//!
//! ```ignore
//! use elyra::seeder::Seeder;
//! use elyra::db::{Database, Result};
//!
//! struct DemoData;
//!
//! impl Seeder for DemoData {
//!     fn name(&self) -> &str { "DemoData" }
//!     fn run<'a>(&'a self, db: &'a Database) -> elyra::seeder::BoxSeed<'a> {
//!         Box::pin(async move {
//!             elyra::db::sqlx::query("INSERT INTO tags (name) VALUES ('demo')")
//!                 .execute(db.pool())
//!                 .await?;
//!             Ok(())
//!         })
//!     }
//! }
//!
//! App::new().database("sqlite://app.db?mode=rwc").seeder(DemoData).run()
//! ```
//!
//! `ELYRA_SEED=1 cargo run` runs every registered seeder and exits without
//! opening a window — the same pattern `rata codegen` and `ELYRA_MIGRATE` use, so
//! seeding needs no separate binary.

use std::future::Future;
use std::pin::Pin;

use elyra_db::{Database, Result};

/// The future a seeder returns.
pub type BoxSeed<'a> = Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

/// One unit of seed data.
pub trait Seeder: Send + Sync {
    /// A label for the log line (`"DemoData"`).
    fn name(&self) -> &str;

    /// Insert the data. Runs inside the app's tokio runtime.
    fn run<'a>(&'a self, db: &'a Database) -> BoxSeed<'a>;
}
