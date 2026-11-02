//! Broadcasting into live queries (RFC 0006 step 2): an event on a followed
//! channel re-runs what read its resource; a reconnect re-runs everything
//! followed, since events sent meanwhile are lost. An event that names its
//! record re-runs the lists and what showed that record (RFC 0017).
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::Arc;
use std::time::Duration;

use elyra::auth::MemoryTokens;
use elyra::backend::Backend;
use elyra::broadcasting::PusherFake;
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::json;

const BASE: &str = "https://crm.test";

#[command(live)]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[command(live)]
async fn orders_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx.get::<Backend>().get("/api/orders/count").json().await?)
}

#[command(live)]
async fn customer_show(ctx: Ctx, id: i64) -> elyra::Result<serde_json::Value> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .json()
        .await?)
}

#[command(live)]
async fn invoice_show(ctx: Ctx, uuid: String) -> elyra::Result<serde_json::Value> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/invoices/{uuid}"))
        .json()
        .await?)
}

const INVOICE_A: &str = "9b2f5c1e-3d4a-4e5f-8a9b-0c1d2e3f4a5b";
const INVOICE_B: &str = "1c2d3e4f-5a6b-4c7d-8e9f-0a1b2c3d4e5f";

/// How many times `path` was read.
fn reads(fake: &HttpFake, path: &str) -> usize {
    fake.sent_to("GET", &format!("{BASE}{path}")).len()
}

/// Wait until `path` has been read `n` times, then a moment more for any
/// re-run that shouldn't happen to show.
async fn read_times(fake: &HttpFake, path: &str, n: usize) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while reads(fake, path) < n {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{path} read {} times",
            reads(fake, path)
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
}

fn app(fake: &HttpFake, pusher: &PusherFake) -> TestApp {
    TestApp::new(
        App::new()
            .backend(
                Backend::new(BASE)
                    .broadcasting(pusher.config())
                    .follow("customers", "/api/customers")
                    .follow("invoices", "/api/invoices")
                    .record_key("/api/invoices", "uuid"),
            )
            .token_store(Arc::new(
                MemoryTokens::new().with("backend-token:https://crm.test", "7|s3cr3t"),
            ))
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![
                customers_count,
                orders_count,
                customer_show,
                invoice_show
            ])
            .swap(Http::fake(fake.clone())),
    )
}

fn laravel(customers: i64) -> HttpFake {
    HttpFake::new()
        .on(
            Method::Post,
            format!("{BASE}/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:sig" }),
        )
        .get(format!("{BASE}/api/customers/count"), 200, json!(customers))
        .get(format!("{BASE}/api/orders/count"), 200, json!(5))
        .get(format!("{BASE}/api/customers/40"), 200, json!({ "id": 40 }))
        .get(format!("{BASE}/api/customers/42"), 200, json!({ "id": 42 }))
        .get(
            format!("{BASE}/api/invoices/{INVOICE_A}"),
            200,
            json!({ "uuid": INVOICE_A }),
        )
        .get(
            format!("{BASE}/api/invoices/{INVOICE_B}"),
            200,
            json!({ "uuid": INVOICE_B }),
        )
}

#[tokio::test]
async fn a_change_on_the_web_updates_the_open_window() {
    let fake = laravel(3);
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher);
    let mut customers = app.live::<i64>("customers_count", ()).await;
    let mut orders = app.live::<i64>("orders_count", ()).await;
    pusher.wait_for_subscriptions(1).await;
    assert_eq!(*customers.value(), 3);

    // Someone adds a customer on the web; Laravel says so.
    fake.clone()
        .get(format!("{BASE}/api/customers/count"), 200, json!(4));
    pusher.broadcast(
        "private-customers",
        "CustomerCreated",
        json!({ "model": { "id": 4 } }),
    );
    customers.next().await;
    assert_eq!(*customers.value(), 4);

    // What doesn't read the resource doesn't re-run.
    let order_reads = fake
        .sent_to("GET", &format!("{BASE}/api/orders/count"))
        .len();
    assert!(!orders.updated_within(Duration::from_millis(150)).await);
    assert_eq!(
        fake.sent_to("GET", &format!("{BASE}/api/orders/count"))
            .len(),
        order_reads
    );
}

#[tokio::test]
async fn a_reconnect_re_runs_what_it_follows() {
    let fake = laravel(3);
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher);
    let mut customers = app.live::<i64>("customers_count", ()).await;
    pusher.wait_for_subscriptions(1).await;

    // Gone for a while: a customer was added, and the event was lost.
    pusher.disconnect_all();
    fake.clone()
        .get(format!("{BASE}/api/customers/count"), 200, json!(9));
    pusher.wait_for_subscriptions(2).await;
    customers.next().await;
    assert_eq!(*customers.value(), 9, "caught up on reconnect");
}

#[tokio::test]
async fn an_event_naming_a_record_re_runs_the_lists_and_what_showed_it() {
    let fake = laravel(3);
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher);
    let _count = app.live::<i64>("customers_count", ()).await;
    let _forty = app
        .live::<serde_json::Value>("customer_show", (40i64,))
        .await;
    let _other = app
        .live::<serde_json::Value>("customer_show", (42i64,))
        .await;
    pusher.wait_for_subscriptions(2).await;

    // Ada renames customer 40 on the web.
    pusher.broadcast(
        "private-customers",
        "CustomerUpdated",
        json!({ "model": { "id": 40, "name": "Forty" } }),
    );
    read_times(&fake, "/api/customers/40", 2).await;
    read_times(&fake, "/api/customers/count", 2).await;
    assert_eq!(
        reads(&fake, "/api/customers/42"),
        1,
        "another customer's view"
    );

    // An id as text is one too.
    pusher.broadcast(
        "private-customers",
        "CustomerUpdated",
        json!({ "model": { "id": "40" } }),
    );
    read_times(&fake, "/api/customers/40", 3).await;
    assert_eq!(reads(&fake, "/api/customers/42"), 1);
}

#[tokio::test]
async fn an_event_naming_no_record_re_runs_every_view_of_the_resource() {
    let fake = laravel(3);
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher);
    let _forty = app
        .live::<serde_json::Value>("customer_show", (40i64,))
        .await;
    let _other = app
        .live::<serde_json::Value>("customer_show", (42i64,))
        .await;
    pusher.wait_for_subscriptions(2).await;

    // A bulk import says nothing of which; an id that isn't one names none.
    for (n, data) in [
        json!({ "imported": 120 }),
        json!({ "model": { "name": "no id" } }),
        json!({ "model": { "id": "../../etc" } }),
        json!({ "model": { "id": -4 } }),
        // An id outside the model isn't Laravel's model broadcasting.
        json!({ "id": 40 }),
    ]
    .into_iter()
    .enumerate()
    {
        pusher.broadcast("private-customers", "CustomersChanged", data);
        read_times(&fake, "/api/customers/40", n + 2).await;
        read_times(&fake, "/api/customers/42", n + 2).await;
    }
}

#[tokio::test]
async fn a_resource_can_name_its_records_by_another_key() {
    let fake = laravel(3);
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher);
    let _a = app
        .live::<serde_json::Value>("invoice_show", (INVOICE_A.to_string(),))
        .await;
    let _b = app
        .live::<serde_json::Value>("invoice_show", (INVOICE_B.to_string(),))
        .await;
    pusher.wait_for_subscriptions(2).await;

    // `record_key("/api/invoices", "uuid")`: the model's uuid, not its id.
    pusher.broadcast(
        "private-invoices",
        "InvoiceUpdated",
        json!({ "model": { "id": 7, "uuid": INVOICE_A } }),
    );
    read_times(&fake, &format!("/api/invoices/{INVOICE_A}"), 2).await;
    assert_eq!(reads(&fake, &format!("/api/invoices/{INVOICE_B}")), 1);
}
