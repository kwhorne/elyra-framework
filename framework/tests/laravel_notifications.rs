//! RFC 0008 against a real Laravel app (scripts/laravel-backend.sh) and a
//! real Pusher-protocol server (Reverb, or Askr's `--pusher`): a customer
//! created on the web notifies the user, and the desktop app shows it in the
//! OS; read on the desktop, it's read on the web; read on the web, it's read
//! on the desktop. Skipped unless `ELYRA_LARAVEL_URL` and `ELYRA_PUSHER`
//! (`<key>@<host>:<port>`) say where they are; CI starts them.
#![cfg(feature = "backend")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::Auth;
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, Broadcasting, State};
use elyra::http::Http;
use elyra::notifications::{NotificationReceived, Notifications};
use elyra::notifier::Notifier;
use elyra::store::Store;
use elyra::testing::TestApp;
use elyra::{App, Ctx};
use serde_json::{json, Value};

async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while !check().await {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn a_notification_reaches_the_desktop_and_is_read_everywhere() {
    let (Ok(url), Ok(pusher)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_PUSHER"),
    ) else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL and ELYRA_PUSHER=<key>@<host>:<port>");
        return;
    };
    let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
    let (host, port) = address.rsplit_once(':').expect("<host>:<port>");
    let heard: Arc<Mutex<Vec<NotificationReceived>>> = Arc::default();
    let into = heard.clone();
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
                    .notifications(),
            )
            .listen(move |e: NotificationReceived, _ctx: Ctx| {
                let into = into.clone();
                async move {
                    into.lock().unwrap().push(e);
                    Ok(())
                }
            })
            .swap(Notifier::fake())
            .swap(Store::fake()),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");
    let broadcaster = app.get::<Broadcaster>();
    let notifications = app.get::<Notifications>();
    eventually("connected, and the list read", async || {
        broadcaster.state() == State::Connected && notifications.loaded()
    })
    .await;
    let before = notifications.unread();

    // "The web": Ada's own token, straight to Laravel, no socket id.
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
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let create = |name: String| {
        let (http, url, token) = (http.clone(), url.clone(), token.clone());
        async move {
            let customer: Value = http
                .post(format!("{url}/api/customers"))
                .token(&token)
                .json(&json!({ "name": name, "email": format!("{}@example.com", name.replace(' ', ".")) }))
                .send()
                .await
                .unwrap()
                .json()
                .unwrap();
            customer["id"].as_i64().expect("created")
        }
    };
    let heard_for = |name: &str| {
        heard
            .lock()
            .unwrap()
            .iter()
            .find(|n| n.presented.body.as_deref() == Some(name))
            .map(|n| n.notification.id.clone())
    };

    // 1. Created on the web: the desktop hears it, shows it in the OS, counts it.
    let first = format!("Notified {stamp}");
    let first_customer = create(first.clone()).await;
    eventually("heard", async || heard_for(&first).is_some()).await;
    let first_id = heard_for(&first).unwrap();
    app.get::<Notifier>()
        .assert_shown(|n| n.title == "New customer" && n.body.as_deref() == Some(first.as_str()));
    eventually("counted, by the server", async || {
        notifications.unread() == before + 1
    })
    .await;

    // 2. Read on the desktop: read on the web.
    notifications.mark_read(&first_id).await.unwrap();
    let on_the_web = |id: String| {
        let (http, url, token) = (http.clone(), url.clone(), token.clone());
        async move {
            let page: Value = http
                .get(format!("{url}/api/notifications"))
                .token(&token)
                .send()
                .await
                .unwrap()
                .json()
                .unwrap();
            page["data"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["id"] == id.as_str())
                .cloned()
                .expect("on the web's list")
        }
    };
    assert!(on_the_web(first_id.clone()).await["read_at"].is_string());

    // 3. Read on the web: the desktop hears `NotificationsRead` and reads the
    //    list again.
    let second = format!("Read elsewhere {stamp}");
    let second_customer = create(second.clone()).await;
    eventually("heard", async || heard_for(&second).is_some()).await;
    let second_id = heard_for(&second).unwrap();
    eventually("counted", async || notifications.unread() == before + 1).await;
    http.post(format!("{url}/api/notifications/{second_id}/read"))
        .token(&token)
        .send()
        .await
        .unwrap();
    eventually("read here too", async || {
        notifications.unread() == before
            && notifications
                .state()
                .items
                .iter()
                .any(|n| n.id == second_id && n.read_at.is_some())
    })
    .await;

    // Tidy up, and sign out.
    for id in [first_customer, second_customer] {
        http.delete(format!("{url}/api/customers/{id}"))
            .token(&token)
            .send()
            .await
            .unwrap();
    }
    app.get::<Auth>().sign_out().await.unwrap();
}
