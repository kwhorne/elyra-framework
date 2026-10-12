//! RFC 0006 against a real Laravel app and a real Pusher-protocol server —
//! Reverb or Askr (`--pusher`): a customer created on the web reaches the
//! desktop app's open list. Skipped unless `ELYRA_LARAVEL_URL` and
//! `ELYRA_PUSHER` (`<key>@<host>:<port>`) say where they are; CI starts them
//! (scripts/laravel-backend.sh).
#![cfg(all(feature = "backend", feature = "database"))]

use std::sync::Mutex;
use std::time::Duration;

use elyra::auth::Auth;
use elyra::backend::Backend;
use elyra::broadcasting::{BackendEvent, Broadcaster, Broadcasting, State};
use elyra::http::Http;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

static HEARD: Mutex<Vec<BackendEvent>> = Mutex::new(Vec::new());

/// How many customers there are, live.
#[command(live)]
async fn customers_total(ctx: Ctx) -> elyra::Result<i64> {
    let page: Value = ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&json!({ "per_page": 1 }))
        .json()
        .await?;
    Ok(page["total"].as_i64().unwrap_or(-1))
}

#[tokio::test]
async fn a_customer_created_on_the_web_reaches_the_open_list() {
    let (Ok(url), Ok(pusher)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_PUSHER"),
    ) else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL and ELYRA_PUSHER=<key>@<host>:<port>");
        return;
    };
    let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
    let (host, port) = address.rsplit_once(':').expect("<host>:<port>");
    let app = TestApp::new(
        App::new()
            .backend(
                Backend::new(&url)
                    .broadcasting(
                        Broadcasting::pusher(key)
                            .host(host)
                            .port(port.parse().expect("a port"))
                            .scheme("ws"),
                    )
                    .follow("customers", "/api/customers"),
            )
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![customers_total])
            .listen(|e: BackendEvent, _ctx: Ctx| async move {
                HEARD.lock().unwrap().push(e);
                Ok(())
            }),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");
    let broadcaster = app.get::<Broadcaster>();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while broadcaster.state() != State::Connected {
        assert!(tokio::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut total = app.live::<i64>("customers_total", ()).await;
    let before = *total.value();

    // Ada, on the web: her own token, and no socket id — another client.
    let http = Http::new();
    let token = http
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": "ada@example.com", "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let created: Value = http
        .post(format!("{url}/api/customers"))
        .token(token.trim())
        .json(
            &json!({ "name": format!("Web {stamp}"), "email": format!("web{stamp}@example.com") }),
        )
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();

    // Laravel broadcast it; the open list followed.
    total.next().await;
    assert_eq!(*total.value(), before + 1);
    let heard = HEARD.lock().unwrap().clone();
    assert!(
        heard.iter().any(|e| e.channel == "customers"
            && e.event == "CustomerCreated"
            && e.data["model"]["id"] == created["id"]),
        "{heard:?}"
    );

    // Tidy up, and sign out: the connection closes.
    http.delete(format!("{url}/api/customers/{}", created["id"]))
        .token(token.trim())
        .send()
        .await
        .unwrap();
    app.get::<Auth>().sign_out().await.unwrap();
    assert_eq!(broadcaster.state(), State::Offline);
}
