//! RFC 0006 against a real Laravel app and a real Pusher-protocol server —
//! Reverb or Askr (`--pusher`): a customer created on the web reaches the
//! desktop app's open list; a customer edited on the web re-runs the view of
//! that customer, not another's (RFC 0017). Skipped unless `ELYRA_LARAVEL_URL` and
//! `ELYRA_PUSHER` (`<key>@<host>:<port>`) say where they are; CI starts them
//! (scripts/laravel-backend.sh).
#![cfg(all(feature = "backend", feature = "database"))]

use std::collections::HashMap;
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

/// How often each customer's view ran.
static SHOWN: Mutex<Option<HashMap<i64, usize>>> = Mutex::new(None);

/// One customer, as a detail view shows it.
#[command(live)]
async fn customer_name(ctx: Ctx, id: i64) -> elyra::Result<String> {
    *SHOWN
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .entry(id)
        .or_default() += 1;
    let customer: Value = ctx
        .get::<Backend>()
        .get(format!("/api/customers/{id}"))
        .json()
        .await?;
    Ok(customer["name"].as_str().unwrap_or_default().to_owned())
}

fn shown(id: i64) -> usize {
    SHOWN
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&id).copied())
        .unwrap_or(0)
}

/// Where the Laravel app and its Pusher-protocol server are, if CI says.
fn servers() -> Option<(String, String)> {
    match (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_PUSHER"),
    ) {
        (Ok(url), Ok(pusher)) => Some((url, pusher)),
        _ => {
            eprintln!("skipping: set ELYRA_LARAVEL_URL and ELYRA_PUSHER=<key>@<host>:<port>");
            None
        }
    }
}

/// Ada's token, as the web has one.
async fn web_token(http: &Http, url: &str) -> String {
    http.post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": "ada@example.com", "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text()
        .trim()
        .to_owned()
}

#[tokio::test]
async fn a_customer_edited_on_the_web_re_runs_that_customers_view_only() {
    let Some((url, pusher)) = servers() else {
        return;
    };
    let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
    let (host, port) = address.rsplit_once(':').expect("<host>:<port>");
    let http = Http::new();
    let token = web_token(&http, &url).await;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut ids = Vec::new();
    for n in ["one", "two"] {
        let created: Value = http
            .post(format!("{url}/api/customers"))
            .token(&token)
            .json(&json!({ "name": format!("{n} {stamp}"), "email": format!("{n}{stamp}@example.com") }))
            .send()
            .await
            .unwrap()
            .json()
            .unwrap();
        ids.push(created["id"].as_i64().unwrap());
    }
    let (edited, other) = (ids[0], ids[1]);

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
            .commands(commands![customer_name]),
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
    let mut one = app.live::<String>("customer_name", (edited,)).await;
    let _two = app.live::<String>("customer_name", (other,)).await;
    let other_before = shown(other);

    // On the web, Ada renames the first: Laravel broadcasts it with its id.
    http.put(format!("{url}/api/customers/{edited}"))
        .token(&token)
        .json(&json!({ "name": format!("renamed {stamp}") }))
        .send()
        .await
        .unwrap();
    one.next().await;
    assert_eq!(one.value(), &format!("renamed {stamp}"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        shown(other),
        other_before,
        "the other customer's view didn't re-run"
    );

    for id in ids {
        http.delete(format!("{url}/api/customers/{id}"))
            .token(&token)
            .send()
            .await
            .unwrap();
    }
    app.get::<Auth>().sign_out().await.unwrap();
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
