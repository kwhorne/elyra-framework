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
//! `bool`<->INTEGER mapping on a real backend — plus casts, a global scope on a
//! bulk `UPDATE` (where `$n` numbering across `SET` and `WHERE` matters), and a
//! `belongs_to_many` pivot (multi-row insert, joined count, one-query eager load).
//!
//! Only compiled with `--features database`.
#![cfg(feature = "database")]

use elyra::db::sqlx;
use elyra::{Database, Driver, Model, Query};

#[derive(Model, Debug)]
#[model(table = "elyra_widgets")]
struct Widget {
    id: i64,
    #[model(column = "label")]
    name: String,
    qty: i64,
    price: f64,
    active: bool,
    /// Nullable bool <-> nullable INTEGER.
    featured: Option<bool>,
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
                active INT NOT NULL, \
                featured INT NULL)"
        }
        Driver::Postgres => {
            "CREATE TABLE elyra_widgets (\
                id BIGSERIAL PRIMARY KEY, \
                label VARCHAR(255) NOT NULL, \
                qty BIGINT NOT NULL, \
                price DOUBLE PRECISION NOT NULL, \
                active INT NOT NULL, \
                featured INT NULL)"
        }
        Driver::Sqlite => {
            "CREATE TABLE elyra_widgets (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, \
                label TEXT NOT NULL, \
                qty INTEGER NOT NULL, \
                price REAL NOT NULL, \
                active INTEGER NOT NULL, \
                featured INTEGER)"
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
        featured: Some(true),
    };
    bolt.insert(&db).await.unwrap();
    assert!(bolt.id > 0, "insert should populate the primary key");

    let mut nut = Widget {
        id: 0,
        name: "nut".into(),
        qty: 5,
        price: 0.25,
        active: false,
        featured: None,
    };
    nut.insert(&db).await.unwrap();

    // find(): column override (`label`) + bool<->INTEGER roundtrip.
    let found = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    assert_eq!(found.name, "bolt");
    assert!(found.active);
    assert_eq!(found.featured, Some(true));
    let nut_found = Widget::find(&db, nut.id).await.unwrap().unwrap();
    assert_eq!(nut_found.featured, None);

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
    b.featured = Some(false);
    b.update(&db).await.unwrap();

    let refreshed = Widget::find(&db, bolt.id).await.unwrap().unwrap();
    assert_eq!(refreshed.qty, 99);
    assert!(!refreshed.active);
    assert_eq!(refreshed.featured, Some(false));

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

// --- casts, global scopes and belongs_to_many ---------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Alpha,
    Beta,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Alpha => "alpha",
            Kind::Beta => "beta",
        })
    }
}

impl std::str::FromStr for Kind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "alpha" => Ok(Kind::Alpha),
            "beta" => Ok(Kind::Beta),
            other => Err(format!("not a kind: {other}")),
        }
    }
}

fn workspace_one(q: Query<Tagged>) -> Query<Tagged> {
    q.where_eq("workspace_id", 1)
}

#[derive(Model, Debug)]
#[model(table = "elyra_tagged", global_scope = workspace_one)]
struct Tagged {
    id: i64,
    workspace_id: i64,
    #[model(cast = "json")]
    tags: Vec<String>,
    #[model(cast = "text")]
    kind: Kind,
}

#[derive(Model, Debug)]
#[model(
    table = "elyra_people",
    belongs_to_many(
        Group,
        pivot = "elyra_memberships",
        fk = "person_id",
        related_fk = "group_id"
    )
)]
struct Person {
    id: i64,
    name: String,
}

#[derive(Model, Debug)]
#[model(table = "elyra_groups")]
struct Group {
    id: i64,
    name: String,
}

const ELOQUENT_TABLES: [&str; 4] = [
    "elyra_memberships",
    "elyra_tagged",
    "elyra_people",
    "elyra_groups",
];

fn eloquent_ddl(driver: Driver) -> &'static str {
    match driver {
        Driver::MySql => {
            "CREATE TABLE elyra_tagged (id BIGINT AUTO_INCREMENT PRIMARY KEY, \
                 workspace_id BIGINT NOT NULL, tags TEXT NOT NULL, kind VARCHAR(32) NOT NULL);\
             CREATE TABLE elyra_people (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL);\
             CREATE TABLE elyra_groups (id BIGINT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(255) NOT NULL);\
             CREATE TABLE elyra_memberships (person_id BIGINT NOT NULL, group_id BIGINT NOT NULL, \
                 PRIMARY KEY (person_id, group_id));"
        }
        Driver::Postgres => {
            "CREATE TABLE elyra_tagged (id BIGSERIAL PRIMARY KEY, \
                 workspace_id BIGINT NOT NULL, tags TEXT NOT NULL, kind TEXT NOT NULL);\
             CREATE TABLE elyra_people (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);\
             CREATE TABLE elyra_groups (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL);\
             CREATE TABLE elyra_memberships (person_id BIGINT NOT NULL, group_id BIGINT NOT NULL, \
                 PRIMARY KEY (person_id, group_id));"
        }
        Driver::Sqlite => {
            "CREATE TABLE elyra_tagged (id INTEGER PRIMARY KEY AUTOINCREMENT, \
                 workspace_id INTEGER NOT NULL, tags TEXT NOT NULL, kind TEXT NOT NULL);\
             CREATE TABLE elyra_people (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
             CREATE TABLE elyra_groups (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
             CREATE TABLE elyra_memberships (person_id INTEGER NOT NULL, group_id INTEGER NOT NULL, \
                 PRIMARY KEY (person_id, group_id));"
        }
    }
}

async fn drop_eloquent_tables(db: &Database) {
    for table in ELOQUENT_TABLES {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
}

async fn run_eloquent(url: &str) {
    let db = Database::connect(url)
        .await
        .expect("connect to test database");
    drop_eloquent_tables(&db).await;
    sqlx::raw_sql(eloquent_ddl(db.driver()))
        .execute(db.pool())
        .await
        .expect("create tables");

    // Casts: JSON + Display/FromStr through a real backend.
    let mut rows = Vec::new();
    for (workspace_id, kind) in [(1, Kind::Alpha), (1, Kind::Alpha), (2, Kind::Alpha)] {
        let mut row = Tagged {
            id: 0,
            workspace_id,
            tags: vec!["x".into(), format!("ws{workspace_id}")],
            kind,
        };
        row.insert(&db).await.unwrap();
        rows.push(row);
    }
    let found = Tagged::find(&db, rows[0].id).await.unwrap().unwrap();
    assert_eq!(found.tags, vec!["x", "ws1"]);
    assert_eq!(found.kind, Kind::Alpha);

    // Global scope on a bulk UPDATE: SET binds first, then the scope's
    // `workspace_id`, then the caller's `kind` — `$1, $2, $3` on Postgres.
    let touched = Tagged::query()
        .where_eq("kind", "alpha")
        .update(&db, &[("kind", "beta".into())])
        .await
        .unwrap();
    assert_eq!(touched, 2, "only workspace 1's rows");
    assert!(Tagged::find(&db, rows[2].id).await.unwrap().is_none());
    let outside = Tagged::query()
        .without_global_scopes()
        .find(&db, rows[2].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outside.kind, Kind::Alpha, "the scope kept the UPDATE out");
    assert_eq!(
        Tagged::query()
            .where_eq("kind", "beta")
            .count(&db)
            .await
            .unwrap(),
        2
    );

    // belongs_to_many: multi-row attach, joined count, sync, one-query eager load.
    let mut people = Vec::new();
    for name in ["ada", "grace"] {
        let mut p = Person {
            id: 0,
            name: name.into(),
        };
        p.insert(&db).await.unwrap();
        people.push(p);
    }
    let mut groups = Vec::new();
    for name in ["core", "docs", "infra"] {
        let mut g = Group {
            id: 0,
            name: name.into(),
        };
        g.insert(&db).await.unwrap();
        groups.push(g);
    }
    let ids: Vec<i64> = groups.iter().map(|g| g.id).collect();

    assert_eq!(
        people[0]
            .attach_groups(&db, [ids[0], ids[1]])
            .await
            .unwrap(),
        2
    );
    assert_eq!(people[1].attach_groups(&db, [ids[0]]).await.unwrap(), 1);
    assert_eq!(people[0].groups_query().count(&db).await.unwrap(), 2);
    let ordered = people[0]
        .groups_query()
        .order_by("elyra_groups.name")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(ordered[0].name, "core");

    let changes = people[0].sync_groups(&db, [ids[1], ids[2]]).await.unwrap();
    assert_eq!(changes.detached, vec![ids[0]]);
    assert_eq!(changes.attached, vec![ids[2]]);

    let eager = Person::load_groups(&db, &people).await.unwrap();
    assert_eq!(eager[&people[0].id].len(), 2);
    assert_eq!(eager[&people[1].id].len(), 1);
    assert_eq!(eager[&people[1].id][0].name, "core");

    assert_eq!(people[0].detach_all_groups(&db).await.unwrap(), 2);
    assert!(people[0].groups(&db).await.unwrap().is_empty());

    drop_eloquent_tables(&db).await;
}

#[tokio::test]
async fn mysql_eloquent() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => run_eloquent(&url).await,
        _ => eprintln!("skipping MySQL eloquent test: set ELYRA_TEST_MYSQL_URL to run it"),
    }
}

#[tokio::test]
async fn postgres_eloquent() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => run_eloquent(&url).await,
        _ => eprintln!("skipping Postgres eloquent test: set ELYRA_TEST_POSTGRES_URL to run it"),
    }
}

// --- the durable queue's journal on a real server -------------------------------

async fn run_durable_queue(url: &str) {
    use elyra::queue::{JobOptions, Queue, QueueProvider};
    use elyra::testing::TestApp;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let db = Database::connect(url)
        .await
        .expect("connect to test database");
    for table in ["elyra_jobs", "elyra_failed_jobs"] {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
    async fn rows(db: &Database, table: &str) -> Option<i64> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) AS n FROM {table}"
        )))
        .fetch_one(db.pool())
        .await
        .ok()
        .and_then(|r| sqlx::Row::try_get::<i64, _>(&r, "n").ok())
    }
    async fn until(db: &Database, table: &str, want: i64) -> bool {
        for _ in 0..500 {
            if rows(db, table).await == Some(want) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    // Run 1: the tables are created (driver-specific DDL), a job is journaled
    // and never finishes, and another fails for good. Two workers: the stuck job
    // holds one forever, and the failing one needs the other.
    {
        let app = TestApp::new(
            elyra::App::new()
                .bind(Database::connect(url).await.unwrap())
                .provider(QueueProvider::with_workers(2).durable()),
        );
        let queue = app.get::<Queue>();
        queue.on("stuck", |_| std::future::pending::<Result<(), String>>());
        queue.on_with(
            "broken",
            JobOptions::default()
                .attempts(2)
                .retry_base(Duration::from_millis(5)),
            |_| async { Err("nope".to_string()) },
        );
        queue
            .push_confirmed("stuck", serde_json::json!({"n": 1}))
            .await
            .unwrap();
        queue.push("broken", serde_json::json!({}));
        assert!(
            until(&db, "elyra_failed_jobs", 1).await,
            "failed row written"
        );
        assert!(
            until(&db, "elyra_jobs", 1).await,
            "only the stuck job is pending"
        );
    }

    // Run 2: the pending job is recovered and completes; the failure is listed.
    let runs = Arc::new(AtomicUsize::new(0));
    let r = runs.clone();
    struct Handler(Arc<AtomicUsize>);
    impl elyra::Provider for Handler {
        fn boot(&self, ctx: &elyra::Ctx) {
            let runs = self.0.clone();
            ctx.get::<Queue>().on("stuck", move |payload| {
                let runs = runs.clone();
                async move {
                    assert_eq!(payload["n"], 1, "the payload round-trips");
                    runs.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            });
        }
    }
    let app = TestApp::new(
        elyra::App::new()
            .bind(Database::connect(url).await.unwrap())
            .provider(QueueProvider::new().durable())
            .provider(Handler(r)),
    );
    let queue = app.get::<Queue>();
    for _ in 0..500 {
        if runs.load(Ordering::SeqCst) == 1 && queue.failed().len() == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1, "recovered job ran once");
    assert_eq!(queue.failed().len(), 1);
    assert_eq!(queue.failed()[0].error, "nope");
    assert!(until(&db, "elyra_jobs", 0).await);

    queue.clear_failed();
    assert!(until(&db, "elyra_failed_jobs", 0).await);
    for table in ["elyra_jobs", "elyra_failed_jobs"] {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
}

#[tokio::test]
async fn mysql_durable_queue() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => run_durable_queue(&url).await,
        _ => eprintln!("skipping MySQL durable queue test: set ELYRA_TEST_MYSQL_URL to run it"),
    }
}

#[tokio::test]
async fn postgres_durable_queue() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => run_durable_queue(&url).await,
        _ => {
            eprintln!("skipping Postgres durable queue test: set ELYRA_TEST_POSTGRES_URL to run it")
        }
    }
}

// --- `unique` / `exists` validation on a real server ----------------------------

async fn run_validation_db(url: &str) {
    use elyra::validation::Validator;
    let db = Database::connect(url)
        .await
        .expect("connect to test database");
    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_vusers")
        .execute(db.pool())
        .await;
    sqlx::raw_sql("CREATE TABLE elyra_vusers (id BIGINT PRIMARY KEY, email VARCHAR(255) NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    sqlx::raw_sql(
        "INSERT INTO elyra_vusers (id, email) VALUES (1, 'ada@x.test'), (2, 'grace@x.test')",
    )
    .execute(db.pool())
    .await
    .unwrap();

    let own = serde_json::json!({ "email": "ada@x.test", "id": 2 });
    // Two placeholders on this path: `$1` / `$2` on Postgres, `?` on MySQL.
    assert!(Validator::new(&own)
        .rule("email", "unique:elyra_vusers,email,1")
        .validate_with(&db)
        .await
        .is_ok());
    assert!(Validator::new(&own)
        .rule("email", "unique:elyra_vusers,email,2")
        .errors_with(&db)
        .await
        .has("email"));
    assert!(Validator::new(&own)
        .rule("id", "exists:elyra_vusers,id")
        .validate_with(&db)
        .await
        .is_ok());

    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_vusers")
        .execute(db.pool())
        .await;
}

#[tokio::test]
async fn mysql_validation_db() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => run_validation_db(&url).await,
        _ => eprintln!("skipping MySQL validation test: set ELYRA_TEST_MYSQL_URL to run it"),
    }
}

#[tokio::test]
async fn postgres_validation_db() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => run_validation_db(&url).await,
        _ => eprintln!("skipping Postgres validation test: set ELYRA_TEST_POSTGRES_URL to run it"),
    }
}
