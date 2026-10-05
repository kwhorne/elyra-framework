//! `rata migrate` family — database migrations, like `php artisan migrate`.
//!
//! SQL-file migrations need no app binary: the CLI applies them itself. Rust
//! migrations (`RustMigration`, as `make:resource` and `make:migration --rust`
//! write them) are compiled into the app, so when the project has any, the
//! CLI runs the app in its migrate mode (`ELYRA_MIGRATE`), which handles both
//! kinds in one batch.

use std::path::Path;
use std::process::Command;
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

/// Whether the project has Rust migrations: any `.rs` under `src/` that
/// implements `RustMigration`.
fn has_rust_migrations(root: &Path) -> bool {
    fn walk(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                walk(&path)
            } else {
                path.extension().is_some_and(|e| e == "rs")
                    && std::fs::read_to_string(&path)
                        .is_ok_and(|text| text.contains("RustMigration for"))
            }
        })
    }
    walk(&root.join("src"))
}

/// Run the app in its migrate mode (`up`, `down` or `status`): it knows the
/// Rust migrations, and gets the SQL directory and the database from here.
fn through_app(cfg: &Config, mode: &str) -> Result<(), String> {
    eprintln!(
        "rata: the project has Rust migrations, so `{}` runs them (and the SQL files) in migrate mode",
        cfg.app_crate
    );
    let mut cargo = Command::new("cargo");
    cargo
        .args(["run", "--quiet", "-p", &cfg.app_crate])
        .env("ELYRA_MIGRATE", mode)
        .env("ELYRA_MIGRATIONS_DIR", cfg.root.join(&cfg.migrations_dir))
        .current_dir(&cfg.root);
    // The app's own `App::database` wins; otherwise it reads this.
    if let Some(url) = &cfg.database_url {
        cargo.env("DATABASE_URL", url);
    }
    let status = cargo
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("the app's migrate mode exited with {status}"))
    }
}

/// `rata migrate` — apply all pending migrations.
pub fn migrate(cfg: &Config) -> Result<(), String> {
    if has_rust_migrations(&cfg.root) {
        return through_app(cfg, "up");
    }
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
    if has_rust_migrations(&cfg.root) {
        return through_app(cfg, "down");
    }
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
    if has_rust_migrations(&cfg.root) {
        return through_app(cfg, "status");
    }
    let dir = cfg.root.join(&cfg.migrations_dir);
    runtime()?.block_on(async {
        let db = open(cfg).await?;
        let statuses = db
            .migrator(dir)
            .status_all(&[])
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
        .ok_or("usage: rata make:migration <name> [--rust]")?;
    // `--rust` scaffolds a `RustMigration` built with the schema builder instead
    // of a pair of hand-written .sql files.
    let rust = std::env::args().any(|a| a == "--rust");
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

    if rust {
        return make_rust_migration(cfg, &slug, version);
    }

    let dir = cfg.root.join(&cfg.migrations_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let up = dir.join(format!("{version}_{slug}.sql"));
    let down = dir.join(format!("{version}_{slug}.down.sql"));
    std::fs::write(&up, format!("-- up: {name}\n")).map_err(|e| e.to_string())?;
    std::fs::write(&down, format!("-- down: {name}\n")).map_err(|e| e.to_string())?;

    println!("Created {}", up.display());
    println!("Created {}", down.display());
    println!("\nTip: `rata make:migration {name} --rust` scaffolds a schema-builder");
    println!("migration instead, which renders DDL for SQLite/MySQL/Postgres.");
    Ok(())
}

/// Scaffold a Rust migration under `src/migrations/`.
fn make_rust_migration(cfg: &Config, slug: &str, version: u64) -> Result<(), String> {
    let dir = cfg.root.join("src/migrations");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{slug}.rs"));
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }

    let struct_name: String = slug
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    // `create_users_table` -> `users`
    let table = slug
        .strip_prefix("create_")
        .and_then(|s| s.strip_suffix("_table"))
        .unwrap_or(slug)
        .to_string();

    let contents = format!(
        r#"//! Migration `{slug}` — generated by `rata make:migration --rust`.

use elyra::db::{{Driver, RustMigration, Schema}};

pub struct {struct_name};

impl RustMigration for {struct_name} {{
    fn version(&self) -> &str {{
        "{version}"
    }}

    fn name(&self) -> &str {{
        "{slug}"
    }}

    fn up(&self, driver: Driver) -> Vec<String> {{
        Schema::create("{table}", |t| {{
            t.id();
            // t.string("name");
            // t.string("email").unique();
            t.timestamps();
        }})
        .to_sql(driver)
    }}

    fn down(&self, driver: Driver) -> Vec<String> {{
        Schema::drop_if_exists("{table}").to_sql(driver)
    }}
}}
"#
    );
    std::fs::write(&path, contents).map_err(|e| e.to_string())?;

    println!("Created {}", path.display());
    println!("\nWire it up in src/main.rs:");
    println!("  mod migrations {{ pub mod {slug}; }}");
    println!("  App::new().migrations(vec![Box::new(migrations::{slug}::{struct_name})])");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_migrations_are_found_anywhere_under_src() {
        let root = std::env::temp_dir().join(format!("rata-migrate-{}", std::process::id()));
        let deep = root.join("src/resources/team");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        assert!(!has_rust_migrations(&root), "SQL files only");

        std::fs::write(
            deep.join("migration.rs"),
            "impl RustMigration for CreateTeamsTable {}",
        )
        .unwrap();
        assert!(has_rust_migrations(&root));
        let _ = std::fs::remove_dir_all(root);
    }
}
