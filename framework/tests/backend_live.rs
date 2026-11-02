//! Live queries over the backend (RFC 0005 step 4): a live command that reads
//! `/api/customers` re-runs when this app writes to it, and not otherwise.
#![cfg(all(feature = "backend", feature = "database"))]

use std::time::Duration;

use elyra::backend::Backend;
use elyra::http::{Http, HttpFake};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::json;

const BASE: &str = "https://crm.test";

#[derive(Serialize, Deserialize, specta::Type, Debug, PartialEq, Clone)]
struct Customer {
    id: i64,
    name: String,
}

#[command(live)]
async fn customers_list(ctx: Ctx) -> elyra::Result<Vec<Customer>> {
    Ok(ctx.get::<Backend>().get("/api/customers").json().await?)
}

#[command]
async fn customers_create(ctx: Ctx, name: String) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&json!({ "name": name }))
        .json()
        .await?)
}

#[command]
async fn customers_rename(ctx: Ctx, id: i64, name: String) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&json!({ "name": name }))
        .json()
        .await?)
}

#[command]
async fn orders_create(ctx: Ctx) -> elyra::Result<serde_json::Value> {
    Ok(ctx.get::<Backend>().post("/api/orders").json().await?)
}

/// One customer.
#[command(live)]
async fn customer_show(ctx: Ctx, id: i64) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .json()
        .await?)
}

/// One customer, with others' data in it: re-run on every change.
#[command(live)]
async fn customer_with_related(ctx: Ctx, id: i64) -> elyra::Result<Customer> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .with_lists()
        .json()
        .await?)
}

/// What the app's own command depends on, read some other way.
#[command(live)]
async fn customers_seen(ctx: Ctx) -> i64 {
    ctx.depends_on(&elyra::backend::list_key("/api/customers"));
    SEEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

static SEEN: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

#[command]
async fn customers_refresh(ctx: Ctx) {
    ctx.invalidate(&elyra::backend::resource_key("/api/customers"));
}

/// Writes — which a live command must not.
#[command(live)]
async fn sneaky(ctx: Ctx) -> elyra::Result<serde_json::Value> {
    Ok(ctx.get::<Backend>().post("/api/orders").json().await?)
}

fn app(fake: &HttpFake) -> TestApp {
    TestApp::new(
        App::new()
            .backend(Backend::new(BASE))
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![
                customers_list,
                customers_create,
                customers_rename,
                orders_create,
                sneaky,
                customer_show,
                customer_with_related,
                customers_seen,
                customers_refresh
            ])
            .swap(Http::fake(fake.clone())),
    )
}

#[tokio::test]
async fn a_write_to_a_resource_re_runs_what_read_it() {
    let fake = HttpFake::new()
        .get(format!("{BASE}/api/customers"), 200, json!([]))
        .post(
            format!("{BASE}/api/customers"),
            201,
            json!({ "id": 1, "name": "Ada" }),
        )
        .put(
            format!("{BASE}/api/customers/1"),
            200,
            json!({ "id": 1, "name": "Ada L." }),
        )
        .post(format!("{BASE}/api/orders"), 201, json!({ "id": 9 }));
    let app = app(&fake);
    let mut list = app.live::<Vec<Customer>>("customers_list", ()).await;
    assert!(list.value().is_empty());

    // The server has her now; this app's write says so.
    fake.clone().get(
        format!("{BASE}/api/customers"),
        200,
        json!([{ "id": 1, "name": "Ada" }]),
    );
    app.invoke_ok::<Customer>("customers_create", ("Ada".to_string(),))
        .await;
    list.next().await;
    assert_eq!(
        list.value(),
        &[Customer {
            id: 1,
            name: "Ada".into()
        }]
    );

    // `/api/customers/1` is the same resource.
    fake.clone().get(
        format!("{BASE}/api/customers"),
        200,
        json!([{ "id": 1, "name": "Ada L." }]),
    );
    app.invoke_ok::<Customer>("customers_rename", (1, "Ada L.".to_string()))
        .await;
    list.next().await;
    assert_eq!(list.value()[0].name, "Ada L.");

    // Another resource: nothing re-runs.
    let gets = fake.sent_to("GET", &format!("{BASE}/api/customers")).len();
    app.invoke_ok::<serde_json::Value>("orders_create", ())
        .await;
    assert!(!list.updated_within(Duration::from_millis(150)).await);
    assert_eq!(
        fake.sent_to("GET", &format!("{BASE}/api/customers")).len(),
        gets
    );
}

#[tokio::test]
async fn a_failed_write_re_runs_nothing() {
    let fake = HttpFake::new()
        .get(format!("{BASE}/api/customers"), 200, json!([]))
        .post(
            format!("{BASE}/api/customers"),
            422,
            json!({ "errors": { "name": ["Taken."] } }),
        );
    let app = app(&fake);
    let mut list = app.live::<Vec<Customer>>("customers_list", ()).await;
    let gets = fake.sent_to("GET", &format!("{BASE}/api/customers")).len();
    let _ = app
        .invoke_err("customers_create", ("Ada".to_string(),))
        .await;
    assert!(!list.updated_within(Duration::from_millis(150)).await);
    // Not even re-run (an unchanged result wouldn't be pushed anyway).
    assert_eq!(
        fake.sent_to("GET", &format!("{BASE}/api/customers")).len(),
        gets
    );
}

#[tokio::test]
async fn a_live_command_may_not_write_to_the_backend() {
    let fake = HttpFake::new().post(format!("{BASE}/api/orders"), 201, json!({ "id": 9 }));
    let app = app(&fake);
    let refused = app.try_live::<serde_json::Value>("sneaky", ()).await;
    let err = refused.err().expect("refused").to_string();
    assert!(err.contains("live command"), "{err}");
    fake.assert_not_sent("POST", &format!("{BASE}/api/orders"));
}

#[tokio::test]
async fn a_write_to_a_record_re_runs_its_view_and_the_lists_not_anothers() {
    let fake = HttpFake::new()
        .get(format!("{BASE}/api/customers"), 200, json!([]))
        .get(
            format!("{BASE}/api/customers/1"),
            200,
            json!({ "id": 1, "name": "Ada" }),
        )
        .get(
            format!("{BASE}/api/customers/2"),
            200,
            json!({ "id": 2, "name": "Grace" }),
        )
        .put(
            format!("{BASE}/api/customers/1"),
            200,
            json!({ "id": 1, "name": "Ada L." }),
        );
    let app = app(&fake);
    let reads = |path: &str| fake.sent_to("GET", &format!("{BASE}{path}")).len();
    let mut ada = app.live::<Customer>("customer_show", (1i64,)).await;
    let mut grace = app.live::<Customer>("customer_show", (2i64,)).await;
    let mut related = app.live::<Customer>("customer_with_related", (2i64,)).await;
    let mut list = app.live::<Vec<Customer>>("customers_list", ()).await;
    let mut seen = app.live::<i64>("customers_seen", ()).await;

    fake.clone().get(
        format!("{BASE}/api/customers/1"),
        200,
        json!({ "id": 1, "name": "Ada L." }),
    );
    app.invoke_ok::<Customer>("customers_rename", (1i64, "Ada L.".to_string()))
        .await;
    assert_eq!(ada.next().await.name, "Ada L.");
    // The list re-ran (found the same), and what depends on the lists.
    seen.next().await;
    assert!(!list.updated_within(Duration::from_millis(50)).await);
    assert_eq!(reads("/api/customers"), 2);
    // Grace's view didn't; the one that shows others did.
    assert!(!grace.updated_within(Duration::from_millis(100)).await);
    assert!(!related.updated_within(Duration::from_millis(1)).await);
    assert_eq!(
        reads("/api/customers/2"),
        1 + 2,
        "grace once, related twice"
    );

    // The app's own `invalidate` of the resource: every view of it.
    app.invoke_ok::<()>("customers_refresh", ()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(reads("/api/customers/2"), 2 + 3);
    assert_eq!(reads("/api/customers/1"), 3);
}
