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

// --- JSON casts on real JSON columns (JSONB on Postgres) ---------------------

#[derive(Model, Debug)]
#[model(table = "elyra_docs")]
struct Doc {
    id: i64,
    title: String,
    /// `t.json` — JSONB on Postgres, TEXT elsewhere.
    #[model(cast = "json")]
    meta: Option<serde_json::Value>,
    #[model(cast = "json")]
    tags: Vec<String>,
    /// `t.text` — the column the JSON cast always worked with.
    #[model(cast = "json")]
    note: Option<serde_json::Value>,
}

async fn run_json_columns(db: &Database) {
    use elyra::db::schema::Schema;
    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_docs")
        .execute(db.pool())
        .await;
    Schema::create("elyra_docs", |t| {
        t.id();
        t.string("title");
        t.json("meta").nullable();
        t.json("tags");
        t.text("note").nullable();
    })
    .execute(db)
    .await
    .expect("create elyra_docs");

    if db.driver() == Driver::Postgres {
        // The point of the test: these really are JSONB columns.
        let ty: String = sqlx::query_scalar(
            "SELECT data_type FROM information_schema.columns \
             WHERE table_name = 'elyra_docs' AND column_name = 'meta'",
        )
        .fetch_one(db.pool())
        .await
        .unwrap();
        assert_eq!(ty, "jsonb");
    }

    let meta = serde_json::json!({ "b": 1, "a": [1, 2, { "deep": true }] });
    let mut doc = Doc {
        id: 0,
        title: "first".into(),
        meta: Some(meta.clone()),
        tags: vec!["x".into(), "y".into()],
        note: Some(serde_json::json!({ "k": "v" })),
    };
    doc.insert(db).await.expect("insert into JSON columns");
    let found = Doc::find(db, doc.id).await.unwrap().expect("found");
    assert_eq!(found.meta, Some(meta), "read back as the same value");
    assert_eq!(found.tags, ["x", "y"]);
    assert_eq!(found.note, Some(serde_json::json!({ "k": "v" })));

    // NULLs both ways, and an empty array, through update.
    let mut edited = found;
    edited.meta = None;
    edited.tags = Vec::new();
    edited.note = None;
    edited.update(db).await.expect("update JSON columns");
    let back = Doc::find(db, doc.id).await.unwrap().unwrap();
    assert_eq!((back.meta, back.tags, back.note), (None, Vec::new(), None));

    // The query builder reads them too: get, paginate, a filtered first.
    let mut second = Doc {
        id: 0,
        title: "second".into(),
        meta: None,
        tags: vec!["z".into()],
        note: None,
    };
    second.insert(db).await.unwrap();
    let all = Doc::query().order_by("id").get(db).await.unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].tags, ["z"]);
    let page = Doc::query().paginate(db, 1, 10).await.unwrap();
    assert_eq!(page.total, 2);
    let first = Doc::query()
        .where_eq("title", "second")
        .first(db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, second.id);

    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_docs")
        .execute(db.pool())
        .await;
}

#[tokio::test]
async fn postgres_json_columns() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => {
            run_json_columns(&Database::connect(&url).await.unwrap()).await
        }
        _ => eprintln!("skipping Postgres JSON test: set ELYRA_TEST_POSTGRES_URL to run it"),
    }
}

#[tokio::test]
async fn mysql_json_columns() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => {
            run_json_columns(&Database::connect(&url).await.unwrap()).await
        }
        _ => eprintln!("skipping MySQL JSON test: set ELYRA_TEST_MYSQL_URL to run it"),
    }
}

#[tokio::test]
async fn sqlite_json_columns() {
    let path = std::env::temp_dir().join(format!("elyra-json-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    run_json_columns(
        &Database::connect(&elyra::db::sqlite_url(&path))
            .await
            .unwrap(),
    )
    .await;
    let _ = std::fs::remove_file(path);
}

// --- the offline copy on a real server (RFC 0007) ---------------------------------

#[cfg(feature = "backend")]
#[elyra::command]
async fn offline_count(ctx: elyra::Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<elyra::Backend>()
        .get("/api/customers/count")
        .query(&serde_json::json!({ "search": "ada" }))
        .json()
        .await?)
}

#[cfg(feature = "backend")]
async fn run_offline_copy(url: &str) {
    use elyra::http::{Http, HttpFake, Method};
    use elyra::testing::TestApp;
    use serde_json::json;

    let db = Database::connect(url)
        .await
        .expect("connect to test database");
    let _ = sqlx::raw_sql("DROP TABLE IF EXISTS elyra_backend_cache")
        .execute(db.pool())
        .await;
    let rows = || async {
        sqlx::query("SELECT COUNT(*) AS n FROM elyra_backend_cache")
            .fetch_one(db.pool())
            .await
            .ok()
            .and_then(|r| sqlx::Row::try_get::<i64, _>(&r, "n").ok())
    };

    let base = "https://crm.test";
    let fake = HttpFake::new()
        .post(format!("{base}/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(format!("{base}/api/user"), 200, json!({ "id": 1 }))
        .delete(format!("{base}/api/sanctum/token"), 204, json!(null))
        .get(format!("{base}/api/customers/count"), 200, json!(3));
    let app = TestApp::new(
        elyra::App::new()
            .bind(Database::connect(url).await.unwrap())
            .backend(elyra::Backend::new(base).offline())
            .commands(elyra::commands![offline_count])
            .swap(Http::fake(fake.clone())),
    );
    let auth = app.get::<elyra::Auth>();
    auth.sign_in("ada@example.com", "secret").await.unwrap();

    // The table is created (driver-specific DDL), and the answer kept twice
    // over: the second read replaces the first.
    assert_eq!(app.invoke_ok::<i64>("offline_count", ()).await, 3);
    assert_eq!(app.invoke_ok::<i64>("offline_count", ()).await, 3);
    assert_eq!(rows().await, Some(1));

    fake.clone()
        .unreachable(Method::Get, format!("{base}/*"))
        .unreachable(Method::Delete, format!("{base}/*"));
    assert_eq!(
        app.invoke_ok::<i64>("offline_count", ()).await,
        3,
        "the copy"
    );

    auth.sign_out().await.unwrap();
    assert_eq!(rows().await, Some(0), "signing out clears it");
}

// --- the outbox on a real server (RFC 0007 step 2) -------------------------------

#[cfg(feature = "backend")]
#[elyra::command]
async fn outbox_store(ctx: elyra::Ctx, name: String) -> elyra::Result<serde_json::Value> {
    Ok(ctx
        .get::<elyra::Backend>()
        .post("/api/customers")
        .body(&serde_json::json!({ "name": name }))
        .queue_offline()
        .json()
        .await?)
}

#[cfg(feature = "backend")]
async fn run_outbox(url: &str) {
    use elyra::http::{Http, HttpFake, Method};
    use elyra::offline::Outbox;
    use elyra::testing::TestApp;
    use serde_json::json;

    let db = Database::connect(url)
        .await
        .expect("connect to test database");
    for table in ["elyra_outbox", "elyra_outbox_failed"] {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
    let base = "https://crm.test";
    let fake = HttpFake::new()
        .post(format!("{base}/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(format!("{base}/api/user"), 200, json!({ "id": 2 }));
    let app = TestApp::new(
        elyra::App::new()
            .bind(Database::connect(url).await.unwrap())
            .backend(elyra::Backend::new(base).offline())
            .commands(elyra::commands![outbox_store])
            .swap(Http::fake(fake.clone())),
    );
    let auth = app.get::<elyra::Auth>();
    auth.sign_in("ada@example.com", "secret").await.unwrap();

    // Two writes kept (driver-specific DDL, the id back from each insert).
    fake.clone()
        .unreachable(Method::Post, format!("{base}/*"))
        .unreachable(Method::Get, format!("{base}/*"));
    for name in ["", "Beta"] {
        assert_eq!(
            app.invoke_err_kind("outbox_store", (name.to_string(),))
                .await
                .as_deref(),
            Some("queued")
        );
    }
    let outbox = app.get::<Outbox>();
    let pending = outbox.pending().await;
    assert_eq!(pending.len(), 2);
    assert!(pending[0].id < pending[1].id);

    // Back: the first is refused (moved aside in a transaction), the second sent.
    fake.clone()
        .get(format!("{base}/up"), 200, json!("ok"))
        .post(format!("{base}/api/customers"), 201, json!({ "id": 2 }))
        .on(
            Method::Post,
            format!("{base}/api/customers"),
            422,
            json!({ "errors": { "name": ["Required."] } }),
        );
    outbox.flush().await;
    // The 422 answers every POST now: both refused, in order.
    let failed = outbox.failed().await;
    assert_eq!(failed.len(), 2, "{failed:?}");
    assert_eq!(failed[0].errors.as_ref().unwrap()["name"][0], "Required.");

    // Retried with its id (explicit, into the sequence's table) and sent.
    fake.clone()
        .post(format!("{base}/api/customers"), 201, json!({ "id": 3 }));
    for item in &failed {
        outbox
            .retry(item.id, Some(json!({ "name": "Fixed" })))
            .await
            .unwrap();
    }
    outbox.flush().await;
    assert!(outbox.items().await.is_empty());
    assert_eq!(outbox.counts(), (0, 0));
}

// One test per server, the copy and then the outbox: they share its tables,
// and signing in as someone else rightly clears the others' copy.
#[cfg(feature = "backend")]
#[tokio::test]
async fn mysql_offline() {
    match std::env::var("ELYRA_TEST_MYSQL_URL") {
        Ok(url) if !url.is_empty() => {
            run_offline_copy(&url).await;
            run_outbox(&url).await;
        }
        _ => eprintln!("skipping MySQL offline tests: set ELYRA_TEST_MYSQL_URL to run them"),
    }
}

#[cfg(feature = "backend")]
#[tokio::test]
async fn postgres_offline() {
    match std::env::var("ELYRA_TEST_POSTGRES_URL") {
        Ok(url) if !url.is_empty() => {
            run_offline_copy(&url).await;
            run_outbox(&url).await;
        }
        _ => eprintln!("skipping Postgres offline tests: set ELYRA_TEST_POSTGRES_URL to run them"),
    }
}

// --- composite keys (RFC 0044) ----------------------------------------------

#[derive(Model, Debug, Clone)]
#[model(table = "elyra_order_lines", timestamps)]
struct OrderLine {
    #[model(id)]
    order_id: i64,
    #[model(id)]
    line_no: i64,
    product: String,
    qty: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Model, Debug, Clone)]
#[model(table = "elyra_post_tags")]
struct PostTag {
    #[model(id)]
    post_id: i64,
    #[model(id)]
    tag: String,
}

async fn run_composite(url: &str) {
    let db = Database::connect(url).await.expect("connect");
    for table in ["elyra_order_lines", "elyra_post_tags"] {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
    let text = if db.driver() == Driver::MySql {
        "VARCHAR(50)"
    } else {
        "TEXT"
    };
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE elyra_order_lines (order_id BIGINT NOT NULL, line_no BIGINT NOT NULL, \
         product {text} NOT NULL, qty BIGINT NOT NULL, created_at BIGINT NOT NULL, \
         updated_at BIGINT NOT NULL, PRIMARY KEY (order_id, line_no))"
    )))
    .execute(db.pool())
    .await
    .expect("create order lines");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE TABLE elyra_post_tags (post_id BIGINT NOT NULL, tag {text} NOT NULL, \
         PRIMARY KEY (post_id, tag))"
    )))
    .execute(db.pool())
    .await
    .expect("create post tags");

    let line = |line_no: i64, product: &str, qty: i64| OrderLine {
        order_id: 42,
        line_no,
        product: product.into(),
        qty,
        created_at: 0,
        updated_at: 0,
    };
    line(1, "bolt", 1).insert(&db).await.unwrap();
    line(2, "nut", 2).insert(&db).await.unwrap();
    let mut nut = OrderLine::find(&db, (42, 2)).await.unwrap().unwrap();
    assert_eq!(nut.product, "nut");
    nut.qty = 9;
    nut.update(&db).await.unwrap();
    assert_eq!(OrderLine::find(&db, (42, 1)).await.unwrap().unwrap().qty, 1);
    assert_eq!(OrderLine::find(&db, (42, 2)).await.unwrap().unwrap().qty, 9);

    // save(): the driver's upsert, keeping the row's created_at.
    let created = OrderLine::find(&db, (42, 1))
        .await
        .unwrap()
        .unwrap()
        .created_at;
    let mut again = line(1, "bolt", 5);
    again.created_at = 1;
    again.save(&db).await.unwrap();
    let row = OrderLine::find(&db, (42, 1)).await.unwrap().unwrap();
    assert_eq!(row.qty, 5);
    assert_eq!(row.created_at, created);
    line(3, "washer", 1).save(&db).await.unwrap();
    assert_eq!(OrderLine::query().count(&db).await.unwrap(), 3);

    nut.delete(&db).await.unwrap();
    assert!(OrderLine::find(&db, (42, 2)).await.unwrap().is_none());
    assert_eq!(OrderLine::query().count(&db).await.unwrap(), 2);

    // Every column the key's: saved twice is one row.
    let mut tag = PostTag {
        post_id: 1,
        tag: "rust".into(),
    };
    tag.save(&db).await.unwrap();
    tag.save(&db).await.unwrap();
    assert_eq!(PostTag::query().count(&db).await.unwrap(), 1);
    assert!(PostTag::find(&db, (1, "rust".to_string()))
        .await
        .unwrap()
        .is_some());

    for table in ["elyra_order_lines", "elyra_post_tags"] {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
}

#[tokio::test]
async fn mysql_composite_keys() {
    let Ok(url) = std::env::var("ELYRA_TEST_MYSQL_URL") else {
        return;
    };
    run_composite(&url).await;
}

#[tokio::test]
async fn postgres_composite_keys() {
    let Ok(url) = std::env::var("ELYRA_TEST_POSTGRES_URL") else {
        return;
    };
    run_composite(&url).await;
}

// --- pivot columns (RFC 0046) -----------------------------------------------

/// The pivot's own model: two keys, data, timestamps.
#[derive(Model, Debug, Clone, Default, PartialEq)]
#[model(table = "elyra_pv_role_user", timestamps)]
struct Grant {
    #[model(id)]
    user_id: i64,
    #[model(id)]
    role_id: i64,
    granted_by: Option<i64>,
    note: String,
    created_at: i64,
    updated_at: i64,
}

#[derive(Model, Debug)]
#[model(
    table = "elyra_pv_users",
    belongs_to_many(PvRole, using = Grant, fk = "user_id", related_fk = "role_id", as = "roles")
)]
struct PvUser {
    id: i64,
    name: String,
}

/// Not `Clone`: a role shared by two users is read twice, not copied.
#[derive(Model, Debug, PartialEq)]
#[model(table = "elyra_pv_roles")]
struct PvRole {
    id: i64,
    name: String,
}

/// The field form: each related row hydrated with its pivot row.
#[derive(Model, Debug)]
#[model(table = "elyra_pv_users")]
struct PvUserWithGrants {
    id: i64,
    name: String,
    #[model(belongs_to_many(PvRole, using = Grant, fk = "user_id", related_fk = "role_id"))]
    grants: Vec<(PvRole, Grant)>,
}

/// A pivot model whose key leaves out `role_id`: the relation says so.
#[derive(Model, Debug, Default)]
#[model(table = "elyra_pv_role_user")]
struct LooseGrant {
    #[model(id)]
    user_id: i64,
    #[model(id)]
    note: String,
    role_id: i64,
    granted_by: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

#[derive(Model, Debug)]
#[model(
    table = "elyra_pv_users",
    belongs_to_many(PvRole, using = LooseGrant, fk = "user_id", related_fk = "role_id", as = "roles")
)]
struct PvUserLoose {
    id: i64,
    name: String,
}

const PIVOT_TABLES: [&str; 3] = ["elyra_pv_role_user", "elyra_pv_roles", "elyra_pv_users"];

async fn run_pivot_columns(url: &str) {
    let db = Database::connect(url).await.expect("connect");
    for table in PIVOT_TABLES {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
    let (id, text) = match db.driver() {
        Driver::MySql => ("BIGINT AUTO_INCREMENT PRIMARY KEY", "VARCHAR(255)"),
        Driver::Postgres => ("BIGSERIAL PRIMARY KEY", "TEXT"),
        Driver::Sqlite => ("INTEGER PRIMARY KEY AUTOINCREMENT", "TEXT"),
    };
    for ddl in [
        format!("CREATE TABLE elyra_pv_users (id {id}, name {text} NOT NULL)"),
        format!("CREATE TABLE elyra_pv_roles (id {id}, name {text} NOT NULL)"),
        // `note` and the timestamps have no default: a two-column insert fails.
        format!(
            "CREATE TABLE elyra_pv_role_user (user_id BIGINT NOT NULL, role_id BIGINT NOT NULL, \
             granted_by BIGINT NULL, note {text} NOT NULL, created_at BIGINT NOT NULL, \
             updated_at BIGINT NOT NULL, PRIMARY KEY (user_id, role_id))"
        ),
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(db.pool())
            .await
            .expect("create");
    }

    let mut alice = PvUser {
        id: 0,
        name: "alice".into(),
    };
    alice.insert(&db).await.unwrap();
    let mut bob = PvUser {
        id: 0,
        name: "bob".into(),
    };
    bob.insert(&db).await.unwrap();
    let mut ids = Vec::new();
    for name in ["reader", "lead", "owner"] {
        let mut role = PvRole {
            id: 0,
            name: name.into(),
        };
        role.insert(&db).await.unwrap();
        ids.push(role.id);
    }
    let (reader, lead, owner) = (ids[0], ids[1], ids[2]);

    // By ids, through `Grant::default()`: the timestamps are filled.
    assert_eq!(alice.attach_roles(&db, [reader]).await.unwrap(), 1);
    let first = Grant::find(&db, (alice.id, reader)).await.unwrap().unwrap();
    assert!(first.created_at > 0 && first.updated_at > 0);
    assert_eq!(first.note, "");

    // With data; the keys come from the relation, a repeated id keeps its first row.
    let granted = |note: &str| Grant {
        granted_by: Some(bob.id),
        note: note.into(),
        ..Grant::default()
    };
    assert_eq!(
        alice
            .attach_roles_with(&db, [(lead, granted("lead")), (lead, granted("again"))])
            .await
            .unwrap(),
        1
    );

    // Each related row with its pivot row, in the relation's order.
    let mut pairs = alice.roles_with_pivot(&db).await.unwrap();
    pairs.sort_by_key(|(role, _)| role.id);
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[1].0.name, "lead");
    assert_eq!(
        (
            pairs[1].1.user_id,
            pairs[1].1.role_id,
            pairs[1].1.granted_by
        ),
        (alice.id, lead, Some(bob.id))
    );
    assert_eq!(pairs[1].1.note, "lead");
    assert!(bob.roles_with_pivot(&db).await.unwrap().is_empty());

    // Narrowing on a pivot column is the joined table's column.
    let leads = alice
        .roles_query()
        .where_eq("elyra_pv_role_user.note", "lead")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(leads.iter().map(|r| r.id).collect::<Vec<_>>(), vec![lead]);

    // Changing one pivot row; one that isn't attached says so.
    assert!(alice
        .update_role_pivot(&db, reader, |g| {
            g.note = "member".into();
            g.role_id = 999; // the relation's keys win
        })
        .await
        .unwrap());
    assert_eq!(
        Grant::find(&db, (alice.id, reader))
            .await
            .unwrap()
            .unwrap()
            .note,
        "member"
    );
    assert!(!alice
        .update_role_pivot(&db, owner, |g| g.note = "x".into())
        .await
        .unwrap());

    // sync with data: detach, attach, and update only what differs.
    let changes = alice
        .sync_roles_with(&db, [(lead, granted("lead")), (owner, granted("new"))])
        .await
        .unwrap();
    assert_eq!(changes.detached, vec![reader]);
    assert_eq!(changes.attached, vec![owner]);
    assert!(changes.updated.is_empty(), "{changes:?}");
    let changes = alice
        .sync_roles_with(&db, [(lead, granted("boss")), (owner, granted("new"))])
        .await
        .unwrap();
    assert_eq!(
        (changes.attached, changes.detached, changes.updated),
        (vec![], vec![], vec![lead])
    );
    let boss = Grant::find(&db, (alice.id, lead)).await.unwrap().unwrap();
    assert_eq!(boss.note, "boss");
    assert!(boss.created_at > 0);
    // NULL against a value is a difference too.
    let changes = alice
        .sync_roles_with(
            &db,
            [
                (
                    lead,
                    Grant {
                        granted_by: None,
                        ..granted("boss")
                    },
                ),
                (owner, granted("new")),
            ],
        )
        .await
        .unwrap();
    assert_eq!(changes.updated, vec![lead]);

    // Eager, for a batch: a role shared by two users, a user with none.
    bob.attach_roles(&db, [lead]).await.unwrap();
    let mut carol = PvUser {
        id: 0,
        name: "carol".into(),
    };
    carol.insert(&db).await.unwrap();
    let users = vec![alice, bob, carol];
    let map = PvUser::load_roles_with_pivot(&db, &users).await.unwrap();
    assert_eq!(map[&users[0].id].len(), 2);
    assert_eq!(map[&users[1].id].len(), 1);
    assert_eq!(map[&users[1].id][0].0.id, lead);
    assert_eq!(map[&users[1].id][0].1.user_id, users[1].id);
    assert!(!map.contains_key(&users[2].id));

    // The field form.
    let mut hydrated = PvUserWithGrants::query()
        .order_by("id")
        .get(&db)
        .await
        .unwrap();
    PvUserWithGrants::with_grants(&db, &mut hydrated)
        .await
        .unwrap();
    let counts: Vec<usize> = hydrated.iter().map(|u| u.grants.len()).collect();
    assert_eq!(counts, vec![2, 1, 0]);
    assert!(hydrated[0]
        .grants
        .iter()
        .all(|(role, grant)| grant.role_id == role.id && grant.user_id == hydrated[0].id));
    assert!(hydrated[1]
        .update_grant_pivot(&db, lead, |g| g.note = "co".into())
        .await
        .unwrap());

    // sync by ids leaves the attached rows' data alone.
    let changes = users[0].sync_roles(&db, [lead]).await.unwrap();
    assert_eq!(changes.detached, vec![owner]);
    assert_eq!(
        Grant::find(&db, (users[0].id, lead))
            .await
            .unwrap()
            .unwrap()
            .note,
        "boss"
    );

    // A pivot model whose key leaves out a relation key is an error, not a guess.
    let loose = PvUserLoose {
        id: users[0].id,
        name: "alice".into(),
    };
    let err = loose.roles_with_pivot(&db).await.unwrap_err().to_string();
    assert!(err.contains("role_id"), "{err}");

    for table in PIVOT_TABLES {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
}

#[tokio::test]
async fn sqlite_pivot_columns() {
    let path = std::env::temp_dir().join(format!("elyra-pivot-columns-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    run_pivot_columns(&elyra::db::sqlite_url(&path)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn mysql_pivot_columns() {
    let Ok(url) = std::env::var("ELYRA_TEST_MYSQL_URL") else {
        return;
    };
    run_pivot_columns(&url).await;
}

#[tokio::test]
async fn postgres_pivot_columns() {
    let Ok(url) = std::env::var("ELYRA_TEST_POSTGRES_URL") else {
        return;
    };
    run_pivot_columns(&url).await;
}

// --- relations on any key (RFC 0047) ----------------------------------------

/// A parent keyed by a string.
#[derive(Model, Debug, Clone, PartialEq)]
#[model(
    table = "elyra_ak_teams",
    has_many(AkMember, fk = "team_code", as = "members"),
    has_one(AkBadge, fk = "team_code", as = "badge")
)]
struct AkTeam {
    #[model(id)]
    code: String,
    name: String,
}

/// Two `belongs_to`: a string key, and a nullable `i64` one.
#[derive(Model, Debug)]
#[model(
    table = "elyra_ak_members",
    belongs_to(AkTeam, fk = "team_code", as = "team"),
    belongs_to(AkMentor, fk = "mentor_id", as = "mentor")
)]
struct AkMember {
    id: i64,
    team_code: String,
    mentor_id: Option<i64>,
    name: String,
}

#[derive(Model, Debug)]
#[model(table = "elyra_ak_badges")]
struct AkBadge {
    id: i64,
    team_code: String,
    label: String,
}

#[derive(Model, Debug, Clone)]
#[model(table = "elyra_ak_mentors")]
struct AkMentor {
    id: i64,
    name: String,
}

/// A parent keyed by two columns.
#[derive(Model, Debug, Clone, PartialEq)]
#[model(
    table = "elyra_ak_lines",
    has_many(AkNote, fk = ("order_id", "line_no"), as = "notes")
)]
struct AkLine {
    #[model(id)]
    order_id: i64,
    #[model(id)]
    line_no: i64,
    qty: i64,
}

/// Its child, pointing back with both columns; `line_no` nullable.
#[derive(Model, Debug)]
#[model(
    table = "elyra_ak_notes",
    belongs_to(AkLine, fk = ("order_id", "line_no"), as = "line")
)]
struct AkNote {
    id: i64,
    order_id: i64,
    line_no: Option<i64>,
    body: String,
}

/// The field forms over both keys.
#[derive(Model, Debug)]
#[model(table = "elyra_ak_teams")]
struct AkTeamWithMembers {
    #[model(id)]
    code: String,
    name: String,
    #[model(has_many(AkMember, fk = "team_code"))]
    members: Vec<AkMember>,
    #[model(has_one(AkBadge, fk = "team_code"))]
    badge: Option<AkBadge>,
}

#[derive(Model, Debug)]
#[model(table = "elyra_ak_notes")]
struct AkNoteWithLine {
    id: i64,
    order_id: i64,
    line_no: Option<i64>,
    body: String,
    #[model(belongs_to(AkLine, fk = ("order_id", "line_no")))]
    line: Option<AkLine>,
}

const ANY_KEY_TABLES: [&str; 6] = [
    "elyra_ak_teams",
    "elyra_ak_members",
    "elyra_ak_badges",
    "elyra_ak_mentors",
    "elyra_ak_lines",
    "elyra_ak_notes",
];

async fn run_any_key(url: &str) {
    let db = Database::connect(url).await.expect("connect");
    for table in ANY_KEY_TABLES {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
    let (id, text) = match db.driver() {
        Driver::MySql => ("BIGINT AUTO_INCREMENT PRIMARY KEY", "VARCHAR(255)"),
        Driver::Postgres => ("BIGSERIAL PRIMARY KEY", "TEXT"),
        Driver::Sqlite => ("INTEGER PRIMARY KEY AUTOINCREMENT", "TEXT"),
    };
    for ddl in [
        format!("CREATE TABLE elyra_ak_teams (code VARCHAR(50) PRIMARY KEY, name {text} NOT NULL)"),
        format!(
            "CREATE TABLE elyra_ak_members (id {id}, team_code VARCHAR(50) NOT NULL, \
             mentor_id BIGINT NULL, name {text} NOT NULL)"
        ),
        format!("CREATE TABLE elyra_ak_badges (id {id}, team_code VARCHAR(50) NOT NULL, label {text} NOT NULL)"),
        format!("CREATE TABLE elyra_ak_mentors (id {id}, name {text} NOT NULL)"),
        "CREATE TABLE elyra_ak_lines (order_id BIGINT NOT NULL, line_no BIGINT NOT NULL, \
         qty BIGINT NOT NULL, PRIMARY KEY (order_id, line_no))"
            .to_string(),
        format!(
            "CREATE TABLE elyra_ak_notes (id {id}, order_id BIGINT NOT NULL, line_no BIGINT NULL, \
             body {text} NOT NULL)"
        ),
    ] {
        sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(db.pool())
            .await
            .expect("create");
    }

    // A string-keyed parent: lazy and eager, both directions.
    let team = |code: &str| AkTeam {
        code: code.into(),
        name: code.to_uppercase(),
    };
    let teams = vec![team("core"), team("web"), team("idle")];
    for t in &teams {
        t.clone().insert(&db).await.unwrap();
    }
    let mut mentor = AkMentor {
        id: 0,
        name: "ada".into(),
    };
    mentor.insert(&db).await.unwrap();
    for (code, name, mentored) in [
        ("core", "a", true),
        ("core", "b", false),
        ("web", "c", false),
    ] {
        AkMember {
            id: 0,
            team_code: code.into(),
            mentor_id: mentored.then_some(mentor.id),
            name: name.into(),
        }
        .insert(&db)
        .await
        .unwrap();
    }
    AkBadge {
        id: 0,
        team_code: "web".into(),
        label: "fast".into(),
    }
    .insert(&db)
    .await
    .unwrap();

    assert_eq!(teams[0].members(&db).await.unwrap().len(), 2);
    assert_eq!(teams[1].badge(&db).await.unwrap().unwrap().label, "fast");
    assert!(teams[0].badge(&db).await.unwrap().is_none());
    let by_team = AkTeam::load_members(&db, &teams).await.unwrap();
    assert_eq!(by_team[&teams[0].key()].len(), 2);
    assert_eq!(by_team["web"].len(), 1);
    assert!(!by_team.contains_key("idle"));
    let badges = AkTeam::load_badge(&db, &teams).await.unwrap();
    assert_eq!(badges.len(), 1);
    assert_eq!(badges["web"].label, "fast");

    let members = AkMember::query().order_by("id").get(&db).await.unwrap();
    assert_eq!(members[2].team(&db).await.unwrap().unwrap(), teams[1]);
    let owners = AkMember::load_team(&db, &members).await.unwrap();
    assert_eq!(owners.len(), 2);
    assert_eq!(owners["core"].name, "CORE");

    // A nullable `i64` foreign key: set finds its owner, NULL is `None`.
    assert_eq!(members[0].mentor(&db).await.unwrap().unwrap().name, "ada");
    assert!(members[1].mentor(&db).await.unwrap().is_none());
    let mentors = AkMember::load_mentor(&db, &members).await.unwrap();
    assert_eq!(mentors.len(), 1);
    assert_eq!(mentors[&mentor.id].name, "ada");

    // A composite parent.
    for (order_id, line_no) in [(1, 1), (1, 2), (2, 1)] {
        AkLine {
            order_id,
            line_no,
            qty: order_id * 10 + line_no,
        }
        .insert(&db)
        .await
        .unwrap();
    }
    for (order_id, line_no, body) in [
        (1, Some(1), "x"),
        (1, Some(1), "y"),
        (2, Some(1), "z"),
        (1, None, "loose"),
    ] {
        AkNote {
            id: 0,
            order_id,
            line_no,
            body: body.into(),
        }
        .insert(&db)
        .await
        .unwrap();
    }
    let lines = AkLine::query()
        .order_by("order_id")
        .order_by("line_no")
        .get(&db)
        .await
        .unwrap();
    assert_eq!(lines[0].notes(&db).await.unwrap().len(), 2);
    assert!(lines[1].notes(&db).await.unwrap().is_empty());
    let by_line = AkLine::load_notes(&db, &lines).await.unwrap();
    assert_eq!(by_line[&lines[0].key()].len(), 2);
    assert_eq!(by_line[&(2, 1)][0].body, "z");
    assert!(!by_line.contains_key(&(1, 2)));

    let notes = AkNote::query().order_by("id").get(&db).await.unwrap();
    assert_eq!(notes[2].line(&db).await.unwrap().unwrap(), lines[2]);
    assert!(
        notes[3].line(&db).await.unwrap().is_none(),
        "a NULL part is no owner"
    );
    let line_owners = AkNote::load_line(&db, &notes).await.unwrap();
    assert_eq!(line_owners.len(), 2);
    assert_eq!(line_owners[&(1, 1)].qty, 11);

    // The field forms.
    let mut hydrated = AkTeamWithMembers::query()
        .order_by("code")
        .get(&db)
        .await
        .unwrap();
    AkTeamWithMembers::with_members(&db, &mut hydrated)
        .await
        .unwrap();
    AkTeamWithMembers::with_badge(&db, &mut hydrated)
        .await
        .unwrap();
    let shape: Vec<(&str, usize, bool)> = hydrated
        .iter()
        .map(|t| (t.code.as_str(), t.members.len(), t.badge.is_some()))
        .collect();
    assert_eq!(
        shape,
        vec![("core", 2, false), ("idle", 0, false), ("web", 1, true)]
    );
    let mut with_lines = AkNoteWithLine::query()
        .order_by("id")
        .get(&db)
        .await
        .unwrap();
    AkNoteWithLine::with_line(&db, &mut with_lines)
        .await
        .unwrap();
    let qtys: Vec<Option<i64>> = with_lines
        .iter()
        .map(|n| n.line.as_ref().map(|l| l.qty))
        .collect();
    assert_eq!(qtys, vec![Some(11), Some(11), Some(21), None]);

    // More parents than one query takes: the eager loads are chunked.
    let values: Vec<String> = (0..1500).map(|n| format!("(9, {n}, {n})")).collect();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO elyra_ak_lines (order_id, line_no, qty) VALUES {}",
        values.join(", ")
    )))
    .execute(db.pool())
    .await
    .unwrap();
    AkNote {
        id: 0,
        order_id: 9,
        line_no: Some(1499),
        body: "last".into(),
    }
    .insert(&db)
    .await
    .unwrap();
    let many = AkLine::query()
        .where_eq("order_id", 9)
        .get(&db)
        .await
        .unwrap();
    assert_eq!(many.len(), 1500);
    let by_line = AkLine::load_notes(&db, &many).await.unwrap();
    assert_eq!(by_line[&(9, 1499)][0].body, "last");

    for table in ANY_KEY_TABLES {
        let _ = sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TABLE IF EXISTS {table}")))
            .execute(db.pool())
            .await;
    }
}

#[tokio::test]
async fn sqlite_relations_on_any_key() {
    let path = std::env::temp_dir().join(format!("elyra-any-key-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    run_any_key(&elyra::db::sqlite_url(&path)).await;
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn mysql_relations_on_any_key() {
    let Ok(url) = std::env::var("ELYRA_TEST_MYSQL_URL") else {
        return;
    };
    run_any_key(&url).await;
}

#[tokio::test]
async fn postgres_relations_on_any_key() {
    let Ok(url) = std::env::var("ELYRA_TEST_POSTGRES_URL") else {
        return;
    };
    run_any_key(&url).await;
}
