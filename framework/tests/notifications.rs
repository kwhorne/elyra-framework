//! Laravel's notifications on the desktop (RFC 0008 step 1): heard on the
//! signed-in user's channel, against a `PusherFake`, with the backend (the
//! user, channel auth) answered by an `HttpFake`.
#![cfg(feature = "backend")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{BackendEvent, PusherFake};
use elyra::http::{Http, HttpFake, Method};
use elyra::notifications::{NotificationReceived, Presented, BROADCAST_EVENT};
use elyra::testing::TestApp;
use elyra::{App, Ctx};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const KEY: &str = "backend-token:https://crm.test";

fn laravel(user: i64) -> HttpFake {
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
            json!({ "id": user, "name": "Ada" }),
        )
        .delete(format!("{BASE}/api/sanctum/token"), 204, Value::Null)
        .post(
            format!("{BASE}/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:signed-by-laravel" }),
        )
}

#[derive(Default, Clone)]
struct Heard {
    notifications: Arc<Mutex<Vec<NotificationReceived>>>,
    events: Arc<Mutex<Vec<BackendEvent>>>,
}

impl Heard {
    fn notifications(&self) -> Vec<NotificationReceived> {
        self.notifications.lock().unwrap().clone()
    }

    fn events(&self) -> Vec<BackendEvent> {
        self.events.lock().unwrap().clone()
    }
}

fn app(fake: &HttpFake, backend: Backend, signed_in: bool) -> (TestApp, Heard) {
    let tokens = if signed_in {
        MemoryTokens::new().with(KEY, "7|earlier")
    } else {
        MemoryTokens::new()
    };
    let heard = Heard::default();
    let (notifications, events) = (heard.notifications.clone(), heard.events.clone());
    let app = TestApp::new(
        App::new()
            .backend(backend)
            .token_store(Arc::new(tokens))
            .listen(move |e: NotificationReceived, _ctx: Ctx| {
                let notifications = notifications.clone();
                async move {
                    notifications.lock().unwrap().push(e);
                    Ok(())
                }
            })
            .listen(move |e: BackendEvent, _ctx: Ctx| {
                let events = events.clone();
                async move {
                    events.lock().unwrap().push(e);
                    Ok(())
                }
            })
            .swap(Http::fake(fake.clone())),
    );
    (app, heard)
}

async fn eventually(what: &str, check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !check() {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn invoice_paid(id: &str) -> Value {
    json!({
        "id": id,
        "type": "invoice-paid",
        "title": "Invoice paid",
        "body": "Invoice 7 was paid.",
        "url": "/invoices/7",
    })
}

#[tokio::test]
async fn a_notification_is_heard_on_the_users_channel() {
    let fake = laravel(1);
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers")
        .notifications();
    let (app, heard) = app(&fake, backend, true);
    app.listen();
    pusher.wait_for_subscriptions(2).await;

    // Laravel's channel for user 1, authorized by the backend like any other.
    let subs = pusher.subscriptions();
    let mine = subs
        .iter()
        .find(|s| s.channel == "private-App.Models.User.1")
        .expect("the user's channel");
    assert_eq!(mine.auth.as_deref(), Some("fake-key:signed-by-laravel"));
    let asked = fake.sent_to("POST", &format!("{BASE}/api/broadcasting/auth"));
    assert!(asked
        .iter()
        .any(|r| r.body.as_ref().unwrap()["channel_name"] == "private-App.Models.User.1"));

    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("the notification", || !heard.notifications().is_empty()).await;
    let got = &heard.notifications()[0];
    assert_eq!(got.notification.id, "9b1");
    assert_eq!(got.notification.kind, "invoice-paid");
    assert_eq!(got.notification.data["body"], "Invoice 7 was paid.");
    assert!(got.notification.data.get("id").is_none(), "beside the data");
    assert_eq!(
        got.presented,
        Presented {
            title: "Invoice paid".into(),
            body: Some("Invoice 7 was paid.".into()),
            url: Some("/invoices/7".into()),
        }
    );

    // The windows get it too, camelCase like the rest.
    let sent: Vec<Value> = app.events_on("elyra:notification").await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["notification"]["id"], "9b1");
    assert_eq!(sent[0]["notification"]["readAt"], Value::Null);
    assert_eq!(sent[0]["presented"]["title"], "Invoice paid");

    // Another event on the user's channel is an ordinary `BackendEvent`; the
    // notification wasn't.
    pusher.broadcast(
        "private-App.Models.User.1",
        "NotificationsRead",
        json!({ "ids": ["9b1"] }),
    );
    eventually("the other event", || !heard.events().is_empty()).await;
    let events = heard.events();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].channel, "App.Models.User.1");
    assert_eq!(events[0].event, "NotificationsRead");
    assert_eq!(heard.notifications().len(), 1);

    // Laravel's notification event, but on another channel: not the user's
    // notification.
    pusher.broadcast("private-customers", BROADCAST_EVENT, invoice_paid("x"));
    eventually("on customers", || heard.events().len() == 2).await;
    assert_eq!(heard.events()[1].channel, "customers");
    assert_eq!(heard.notifications().len(), 1);
}

#[tokio::test]
async fn without_notifications_the_users_channel_isnt_followed() {
    let fake = laravel(1);
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers");
    let (_app, heard) = app(&fake, backend, true);
    pusher.wait_for_subscriptions(1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let channels: Vec<String> = pusher
        .subscriptions()
        .iter()
        .map(|s| s.channel.clone())
        .collect();
    assert_eq!(channels, ["private-customers"]);

    // Sent anyway: it isn't followed, so it's nothing.
    pusher.send_to_all(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(heard.notifications().is_empty());
}

#[tokio::test]
async fn each_sign_in_follows_that_users_channel() {
    let fake = laravel(1);
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .notifications();
    let (app, heard) = app(&fake, backend, false);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(pusher.connections(), 0, "no one's channel to follow");

    let auth = app.get::<Auth>();
    auth.sign_in("ada@example.com", "secret").await.unwrap();
    pusher.wait_for_subscriptions(1).await;
    assert_eq!(
        pusher.subscriptions()[0].channel,
        "private-App.Models.User.1"
    );

    auth.sign_out().await.unwrap();
    pusher
        .wait_until(|p| p.connected() == 0, "disconnect")
        .await;

    // Bob signs in at the same machine.
    fake.clone().get(
        format!("{BASE}/api/user"),
        200,
        json!({ "id": 2, "name": "Bob" }),
    );
    auth.sign_in("bob@example.com", "secret").await.unwrap();
    pusher.wait_for_subscriptions(2).await;
    assert_eq!(
        pusher.subscriptions()[1].channel,
        "private-App.Models.User.2"
    );

    // Ada's notifications aren't Bob's.
    pusher.send_to_all(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("ada"),
    );
    pusher.broadcast(
        "private-App.Models.User.2",
        BROADCAST_EVENT,
        invoice_paid("bob"),
    );
    eventually("Bob's", || !heard.notifications().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let ids: Vec<String> = heard
        .notifications()
        .iter()
        .map(|n| n.notification.id.clone())
        .collect();
    assert_eq!(ids, ["bob"]);
}

#[tokio::test]
async fn a_channel_and_a_presentation_of_the_apps_own() {
    let fake = laravel(5);
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .notifications_channel("users.{id}")
        .present_notifications(|n| Presented {
            title: n.data["subject"].as_str().unwrap_or("?").to_owned(),
            body: None,
            url: Some(format!("/tickets/{}", n.data["ticket"])),
        });
    let (_app, heard) = app(&fake, backend, true);
    pusher.wait_for_subscriptions(1).await;
    assert_eq!(pusher.subscriptions()[0].channel, "private-users.5");

    pusher.broadcast(
        "private-users.5",
        BROADCAST_EVENT,
        json!({ "id": "t1", "type": "reply", "subject": "Re: invoice", "ticket": 12 }),
    );
    eventually("the notification", || !heard.notifications().is_empty()).await;
    assert_eq!(
        heard.notifications()[0].presented,
        Presented {
            title: "Re: invoice".into(),
            body: None,
            url: Some("/tickets/12".into()),
        }
    );
}

#[tokio::test]
async fn a_user_without_an_id_has_no_channel_but_the_rest_is_followed() {
    let fake = laravel(1).get(
        format!("{BASE}/api/user"),
        200,
        json!({ "email": "ada@example.com" }),
    );
    let pusher = PusherFake::start().await;
    let backend = Backend::new(BASE)
        .broadcasting(pusher.config())
        .follow("customers", "/api/customers")
        .notifications();
    let (_app, _heard) = app(&fake, backend, true);
    pusher.wait_for_subscriptions(1).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let channels: Vec<String> = pusher
        .subscriptions()
        .iter()
        .map(|s| s.channel.clone())
        .collect();
    assert_eq!(channels, ["private-customers"]);
}

#[test]
#[should_panic(expected = "needs broadcasting")]
fn notifications_without_broadcasting_wont_start() {
    let _ = TestApp::new(
        App::new()
            .backend(Backend::new(BASE).notifications())
            .token_store(Arc::new(MemoryTokens::new())),
    );
}
