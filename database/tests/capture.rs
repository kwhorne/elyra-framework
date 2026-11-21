//! Every write seen (RFC 0043): raw SQL, another connection's writes, a rolled
//! back one, a table made later, a migration's table, and taking it all out.
//!
//! SQLite always; MySQL and Postgres when `ELYRA_TEST_MYSQL_URL` /
//! `ELYRA_TEST_POSTGRES_URL` are set (CI's database job), skipped otherwise.

use std::time::Duration;

use elyra_db::{Database, Driver};
use tokio::sync::broadcast::{error::RecvError, Receiver};

/// Whether `key` is reported within `within`.
async fn heard(rx: &mut Receiver<std::sync::Arc<str>>, key: &str, within: Duration) -> bool {
    let until = tokio::time::Instant::now() + within;
    loop {
        match tokio::time::timeout_at(until, rx.recv()).await {
            Ok(Ok(k)) if &*k == key => return true,
            Ok(Ok(_)) | Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) | Err(_) => return false,
        }
    }
}

/// Until the watcher has installed `table` and looked once (its first look
/// only remembers).
async fn watching(db: &Database, table: &str) {
    for _ in 0..200 {
        let status = db.capture_status();
        if status.tables.iter().any(|t| t == table) && status.last_look_ms.is_some() {
            // One more look, so the first one's baseline is behind us.
            tokio::time::sleep(Duration::from_millis(120)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("not watching {table}: {:?}", db.capture_status());
}

async fn exec(db: &Database, sql: &str) {
    elyra_db::sqlx::raw_sql(elyra_db::sqlx::AssertSqlSafe(sql.to_owned()))
        .execute(db.pool())
        .await
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn create(driver: Driver, table: &str) -> String {
    match driver {
        Driver::Sqlite => format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY, label TEXT)"),
        Driver::MySql => {
            format!(
                "CREATE TABLE {table} (id BIGINT AUTO_INCREMENT PRIMARY KEY, label VARCHAR(50))"
            )
        }
        Driver::Postgres => format!("CREATE TABLE {table} (id BIGSERIAL PRIMARY KEY, label TEXT)"),
    }
}

/// The whole story against `url`; `other` is a second connection to the same
/// database, as another process would have.
async fn every_write_seen(url: &str) {
    let db = Database::connect(url).await.unwrap();
    let driver = db.driver();
    let (orders, notes) = ("capture_orders", "capture_notes");
    for table in [orders, notes] {
        exec(&db, &format!("DROP TABLE IF EXISTS {table}")).await;
    }
    exec(&db, &create(driver, orders)).await;

    let db = db
        .capture_changes()
        .capture_changes_every(Duration::from_millis(50));
    let mut rx = db.changes().subscribe();
    watching(&db, orders).await;
    while rx.try_recv().is_ok() {}

    // Raw SQL, through this pool.
    exec(&db, &format!("INSERT INTO {orders} (label) VALUES ('raw')")).await;
    assert!(
        heard(&mut rx, orders, Duration::from_secs(3)).await,
        "raw insert"
    );

    // Another connection: as far as the app can tell, another process.
    let other = Database::connect(url).await.unwrap();
    exec(&other, &format!("UPDATE {orders} SET label = 'elsewhere'")).await;
    assert!(
        heard(&mut rx, orders, Duration::from_secs(3)).await,
        "another connection"
    );

    // Rolled back: nothing.
    {
        let mut tx = db.begin().await.unwrap();
        elyra_db::sqlx::raw_sql(elyra_db::sqlx::AssertSqlSafe(format!(
            "DELETE FROM {orders}"
        )))
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.rollback().await.unwrap();
    }
    assert!(
        !heard(&mut rx, orders, Duration::from_millis(400)).await,
        "rolled back"
    );

    // A table made later isn't watched until it's installed again.
    exec(&db, &create(driver, notes)).await;
    exec(&db, &format!("INSERT INTO {notes} (label) VALUES ('a')")).await;
    assert!(
        !heard(&mut rx, notes, Duration::from_millis(400)).await,
        "not yet"
    );
    let tables = db.watch_tables().await.unwrap();
    assert!(tables.iter().any(|t| t == notes), "{tables:?}");
    tokio::time::sleep(Duration::from_millis(150)).await;
    exec(&db, &format!("DELETE FROM {notes}")).await;
    assert!(
        heard(&mut rx, notes, Duration::from_secs(3)).await,
        "after watch_tables"
    );

    // The capture's own and the framework's tables aren't watched.
    assert!(!db
        .capture_status()
        .tables
        .iter()
        .any(|t| t.starts_with("_elyra")));

    // Postgres: the listener's connection killed; when it's back, every
    // watched table is said once, since what was said meanwhile is lost.
    if driver == Driver::Postgres {
        while rx.try_recv().is_ok() {}
        elyra_db::sqlx::raw_sql(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
             WHERE query ILIKE 'LISTEN%' AND pid <> pg_backend_pid()",
        )
        .execute(other.pool())
        .await
        .unwrap();
        assert!(
            heard(&mut rx, orders, Duration::from_secs(10)).await,
            "after reconnecting"
        );
    }

    // Taken out: no triggers, nothing heard, the counts gone.
    db.unwatch_tables().await.unwrap();
    assert!(!db.capture_status().on);
    exec(
        &db,
        &format!("INSERT INTO {orders} (label) VALUES ('after')"),
    )
    .await;
    assert!(
        !heard(&mut rx, orders, Duration::from_millis(400)).await,
        "unwatched"
    );
    if driver != Driver::Postgres {
        let left = elyra_db::sqlx::query("SELECT * FROM _elyra_changes")
            .fetch_all(db.pool())
            .await;
        assert!(left.is_err(), "_elyra_changes is gone");
    }

    for table in [orders, notes] {
        exec(&db, &format!("DROP TABLE IF EXISTS {table}")).await;
    }
}

fn sqlite_file(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("elyra-capture-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    elyra_db::sqlite_url(dir.join("app.db"))
}

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_sees_every_write() {
    every_write_seen(&sqlite_file("all")).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mysql_sees_every_write() {
    let Ok(url) = std::env::var("ELYRA_TEST_MYSQL_URL") else {
        return;
    };
    every_write_seen(&url).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn postgres_sees_every_write() {
    let Ok(url) = std::env::var("ELYRA_TEST_POSTGRES_URL") else {
        return;
    };
    every_write_seen(&url).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_migration_s_table_is_watched_after_it_runs() {
    let url = sqlite_file("migrate");
    let db = Database::connect(&url)
        .await
        .unwrap()
        .capture_changes()
        .capture_changes_every(Duration::from_millis(50));
    let dir = std::env::temp_dir().join(format!("elyra-capture-migrations-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("20260101000000_create_invoices.sql"),
        "CREATE TABLE invoices (id INTEGER PRIMARY KEY, total INTEGER);",
    )
    .unwrap();
    let applied = db.migrator(&dir).run().await.unwrap();
    assert_eq!(applied.len(), 1);
    assert!(
        db.capture_status().tables.iter().any(|t| t == "invoices"),
        "{:?}",
        db.capture_status()
    );
    let mut rx = db.changes().subscribe();
    watching(&db, "invoices").await;
    exec(&db, "INSERT INTO invoices (total) VALUES (5)").await;
    assert!(heard(&mut rx, "invoices", Duration::from_secs(3)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn without_capture_nothing_is_installed() {
    let url = sqlite_file("off");
    let db = Database::connect(&url).await.unwrap();
    exec(&db, "CREATE TABLE plain (id INTEGER PRIMARY KEY)").await;
    let mut rx = db.changes().subscribe();
    exec(&db, "INSERT INTO plain (id) VALUES (1)").await;
    assert!(!heard(&mut rx, "plain", Duration::from_millis(300)).await);
    let triggers: Vec<(String,)> =
        elyra_db::sqlx::query_as("SELECT name FROM sqlite_master WHERE type = 'trigger'")
            .fetch_all(db.pool())
            .await
            .unwrap();
    assert!(triggers.is_empty(), "{triggers:?}");
    assert!(!db.capture_status().on);
}
