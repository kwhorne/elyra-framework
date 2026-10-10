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
                sneaky
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
