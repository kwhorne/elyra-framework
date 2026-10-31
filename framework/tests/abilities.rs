//! What the signed-in user may do, by Laravel (RFC 0010 step 1): read at
//! sign-in, forgotten at sign-out, kept offline, and each user's own.
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::abilities::Abilities;
use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{App, Database};
use serde_json::json;

const BASE: &str = "https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

fn laravel(user: i64, abilities: serde_json::Value) -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": user, "name": "Ada" }))
        .delete(url("/api/sanctum/token"), 204, serde_json::Value::Null)
        .get(url("/up"), 200, json!("ok"))
        .get(url("/api/abilities"), 200, abilities)
}

async fn db() -> Database {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-abilities-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap()
}

async fn app(backend: Backend, fake: &HttpFake) -> TestApp {
    TestApp::new(
        App::new()
            .bind(db().await)
            .backend(backend)
            .token_store(Arc::new(MemoryTokens::new()))
            .swap(Http::fake(fake.clone())),
    )
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    for _ in 0..200 {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never: {what}");
}

#[tokio::test]
async fn read_at_sign_in_and_forgotten_at_sign_out() {
    let fake = laravel(1, json!(["customers.view", "invoices.*"]));
    let app = app(Backend::new(BASE).abilities(), &fake).await;
    let abilities = app.get::<Abilities>();
    assert!(!abilities.loaded() && !abilities.allows("customers.view"));

    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    eventually("read", || abilities.loaded()).await;
    assert!(abilities.allows("customers.view"));
    assert!(abilities.allows("invoices.delete"), "a namespace");
    assert!(!abilities.allows("customers.delete"));
    let sent = fake.assert_sent("GET", &url("/api/abilities"));
    assert_eq!(sent.header("authorization"), Some("Bearer 7|s3cr3t"));

    app.get::<Auth>().sign_out().await.unwrap();
    assert!(!abilities.loaded());
    assert!(!abilities.allows("customers.view"));
}

#[tokio::test]
async fn another_route_and_laravels_wrapped_shape() {
    let fake = laravel(1, json!([])).get(
        url("/api/me/permissions"),
        200,
        json!({ "data": ["reports.view"] }),
    );
    let app = app(
        Backend::new(BASE).abilities_route("/api/me/permissions"),
        &fake,
    )
    .await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    assert_eq!(abilities.refresh().await.unwrap(), ["reports.view"]);
    assert!(abilities.allows("reports.view"));
    fake.assert_not_sent("GET", &url("/api/abilities"));
}

#[tokio::test]
async fn the_last_list_holds_offline() {
    let fake = laravel(1, json!(["customers.view"]));
    let app = app(Backend::new(BASE).abilities().offline(), &fake).await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    abilities.refresh().await.unwrap();

    for method in [Method::Get, Method::Post] {
        fake.clone().unreachable(method, url("/*"));
    }
    assert_eq!(abilities.refresh().await.unwrap(), ["customers.view"]);
    assert!(abilities.allows("customers.view"));
}

#[tokio::test]
async fn without_abilities_nothing_changes() {
    let fake = laravel(1, json!(["customers.view"]));
    let app = app(Backend::new(BASE), &fake).await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    assert!(app.ctx().try_get::<Abilities>().is_none());
    tokio::time::sleep(Duration::from_millis(50)).await;
    fake.assert_not_sent("GET", &url("/api/abilities"));
}

#[tokio::test]
async fn a_list_that_isnt_one_is_an_error_and_changes_nothing() {
    let fake = laravel(1, json!({ "message": "nope" }));
    let app = app(Backend::new(BASE).abilities(), &fake).await;
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    let err = abilities.refresh().await.unwrap_err();
    assert!(err.to_string().contains("a list of abilities"), "{err}");
    assert!(!abilities.loaded());
}

// ---------------------------------------------------------------------------
// The gate (step 2): through the shell, as the frontend calls.
// ---------------------------------------------------------------------------

#[elyra::command(can = "customers.view")]
async fn customers_count(ctx: elyra::Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[elyra::command(can = "customers.delete")]
async fn customers_destroy(ctx: elyra::Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .queue_offline()
        .send()
        .await?;
    Ok(())
}

/// `/__<route>` as the runtime calls it: the answer, or the error's kind.
async fn call(
    shell: &elyra::testing::TestShell,
    route: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, String> {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__{route}"))
        .header("x-elyra-token", shell.token())
        .body(body)
        .unwrap();
    let response = shell.handle(request).await;
    if response
        .headers()
        .get("x-elyra-status")
        .map(|v| v.as_bytes())
        == Some(b"error")
    {
        return Err(response
            .headers()
            .get("x-elyra-error-kind")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned());
    }
    Ok(response.body().to_vec())
}

#[tokio::test]
async fn a_command_runs_only_when_the_user_may() {
    let fake = laravel(1, json!(["customers.view"]))
        .get(url("/api/customers/count"), 200, json!(3))
        .delete(url("/api/customers/1"), 204, serde_json::Value::Null);
    let prepared = App::new()
        .bind(db().await)
        .backend(Backend::new(BASE).abilities().offline())
        .token_store(Arc::new(MemoryTokens::new()))
        .allow_abilities(["customers.view", "customers.delete"])
        .commands(elyra::commands![customers_count, customers_destroy])
        .swap(Http::fake(fake.clone()))
        .prepare();
    let ctx = prepared.ctx.clone();
    let shell = elyra::testing::TestShell::new(prepared);
    let args = |v: serde_json::Value| rmp_serde::to_vec(&v).unwrap();

    // Signed out: the user may do nothing.
    assert_eq!(
        call(&shell, "cmd/customers_count", args(json!([]))).await,
        Err("denied".into())
    );

    let sign_in =
        rmp_serde::to_vec_named(&json!({ "email": "ada@example.com", "password": "secret" }))
            .unwrap();
    call(&shell, "auth/sign-in", sign_in).await.unwrap();
    let abilities = ctx.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;

    let count = call(&shell, "cmd/customers_count", args(json!([])))
        .await
        .unwrap();
    assert_eq!(rmp_serde::from_slice::<i64>(&count).unwrap(), 3);
    // The app grants it, Laravel says this user may not: refused here,
    // without a request.
    assert_eq!(
        call(&shell, "cmd/customers_destroy", args(json!([1]))).await,
        Err("denied".into())
    );
    fake.assert_not_sent("DELETE", &url("/api/customers/1"));

    // Offline, it isn't queued to fail tomorrow either.
    for method in [Method::Get, Method::Delete] {
        fake.clone().unreachable(method, url("/api/customers/*"));
    }
    assert_eq!(
        call(&shell, "cmd/customers_destroy", args(json!([1]))).await,
        Err("denied".into())
    );
    assert!(ctx.get::<elyra::offline::Outbox>().items().await.is_empty());
}

#[tokio::test]
async fn an_mcp_agent_has_the_apps_grant_not_the_users() {
    let fake = laravel(1, json!(["customers.view"])).delete(
        url("/api/customers/1"),
        204,
        serde_json::Value::Null,
    );
    let app = TestApp::new(
        App::new()
            .bind(db().await)
            .backend(Backend::new(BASE).abilities())
            .token_store(Arc::new(MemoryTokens::new()))
            .commands(elyra::commands![customers_count, customers_destroy])
            .mcp(elyra::Mcp::new().allow_ability("customers.delete"))
            .swap(Http::fake(fake.clone())),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;
    assert!(!abilities.allows("customers.delete"));

    // RFC 0010: an external agent's grants are the app's.
    let result = app
        .mcp()
        .call("customers_destroy", json!({ "id": 1 }))
        .await;
    assert!(!result.is_error, "{}", result.text);
    fake.assert_sent("DELETE", &url("/api/customers/1"));
}

// ---------------------------------------------------------------------------
// Keeping it current (step 3).
// ---------------------------------------------------------------------------

#[elyra::command(live)]
async fn may_export(ctx: elyra::Ctx) -> elyra::Result<bool> {
    Ok(ctx.get::<Abilities>().allows("customers.export"))
}

#[tokio::test]
async fn laravel_says_they_changed_and_the_open_screens_follow() {
    use elyra::broadcasting::PusherFake;
    let pusher = PusherFake::start().await;
    let fake = laravel(1, json!(["customers.view"])).post(
        url("/api/broadcasting/auth"),
        200,
        json!({ "auth": "key:signature" }),
    );
    let app = TestApp::new(
        App::new()
            .bind(db().await)
            .backend(Backend::new(BASE).broadcasting(pusher.config()).abilities())
            .token_store(Arc::new(MemoryTokens::new()))
            .live_batch_window(Duration::from_millis(5))
            .commands(elyra::commands![may_export])
            .swap(Http::fake(fake.clone())),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;
    // The user's own channel, followed for the abilities alone.
    pusher.wait_for_subscriptions(1).await;
    let mut may = app.live::<bool>("may_export", ()).await;
    assert!(!*may.value());

    // An admin gives Ada the export on the web.
    fake.clone().get(
        url("/api/abilities"),
        200,
        json!(["customers.view", "customers.export"]),
    );
    pusher.broadcast(
        "private-App.Models.User.1",
        "App\\Events\\AbilitiesChanged",
        json!({}),
    );
    eventually("read again", || abilities.allows("customers.export")).await;
    may.next().await;
    assert!(*may.value(), "the live query re-ran");
}

#[elyra::command(live)]
async fn customers_total(ctx: elyra::Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[tokio::test]
async fn read_again_when_the_connection_comes_back() {
    let fake =
        laravel(1, json!(["customers.view"])).get(url("/api/customers/count"), 200, json!(3));
    let app = TestApp::new(
        App::new()
            .bind(db().await)
            .backend(Backend::new(BASE).abilities().offline())
            .token_store(Arc::new(MemoryTokens::new()))
            .live_batch_window(Duration::from_millis(5))
            .commands(elyra::commands![customers_total])
            .swap(Http::fake(fake.clone())),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;
    let mut total = app.live::<i64>("customers_total", ()).await;
    assert_eq!(*total.value(), 3);

    fake.clone().unreachable(Method::Get, url("/*"));
    abilities.refresh().await.unwrap();
    assert_eq!(
        app.get::<Backend>().connection().unwrap().state,
        elyra::offline::Connection::Offline
    );

    // Changed meanwhile; then the server answers again.
    fake.clone()
        .get(url("/up"), 200, json!("ok"))
        .get(url("/api/user"), 200, json!({ "id": 1 }))
        .get(url("/api/customers/count"), 200, json!(4))
        .get(
            url("/api/abilities"),
            200,
            json!(["customers.view", "customers.delete"]),
        );
    eventually("read again once back", || {
        abilities.allows("customers.delete")
    })
    .await;
    // …and what was read re-runs too: both hooks run.
    total.next().await;
    assert_eq!(*total.value(), 4);
}

#[tokio::test]
async fn the_frontend_reads_them_through_its_route() {
    let fake = laravel(1, json!(["customers.view"]));
    let prepared = App::new()
        .bind(db().await)
        .backend(Backend::new(BASE).abilities())
        .token_store(Arc::new(MemoryTokens::new()))
        .swap(Http::fake(fake.clone()))
        .prepare();
    let shell = elyra::testing::TestShell::new(prepared);
    let state = |bytes: Vec<u8>| rmp_serde::from_slice::<serde_json::Value>(&bytes).unwrap();
    let null = || rmp_serde::to_vec_named(&serde_json::Value::Null).unwrap();

    let before = state(call(&shell, "auth/abilities", null()).await.unwrap());
    assert_eq!(before, json!({ "loaded": false, "abilities": [] }));
    let sign_in =
        rmp_serde::to_vec_named(&json!({ "email": "ada@example.com", "password": "secret" }))
            .unwrap();
    call(&shell, "auth/sign-in", sign_in).await.unwrap();
    let after = state(
        call(&shell, "auth/abilities-refresh", null())
            .await
            .unwrap(),
    );
    assert_eq!(
        after,
        json!({ "loaded": true, "abilities": ["customers.view"] })
    );

    // Without `.abilities()`, the app's own grants are all there is.
    let plain = elyra::testing::TestShell::new(App::new().prepare());
    let open = state(call(&plain, "auth/abilities", null()).await.unwrap());
    assert_eq!(open, json!({ "loaded": true, "abilities": ["*"] }));
}

// ---------------------------------------------------------------------------
// The app's assistant (RFC 0011): held to the user's list.
// ---------------------------------------------------------------------------

#[cfg(feature = "ai")]
#[tokio::test]
async fn the_assistant_has_no_more_than_the_user() {
    use elyra::ai::{Ai, AiFake};
    use elyra::assistant::Assistant;

    let fake = AiFake::new()
        .tool_calls([
            ("customers_count", json!({})),
            ("customers_destroy", json!({ "id": 1 })),
        ])
        .text("You have 3 customers; I can't delete one.")
        .text("Still nothing.");
    let http = laravel(1, json!(["customers.view"]))
        .get(url("/api/customers/count"), 200, json!(3))
        .delete(url("/api/customers/1"), 204, serde_json::Value::Null);
    let app = TestApp::new(
        App::new()
            .bind(db().await)
            .backend(Backend::new(BASE).abilities())
            .token_store(Arc::new(MemoryTokens::new()))
            .commands(elyra::commands![customers_count, customers_destroy])
            .swap(Http::fake(http.clone()))
            .swap(Ai::fake(fake.clone()))
            .assistant(
                Assistant::new()
                    .allow_abilities(["customers.*"])
                    .confirm("customers.delete"),
            ),
    );
    let assistant = app.assistant().accepting();

    // Signed out: nothing is offered, and nothing would run.
    assert!(assistant.tools().is_empty());

    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;
    assert_eq!(assistant.tools(), ["customers_count"]);

    let turn = assistant.prompt("How many, and delete customer 1").await;
    assert!(turn.answer.is_ok(), "{:?}", turn.answer);
    assert_eq!(fake.requests()[0].tools, ["customers_count"]);
    let outputs: Vec<String> = fake.calls().into_iter().map(|c| c.output).collect();
    assert_eq!(
        outputs,
        [
            "3",
            // Never offered, so not a tool at all.
            "error: unknown tool `customers_destroy`",
        ]
    );
    assert!(turn.asked().is_empty());
    http.assert_not_sent("DELETE", &url("/api/customers/1"));
    // The system prompt names the user.
    assert!(fake.requests()[0].system.contains("You are helping Ada."));
}

/// Ends the session mid-turn, as a sign-out elsewhere would.
#[cfg(feature = "ai")]
#[elyra::command(can = "session.end")]
async fn session_end(ctx: elyra::Ctx) -> elyra::Result<()> {
    ctx.get::<Auth>().sign_out().await?;
    Ok(())
}

#[cfg(feature = "ai")]
#[tokio::test]
async fn abilities_lost_mid_turn_stop_the_next_call_before_the_question() {
    use elyra::ai::{Ai, AiFake};
    use elyra::assistant::Assistant;

    let fake = AiFake::new()
        .tool_calls([("session_end", json!({})), ("customers_count", json!({}))])
        .text("You're signed out.");
    let http = laravel(1, json!(["customers.view", "session.end"])).get(
        url("/api/customers/count"),
        200,
        json!(3),
    );
    let app = TestApp::new(
        App::new()
            .bind(db().await)
            .backend(Backend::new(BASE).abilities())
            .token_store(Arc::new(MemoryTokens::new()))
            .commands(elyra::commands![customers_count, session_end])
            .swap(Http::fake(http.clone()))
            .swap(Ai::fake(fake.clone()))
            .assistant(
                Assistant::new()
                    .allow_abilities(["customers.*", "session.*"])
                    .confirm("customers.view"),
            ),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<Abilities>();
    eventually("read", || abilities.loaded()).await;

    let turn = app.assistant().accepting().prompt("…").await;
    let outputs: Vec<String> = fake.calls().into_iter().map(|c| c.output).collect();
    assert_eq!(
        outputs,
        [
            "null",
            "The user may not do this: it takes the `customers.view` ability.",
        ]
    );
    // Not asked about a call that couldn't run, and it didn't.
    assert!(turn.asked().is_empty(), "{:?}", turn.events);
    http.assert_not_sent("GET", &url("/api/customers/count"));
}

/// The inspector's mask (RFC 0014) reaches what Laravel said: the app's
/// `Abilities` checks it, and the state the `can` store reads carries it.
#[cfg(debug_assertions)]
#[tokio::test]
async fn the_inspectors_mask_reaches_the_users_abilities() {
    let fake = laravel(1, json!(["customers.*"]));
    let app = app(Backend::new(BASE).abilities(), &fake).await;
    let abilities = app.get::<Abilities>();
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    eventually("read", || abilities.loaded()).await;
    assert!(abilities.allows("customers.delete"));
    app.get::<elyra::inspector::Mask>()
        .set("customers.delete", true);
    assert!(!abilities.allows("customers.delete"));
    assert_eq!(abilities.state().masked, ["customers.delete"]);
}
