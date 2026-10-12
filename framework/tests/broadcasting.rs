//! Laravel's broadcasting on the desktop (RFC 0006 step 1): the Pusher
//! protocol against a `PusherFake`, with the backend (channel auth, the user)
//! answered by an `HttpFake`. No network, no PHP.
#![cfg(feature = "backend")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{BackendEvent, Broadcaster, PusherFake, State};
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const KEY: &str = "backend-token:https://crm.test";

/// Laravel: the token route, the user, channel auth, and one API route.
fn laravel() -> HttpFake {
    HttpFake::new()
        .on(
            Method::Post,
            format!("{BASE}/api/sanctum/token"),
            200,
            json!("7|s3cr3t"),
        )
        .get(
            format!("{BASE}/api/user"),
            200,
            json!({ "id": 1, "name": "Ada" }),
        )
        .delete(format!("{BASE}/api/sanctum/token"), 204, Value::Null)
        .post(
            format!("{BASE}/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:signed-by-laravel" }),
        )
        .get(format!("{BASE}/api/customers/count"), 200, json!(3))
}

static HEARD: Mutex<Vec<BackendEvent>> = Mutex::new(Vec::new());

#[command]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

fn app(fake: &HttpFake, pusher: &PusherFake, signed_in: bool) -> TestApp {
    let tokens = if signed_in {
        MemoryTokens::new().with(KEY, "7|earlier")
    } else {
        MemoryTokens::new()
    };
    TestApp::new(
        App::new()
            .backend(
                Backend::new(BASE)
                    .broadcasting(pusher.config())
                    .follow("customers", "/api/customers")
                    .follow_public("announcements", "/api/announcements"),
            )
            .token_store(Arc::new(tokens))
            .commands(commands![customers_count])
            .listen(|e: BackendEvent, _ctx: Ctx| async move {
                HEARD.lock().unwrap().push(e);
                Ok(())
            })
            .swap(Http::fake(fake.clone())),
    )
}

#[tokio::test]
async fn a_signed_in_app_subscribes_with_laravels_signature() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher, true);
    pusher.wait_for_subscriptions(2).await;

    let subs = pusher.subscriptions();
    let private = subs
        .iter()
        .find(|s| s.channel == "private-customers")
        .unwrap();
    assert_eq!(private.auth.as_deref(), Some("fake-key:signed-by-laravel"));
    let public = subs.iter().find(|s| s.channel == "announcements").unwrap();
    assert_eq!(public.auth, None, "a public channel needs no auth");

    // The private channel was authorized by the backend, as the user.
    let auth = fake.assert_sent("POST", &format!("{BASE}/api/broadcasting/auth"));
    assert_eq!(auth.header("authorization"), Some("Bearer 7|earlier"));
    assert_eq!(
        auth.body.as_ref().unwrap()["channel_name"],
        "private-customers"
    );
    assert_eq!(
        auth.body.as_ref().unwrap()["socket_id"],
        private.socket_id.as_str()
    );
    assert_eq!(
        fake.sent_to("POST", &format!("{BASE}/api/broadcasting/auth"))
            .len(),
        1
    );

    // While connected, requests carry the socket id (`->toOthers()`).
    app.invoke_ok::<i64>("customers_count", ()).await;
    let sent = fake.assert_sent("GET", &format!("{BASE}/api/customers/count"));
    assert_eq!(sent.header("x-socket-id"), Some(private.socket_id.as_str()));
    assert_eq!(app.get::<Broadcaster>().state(), State::Connected);
}

#[tokio::test]
async fn an_event_is_a_domain_event_and_reaches_the_windows() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher, true);
    app.listen();
    pusher.wait_for_subscriptions(2).await;

    pusher.broadcast(
        "private-customers",
        "CustomerUpdated",
        json!({ "model": { "id": 7, "name": "Acme" } }),
    );
    // Not followed: ignored, even if a server sends it anyway.
    pusher.send_to_all("private-orders", "OrderShipped", json!({}));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let heard = loop {
        let heard: Vec<BackendEvent> = HEARD
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.data["model"]["id"] == 7)
            .cloned()
            .collect();
        if !heard.is_empty() || tokio::time::Instant::now() > deadline {
            break heard;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].channel, "customers", "the name the app uses");
    assert_eq!(heard[0].event, "CustomerUpdated");
    assert_eq!(
        heard[0].data["model"]["name"], "Acme",
        "data decoded from Pusher's string"
    );

    let events: Vec<Value> = app.events_on("elyra:backend").await;
    assert!(events
        .iter()
        .any(|e| e["channel"] == "customers" && e["event"] == "CustomerUpdated"));
    assert!(!events.iter().any(|e| e["event"] == "OrderShipped"));
}

#[tokio::test]
async fn it_listens_while_signed_in_and_not_otherwise() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher, false);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(pusher.connections(), 0, "no one to listen for");

    let auth = app.get::<Auth>();
    auth.sign_in("ada@example.com", "secret").await.unwrap();
    pusher.wait_for_subscriptions(2).await;
    assert_eq!(pusher.connected(), 1);

    auth.sign_out().await.unwrap();
    pusher
        .wait_until(|p| p.connected() == 0, "disconnect")
        .await;
    assert_eq!(app.get::<Broadcaster>().state(), State::Offline);
    // No socket id once offline.
    app.get::<Backend>().use_token(Some("7|again".into()));
    app.invoke_ok::<i64>("customers_count", ()).await;
    let last = fake
        .sent_to("GET", &format!("{BASE}/api/customers/count"))
        .pop()
        .unwrap();
    assert_eq!(last.header("x-socket-id"), None);
}

#[tokio::test]
async fn a_dropped_connection_reconnects_and_subscribes_again() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let _app = app(&fake, &pusher, true);
    pusher.wait_for_subscriptions(2).await;

    pusher.disconnect_all();
    pusher.wait_for_subscriptions(4).await;
    assert_eq!(pusher.connections(), 2);
    let channels: Vec<String> = pusher
        .subscriptions()
        .iter()
        .skip(2)
        .map(|s| s.channel.clone())
        .collect();
    assert!(channels.contains(&"private-customers".to_string()));
}

#[tokio::test]
async fn pings_both_ways() {
    let fake = laravel();
    // The server asks for a ping after a second of quiet.
    let pusher = PusherFake::start_with_activity_timeout(1).await;
    let _app = app(&fake, &pusher, true);
    pusher.wait_for_subscriptions(2).await;
    pusher
        .wait_until(
            |p| p.received().iter().any(|f| f["event"] == "pusher:ping"),
            "ping from the client",
        )
        .await;

    pusher.ping();
    pusher
        .wait_until(
            |p| p.received().iter().any(|f| f["event"] == "pusher:pong"),
            "pong from the client",
        )
        .await;
}

#[tokio::test]
async fn a_fatal_error_stops_reconnecting() {
    let fake = laravel();
    let pusher = PusherFake::start().await;
    let app = app(&fake, &pusher, true);
    pusher.wait_for_subscriptions(2).await;
    // 4001: the app doesn't exist — trying again won't help.
    pusher.error(4001, "Application does not exist");
    pusher
        .wait_until(|p| p.connected() == 0, "the client to leave")
        .await;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(pusher.connections(), 1, "no reconnect");
    assert_eq!(app.get::<Broadcaster>().state(), State::Offline);
}
