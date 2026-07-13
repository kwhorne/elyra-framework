//! Active-Record model tests against real **MySQL** and **Postgres** servers.
//!
//! These are opt-in. Each test reads a connection URL from an env var and
//! *skips* (returns early) when it is unset, so `cargo test` stays green locally
//! without a database. CI sets the env vars against service containers:
//!
//! - `ELYRA_TEST_MYSQL_URL`    e.g. `mysql://root:root@127.0.0.1:3306/elyra_test`
//! - `ELYRA_TEST_POSTGRES_URL` e.g. `postgres://postgres:postgres@127.0.0.1:5432/elyra_test`
//!
//! They exercise what the SQLite tests can't: per-driver placeholders (`?` vs
//! `$n`), key retrieval (`last_insert_id` vs `RETURNING`), and the
//! `bool`<->INTEGER mapping on a real backend.
//!
//! Only compiled with `--features database`.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::{Database, Driver, Model};

#[derive(Model, Debug)]
#[model(table = "elyra_widgets")]
struct Widget {
    id: i64,
    #[model(column = "label")]
    name: String,
    qty: i64,
    price: f64,
    active: bool,
}

/// Driver-specific `CREATE TABLE` (a `&'static str`, so no `AssertSqlSafe`).
fn create_ddl(driver: Driver) -> &'static str {
    match driver {
        Driver::MySql => {
            "CREATE TABLE elyra_widgets (\
                id BIGINT AUTO_INCREMENT PRIMARY KEY, \
                label VARCHAR(255) NOT NULL, \
                qty BIGINT NOT NULL, \
                price DOUBLE NOT NULL, \
                active INT NOT NULL)"
        }
        Driver::Postgres => {
            "CREATE TABLE elyra_widgets (\
                id BIGSERIAL PRIMARY KEY, \
                label VARCHAR(255) NOT NULL, \
                qty BIGINT NOT NULL, \
                price DOUBLE PRECISION NOT NULL, \
                active INT NOT NULL)"
        }
        Driver::Sqlite => {
            "CREATE TABLE elyra_widgets (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                label TEXT NOT NULL, \
                qty INTEGER NOT NULL, \
                price REAL NOT NULL, \
                active INTEGER NOT NULL)"
        }
    }
}

async fn run_crud(url: &str) {
    let db = Database::connect(url)
        .await
        .expect("connect to test database");

    // Start from a clean table.
    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_widgets")
        .execute(db.pool())
        .await;
    sqlx::raw_sql(create_ddl(db.driver()))
        .execute(db.pool())
        .await
        .expect("create table");

    // Insert — the key comes back via RETURNING (pg) or last_insert_id (mysql).
    let mut bolt = Widget {
        id: 0,
        name: "bolt".into(),
        qty: 10,
        price: 1.5,
        active: true,
    };
    bolt.insert(&db).await.unwrap();
    assert!(bolt.id > 0, "insert should populate the primary key");

    let mut nut = Widget {
        id: 0,
        name: "nut".into(),
        qty: 5,
        price: 0.25,
        active: false,
    };
    nut.insert(&db).await.unwrap();

    // find(): column override (`label`) + bool<->INTEGER roundtrip.
    let found = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    assert_eq!(found.name, "bolt");
    assert!(found.active);

    // Query builder: per-driver placeholders and the bool bound as 0/1.
    let active = Widget::query()
        .where_eq("active", true)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].name, "bolt");

    let all = Widget::query().order_by("id").get(&db).await.unwrap();
    assert_eq!(all.len(), 2);

    // Update + delete by primary key.
    let mut b = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    b.qty = 99;
    b.active = false;
    b.update(&db).await.unwrap();

    let refreshed = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    assert_eq!(refreshed.qty, 99);
    assert!(!refreshed.active);

    refreshed.delete(&db).await.unwrap();
    assert!(Widget::find(&db, bolt.id).await.unwrap().is_none());

    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_widgets")
        .execute(db.pool())
        .await;
}

#[tokio::test]
async fn mysql_crud() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => run_crud(&url).await,
        _ => eprintln!("skipping MySQL model test: set ELYRA_TEST_MYSQL_URL to run it"),
    }
}

#[tokio::test]
async fn postgres_crud() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => run_crud(&url).await,
        _ => eprintln!("skipping Postgres model test: set ELYRA_TEST_POSTGRES_URL to run it"),
    }
}
