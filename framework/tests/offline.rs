//! Working offline (RFC 0007 step 1): the connection state, and the copy of
//! the last answer to each read — answered by an `HttpFake` standing in for
//! Laravel, kept in a real SQLite database.
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::db::sqlx;
use elyra::http::{Http, HttpFake, Method};
use elyra::offline::{Connection, ConnectionState};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const TOKEN_KEY: &str = "backend-token:https://crm.test";
const USER_KEY: &str = "backend-user:https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

#[command(live)]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[command]
async fn customers_page(ctx: Ctx, page: i64) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&json!({ "page": page }))
        .json()
        .await?)
}

/// Whether the count came from the copy.
#[command]
async fn count_is_stale(ctx: Ctx) -> elyra::Result<bool> {
    let response = ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .send()
        .await?;
    Ok(response.stale_since().is_some())
}

#[command]
async fn one_time_code(ctx: Ctx) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/code")
        .no_offline()
        .json()
        .await?)
}

/// Laravel, signing in user `user`, with three customers and its health route.
fn laravel(user: i64) -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": user, "name": "Ada" }))
        .delete(url("/api/sanctum/token"), 204, Value::Null)
        .get(url("/up"), 200, json!("ok"))
        .get(url("/api/customers/count"), 200, json!(3))
        .get(
            url("/api/customers"),
            200,
            json!({ "data": [{ "id": 1 }], "page": 1 }),
        )
        .get(url("/api/code"), 200, json!({ "code": "123456" }))
}

/// The network goes: nothing on the backend answers.
fn go_offline(fake: &HttpFake) {
    fake.clone()
        .unreachable(Method::Get, url("/*"))
        .unreachable(Method::Post, url("/*"))
        .unreachable(Method::Delete, url("/*"));
}

fn db_path() -> std::path::PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-offline-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

async fn connect(path: &std::path::Path) -> Database {
    Database::connect(&elyra::db::sqlite_url(path))
        .await
        .unwrap()
}

fn app_with(db: Database, fake: &HttpFake, backend: Backend, tokens: MemoryTokens) -> TestApp {
    TestApp::new(
        App::new()
            .bind(db)
            .backend(backend)
            .token_store(Arc::new(tokens))
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![
                customers_count,
                customers_page,
                count_is_stale,
                one_time_code
            ])
            .swap(Http::fake(fake.clone())),
    )
}

async fn app(path: &std::path::Path, fake: &HttpFake) -> TestApp {
    app_with(
        connect(path).await,
        fake,
        Backend::new(BASE).offline(),
        MemoryTokens::new(),
    )
}

async fn rows(path: &std::path::Path) -> i64 {
    let db = connect(path).await;
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM elyra_backend_cache")
        .fetch_one(db.pool())
        .await
        .unwrap_or(0)
}

fn connection(app: &TestApp) -> ConnectionState {
    app.get::<Backend>().connection().expect("offline is on")
}

#[tokio::test]
async fn a_read_answers_from_the_copy_offline() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    assert_eq!(app.invoke_ok::<i64>("customers_count", ()).await, 3);
    assert!(!app.invoke_ok::<bool>("count_is_stale", ()).await);

    go_offline(&fake);
    assert_eq!(
        app.invoke_ok::<i64>("customers_count", ()).await,
        3,
        "the last answer, not an error"
    );
    assert!(app.invoke_ok::<bool>("count_is_stale", ()).await);

    // Known offline: answered from the copy without asking the network.
    let asked = fake.sent_to("GET", &url("/api/customers/count")).len();
    app.invoke_ok::<i64>("customers_count", ()).await;
    assert_eq!(
        fake.sent_to("GET", &url("/api/customers/count")).len(),
        asked
    );

    // The copy is per URL, query included: page 2 was never read.
    let err = app.invoke_err("customers_page", (2,)).await;
    assert!(err.contains("can't reach the server"), "{err}");
}

#[tokio::test]
async fn the_connection_goes_offline_and_comes_back_rerunning_what_was_read() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    app.listen();
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let mut count = app.live::<i64>("customers_count", ()).await;
    assert_eq!(*count.value(), 3);
    assert_eq!(connection(&app).state, Connection::Online);

    go_offline(&fake);
    // No answer: offline — and the count still reads, from the copy.
    assert_eq!(app.invoke_ok::<i64>("customers_count", ()).await, 3);
    let state = connection(&app);
    assert_eq!(state.state, Connection::Offline);
    assert!(state.since.is_some());
    let events: Vec<Value> = app.events_on("elyra:connection").await;
    assert!(events.iter().any(|e| e["state"] == "offline"), "{events:?}");

    // Still offline while the health route doesn't answer — not even for a
    // moment.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(connection(&app).state, Connection::Offline);
    let events: Vec<Value> = app.events_on("elyra:connection").await;
    assert!(!events.iter().any(|e| e["state"] == "online"), "{events:?}");

    // The server's back, with a customer added meanwhile.
    fake.clone()
        .get(url("/up"), 200, json!("ok"))
        .get(url("/api/customers/count"), 200, json!(4));
    count.next().await;
    assert_eq!(*count.value(), 4, "re-run once back");
    assert_eq!(connection(&app).state, Connection::Online);
    let events: Vec<Value> = app.events_on("elyra:connection").await;
    assert!(events.iter().any(|e| e["state"] == "online"), "{events:?}");
}

#[tokio::test]
async fn a_read_marked_no_offline_is_never_kept() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    app.invoke_ok::<Value>("one_time_code", ()).await;
    let kept = rows(&path).await;
    go_offline(&fake);
    let err = app.invoke_err("one_time_code", ()).await;
    assert!(err.contains("can't reach the server"), "{err}");
    assert_eq!(rows(&path).await, kept);
}

#[tokio::test]
async fn signing_out_forgets_the_copy_and_someone_else_sees_nothing() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    let auth = app.get::<Auth>();
    auth.sign_in("ada@example.com", "secret").await.unwrap();
    app.invoke_ok::<i64>("customers_count", ()).await;
    assert!(rows(&path).await > 0);
    auth.sign_out().await.unwrap();
    assert_eq!(rows(&path).await, 0, "signing out clears it");

    // Ada's sign-in expires instead (a 401): her copy stays, for her…
    auth.sign_in("ada@example.com", "secret").await.unwrap();
    app.invoke_ok::<i64>("customers_count", ()).await;
    fake.clone()
        .get(url("/api/customers/count"), 401, json!({}));
    app.invoke_err("customers_count", ()).await;
    assert!(!auth.is_signed_in());
    assert!(rows(&path).await > 0, "an expiry isn't a sign-out");

    // …until Bob signs in on this machine.
    fake.clone()
        .get(url("/api/user"), 200, json!({ "id": 2, "name": "Bob" }))
        .get(url("/api/customers/count"), 200, json!(3));
    auth.sign_in("bob@example.com", "secret").await.unwrap();
    assert_eq!(rows(&path).await, 0, "Ada's copy is gone");
    go_offline(&fake);
    let err = app.invoke_err("customers_count", ()).await;
    assert!(err.contains("can't reach the server"), "{err}");
}

#[tokio::test]
async fn the_copy_outlives_a_restart() {
    let path = db_path();
    let fake = laravel(1);
    {
        let app = app(&path, &fake).await;
        app.get::<Auth>()
            .sign_in("ada@example.com", "secret")
            .await
            .unwrap();
        app.invoke_ok::<i64>("customers_count", ()).await;
    }
    // The next launch, still signed in, on a train.
    let offline = laravel(1);
    go_offline(&offline);
    let app = app_with(
        connect(&path).await,
        &offline,
        Backend::new(BASE).offline(),
        MemoryTokens::new()
            .with(TOKEN_KEY, "7|s3cr3t")
            .with(USER_KEY, "1"),
    );
    assert_eq!(app.invoke_ok::<i64>("customers_count", ()).await, 3);
}

#[tokio::test]
async fn past_the_limit_the_oldest_answers_go() {
    let path = db_path();
    let fake = laravel(1);
    let app = app_with(
        connect(&path).await,
        &fake,
        // Room for one page (28 bytes), not two.
        Backend::new(BASE).offline().offline_cache_limit(40),
        MemoryTokens::new(),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    fake.clone().get(
        url("/api/customers"),
        200,
        json!({ "data": [{ "id": 1 }], "page": 1 }),
    );
    app.invoke_ok::<Value>("customers_page", (1,)).await;
    tokio::time::sleep(Duration::from_millis(5)).await;
    fake.clone().get(
        url("/api/customers"),
        200,
        json!({ "data": [{ "id": 2 }], "page": 2 }),
    );
    app.invoke_ok::<Value>("customers_page", (2,)).await;

    go_offline(&fake);
    let newest: Value = app.invoke_ok("customers_page", (2,)).await;
    assert_eq!(newest["page"], 2);
    let err = app.invoke_err("customers_page", (1,)).await;
    assert!(
        err.contains("can't reach the server"),
        "the oldest went: {err}"
    );
}

#[tokio::test]
async fn nothing_is_kept_while_signed_out() {
    let path = db_path();
    let fake = laravel(1);
    let app = app(&path, &fake).await;
    app.invoke_ok::<i64>("customers_count", ()).await;
    assert_eq!(rows(&path).await, 0);
}

#[tokio::test]
#[should_panic(expected = "needs a Database")]
async fn offline_needs_a_database() {
    let fake = laravel(1);
    let _ = TestApp::new(
        App::new()
            .backend(Backend::new(BASE).offline())
            .swap(Http::fake(fake)),
    );
}

#[tokio::test]
async fn broadcasting_connecting_again_means_the_server_is_back() {
    use elyra::broadcasting::PusherFake;
    let path = db_path();
    let fake = laravel(1);
    let pusher = PusherFake::start().await;
    let app = app_with(
        connect(&path).await,
        &fake,
        Backend::new(BASE)
            .offline()
            .broadcasting(pusher.config())
            .follow_public("news", "/api/news"),
        MemoryTokens::new()
            .with(TOKEN_KEY, "7|s3cr3t")
            .with(USER_KEY, "1"),
    );
    pusher.wait_for_subscriptions(1).await;
    app.invoke_ok::<i64>("customers_count", ()).await;

    // The API stops answering — its health route too.
    go_offline(&fake);
    app.invoke_ok::<i64>("customers_count", ()).await;
    assert_eq!(connection(&app).state, Connection::Offline);

    // Only the socket comes back: that's enough to know.
    pusher.disconnect_all();
    pusher.wait_for_subscriptions(2).await;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while connection(&app).state != Connection::Online {
        assert!(tokio::time::Instant::now() < deadline, "still offline");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
