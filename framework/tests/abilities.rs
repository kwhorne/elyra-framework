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
