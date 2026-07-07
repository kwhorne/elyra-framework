//! `rata migrate` family — database migrations, driven directly by the CLI
//! (no app binary needed), like `php artisan migrate`.

use std::time::{SystemTime, UNIX_EPOCH};

use elyra_db::{Database, MigrationState};

use crate::config::Config;

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to build tokio runtime: {e}"))
}

fn require_url(cfg: &Config) -> Result<String, String> {
    cfg.database_url.clone().ok_or_else(|| {
        "no database url — set `[database].url` in elyra.toml or the DATABASE_URL env var".into()
    })
}

async fn open(cfg: &Config) -> Result<Database, String> {
    let url = require_url(cfg)?;
    Database::connect(&url)
        .await
        .map_err(|e| format!("connect: {e}"))
}

/// `rata migrate` — apply all pending migrations.
pub fn migrate(cfg: &Config) -> Result<(), String> {
    let dir = cfg.root.join(&cfg.migrations_dir);
    runtime()?.block_on(async {
        let db = open(cfg).await?;
        let applied = db
            .migrator(dir)
            .run()
            .await
            .map_err(|e| format!("migrate: {e}"))?;
        if applied.is_empty() {
            println!("Nothing to migrate.");
        } else {
            for version in applied {
                println!("  migrated  {version}");
            }
        }
        Ok(())
    })
}

/// `rata migrate:rollback` — roll back the most recent batch.
pub fn rollback(cfg: &Config) -> Result<(), String> {
    let dir = cfg.root.join(&cfg.migrations_dir);
    runtime()?.block_on(async {
        let db = open(cfg).await?;
        let rolled = db
            .migrator(dir)
            .rollback()
            .await
            .map_err(|e| format!("rollback: {e}"))?;
        if rolled.is_empty() {
            println!("Nothing to roll back.");
        } else {
            for version in rolled {
                println!("  rolled back  {version}");
            }
        }
        Ok(())
    })
}

/// `rata migrate:status` — list migrations and whether they're applied.
pub fn status(cfg: &Config) -> Result<(), String> {
    let dir = cfg.root.join(&cfg.migrations_dir);
    runtime()?.block_on(async {
        let db = open(cfg).await?;
        let statuses = db
            .migrator(dir)
            .status()
            .await
            .map_err(|e| format!("status: {e}"))?;
        if statuses.is_empty() {
            println!("No migrations found in {}.", cfg.migrations_dir);
            return Ok(());
        }
        for s in statuses {
            let state = match s.state {
                MigrationState::Applied { batch } => format!("applied (batch {batch})"),
                MigrationState::Pending => "pending".to_string(),
            };
            println!("  [{state:>18}]  {}_{}", s.version, s.name);
        }
        Ok(())
    })
}

/// `rata make:migration <name>` — scaffold up/down SQL files (no DB needed).
pub fn make_migration(cfg: &Config) -> Result<(), String> {
    let name = std::env::args()
        .nth(2)
        .ok_or("usage: rata make:migration <name>")?;
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if slug.is_empty() {
        return Err("migration name must contain alphanumeric characters".into());
    }

    let version = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let dir = cfg.root.join(&cfg.migrations_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let up = dir.join(format!("{version}_{slug}.sql"));
    let down = dir.join(format!("{version}_{slug}.down.sql"));
    std::fs::write(&up, format!("-- up: {name}\n")).map_err(|e| e.to_string())?;
    std::fs::write(&down, format!("-- down: {name}\n")).map_err(|e| e.to_string())?;

    println!("Created {}", up.display());
    println!("Created {}", down.display());
    Ok(())
}
