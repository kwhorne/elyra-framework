//! Broadcasting into live queries (RFC 0006 step 2): an event on a followed
//! channel re-runs what read its resource; a reconnect re-runs everything
//! followed, since events sent meanwhile are lost.
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

fn app(fake: &HttpFake, pusher: &PusherFake) -> TestApp {
    TestApp::new(
        App::new()
            .backend(
                Backend::new(BASE)
                    .broadcasting(pusher.config())
                    .follow("customers", "/api/customers"),
            )
            .token_store(Arc::new(
                MemoryTokens::new().with("backend-token:https://crm.test", "7|s3cr3t"),
            ))
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![customers_count, orders_count])
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
