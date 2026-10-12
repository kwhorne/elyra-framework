//! RFC 0007 against a real Laravel app (scripts/laravel-backend.sh): the app
//! reaches it through a proxy this test can pull the plug on. A write made
//! with no network is sent once it's back — once, with its key; a repeated
//! key is answered from Laravel's record; a stale update is a `412`, and
//! "keep mine" sends it anyway. Skipped unless `ELYRA_LARAVEL_URL` says where
//! the app is; CI starts one.
#![cfg(all(feature = "backend", feature = "database"))]

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::Auth;
use elyra::backend::Backend;
use elyra::http::Http;
use elyra::offline::Outbox;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};

/// A TCP proxy to Laravel whose plug can be pulled: open connections are cut,
/// and new ones closed at once — as when the Wi-Fi goes.
struct Plug {
    addr: SocketAddr,
    up: Arc<AtomicBool>,
    open: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Plug {
    async fn to(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let up = Arc::new(AtomicBool::new(true));
        let open = Arc::new(Mutex::new(Vec::new()));
        let (accepting, tasks) = (up.clone(), open.clone());
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                if !accepting.load(Ordering::SeqCst) {
                    continue; // dropped: closed at once
                }
                tasks.lock().unwrap().push(tokio::spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                }));
            }
        });
        Plug { addr, up, open }
    }

    fn pull(&self) {
        self.up.store(false, Ordering::SeqCst);
        for task in self.open.lock().unwrap().drain(..) {
            task.abort();
        }
    }

    fn push_in(&self) {
        self.up.store(true, Ordering::SeqCst);
    }
}

#[command]
async fn customers_store(ctx: Ctx, name: String, email: String) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&json!({ "name": name, "email": email }))
        .queue_offline()
        .resource()
        .await?)
}

#[command]
async fn customers_rename(ctx: Ctx, id: i64, name: String, seen: String) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&json!({ "name": name }))
        .unless_changed_since(seen)
        .queue_offline()
        .json()
        .await?)
}

async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn working_offline_against_a_real_laravel_app() {
    let Ok(url) = std::env::var("ELYRA_LARAVEL_URL") else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
        return;
    };
    let target: SocketAddr = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .parse()
        .expect("ELYRA_LARAVEL_URL as http://<ip>:<port>");
    let plug = Plug::to(target).await;
    let path =
        std::env::temp_dir().join(format!("elyra-laravel-offline-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let app = TestApp::new(
        App::new()
            .bind(
                Database::connect(&elyra::db::sqlite_url(&path))
                    .await
                    .unwrap(),
            )
            .backend(Backend::new(format!("http://{}", plug.addr)).offline())
            .commands(commands![customers_store, customers_rename]),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");

    // "The web": Ada's own token, straight to Laravel.
    let http = Http::new();
    let token = http
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": "ada@example.com", "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text();
    let token = token.trim().to_owned();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let how_many = |name: String| {
        let (http, url, token) = (http.clone(), url.clone(), token.clone());
        async move {
            let page: Value = http
                .get(format!("{url}/api/customers"))
                .query(&json!({ "search": name }))
                .token(&token)
                .send()
                .await
                .unwrap()
                .json()
                .unwrap();
            page["total"].as_i64().unwrap()
        }
    };

    // 1. Laravel answers a repeated key from its record: one customer.
    let key = format!("elyra-test-{stamp}");
    let twice = format!("Twice {stamp}");
    let mut ids = Vec::new();
    for _ in 0..2 {
        let created: Value = http
            .post(format!("{url}/api/customers"))
            .token(&token)
            .header("Idempotency-Key", key.clone())
            .json(&json!({ "name": twice, "email": format!("twice{stamp}@example.com") }))
            .send()
            .await
            .unwrap()
            .json()
            .unwrap();
        ids.push(created["id"].clone());
    }
    assert_eq!(ids[0], ids[1], "the same answer");
    assert_eq!(how_many(twice.clone()).await, 1);

    // 2. No network: the create waits in the outbox, and is made once back.
    plug.pull();
    let offline = format!("Offline {stamp}");
    assert_eq!(
        app.invoke_err_kind(
            "customers_store",
            (offline.clone(), format!("offline{stamp}@example.com"))
        )
        .await
        .as_deref(),
        Some("queued")
    );
    assert_eq!(how_many(offline.clone()).await, 0, "not yet");
    plug.push_in();
    let outbox = app.get::<Outbox>();
    eventually("the outbox sent", async || outbox.items().await.is_empty()).await;
    assert_eq!(how_many(offline.clone()).await, 1, "made once");

    // 3. A stale update: Bob renamed the customer meanwhile.
    let mine: Value = app
        .invoke_ok(
            "customers_store",
            (format!("Mine {stamp}"), format!("mine{stamp}@example.com")),
        )
        .await;
    let id = mine["id"].as_i64().unwrap();
    let seen = mine["updated_at"].as_str().unwrap().to_owned();
    plug.pull();
    assert_eq!(
        app.invoke_err_kind("customers_rename", (id, format!("Ada's {stamp}"), seen))
            .await
            .as_deref(),
        Some("queued")
    );
    tokio::time::sleep(Duration::from_millis(1100)).await; // Laravel's seconds
    http.put(format!("{url}/api/customers/{id}"))
        .token(&token)
        .json(&json!({ "name": format!("Bob's {stamp}") }))
        .send()
        .await
        .unwrap();
    plug.push_in();
    eventually("refused as a conflict", async || {
        outbox
            .failed()
            .await
            .first()
            .is_some_and(|w| w.reason.as_deref() == Some("conflict"))
    })
    .await;
    // Keep mine: sent again without the precondition.
    let refused = outbox.failed().await[0].id;
    outbox.keep_mine(refused).await.unwrap();
    eventually("mine sent", async || outbox.items().await.is_empty()).await;
    let now: Value = http
        .get(format!("{url}/api/customers/{id}"))
        .token(&token)
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();
    assert_eq!(now["name"], format!("Ada's {stamp}"));

    // Tidy up, and sign out: nothing waits.
    for name in [twice, offline, format!("Ada's {stamp}")] {
        let page: Value = http
            .get(format!("{url}/api/customers"))
            .query(&json!({ "search": name }))
            .token(&token)
            .send()
            .await
            .unwrap()
            .json()
            .unwrap();
        for customer in page["data"].as_array().unwrap() {
            http.delete(format!("{url}/api/customers/{}", customer["id"]))
                .token(&token)
                .send()
                .await
                .unwrap();
        }
    }
    app.get::<Auth>().sign_out().await.unwrap();
    let _ = std::fs::remove_file(&path);
}
