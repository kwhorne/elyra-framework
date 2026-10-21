//! RFC 0010 against a real Laravel app (scripts/laravel-backend.sh) with
//! spatie/laravel-permission and this repository's elyracode/elyra-backend:
//! each user's abilities come from `Route::elyraAbilities()`, a command the
//! user may not run is refused before any request, and a permission given on
//! the web reaches the desktop through `AbilitiesChanged` and Reverb. Skipped
//! unless `ELYRA_LARAVEL_URL` and `ELYRA_PUSHER` say where they are; CI
//! starts them.
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::Arc;
use std::time::Duration;

use elyra::abilities::Abilities;
use elyra::auth::MemoryTokens;
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, Broadcasting, State};
use elyra::http::Http;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Ctx, Database};
use serde_json::{json, Value};

#[command(can = "customers.view")]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    let page: Value = ctx.get::<Backend>().get("/api/customers").json().await?;
    Ok(page["total"].as_i64().unwrap_or(0))
}

#[command(can = "customers.delete")]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .send()
        .await?;
    Ok(())
}

#[command(can = "customers.export")]
async fn customers_export(_ctx: Ctx) -> elyra::Result<bool> {
    Ok(true)
}

async fn call(shell: &TestShell, route: &str, body: Vec<u8>) -> Result<Vec<u8>, String> {
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

fn args(v: Value) -> Vec<u8> {
    rmp_serde::to_vec(&v).unwrap()
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The desktop app, signed in as `email`.
async fn signed_in(url: &str, pusher: &str, email: &str) -> (TestShell, elyra::Ctx) {
    let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
    let (host, port) = address.rsplit_once(':').expect("<host>:<port>");
    let path = std::env::temp_dir().join(format!(
        "elyra-laravel-abilities-{}-{email}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let prepared = App::new()
        .bind(
            Database::connect(&elyra::db::sqlite_url(&path))
                .await
                .unwrap(),
        )
        .backend(
            Backend::new(url)
                .broadcasting(
                    Broadcasting::pusher(key)
                        .host(host)
                        .port(port.parse().unwrap())
                        .scheme("ws"),
                )
                .abilities(),
        )
        .token_store(Arc::new(MemoryTokens::new()))
        .allow_abilities(["customers.*"])
        .commands(commands![
            customers_count,
            customers_destroy,
            customers_export
        ])
        .prepare();
    let ctx = prepared.ctx.clone();
    let shell = TestShell::new(prepared);
    let sign_in =
        rmp_serde::to_vec_named(&json!({ "email": email, "password": "secret" })).unwrap();
    call(&shell, "auth/sign-in", sign_in)
        .await
        .expect("signed in");
    let abilities = ctx.get::<Abilities>();
    eventually("the abilities read", || abilities.loaded()).await;
    (shell, ctx)
}

#[tokio::test]
async fn each_user_may_do_what_laravel_says() {
    let (Ok(url), Ok(pusher)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_PUSHER"),
    ) else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL and ELYRA_PUSHER=<key>@<host>:<port>");
        return;
    };

    // "The web": Ada's own token, straight to Laravel. Bob starts without
    // the export, whatever an earlier run left.
    let http = Http::new();
    let token = http
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": "ada@example.com", "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text()
        .trim()
        .to_owned();
    let permission = |revoke: bool| {
        let (http, url, token) = (http.clone(), url.clone(), token.clone());
        async move {
            let answer = http
                .post(format!("{url}/api/test/permissions"))
                .token(&token)
                .json(&json!({ "email": "bob@example.com", "permission": "customers.export", "revoke": revoke }))
                .send()
                .await
                .unwrap();
            assert!(answer.ok(), "{}", answer.text());
        }
    };
    permission(true).await;

    // Ada may do everything with customers.
    let (ada, ada_ctx) = signed_in(&url, &pusher, "ada@example.com").await;
    let mut listed = ada_ctx.get::<Abilities>().list();
    listed.sort();
    assert_eq!(
        listed,
        [
            "customers.create",
            "customers.delete",
            "customers.update",
            "customers.view"
        ]
    );
    call(&ada, "cmd/customers_count", args(json!([])))
        .await
        .unwrap();

    // Bob may only see them: the delete is refused here, before Laravel
    // would refuse it too.
    let (bob, bob_ctx) = signed_in(&url, &pusher, "bob@example.com").await;
    let bob_abilities = bob_ctx.get::<Abilities>();
    assert_eq!(bob_abilities.list(), ["customers.view"]);
    call(&bob, "cmd/customers_count", args(json!([])))
        .await
        .unwrap();
    assert_eq!(
        call(&bob, "cmd/customers_destroy", args(json!([1]))).await,
        Err("denied".into())
    );
    assert_eq!(
        call(&bob, "cmd/customers_export", args(json!([]))).await,
        Err("denied".into())
    );

    // On the web, Ada gives Bob the export. spatie tells the package, the
    // package broadcasts AbilitiesChanged on Bob's channel, and Bob's desktop
    // reads them again.
    let broadcaster = bob_ctx.get::<Broadcaster>();
    eventually("Bob's desktop listening", || {
        broadcaster.state() == State::Connected
    })
    .await;
    permission(false).await;
    eventually("Bob's desktop heard", || {
        bob_abilities.allows("customers.export")
    })
    .await;
    assert_eq!(
        call(&bob, "cmd/customers_export", args(json!([]))).await,
        Ok(rmp_serde::to_vec(&true).unwrap())
    );
}
