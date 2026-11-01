//! The inspector's offline switch: unplugged, the backend answers as a
//! network with no server would — reads come from the copy, writes wait in
//! the outbox, broadcasting closes, the health route isn't asked — and
//! plugged back in, it all comes back without waiting for the probe. Debug
//! builds only, as the inspector is.
#![cfg(all(debug_assertions, feature = "backend", feature = "database"))]

use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, PusherFake, State};
use elyra::http::{Http, HttpFake, Method};
use elyra::offline::Connection;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Ctx, Database};
use serde::Deserialize;
use serde_json::{json, Value};
use wry::http::{Request, Response};

const BASE: &str = "https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

#[command]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[command]
async fn customers_store(ctx: Ctx, name: String) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&json!({ "name": name }))
        .queue_offline()
        .json()
        .await?)
}

fn laravel() -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 1, "name": "Ada" }))
        .get(url("/up"), 200, json!("ok"))
        .get(url("/api/customers/count"), 200, json!(3))
        .post(url("/api/customers"), 201, json!({ "id": 9 }))
        .post(
            url("/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:signed-by-laravel" }),
        )
}

async fn db() -> Database {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!(
        "elyra-inspector-offline-{}-{n}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap()
}

/// The shell (for the inspector's routes and the commands) and the app's
/// context (for its state).
fn shell(app: App, fake: &HttpFake) -> (TestShell, Ctx) {
    let prepared = app
        .token_store(Arc::new(MemoryTokens::new()))
        .commands(commands![customers_count, customers_store])
        .swap(Http::fake(fake.clone()))
        .prepare();
    let ctx = prepared.ctx.clone();
    (TestShell::new(prepared), ctx)
}

/// …with the bus, window `w1` listening on it.
async fn signed_in_with_bus(
    backend: Backend,
    fake: &HttpFake,
) -> (TestShell, Ctx, elyra::event::EventBus) {
    let prepared = App::new()
        .bind(db().await)
        .backend(backend)
        .token_store(Arc::new(MemoryTokens::new()))
        .commands(commands![customers_count, customers_store])
        .swap(Http::fake(fake.clone()))
        .prepare();
    let (ctx, bus) = (prepared.ctx.clone(), prepared.bus.clone());
    bus.register_client("w1");
    let shell = TestShell::new(prepared);
    ctx.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    (shell, ctx, bus)
}

async fn signed_in(backend: Backend, fake: &HttpFake) -> (TestShell, Ctx) {
    let (shell, ctx) = shell(App::new().bind(db().await).backend(backend), fake);
    ctx.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    (shell, ctx)
}

async fn post(shell: &TestShell, path: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "w1")
        .body(body)
        .unwrap();
    shell.handle(request).await
}

#[derive(Deserialize, Debug, PartialEq)]
struct Switch {
    backend: bool,
    on: bool,
    copy: bool,
}

async fn unplug(shell: &TestShell, on: Option<bool>) -> Switch {
    #[derive(serde::Serialize)]
    struct Unplug {
        on: Option<bool>,
    }
    let res = post(
        shell,
        "/__inspector/offline",
        rmp_serde::to_vec_named(&Unplug { on }).unwrap(),
    )
    .await;
    rmp_serde::from_slice(res.body()).unwrap_or_else(|e| {
        panic!("{e}: {}", String::from_utf8_lossy(res.body()));
    })
}

async fn call(shell: &TestShell, command: &str, args: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    post(shell, &format!("/__cmd/{command}"), args).await
}

fn kind(res: &Response<Cow<'static, [u8]>>) -> Option<String> {
    res.headers()
        .get("x-elyra-error-kind")
        .map(|v| v.to_str().unwrap().to_owned())
}

async fn count(shell: &TestShell) -> Response<Cow<'static, [u8]>> {
    call(shell, "customers_count", rmp_serde::to_vec(&()).unwrap()).await
}

fn connection(ctx: &Ctx) -> elyra::offline::ConnectionState {
    ctx.get::<Backend>().connection().expect("offline is on")
}

/// The `elyra:connection` events window `w1` has been sent since the last call.
async fn connection_events(bus: &elyra::event::EventBus) -> Vec<Value> {
    let mut seen = Vec::new();
    while bus.pending_for("w1") > 0 {
        let batch: Vec<(String, Value)> =
            rmp_serde::from_slice(&bus.next_batch_for("w1").await).unwrap();
        seen.extend(
            batch
                .into_iter()
                .filter(|(channel, _)| channel == "elyra:connection")
                .map(|(_, v)| v),
        );
    }
    seen
}

/// Wait, a little at a time, until `done` — up to 2 s.
async fn until(what: &str, done: impl Fn() -> bool) {
    within(Duration::from_secs(2), what, done).await;
}

async fn within(limit: Duration, what: &str, done: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + limit;
    while !done() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "not within {limit:?}: {what}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn unplugged_reads_come_from_the_copy_and_nothing_reaches_the_server() {
    let fake = laravel();
    let (shell, ctx) = signed_in(Backend::new(BASE).offline(), &fake).await;
    assert_eq!(
        unplug(&shell, None).await,
        Switch {
            backend: true,
            on: false,
            copy: true
        }
    );
    let fresh = count(&shell).await;
    assert_eq!(rmp_serde::from_slice::<i64>(fresh.body()).unwrap(), 3);
    assert_eq!(connection(&ctx).state, Connection::Online);

    let switch = unplug(&shell, Some(true)).await;
    assert!(switch.on);
    // Offline at once, marked as simulated — not at the next request.
    let state = connection(&ctx);
    assert_eq!(state.state, Connection::Offline);
    assert!(state.simulated);

    let asked = fake.sent().len();
    let kept = count(&shell).await;
    assert_eq!(rmp_serde::from_slice::<i64>(kept.body()).unwrap(), 3);
    // The probe's first turn is a second away: past it, the health route
    // still wasn't asked — nothing was.
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert_eq!(fake.sent().len(), asked, "{:?}", fake.sent());
    assert_eq!(connection(&ctx).state, Connection::Offline);

    // Plugged back in: online without waiting for the probe's next turn,
    // which is more than a second and a half away.
    assert!(!unplug(&shell, Some(false)).await.on);
    within(Duration::from_millis(500), "back online", || {
        connection(&ctx).state == Connection::Online
    })
    .await;
    assert!(!connection(&ctx).simulated);
    fake.assert_sent("GET", &url("/up"));
}

#[tokio::test]
async fn an_unplugged_write_waits_in_the_outbox_and_is_sent_when_plugged_back_in() {
    let fake = laravel();
    let (shell, ctx) = signed_in(Backend::new(BASE).offline(), &fake).await;
    unplug(&shell, Some(true)).await;
    let res = call(
        &shell,
        "customers_store",
        rmp_serde::to_vec(&("Acme".to_string(),)).unwrap(),
    )
    .await;
    assert_eq!(kind(&res).as_deref(), Some("queued"));
    assert_eq!(connection(&ctx).pending, 1);
    assert!(fake.sent_to("POST", &url("/api/customers")).is_empty());

    unplug(&shell, Some(false)).await;
    until("the outbox sent", || connection(&ctx).pending == 0).await;
    let sent = fake.assert_sent("POST", &url("/api/customers"));
    assert_eq!(sent.body.as_ref().unwrap()["name"], "Acme");
    assert!(sent.header("idempotency-key").is_some());
}

#[tokio::test]
async fn plugged_back_in_while_the_server_is_really_gone_it_stays_offline() {
    let fake = laravel();
    let (shell, ctx, bus) = signed_in_with_bus(Backend::new(BASE).offline(), &fake).await;
    unplug(&shell, Some(true)).await;
    // Meanwhile the network goes for real.
    fake.clone()
        .unreachable(Method::Get, url("/*"))
        .unreachable(Method::Post, url("/*"));
    unplug(&shell, Some(false)).await;
    until("the health route asked", || {
        !fake.sent_to("GET", &url("/up")).is_empty()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let state = connection(&ctx);
    assert_eq!(state.state, Connection::Offline);
    assert!(!state.simulated, "offline for real now");
    // …and the window is told so: the status bar stops saying simulated.
    let events = connection_events(&bus).await;
    let last = events.last().expect("a connection event");
    assert_eq!(last["state"], "offline");
    assert_eq!(last["simulated"], false, "{events:?}");
}

#[tokio::test]
async fn unplugged_while_offline_for_real_the_window_hears_its_simulated() {
    let fake = laravel();
    let (shell, ctx, bus) = signed_in_with_bus(Backend::new(BASE).offline(), &fake).await;
    count(&shell).await;
    fake.clone().unreachable(Method::Get, url("/*"));
    count(&shell).await;
    assert_eq!(connection(&ctx).state, Connection::Offline);
    let events = connection_events(&bus).await;
    assert_eq!(events.last().unwrap()["simulated"], false);

    unplug(&shell, Some(true)).await;
    let events = connection_events(&bus).await;
    assert_eq!(
        events.last().map(|e| e["simulated"].clone()),
        Some(Value::Bool(true)),
        "{events:?}"
    );
}

#[tokio::test]
async fn without_the_offline_copy_an_unplugged_call_is_an_offline_error() {
    let fake = laravel();
    let (shell, _ctx) = signed_in(Backend::new(BASE), &fake).await;
    let switch = unplug(&shell, Some(true)).await;
    assert_eq!(
        switch,
        Switch {
            backend: true,
            on: true,
            copy: false
        }
    );
    let res = count(&shell).await;
    assert_eq!(kind(&res).as_deref(), Some("offline"));
    assert!(
        String::from_utf8_lossy(res.body()).contains("simulated by the inspector"),
        "{}",
        String::from_utf8_lossy(res.body())
    );
    assert!(fake.sent_to("GET", &url("/api/customers/count")).is_empty());

    unplug(&shell, Some(false)).await;
    let res = count(&shell).await;
    assert_eq!(rmp_serde::from_slice::<i64>(res.body()).unwrap(), 3);
}

#[tokio::test]
async fn without_a_backend_there_is_nothing_to_unplug() {
    let (shell, _ctx) = shell(App::new(), &laravel());
    assert_eq!(
        unplug(&shell, Some(true)).await,
        Switch {
            backend: false,
            on: false,
            copy: false
        }
    );
}

#[tokio::test]
async fn unplugged_broadcasting_closes_and_opens_again_after() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers");
    let (shell, ctx) = signed_in(backend, &fake).await;
    pusher.wait_for_subscriptions(1).await;
    let broadcaster = ctx.get::<Broadcaster>();
    until("connected", || broadcaster.state() == State::Connected).await;

    unplug(&shell, Some(true)).await;
    assert_eq!(broadcaster.state(), State::Offline);
    pusher
        .wait_until(|p| p.connected() == 0, "the connection closed")
        .await;
    // A sign-in meanwhile doesn't open it either.
    let connections = pusher.connections();
    broadcaster.start();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(broadcaster.state(), State::Offline);
    assert_eq!(
        pusher.connections(),
        connections,
        "no connection while unplugged"
    );

    unplug(&shell, Some(false)).await;
    pusher.wait_for_subscriptions(2).await;
    until("connected again", || {
        broadcaster.state() == State::Connected
    })
    .await;
}

#[tokio::test]
async fn a_sign_in_while_unplugged_connects_once_plugged_back_in() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers");
    let (shell, ctx) = signed_in(backend, &fake).await;
    pusher.wait_for_subscriptions(1).await;
    let broadcaster = ctx.get::<Broadcaster>();
    // Signed out, then unplugged: nothing to open again…
    broadcaster.stop();
    unplug(&shell, Some(true)).await;
    // …until a sign-in meanwhile asks for it.
    broadcaster.start();
    let connections = pusher.connections();
    unplug(&shell, Some(false)).await;
    pusher
        .wait_until(|p| p.connections() > connections, "a new connection")
        .await;
    pusher.wait_for_subscriptions(2).await;
}

#[tokio::test]
async fn a_signed_out_app_isnt_connected_by_plugging_back_in() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers");
    let (shell, ctx) = signed_in(backend, &fake).await;
    pusher.wait_for_subscriptions(1).await;
    let broadcaster = ctx.get::<Broadcaster>();
    unplug(&shell, Some(true)).await;
    broadcaster.stop();
    let connections = pusher.connections();
    unplug(&shell, Some(false)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(broadcaster.state(), State::Offline);
    assert_eq!(pusher.connections(), connections, "no new connection");
}

#[cfg(feature = "ai")]
#[tokio::test]
async fn an_unplugged_assistant_says_it_cant_reach_the_server() {
    use elyra::assistant::{AssistantEvent, CHANNEL};
    let fake = laravel();
    let prepared = App::new()
        .bind(db().await)
        .backend(Backend::new(BASE).assistant())
        .assistant(elyra::assistant::Assistant::new())
        .token_store(Arc::new(MemoryTokens::new()))
        .swap(Http::fake(fake.clone()))
        .prepare();
    let (ctx, bus) = (prepared.ctx.clone(), prepared.bus.clone());
    let shell = TestShell::new(prepared);
    ctx.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    bus.register_client("w1");
    unplug(&shell, Some(true)).await;
    let res = post(
        &shell,
        "/__assistant/prompt",
        rmp_serde::to_vec("How many customers?").unwrap(),
    )
    .await;
    assert!(
        kind(&res).is_none(),
        "{}",
        String::from_utf8_lossy(res.body())
    );
    let error = async {
        loop {
            let batch: Vec<(String, rmpv::Value)> =
                rmp_serde::from_slice(&bus.next_batch_for("w1").await).unwrap();
            for (channel, value) in batch {
                if channel != CHANNEL {
                    continue;
                }
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).unwrap();
                if let Ok(AssistantEvent::Error { message }) = rmp_serde::from_slice(&buf) {
                    return message;
                }
            }
        }
    };
    let message = tokio::time::timeout(Duration::from_secs(5), error)
        .await
        .expect("the assistant's error");
    assert_eq!(
        message,
        "The assistant can't reach the server (offline, simulated by the inspector)."
    );
}
