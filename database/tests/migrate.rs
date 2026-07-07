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
    let url = format!("sqlite://{}?mode=rwc", db_path.display());

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
