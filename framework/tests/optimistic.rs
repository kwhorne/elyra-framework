//! Shown at once (RFC 0039): a write marked `.optimistic()` shows in the
//! live lists before the server answers, from the offline copy and without
//! asking the server, stays shown while it waits in the outbox (across a
//! restart), and is taken back when the server refuses it.
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const LIST: &str = "https://crm.test/api/customers";
const ACME: &str = "https://crm.test/api/customers/42";

#[command(live)]
async fn customers_index(ctx: Ctx) -> elyra::Result<Value> {
    Ok(ctx.get::<Backend>().get("/api/customers").json().await?)
}

#[command(live)]
async fn customers_show(ctx: Ctx, id: i64) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .json()
        .await?)
}

#[command]
async fn customers_rename(ctx: Ctx, id: i64, name: String) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&json!({ "name": name, "password": "not shown" }))
        .optimistic()
        .queue_offline()
        .json()
        .await?)
}

#[command]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .optimistic()
        .queue_offline()
        .json()
        .await?)
}

fn page(name: &str) -> Value {
    json!({ "data": [{ "id": 41, "name": "Bergen Bakeri" }, { "id": 42, "name": name }], "total": 2 })
}

fn laravel() -> HttpFake {
    HttpFake::new()
        .get(format!("{BASE}/up"), 200, json!("ok"))
        .get(LIST, 200, page("Acme"))
        .get(
            ACME,
            200,
            json!({ "data": { "id": 42, "name": "Acme", "updated_at": "t1" } }),
        )
}

fn db_path() -> std::path::PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-optimistic-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

async fn app(path: &std::path::Path, fake: &HttpFake) -> TestApp {
    let db = Database::connect(&elyra::db::sqlite_url(path))
        .await
        .unwrap();
    let app = TestApp::new(
        App::new()
            .bind(db)
            .backend(Backend::new(BASE).offline())
            .token_store(Arc::new(MemoryTokens::new()))
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![
                customers_index,
                customers_show,
                customers_rename,
                customers_destroy
            ])
            .swap(Http::fake(fake.clone())),
    );
    app.listen();
    app.get::<Auth>()
        .sign_in_with_token(BASE, "t0ken", json!({ "id": 7, "name": "Ada" }))
        .await
        .unwrap();
    app
}

/// Run a command in the background: whether it succeeded.
fn spawn(
    app: &TestApp,
    command: &'static str,
    args: impl serde::Serialize,
) -> tokio::task::JoinHandle<bool> {
    let registry = app.registry();
    let ctx = app.ctx().clone();
    let body = rmp_serde::to_vec(&args).unwrap();
    tokio::spawn(async move { registry.dispatch(ctx, command, &body).await.is_ok() })
}

fn name_of(list: &Value, id: i64) -> Option<String> {
    list["data"]
        .as_array()?
        .iter()
        .find(|c| c["id"] == id)
        .map(|c| c["name"].as_str().unwrap_or_default().to_owned())
}

#[tokio::test]
async fn a_rename_shows_in_every_list_before_the_server_answers() {
    let path = db_path();
    let fake = laravel()
        .put(
            ACME,
            200,
            json!({ "data": { "id": 42, "name": "Acme AS", "updated_at": "t2" } }),
        )
        .slow(Method::Put, ACME, Duration::from_millis(800));
    let app = app(&path, &fake).await;
    let mut list = app.live::<Value>("customers_index", ()).await;
    let mut record = app.live::<Value>("customers_show", (42,)).await;
    assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme"));
    let asked = fake.sent_to("GET", LIST).len();

    let renaming = spawn(&app, "customers_rename", (42, "Acme AS"));

    // Shown at once, from the copy: the server wasn't asked again.
    assert!(list.updated_within(Duration::from_millis(400)).await);
    assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme AS"));
    assert!(record.updated_within(Duration::from_millis(400)).await);
    assert_eq!(record.value()["data"]["name"], "Acme AS");
    // Only the fields the record has: no password.
    assert!(record.value()["data"].get("password").is_none());
    assert_eq!(fake.sent_to("GET", LIST).len(), asked);
    assert!(!renaming.is_finished(), "before the server answered");

    // The server's answer: the lists read it again from the server.
    let _ = fake.clone().get(LIST, 200, page("Acme AS"));
    assert!(renaming.await.unwrap());
    assert!(
        list.updated_within(Duration::from_secs(2)).await
            || fake.sent_to("GET", LIST).len() > asked
    );
    assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme AS"));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_refused_rename_is_taken_back_in_every_list() {
    let path = db_path();
    let fake = laravel()
        .on(
            Method::Put,
            ACME,
            422,
            json!({ "message": "The name has already been taken.", "errors": { "name": ["The name has already been taken."] } }),
        )
        .slow(Method::Put, ACME, Duration::from_millis(500));
    let app = app(&path, &fake).await;
    let mut list = app.live::<Value>("customers_index", ()).await;

    let renaming = spawn(&app, "customers_rename", (42, "Bergen Bakeri"));
    assert!(list.updated_within(Duration::from_millis(300)).await);
    assert_eq!(name_of(list.value(), 42).as_deref(), Some("Bergen Bakeri"));

    assert!(!renaming.await.unwrap(), "the 422 is the command's");
    assert!(list.updated_within(Duration::from_secs(2)).await);
    assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme"));
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_rename_waiting_in_the_outbox_stays_shown_across_a_restart_until_it_is_sent() {
    let path = db_path();
    let fake = laravel();
    {
        let app = app(&path, &fake).await;
        let mut list = app.live::<Value>("customers_index", ()).await;
        let _ = app.live::<Value>("customers_show", (42,)).await;
        let _ = fake
            .clone()
            .unreachable(Method::Put, ACME)
            .unreachable(Method::Get, format!("{BASE}/*"));
        let kind = app
            .invoke_err_kind("customers_rename", (42, "Acme AS"))
            .await;
        assert_eq!(kind.as_deref(), Some("queued"));
        assert!(list.updated_within(Duration::from_secs(1)).await);
        assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme AS"));
        let pending: Vec<Value> = app.events_on("elyra:pending").await;
        assert_eq!(pending.last(), Some(&json!({ "customers": [42] })));
    }

    // Still offline, after a restart: the copy, with the waiting rename.
    let app = app(&path, &fake).await;
    let list = app.live::<Value>("customers_index", ()).await;
    let shown = name_of(list.value(), 42);
    if shown.as_deref() != Some("Acme AS") {
        // The outbox is read once the app is up.
        let mut list = list;
        assert!(list.updated_within(Duration::from_secs(2)).await);
        assert_eq!(name_of(list.value(), 42).as_deref(), Some("Acme AS"));
    }
    let record: Value = app.invoke_ok("customers_show", (42,)).await;
    assert_eq!(record["data"]["name"], "Acme AS");

    // Back online: sent, and what the server says from then on.
    let _ = fake
        .clone()
        .get(format!("{BASE}/*"), 404, Value::Null)
        .get(format!("{BASE}/up"), 200, json!("ok"))
        .get(LIST, 200, page("Acme AS"))
        .get(
            ACME,
            200,
            json!({ "data": { "id": 42, "name": "Acme AS" } }),
        )
        .put(
            ACME,
            200,
            json!({ "data": { "id": 42, "name": "Acme AS" } }),
        );
    app.get::<elyra::offline::Outbox>().flush().await;
    // Up to 10 s: a busy CI runner (macOS) took more than 2.
    for _ in 0..500 {
        // The first try, offline, and the one from the outbox.
        if fake.sent_to("PUT", ACME).len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let sent = fake.sent_to("PUT", ACME);
    assert_eq!(sent.len(), 2);
    assert!(
        sent.iter()
            .all(|s| s.header("x-elyra-optimistic").is_none()),
        "the mark isn't sent"
    );
    assert_eq!(sent[1].body.as_ref().unwrap()["name"], "Acme AS");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn a_delete_takes_the_row_out_at_once() {
    let path = db_path();
    let fake = laravel().delete(ACME, 204, Value::Null).slow(
        Method::Delete,
        ACME,
        Duration::from_millis(500),
    );
    let app = app(&path, &fake).await;
    let mut list = app.live::<Value>("customers_index", ()).await;

    let deleting = spawn(&app, "customers_destroy", (42,));
    assert!(list.updated_within(Duration::from_millis(300)).await);
    assert_eq!(name_of(list.value(), 42), None);
    assert_eq!(list.value()["total"], 1);
    let _ = deleting.await;
    let _ = std::fs::remove_file(&path);
}
