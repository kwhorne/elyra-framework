//! Every SQLite connection a pool opens is in WAL, waits on a lock and checks
//! foreign keys, a lazy pool's too: an app's own database is opened lazily,
//! and was never put in WAL, so its readers blocked its writers.

use std::path::PathBuf;

use elyra_db::{Database, DatabaseOptions};
use sqlx::Row;

fn temp_db(tag: &str) -> (PathBuf, String) {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("elyra-db-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).unwrap();
    let url = format!("sqlite://{}?mode=rwc", dir.join("app.db").display());
    (dir, url)
}

async fn pragmas(db: &Database) -> Vec<(i64, i64, String)> {
    // Held at once: three connections, not one used three times.
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(db.pool().acquire().await.unwrap());
    }
    let mut seen = Vec::new();
    for conn in &mut held {
        let busy: i64 = sqlx::query("PRAGMA busy_timeout")
            .fetch_one(&mut **conn)
            .await
            .unwrap()
            .get(0);
        let keys: i64 = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&mut **conn)
            .await
            .unwrap()
            .get(0);
        let journal: String = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&mut **conn)
            .await
            .unwrap()
            .get(0);
        seen.push((busy, keys, journal));
    }
    seen
}

#[tokio::test]
async fn every_connection_waits_and_checks_foreign_keys() {
    let (dir, url) = temp_db("pragmas");
    let db = Database::connect_with(&url, DatabaseOptions::default().max_connections(3))
        .await
        .unwrap();
    assert_eq!(pragmas(&db).await, vec![(5000, 1, "wal".to_owned()); 3]);
    db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_lazy_pool_too() {
    let (dir, url) = temp_db("pragmas-lazy");
    let db =
        Database::connect_lazy_with(&url, DatabaseOptions::default().max_connections(3)).unwrap();
    assert_eq!(pragmas(&db).await, vec![(5000, 1, "wal".to_owned()); 3]);
    db.pool().close().await;
    let _ = std::fs::remove_dir_all(dir);
}
