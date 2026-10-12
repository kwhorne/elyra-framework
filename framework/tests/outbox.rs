//! The outbox (RFC 0007 step 2): writes marked `.queue_offline()` wait when
//! there's no answer, and are sent in order — once, with their key — when the
//! server is back. An `HttpFake` stands in for Laravel; the outbox is a real
//! SQLite database.
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::{Backend, BackendError};
use elyra::db::sqlx;
use elyra::http::{Http, HttpFake, Method};
use elyra::offline::{Outbox, OutboxFailed, OutboxSent, OutboxStatus};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database, Dispatcher};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const TOKEN_KEY: &str = "backend-token:https://crm.test";
const USER_KEY: &str = "backend-user:https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

/// What the command dispatches once the customer really exists.
#[derive(Clone, Debug)]
struct CustomerCreated;

#[command]
async fn customers_store(ctx: Ctx, name: String) -> elyra::Result<Value> {
    let customer: Value = ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&json!({ "name": name }))
        .queue_offline()
        .resource()
        .await?;
    ctx.dispatch(CustomerCreated).await?;
    Ok(customer)
}

#[command]
async fn customers_update(ctx: Ctx, id: i64, name: String) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .query(&json!({ "notify": true }))
        .header("X-Reason", "edited offline")
        .body(&json!({ "name": name }))
        .queue_offline()
        .json()
        .await?)
}

#[command]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .queue_offline()
        .send()
        .await?;
    Ok(())
}

/// An update only if nobody changed the record since the user saw it.
#[command]
async fn customers_update_since(
    ctx: Ctx,
    id: i64,
    name: String,
    since: String,
) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&json!({ "name": name }))
        .unless_changed_since(since)
        .queue_offline()
        .json()
        .await?)
}

#[command(live)]
async fn customers_show(ctx: Ctx, id: i64) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .json()
        .await?)
}

/// A write that mustn't go later: no `.queue_offline()`.
#[command]
async fn pay(ctx: Ctx) -> elyra::Result<()> {
    ctx.get::<Backend>().post("/api/payments").send().await?;
    Ok(())
}

fn laravel(user: i64) -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": user, "name": "Ada" }))
        .delete(url("/api/sanctum/token"), 204, Value::Null)
        .get(url("/up"), 200, json!("ok"))
}

/// The network goes.
fn go_offline(fake: &HttpFake) {
    for method in [Method::Get, Method::Post, Method::Put, Method::Delete] {
        fake.clone().unreachable(method, url("/*"));
    }
}

/// …and comes back, with these answers.
fn come_back(fake: &HttpFake) -> HttpFake {
    fake.clone()
        .get(url("/up"), 200, json!("ok"))
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 1, "name": "Ada" }))
        .get(
            url("/api/customers/*"),
            200,
            json!({ "id": 1, "name": "Acme" }),
        )
        .post(url("/api/customers"), 201, json!({ "data": { "id": 7 } }))
        .put(url("/api/customers/*"), 200, json!({ "id": 1 }))
        .delete(url("/api/customers/*"), 204, Value::Null)
        .delete(url("/api/sanctum/token"), 204, Value::Null)
}

fn db_path() -> std::path::PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-outbox-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

async fn connect(path: &std::path::Path) -> Database {
    Database::connect(&elyra::db::sqlite_url(path))
        .await
        .unwrap()
}

fn app_with(db: Database, fake: &HttpFake, tokens: MemoryTokens) -> TestApp {
    TestApp::new(
        App::new()
            .bind(db)
            .backend(Backend::new(BASE).offline())
            .token_store(Arc::new(tokens))
            .commands(commands![
                customers_store,
                customers_update,
                customers_destroy,
                customers_update_since,
                customers_show,
                pay
            ])
            .swap(Dispatcher::fake())
            .swap(Http::fake(fake.clone())),
    )
}

/// Signed in as Ada, online.
async fn app(path: &std::path::Path, fake: &HttpFake) -> TestApp {
    let app = app_with(connect(path).await, fake, MemoryTokens::new());
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    app
}

async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while !check().await {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn key_of(sent: &elyra::http::Sent) -> String {
    sent.header("idempotency-key")
        .expect("an Idempotency-Key")
        .to_owned()
}

#[tokio::test]
async fn a_write_with_no_answer_waits_and_is_sent_once_back() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);

    assert_eq!(
        app.invoke_err_kind("customers_store", ("Acme".to_string(),))
            .await
            .as_deref(),
        Some("queued")
    );
    let err = app
        .invoke_err("customers_store", ("Beta".to_string(),))
        .await;
    assert!(err.contains("will be sent"), "{err}");
    let outbox = app.get::<Outbox>();
    let pending = outbox.pending().await;
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].status, OutboxStatus::Pending);
    assert_eq!(pending[0].body, Some(json!({ "name": "Acme" })));
    assert_eq!(app.get::<Backend>().connection().unwrap().pending, 2);
    // The code after the write ran for neither: the customers don't exist yet.
    assert!(app
        .get::<Dispatcher>()
        .dispatched::<CustomerCreated>()
        .is_empty());

    // Back: the probe notices, and the outbox empties.
    come_back(&fake);
    eventually("the outbox sent", async || {
        outbox.pending().await.is_empty()
    })
    .await;
    let sent = fake.sent_to("POST", &url("/api/customers"));
    // Acme was tried at the time (no answer); Beta, known offline by then,
    // went straight to the outbox. Then both, in order, Acme with its key.
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert_eq!(sent[1].body, Some(json!({ "name": "Acme" })));
    assert_eq!(sent[2].body, Some(json!({ "name": "Beta" })));
    assert_eq!(key_of(&sent[0]), key_of(&sent[1]), "the same key again");
    assert_ne!(key_of(&sent[1]), key_of(&sent[2]), "a key per write");
    let events = app.get::<Dispatcher>();
    eventually("OutboxSent twice", async || {
        events.dispatched::<OutboxSent>().len() == 2
    })
    .await;
    let first = &events.dispatched::<OutboxSent>()[0];
    assert_eq!(
        (first.method.as_str(), first.path.as_str(), first.status),
        ("POST", "/api/customers", 201)
    );
    assert_eq!(first.body["data"]["id"], 7, "the server's answer");
    assert_eq!(app.get::<Backend>().connection().unwrap().pending, 0);
}

#[tokio::test]
async fn writes_go_in_order_and_a_new_one_waits_behind_them() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    // The server answers, but can't take the create now.
    fake.clone()
        .post(url("/api/customers"), 503, json!({}))
        .put(url("/api/customers/*"), 200, json!({ "id": 1 }));
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("Acme".to_string(),))
        .await;
    fake.clone()
        .get(url("/up"), 200, json!("ok"))
        .post(url("/api/customers"), 503, json!({}))
        .put(url("/api/customers/*"), 200, json!({ "id": 1 }));
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    assert_eq!(outbox.pending().await[0].attempts, 1, "a 503: later");

    // Online, but the create still waits: the update waits behind it.
    assert_eq!(
        app.invoke_err_kind("customers_update", (1, "Acme AS".to_string()))
            .await
            .as_deref(),
        Some("queued")
    );
    assert!(fake.sent_to("PUT", &url("/api/customers/1")).is_empty());

    fake.clone()
        .post(url("/api/customers"), 201, json!({ "data": { "id": 7 } }));
    outbox.flush().await;
    assert!(outbox.pending().await.is_empty());
    let order: Vec<String> = fake
        .sent()
        .iter()
        .filter(|s| s.url.contains("/api/customers"))
        .map(|s| s.method.to_string())
        .collect();
    assert_eq!(order.last().map(String::as_str), Some("PUT"));
    let put = fake.assert_sent("PUT", &url("/api/customers/1"));
    assert!(put.url.contains("notify=1"), "the query kept: {}", put.url);
    assert_eq!(put.header("x-reason"), Some("edited offline"));
    assert_eq!(put.header("authorization"), Some("Bearer 7|s3cr3t"));
}

#[tokio::test]
async fn a_refused_write_waits_for_the_user_and_the_rest_go_on() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("".to_string(),))
        .await;
    app.invoke_err_kind("customers_update", (1, "Acme".to_string()))
        .await;

    come_back(&fake).post(
        url("/api/customers"),
        422,
        json!({ "message": "The name field is required.",
                "errors": { "name": ["The name field is required."] } }),
    );
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    assert!(outbox.pending().await.is_empty(), "the update went on");
    fake.assert_sent("PUT", &url("/api/customers/1"));
    let failed = outbox.failed().await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].status, OutboxStatus::Failed);
    assert_eq!(
        failed[0].errors.as_ref().unwrap()["name"][0],
        "The name field is required."
    );
    assert_eq!(app.get::<Backend>().connection().unwrap().failed, 1);
    let events = app.get::<Dispatcher>();
    eventually("OutboxFailed", async || {
        !events.dispatched::<OutboxFailed>().is_empty()
    })
    .await;
    assert_eq!(events.dispatched::<OutboxFailed>()[0].status, 422);

    // The user fixes it.
    fake.clone()
        .post(url("/api/customers"), 201, json!({ "data": { "id": 7 } }));
    outbox
        .retry(failed[0].id, Some(json!({ "name": "Acme" })))
        .await
        .unwrap();
    eventually("the retry sent", async || outbox.items().await.is_empty()).await;
    let last = fake.sent_to("POST", &url("/api/customers")).pop().unwrap();
    assert_eq!(last.body, Some(json!({ "name": "Acme" })));
    assert_eq!(app.get::<Backend>().connection().unwrap().failed, 0);

    // Or throws it away.
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("Gone".to_string(),))
        .await;
    let id = outbox.pending().await[0].id;
    outbox.discard(id).await.unwrap();
    assert!(outbox.items().await.is_empty());
    assert!(matches!(
        outbox.discard(id).await,
        Err(BackendError::NotFound)
    ));
}

#[tokio::test]
async fn too_many_requests_waits_as_long_as_the_server_says() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("Acme".to_string(),))
        .await;
    come_back(&fake).on_with_headers(
        Method::Post,
        url("/api/customers"),
        429,
        &[("Retry-After", "120")],
        json!({}),
    );
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    let sent = fake.sent_to("POST", &url("/api/customers")).len();
    let item = &outbox.pending().await[0];
    assert_eq!(item.attempts, 1);
    // Not before its time.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(fake.sent_to("POST", &url("/api/customers")).len(), sent);
}

#[tokio::test]
async fn a_401_pauses_the_outbox_until_the_user_signs_in_again() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("Acme".to_string(),))
        .await;
    come_back(&fake).post(
        url("/api/customers"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    let auth = app.get::<Auth>();
    assert!(!auth.is_signed_in(), "the token expired");
    assert_eq!(outbox.pending().await.len(), 1, "kept for the same user");

    fake.clone()
        .post(url("/api/customers"), 201, json!({ "data": { "id": 7 } }));
    auth.sign_in("ada@example.com", "secret").await.unwrap();
    eventually("sent after signing in", async || {
        outbox.pending().await.is_empty()
    })
    .await;
}

#[tokio::test]
async fn signing_out_refuses_while_writes_wait() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_store", ("Acme".to_string(),))
        .await;

    let auth = app.get::<Auth>();
    match auth.sign_out().await {
        Err(e @ BackendError::PendingChanges(1)) => {
            assert_eq!(elyra::Error::from(e).kind(), Some("pending_changes"));
        }
        other => panic!("{other:?}"),
    }
    assert!(auth.is_signed_in());

    auth.sign_out_discarding().await.unwrap();
    assert!(!auth.is_signed_in());
    come_back(&fake);
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(
        fake.sent_to("POST", &url("/api/customers")).len(),
        1,
        "never sent: only the first try, at the time"
    );
}

#[tokio::test]
async fn the_outbox_outlives_a_restart() {
    let path = db_path();
    let fake = laravel(1);
    {
        let app = app(&path, &fake).await;
        go_offline(&fake);
        app.invoke_err_kind("customers_store", ("Acme".to_string(),))
            .await;
    }
    // The next launch, back online: what was left is sent.
    let online = come_back(&laravel(1));
    let app = app_with(
        connect(&path).await,
        &online,
        MemoryTokens::new()
            .with(TOKEN_KEY, "7|s3cr3t")
            .with(USER_KEY, "1"),
    );
    let outbox = app.get::<Outbox>();
    eventually("sent on launch", async || {
        !online.sent_to("POST", &url("/api/customers")).is_empty()
    })
    .await;
    eventually("emptied", async || outbox.pending().await.is_empty()).await;
}

#[tokio::test]
async fn deleting_what_is_already_gone_counts_as_sent() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_destroy", (9,)).await;
    come_back(&fake).delete(url("/api/customers/9"), 404, json!({}));
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    assert!(
        outbox.items().await.is_empty(),
        "not failed: gone either way"
    );
}

#[tokio::test]
async fn a_write_not_marked_fails_offline_as_before() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    assert_eq!(
        app.invoke_err_kind("pay", ()).await.as_deref(),
        Some("offline")
    );
    assert!(app.get::<Outbox>().items().await.is_empty());
}

#[tokio::test]
async fn the_token_is_never_kept() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    go_offline(&fake);
    app.invoke_err_kind("customers_update", (1, "Acme".to_string()))
        .await;
    let db = connect(&path).await;
    let rows: Vec<String> = sqlx::query_scalar("SELECT headers FROM elyra_outbox")
        .fetch_all(db.pool())
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].contains("X-Reason"), "{}", rows[0]);
    assert!(
        !rows[0].to_lowercase().contains("authorization"),
        "{}",
        rows[0]
    );
    assert!(!rows[0].contains("s3cr3t"), "{}", rows[0]);
}

const SEEN: &str = "2026-10-12T10:00:00.000000Z";

#[tokio::test]
async fn a_changed_record_is_a_conflict() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    fake.clone().put(
        url("/api/customers/1"),
        412,
        json!({ "message": "Bob changed this customer." }),
    );
    let args = (1, "Acme".to_string(), SEEN.to_string());
    assert_eq!(
        app.invoke_err_kind("customers_update_since", args.clone())
            .await
            .as_deref(),
        Some("conflict")
    );
    let err = app.invoke_err("customers_update_since", args.clone()).await;
    assert!(err.contains("Bob changed"), "{err}");
    let sent = fake.assert_sent("PUT", &url("/api/customers/1"));
    assert_eq!(sent.header("if-unmodified-since"), Some(SEEN));

    // A 409 says the same.
    fake.clone().put(url("/api/customers/1"), 409, json!({}));
    assert_eq!(
        app.invoke_err_kind("customers_update_since", args)
            .await
            .as_deref(),
        Some("conflict")
    );
}

/// A write queued offline, refused as a conflict once back.
async fn conflicted(app: &TestApp, fake: &HttpFake) -> elyra::offline::OutboxItem {
    go_offline(fake);
    app.invoke_err_kind(
        "customers_update_since",
        (1, "Acme".to_string(), SEEN.to_string()),
    )
    .await;
    come_back(fake).put(
        url("/api/customers/1"),
        412,
        json!({ "message": "Changed." }),
    );
    let outbox = app.get::<Outbox>();
    outbox.flush().await;
    let failed = outbox.failed().await;
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].reason.as_deref(), Some("conflict"));
    failed[0].clone()
}

#[tokio::test]
async fn keep_mine_sends_it_again_without_the_precondition() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    let item = conflicted(&app, &fake).await;
    let before = fake.sent_to("PUT", &url("/api/customers/1")).len();

    // What the server does when nothing's to compare: the last write wins.
    fake.clone().on_with_headers(
        Method::Put,
        url("/api/customers/1"),
        200,
        &[],
        json!({ "id": 1, "name": "Acme" }),
    );
    let outbox = app.get::<Outbox>();
    outbox.keep_mine(item.id).await.unwrap();
    eventually("sent again", async || outbox.items().await.is_empty()).await;
    let puts = fake.sent_to("PUT", &url("/api/customers/1"));
    assert_eq!(puts.len(), before + 1);
    let last = puts.last().unwrap();
    assert_eq!(last.header("if-unmodified-since"), None, "mine wins");
    assert_eq!(last.body, Some(json!({ "name": "Acme" })));
}

#[tokio::test]
async fn take_theirs_drops_it_and_shows_the_servers_version() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    fake.clone().get(
        url("/api/customers/1"),
        200,
        json!({ "id": 1, "name": "Acme" }),
    );
    let mut shown = app.live::<Value>("customers_show", (1,)).await;
    let item = conflicted(&app, &fake).await;
    let puts = fake.sent_to("PUT", &url("/api/customers/1")).len();

    // What re-ran on the way (back online, the refusal) has settled: nothing
    // reads the record again on its own.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let reads = || fake.sent_to("GET", &url("/api/customers/1")).len();
    let settled = reads();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reads(), settled);

    fake.clone().get(
        url("/api/customers/1"),
        200,
        json!({ "id": 1, "name": "Bob's Acme" }),
    );
    let outbox = app.get::<Outbox>();
    outbox.take_theirs(item.id).await.unwrap();
    assert!(outbox.items().await.is_empty());
    loop {
        shown.next().await;
        if shown.value()["name"] == "Bob's Acme" {
            break;
        }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        fake.sent_to("PUT", &url("/api/customers/1")).len(),
        puts,
        "never sent"
    );
}
