//! Migration engine lifecycle test against a real (temp-file) SQLite database.

use std::path::PathBuf;

use elyra_db::{Database, Driver, MigrationState};
use sqlx::Row;

/// A unique temp directory for one test run.
fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("elyra-db-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn full_migration_lifecycle_on_sqlite() {
    let root = temp_dir("migrate");
    let migrations = root.join("migrations");
    std::fs::create_dir_all(&migrations).unwrap();

    // Two migrations with rollbacks.
    std::fs::write(
        migrations.join("0001_create_todos.sql"),
        "CREATE TABLE todos (id INTEGER PRIMARY KEY, title TEXT NOT NULL);",
    )
    .unwrap();
    std::fs::write(
        migrations.join("0001_create_todos.down.sql"),
        "DROP TABLE todos;",
    )
    .unwrap();
    std::fs::write(
        migrations.join("0002_add_done.sql"),
        "ALTER TABLE todos ADD COLUMN done INTEGER NOT NULL DEFAULT 0;",
    )
    .unwrap();
    std::fs::write(
        migrations.join("0002_add_done.down.sql"),
        "ALTER TABLE todos DROP COLUMN done;",
    )
    .unwrap();

    let db_path = root.join("test.db");
    // Portable: a raw format! breaks on Windows drive letters/backslashes.
    let url = elyra_db::sqlite_url(&db_path);

    let db = Database::connect(&url).await.expect("connect");
    assert_eq!(db.driver(), Driver::Sqlite);
    let migrator = db.migrator(&migrations);

    // Apply everything.
    let applied = migrator.run().await.expect("run");
    assert_eq!(applied, vec!["0001".to_string(), "0002".to_string()]);

    // The table exists with the added column (insert exercises both migrations).
    sqlx::query("INSERT INTO todos (title, done) VALUES ('buy milk', 1)")
        .execute(db.pool())
        .await
        .expect("insert");
    let row = sqlx::query("SELECT title, done FROM todos")
        .fetch_one(db.pool())
        .await
        .expect("select");
    assert_eq!(row.get::<String, _>("title"), "buy milk");
    assert_eq!(row.get::<i64, _>("done"), 1);

    // Re-running is a no-op.
    assert!(migrator.run().await.expect("rerun").is_empty());

    // Status: both applied, batch 1.
    let status = migrator.status().await.expect("status");
    assert_eq!(status.len(), 2);
    assert!(status
        .iter()
        .all(|s| matches!(s.state, MigrationState::Applied { batch: 1 })));

    // Rollback the batch: both down, in reverse order.
    let rolled = migrator.rollback().await.expect("rollback");
    assert_eq!(rolled, vec!["0002".to_string(), "0001".to_string()]);

    // Now all pending again, and the table is gone.
    let status = migrator.status().await.expect("status after rollback");
    assert!(status
        .iter()
        .all(|s| matches!(s.state, MigrationState::Pending)));
    assert!(sqlx::query("SELECT 1 FROM todos")
        .fetch_one(db.pool())
        .await
        .is_err());

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn driver_detection() {
    assert_eq!(Driver::from_url("sqlite://x.db"), Some(Driver::Sqlite));
    assert_eq!(
        Driver::from_url("mysql://root@localhost/app"),
        Some(Driver::MySql)
    );
    assert_eq!(
        Driver::from_url("postgres://localhost/app"),
        Some(Driver::Postgres)
    );
    assert_eq!(Driver::from_url("redis://x"), None);
}

/// A Rust migration, as `make:resource` writes them.
struct CreateTags;

impl elyra_db::RustMigration for CreateTags {
    fn version(&self) -> &str {
        "0002"
    }
    fn name(&self) -> &str {
        "create_tags"
    }
    fn up(&self, driver: Driver) -> Vec<String> {
        elyra_db::Schema::create("tags", |t| {
            t.id();
            t.string("name");
        })
        .to_sql(driver)
    }
    fn down(&self, driver: Driver) -> Vec<String> {
        elyra_db::Schema::drop_if_exists("tags").to_sql(driver)
    }
}

async fn tables(db: &Database) -> Vec<String> {
    let rows = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE '\\_%' ESCAPE '\\' \
         AND name NOT LIKE 'sqlite%' ORDER BY name",
    )
    .fetch_all(db.pool())
    .await
    .unwrap();
    rows.iter().map(|r| r.get::<String, _>("name")).collect()
}

/// SQL files and Rust migrations in one project: one batch, each rolled back
/// with its own `down`.
#[tokio::test]
async fn sql_and_rust_migrations_share_a_batch_and_roll_back_together() {
    let root = temp_dir("mixed");
    let dir = root.join("migrations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("0001_create_todos.sql"),
        "CREATE TABLE todos (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    std::fs::write(dir.join("0001_create_todos.down.sql"), "DROP TABLE todos;").unwrap();
    std::fs::write(
        dir.join("0003_create_notes.sql"),
        "CREATE TABLE notes (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    std::fs::write(dir.join("0003_create_notes.down.sql"), "DROP TABLE notes;").unwrap();

    let db = Database::connect(&elyra_db::sqlite_url(root.join("t.db")))
        .await
        .unwrap();
    let migrator = db.migrator(&dir);
    let rust: Vec<Box<dyn elyra_db::RustMigration>> = vec![Box::new(CreateTags)];

    let applied = migrator.run_all(&rust, db.driver()).await.unwrap();
    assert_eq!(
        applied,
        ["0001", "0002", "0003"],
        "in version order, interleaved"
    );
    assert_eq!(tables(&db).await, ["notes", "tags", "todos"]);
    let status = migrator.status_all(&rust).await.unwrap();
    assert!(status
        .iter()
        .all(|s| s.state == MigrationState::Applied { batch: 1 }));
    assert_eq!(status[1].name, "create_tags");

    // The SQL-only rollback can't run the Rust one's `down`: it refuses, and
    // touches nothing — it used to forget `0002` with its table still there.
    let refused = migrator.rollback().await.unwrap_err();
    assert!(
        matches!(&refused, elyra_db::Error::UnknownMigration(v) if v == "0002"),
        "{refused}"
    );
    assert_eq!(tables(&db).await, ["notes", "tags", "todos"]);
    assert_eq!(migrator.status_all(&rust).await.unwrap().len(), 3);

    let rolled = migrator.rollback_all(&rust, db.driver()).await.unwrap();
    assert_eq!(rolled, ["0003", "0002", "0001"]);
    assert!(tables(&db).await.is_empty(), "every down ran");

    // And again from scratch: nothing was left half-recorded.
    assert_eq!(migrator.run_all(&rust, db.driver()).await.unwrap().len(), 3);
}

#[tokio::test]
async fn a_rust_only_rollback_runs_the_sql_downs_too() {
    let root = temp_dir("rust-rollback");
    let dir = root.join("migrations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("0001_create_todos.sql"),
        "CREATE TABLE todos (id INTEGER PRIMARY KEY);",
    )
    .unwrap();
    std::fs::write(dir.join("0001_create_todos.down.sql"), "DROP TABLE todos;").unwrap();
    let db = Database::connect(&elyra_db::sqlite_url(root.join("t.db")))
        .await
        .unwrap();
    let migrator = db.migrator(&dir);
    let rust: Vec<Box<dyn elyra_db::RustMigration>> = vec![Box::new(CreateTags)];
    migrator.run_all(&rust, db.driver()).await.unwrap();

    // `rollback_rust` (the app's `ELYRA_MIGRATE=down` before) used to drop the
    // SQL migration's record without running its `.down.sql`.
    migrator.rollback_rust(&rust, db.driver()).await.unwrap();
    assert!(tables(&db).await.is_empty());
}

#[tokio::test]
async fn unknown_and_duplicate_versions() {
    let root = temp_dir("unknown");
    let dir = root.join("migrations");
    std::fs::create_dir_all(&dir).unwrap();
    let db = Database::connect(&elyra_db::sqlite_url(root.join("t.db")))
        .await
        .unwrap();
    let migrator = db.migrator(&dir);
    let rust: Vec<Box<dyn elyra_db::RustMigration>> = vec![Box::new(CreateTags)];
    migrator.run_all(&rust, db.driver()).await.unwrap();

    // The Rust migration isn't registered any more: shown, and not forgotten.
    let status = migrator.status_all(&[]).await.unwrap();
    assert_eq!(status[0].name, "(unknown)");
    assert!(matches!(
        migrator.rollback_all(&[], db.driver()).await,
        Err(elyra_db::Error::UnknownMigration(_))
    ));
    assert_eq!(tables(&db).await, ["tags"]);

    // One version, two migrations: refused before anything runs.
    std::fs::write(
        dir.join("0005_twice.sql"),
        "CREATE TABLE twice (id INTEGER);",
    )
    .unwrap();
    struct Twice;
    impl elyra_db::RustMigration for Twice {
        fn version(&self) -> &str {
            "0005"
        }
        fn name(&self) -> &str {
            "twice"
        }
        fn up(&self, _: Driver) -> Vec<String> {
            vec![]
        }
        fn down(&self, _: Driver) -> Vec<String> {
            vec![]
        }
    }
    let both: Vec<Box<dyn elyra_db::RustMigration>> = vec![Box::new(CreateTags), Box::new(Twice)];
    assert!(migrator.run_all(&both, db.driver()).await.is_err());
    assert_eq!(tables(&db).await, ["tags"]);
}
