//! How many live re-runs a write costs, and how many found the result as it
//! was — the measurement RFC 0002 asked for before finer invalidation than
//! per table. A list (page 1 of 25) and ten detail views are open, as a few
//! windows of a CRM would have them; then each kind of write, ten times.
//!
//! Run with `--nocapture` to see the table. The assertions pin today's
//! behaviour (per-table keys): a finer invalidation changes them.
#![cfg(feature = "database")]

use std::time::{Duration, Instant};

use elyra::db::sqlx;
use elyra::live::{LiveRegistry, LiveStats};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database, Model, Page};
use serde::{Deserialize, Serialize};

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[model(table = "customers")]
struct Customer {
    id: i64,
    name: String,
}

/// Page 1 of the list, 25 to a page, as `make:resource`'s index has it.
#[command(live)]
async fn customers_index(ctx: Ctx) -> elyra::Result<Page<Customer>> {
    Ok(Customer::query()
        .order_by("id")
        .paginate(&ctx.get::<Database>(), 1, 25)
        .await?)
}

/// One customer, as a detail view shows it.
#[command(live)]
async fn customers_show(ctx: Ctx, id: i64) -> elyra::Result<Option<Customer>> {
    Ok(Customer::find(&ctx.get::<Database>(), id).await?)
}

#[command]
async fn customers_rename(ctx: Ctx, id: i64, name: String) -> elyra::Result<()> {
    let db = ctx.get::<Database>();
    let mut customer = Customer::find(&db, id).await?.expect("there");
    customer.name = name;
    Ok(customer.save(&db).await?)
}

#[command]
async fn customers_store(ctx: Ctx, name: String) -> elyra::Result<()> {
    let mut customer = Customer { id: 0, name };
    Ok(customer.insert(&ctx.get::<Database>()).await?)
}

/// Re-runs for each kind of write.
struct Row {
    write: &'static str,
    stats: LiveStats,
    took: Duration,
}

async fn db_app() -> (TestApp, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("elyra-reruns-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);",
    )
    .execute(db.pool())
    .await
    .unwrap();
    for i in 1..=50 {
        sqlx::query("INSERT INTO customers (name) VALUES (?)")
            .bind(format!("Customer {i}"))
            .execute(db.pool())
            .await
            .unwrap();
    }
    let app = TestApp::new(
        App::new()
            .bind(db)
            .live_batch_window(Duration::ZERO)
            .commands(commands![
                customers_index,
                customers_show,
                customers_rename,
                customers_store
            ]),
    );
    (app, path)
}

/// The live queries open: the list and ten detail views.
const OPEN: u64 = 11;

/// Wait until `n` more re-runs than `before` have happened.
async fn reran(live: &LiveRegistry, before: LiveStats, n: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while live.stats().reruns < before.reruns + n {
        assert!(Instant::now() < deadline, "only {:?} re-runs", live.stats());
        tokio::task::yield_now().await;
    }
}

/// …and that no more follow.
async fn settled(live: &LiveRegistry, before: LiveStats, n: u64) {
    reran(live, before, n).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(
        live.stats().reruns,
        before.reruns + n,
        "more re-runs than expected"
    );
}

fn print(title: &str, rows: &[Row]) {
    eprintln!("\n{title}");
    eprintln!(
        "{:<34} {:>7} {:>7} {:>9} {:>11}",
        "write (×10)", "re-runs", "pushed", "unchanged", "per re-run"
    );
    for r in rows {
        eprintln!(
            "{:<34} {:>7} {:>7} {:>9} {:>9.0}µs",
            r.write,
            r.stats.reruns,
            r.stats.pushes,
            r.stats.unchanged(),
            r.took.as_secs_f64() * 1e6 / r.stats.reruns.max(1) as f64
        );
    }
}

fn delta(after: LiveStats, before: LiveStats) -> LiveStats {
    LiveStats {
        reruns: after.reruns - before.reruns,
        pushes: after.pushes - before.pushes,
    }
}

#[tokio::test]
async fn a_write_re_runs_every_query_on_its_table() {
    let (app, path) = db_app().await;
    let live = app.get::<LiveRegistry>();
    let _list = app.live::<Page<Customer>>("customers_index", ()).await;
    let mut shown = Vec::new();
    for id in 1..=10i64 {
        shown.push(app.live::<Option<Customer>>("customers_show", (id,)).await);
    }

    let mut rows = Vec::new();
    let mut measure = |write: &'static str, before: LiveStats, after: LiveStats, took| {
        rows.push(Row {
            write,
            stats: delta(after, before),
            took,
        });
    };

    // A customer no window shows (40: past page 1, no detail open).
    let before = live.stats();
    let started = Instant::now();
    for i in 0..10 {
        let each = live.stats();
        app.invoke_ok::<()>("customers_rename", (40i64, format!("Forty {i}")))
            .await;
        reran(&live, each, OPEN).await;
    }
    settled(&live, before, 10 * OPEN).await;
    measure(
        "update a row no view shows",
        before,
        live.stats(),
        started.elapsed(),
    );

    // One a detail view and the list show (5).
    let before = live.stats();
    let started = Instant::now();
    for i in 0..10 {
        let each = live.stats();
        app.invoke_ok::<()>("customers_rename", (5i64, format!("Five {i}")))
            .await;
        reran(&live, each, OPEN).await;
    }
    settled(&live, before, 10 * OPEN).await;
    measure(
        "update a row the list and one view show",
        before,
        live.stats(),
        started.elapsed(),
    );

    // A new one: past page 1, but the list's total changes.
    let before = live.stats();
    let started = Instant::now();
    for i in 0..10 {
        let each = live.stats();
        app.invoke_ok::<()>("customers_store", (format!("New {i}"),))
            .await;
        reran(&live, each, OPEN).await;
    }
    settled(&live, before, 10 * OPEN).await;
    measure("insert a row", before, live.stats(), started.elapsed());

    print("SQLite: 1 list + 10 detail views open", &rows);

    // Per table: every write re-runs all 11.
    assert!(rows.iter().all(|r| r.stats.reruns == 10 * OPEN));
    // Needed: none, the list and one view, the list.
    assert_eq!(rows[0].stats.pushes, 0);
    assert_eq!(rows[0].stats.unchanged(), 110);
    assert_eq!(rows[1].stats.pushes, 20);
    assert_eq!(rows[1].stats.unchanged(), 90);
    assert_eq!(rows[2].stats.pushes, 10);
    drop(shown);
    let _ = std::fs::remove_file(path);
}

#[cfg(feature = "backend")]
mod backend {
    //! The same over the Laravel backend: a re-run is a request to the
    //! server, from each window, for each write anywhere on the resource.
    use super::*;
    use elyra::backend::Backend;
    use elyra::http::{Http, HttpFake};
    use serde_json::json;

    const BASE: &str = "https://crm.test";

    #[command(live)]
    async fn api_customers_index(ctx: Ctx) -> elyra::Result<serde_json::Value> {
        Ok(ctx.get::<Backend>().get("/api/customers").json().await?)
    }

    #[command(live)]
    async fn api_customers_show(ctx: Ctx, id: i64) -> elyra::Result<serde_json::Value> {
        Ok(ctx
            .get::<Backend>()
            .get(format!("/api/customers/{id}"))
            .json()
            .await?)
    }

    #[command]
    async fn api_customers_rename(
        ctx: Ctx,
        id: i64,
        name: String,
    ) -> elyra::Result<serde_json::Value> {
        Ok(ctx
            .get::<Backend>()
            .put(format!("/api/customers/{id}"))
            .body(&json!({ "name": name }))
            .json()
            .await?)
    }

    #[tokio::test]
    async fn a_write_refetches_every_view_of_the_resource_from_the_server() {
        let mut fake = HttpFake::new()
            .get(
                format!("{BASE}/api/customers"),
                200,
                json!({ "data": [], "total": 50 }),
            )
            .put(format!("{BASE}/api/customers/40"), 200, json!({ "id": 40 }));
        for id in 1..=10 {
            fake = fake.get(
                format!("{BASE}/api/customers/{id}"),
                200,
                json!({ "id": id, "name": format!("Customer {id}") }),
            );
        }
        let app = TestApp::new(
            App::new()
                .backend(Backend::new(BASE))
                .live_batch_window(Duration::ZERO)
                .commands(commands![
                    api_customers_index,
                    api_customers_show,
                    api_customers_rename
                ])
                .swap(Http::fake(fake.clone())),
        );
        let live = app.get::<LiveRegistry>();
        let _list = app
            .live::<serde_json::Value>("api_customers_index", ())
            .await;
        let mut shown = Vec::new();
        for id in 1..=10i64 {
            shown.push(
                app.live::<serde_json::Value>("api_customers_show", (id,))
                    .await,
            );
        }
        let gets = || fake.sent().iter().filter(|s| s.method == "GET").count();
        let gets_before = gets();

        let before = live.stats();
        let started = Instant::now();
        for i in 0..10 {
            let each = live.stats();
            app.invoke_ok::<serde_json::Value>(
                "api_customers_rename",
                (40i64, format!("Forty {i}")),
            )
            .await;
            reran(&live, each, OPEN).await;
        }
        settled(&live, before, 10 * OPEN).await;
        let rows = [Row {
            write: "update a row no view shows",
            stats: delta(live.stats(), before),
            took: started.elapsed(),
        }];
        print(
            "Backend: 1 list + 10 detail views open (requests to a fake)",
            &rows,
        );
        let requests = gets() - gets_before;
        eprintln!("GET requests to the server: {requests}");

        // Each write: 11 requests, none of which found anything new.
        assert_eq!(rows[0].stats.reruns, 110);
        assert_eq!(rows[0].stats.pushes, 0);
        assert_eq!(requests, 110);
        drop(shown);
    }
}
