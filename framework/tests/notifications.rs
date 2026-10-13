//! Laravel's notifications on the desktop (RFC 0008): heard on the signed-in
//! user's channel (step 1); shown in the OS or as a toast, clicked, and
//! counted (step 2); listed, read, and caught up on (step 3) — against a
//! `PusherFake`, with the backend (the user, channel auth, the list) answered
//! by an `HttpFake`, the OS by a `Notifier::fake()`.
#![cfg(feature = "backend")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{BackendEvent, PusherFake};
use elyra::http::{Http, HttpFake, Method};
use elyra::notifications::{
    NotificationClicked, NotificationReceived, Notifications, NotificationsState, Presented,
    ShownAs, BROADCAST_EVENT,
};
use elyra::notifier::Notifier;
use elyra::store::Store;
use elyra::testing::TestApp;
use elyra::window::Focus;
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
    clicked: Arc<Mutex<Vec<NotificationClicked>>>,
}

impl Heard {
    fn notifications(&self) -> Vec<NotificationReceived> {
        self.notifications.lock().unwrap().clone()
    }

    fn events(&self) -> Vec<BackendEvent> {
        self.events.lock().unwrap().clone()
    }

    fn clicked(&self) -> Vec<NotificationClicked> {
        self.clicked.lock().unwrap().clone()
    }
}

fn app(fake: &HttpFake, backend: Backend, signed_in: bool) -> (TestApp, Heard) {
    let tokens = if signed_in {
        MemoryTokens::new().with(KEY, "7|earlier")
    } else {
        MemoryTokens::new()
    };
    let heard = Heard::default();
    let (notifications, events, clicked) = (
        heard.notifications.clone(),
        heard.events.clone(),
        heard.clicked.clone(),
    );
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
            .listen(move |e: NotificationClicked, _ctx: Ctx| {
                let clicked = clicked.clone();
                async move {
                    clicked.lock().unwrap().push(e);
                    Ok(())
                }
            })
            .swap(Http::fake(fake.clone()))
            // Never the machine's own notification center, or settings.
            .swap(Notifier::fake())
            .swap(Store::fake()),
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

/// Signed in as user 1, with notifications, connected.
async fn signed_in(backend: impl FnOnce(Backend) -> Backend) -> (TestApp, Heard, PusherFake) {
    signed_in_with(&laravel(1), backend).await
}

async fn signed_in_with(
    fake: &HttpFake,
    backend: impl FnOnce(Backend) -> Backend,
) -> (TestApp, Heard, PusherFake) {
    let fake = fake.clone();
    let pusher = PusherFake::start().await;
    let (app, heard) = app(
        &fake,
        backend(
            Backend::new(BASE)
                .broadcasting(pusher.config())
                .notifications(),
        ),
        true,
    );
    app.listen();
    pusher.wait_for_subscriptions(1).await;
    (app, heard, pusher)
}

#[tokio::test]
async fn with_no_window_in_front_it_shows_in_the_os() {
    let (app, heard, pusher) = signed_in(|b| b).await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("the notification", || !heard.notifications().is_empty()).await;

    assert_eq!(heard.notifications()[0].shown, ShownAs::Native);
    let shown = app
        .get::<Notifier>()
        .assert_shown(|n| n.title == "Invoice paid");
    assert_eq!(shown.body.as_deref(), Some("Invoice 7 was paid."));
    let sent: Vec<Value> = app.events_on("elyra:notification").await;
    assert_eq!(sent[0]["shown"], "native");
    assert_eq!(app.get::<Notifications>().unread(), 1);
}

#[tokio::test]
async fn with_a_window_in_front_it_is_a_toast() {
    let (app, heard, pusher) = signed_in(|b| b).await;
    app.get::<Focus>().set(true);
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("the notification", || !heard.notifications().is_empty()).await;

    assert_eq!(heard.notifications()[0].shown, ShownAs::Toast);
    app.get::<Notifier>().assert_nothing_shown();
    let sent: Vec<Value> = app.events_on("elyra:notification").await;
    assert_eq!(sent[0]["shown"], "toast", "the frontend shows it");
    assert_eq!(
        app.get::<Notifications>().unread(),
        1,
        "counted all the same"
    );
}

#[tokio::test]
async fn without_previews_the_os_is_told_nothing_but_that_there_is_one() {
    let (app, heard, pusher) = signed_in(|b| b.notification_previews(false)).await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("the notification", || !heard.notifications().is_empty()).await;
    let shown = app.get::<Notifier>().shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].title, "New notification");
    assert_eq!(shown[0].body, None);
    // The app itself still has it all.
    assert_eq!(heard.notifications()[0].presented.title, "Invoice paid");
}

#[tokio::test]
async fn a_click_opens_an_in_app_path_and_is_announced() {
    let (app, heard, pusher) = signed_in(|b| b).await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("shown", || app.get::<Notifier>().shown().len() == 1).await;

    assert!(app.get::<Notifier>().click(0));
    eventually("the click", || !heard.clicked().is_empty()).await;
    assert_eq!(heard.clicked()[0].notification.id, "9b1");
    // (`events_on` drains every channel: the click's own is read below.)
    let opened: Vec<String> = app.events_on("elyra:navigate").await;
    assert_eq!(opened, ["/invoices/7"]);
}

#[tokio::test]
async fn a_click_never_opens_a_full_url_by_itself() {
    let (app, heard, pusher) = signed_in(|b| b).await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        json!({ "id": "x", "type": "t", "title": "Look", "url": "https://evil.example/" }),
    );
    eventually("shown", || app.get::<Notifier>().shown().len() == 1).await;
    app.get::<Notifier>().click(0);
    eventually("the click", || !heard.clicked().is_empty()).await;
    // The app's listener has the URL, to open or not.
    assert_eq!(
        heard.clicked()[0].presented.url.as_deref(),
        Some("https://evil.example/")
    );
    app.assert_not_emitted("elyra:navigate").await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        json!({ "id": "y", "type": "t", "title": "Again", "url": "https://evil.example/" }),
    );
    eventually("shown", || app.get::<Notifier>().shown().len() == 2).await;
    app.get::<Notifier>().click(1);
    eventually("the click", || heard.clicked().len() == 2).await;
    let clicked: Vec<Value> = app.events_on("elyra:notification-clicked").await;
    assert_eq!(clicked.len(), 1, "the windows are told");
    assert_eq!(clicked[0]["presented"]["url"], "https://evil.example/");
}

#[tokio::test]
async fn signing_out_clears_the_count() {
    let (app, heard, pusher) = signed_in(|b| b).await;
    for id in ["a", "b"] {
        pusher.broadcast(
            "private-App.Models.User.1",
            BROADCAST_EVENT,
            invoice_paid(id),
        );
    }
    eventually("both", || heard.notifications().len() == 2).await;
    assert_eq!(app.get::<Notifications>().unread(), 2);
    app.get::<Auth>().sign_out().await.unwrap();
    assert_eq!(app.get::<Notifications>().unread(), 0);
}

/// The frontend's `notify(..)` goes through the container's `Notifier` too,
/// so a test sees it instead of the machine's notification center.
#[cfg(feature = "system")]
#[tokio::test]
async fn the_frontends_notify_goes_through_the_notifier() {
    let prepared = App::new().swap(Notifier::fake()).prepare();
    let ctx = prepared.ctx.clone();
    let shell = elyra::testing::TestShell::new(prepared);
    let request = wry::http::Request::builder()
        .method("POST")
        .uri("elyra://localhost/__sys/notify")
        .header("x-elyra-token", shell.token())
        .body(rmp_serde::to_vec_named(&json!({ "title": "Saved", "body": "All done." })).unwrap())
        .unwrap();
    let response = shell.handle(request).await;
    assert_eq!(
        response
            .headers()
            .get("x-elyra-status")
            .map(|v| v.as_bytes()),
        Some(&b"ok"[..])
    );
    let shown = ctx.get::<Notifier>().assert_shown(|n| n.title == "Saved");
    assert_eq!(shown.body.as_deref(), Some("All done."));
}

const LIST: &str = "https://crm.test/api/notifications";

/// A row of Laravel's `notifications` table, as its paginator sends it.
fn row(id: &str, title: &str, created_at: &str, read: bool) -> Value {
    json!({
        "id": id,
        "type": "App\\Notifications\\InvoicePaid",
        "notifiable_type": "App\\Models\\User",
        "notifiable_id": 1,
        "data": { "title": title, "url": format!("/invoices/{id}") },
        "read_at": if read { json!("2026-10-13T09:00:00.000000Z") } else { Value::Null },
        "created_at": created_at,
        "updated_at": created_at,
    })
}

/// `GET /api/notifications`: one page of `rows`, and the unread count.
fn listing(fake: &HttpFake, rows: Vec<Value>, unread: u32, last_page: u32) {
    fake.clone().get(
        LIST,
        200,
        json!({ "data": rows, "current_page": 1, "last_page": last_page, "per_page": 15, "unread": unread }),
    );
}

fn with_reads(fake: HttpFake) -> HttpFake {
    fake.post(format!("{LIST}/*"), 200, json!({}))
}

#[tokio::test]
async fn the_list_and_the_count_come_from_laravels_table() {
    let fake = with_reads(laravel(1));
    listing(
        &fake,
        vec![
            row("b", "Second", "2026-10-13T10:00:00.000000Z", false),
            row("a", "First", "2026-10-13T08:00:00.000000Z", true),
        ],
        7,
        2,
    );
    let pusher = PusherFake::start().await;
    let (app, _heard) = app(
        &fake,
        Backend::new(BASE)
            .broadcasting(pusher.config())
            .notifications(),
        true,
    );
    app.listen();
    let notifications = app.get::<Notifications>();
    let state = notifications.refresh().await.unwrap();

    assert_eq!(state.unread, 7, "the server's count, not the page's");
    assert!(state.has_more);
    assert_eq!(state.items.len(), 2);
    assert_eq!(state.items[0].kind, "App\\Notifications\\InvoicePaid");
    assert_eq!(state.items[0].data["title"], "Second");
    assert_eq!(
        state.items[1].read_at.as_deref(),
        Some("2026-10-13T09:00:00.000000Z")
    );
    assert!(fake
        .sent_to("GET", LIST)
        .iter()
        .any(|s| s.url.ends_with("?page=1")));

    // The windows get it, camelCase.
    let sent: Vec<Value> = app.events_on("elyra:notifications").await;
    let last = sent.last().unwrap();
    assert_eq!(last["unread"], 7);
    assert_eq!(last["hasMore"], true);
    assert_eq!(last["items"][1]["readAt"], "2026-10-13T09:00:00.000000Z");

    // The next page goes after it.
    fake.clone().get(
        LIST,
        200,
        json!({ "data": [row("z", "Old", "2026-10-01T08:00:00.000000Z", true)], "current_page": 2, "last_page": 2, "unread": 7 }),
    );
    let state = notifications.more().await.unwrap();
    let ids: Vec<&str> = state.items.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, ["b", "a", "z"]);
    assert!(!state.has_more);
    assert!(fake
        .sent_to("GET", LIST)
        .iter()
        .any(|s| s.url.ends_with("?page=2")));
}

#[tokio::test]
async fn reading_marks_here_at_once_and_on_the_server() {
    let fake = with_reads(laravel(1));
    listing(
        &fake,
        vec![
            row("b", "Second", "2026-10-13T10:00:00.000000Z", false),
            row("a", "First", "2026-10-13T08:00:00.000000Z", false),
        ],
        2,
        1,
    );
    let pusher = PusherFake::start().await;
    let (app, _heard) = app(
        &fake,
        Backend::new(BASE)
            .broadcasting(pusher.config())
            .notifications(),
        true,
    );
    let notifications = app.get::<Notifications>();
    notifications.refresh().await.unwrap();

    notifications.mark_read("b").await.unwrap();
    assert_eq!(notifications.unread(), 1);
    assert!(notifications.state().items[0].read_at.is_some());
    fake.assert_sent("POST", &format!("{LIST}/b/read"));
    // Twice is harmless, and doesn't count down again.
    notifications.mark_read("b").await.unwrap();
    assert_eq!(notifications.unread(), 1);

    notifications.mark_all_read().await.unwrap();
    assert_eq!(notifications.unread(), 0);
    assert!(notifications
        .state()
        .items
        .iter()
        .all(|n| n.read_at.is_some()));
    fake.assert_sent("POST", &format!("{LIST}/read-all"));
}

#[tokio::test]
async fn heard_it_is_on_the_list_and_the_server_is_asked_for_the_rest() {
    let fake = with_reads(laravel(1));
    listing(&fake, vec![], 0, 1);
    let (app, heard, pusher) = signed_in_with(&fake, |b| b).await;
    let gets = || fake.sent_to("GET", LIST).len();
    eventually("read at the start", || gets() >= 1).await;
    let before = gets();

    // What the server will say once it's stored.
    listing(
        &fake,
        vec![row(
            "9b1",
            "Invoice paid",
            "2026-10-13T10:00:00.000000Z",
            false,
        )],
        1,
        1,
    );
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("heard", || !heard.notifications().is_empty()).await;
    eventually("read again", || gets() > before).await;
    let notifications = app.get::<Notifications>();
    eventually("dated", || {
        notifications
            .state()
            .items
            .first()
            .and_then(|n| n.created_at.clone())
            .is_some()
    })
    .await;
    assert_eq!(notifications.state().items.len(), 1, "once");
    assert_eq!(notifications.unread(), 1);
}

#[tokio::test]
async fn a_click_reads_it() {
    let fake = with_reads(laravel(1));
    listing(&fake, vec![], 0, 1);
    let (app, _heard, pusher) = signed_in_with(&fake, |b| b).await;
    pusher.broadcast(
        "private-App.Models.User.1",
        BROADCAST_EVENT,
        invoice_paid("9b1"),
    );
    eventually("shown", || app.get::<Notifier>().shown().len() == 1).await;
    app.get::<Notifier>().click(0);
    eventually("read on the server", || {
        !fake.sent_to("POST", &format!("{LIST}/9b1/read")).is_empty()
    })
    .await;
}

#[tokio::test]
async fn read_on_the_web_is_read_here() {
    let fake = with_reads(laravel(1));
    listing(
        &fake,
        vec![row("a", "First", "2026-10-13T08:00:00.000000Z", false)],
        1,
        1,
    );
    let (app, _heard, pusher) = signed_in_with(&fake, |b| b).await;
    let notifications = app.get::<Notifications>();
    eventually("counted", || notifications.unread() == 1).await;

    listing(
        &fake,
        vec![row("a", "First", "2026-10-13T08:00:00.000000Z", true)],
        0,
        1,
    );
    pusher.broadcast(
        "private-App.Models.User.1",
        "NotificationsRead",
        json!({ "ids": ["a"] }),
    );
    eventually("read here too", || notifications.unread() == 0).await;
    assert!(notifications.state().items[0].read_at.is_some());
}

#[tokio::test]
async fn signing_out_forgets_the_list() {
    let fake = with_reads(laravel(1));
    listing(
        &fake,
        vec![row("a", "First", "2026-10-13T08:00:00.000000Z", false)],
        1,
        1,
    );
    let (app, _heard, _pusher) = signed_in_with(&fake, |b| b).await;
    let notifications = app.get::<Notifications>();
    eventually("listed", || notifications.state().items.len() == 1).await;
    app.get::<Auth>().sign_out().await.unwrap();
    assert_eq!(notifications.state(), NotificationsState::default());
}

/// Ada was last here at 08:00; what came since is waiting when she's back.
async fn back_after(rows: Vec<Value>, seen: Option<&str>, focused: bool) -> (TestApp, HttpFake) {
    let fake = with_reads(laravel(1));
    listing(&fake, rows, 9, 1);
    let pusher = PusherFake::start().await;
    let (app, _heard) = app(
        &fake,
        Backend::new(BASE)
            .broadcasting(pusher.config())
            .notifications(),
        false,
    );
    app.listen();
    if let Some(at) = seen {
        app.get::<Store>().set(
            "elyra.notifications.seen:/api/notifications",
            json!({ "user": "1", "at": at }),
        );
    }
    app.get::<Focus>().set(focused);
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    pusher.wait_for_subscriptions(1).await;
    let notifications = app.get::<Notifications>();
    eventually("listed", || !notifications.state().items.is_empty()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    (app, fake)
}

fn three_since_eight() -> Vec<Value> {
    vec![
        row("c", "Third", "2026-10-13T11:00:00.000000Z", false),
        row("b", "Second", "2026-10-13T10:00:00.000000Z", false),
        row("r", "Read already", "2026-10-13T09:30:00.000000Z", true),
        row("a", "First", "2026-10-13T09:00:00.000000Z", false),
        row("o", "Old", "2026-10-13T07:00:00.000000Z", false),
    ]
}

#[tokio::test]
async fn what_was_missed_is_one_summary_that_opens_the_list() {
    let (app, _fake) = back_after(
        three_since_eight(),
        Some("2026-10-13T08:00:00.000000Z"),
        false,
    )
    .await;
    let notifier = app.get::<Notifier>();
    let shown = notifier.shown();
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(
        shown[0].title, "3 new notifications",
        "unread, and since 08:00"
    );
    app.drain_events().await;
    notifier.click(0);
    let opened: Vec<String> = app.events_on("elyra:navigate").await;
    assert_eq!(opened, ["/notifications"]);
}

#[tokio::test]
async fn one_missed_is_shown_as_itself() {
    let (app, _fake) = back_after(
        three_since_eight(),
        Some("2026-10-13T10:30:00.000000Z"),
        false,
    )
    .await;
    let shown = app.get::<Notifier>().shown();
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].title, "Third");
}

#[tokio::test]
async fn a_first_sign_in_here_isnt_a_flood() {
    let (app, _fake) = back_after(three_since_eight(), None, false).await;
    app.get::<Notifier>().assert_nothing_shown();
    // …and the next time, it knows where it was.
    let seen = app
        .get::<Store>()
        .get("elyra.notifications.seen:/api/notifications")
        .unwrap();
    assert_eq!(
        seen,
        json!({ "user": "1", "at": "2026-10-13T11:00:00.000000Z" })
    );
}

#[tokio::test]
async fn in_front_of_the_app_nothing_is_shown_for_what_was_missed() {
    let (app, _fake) = back_after(
        three_since_eight(),
        Some("2026-10-13T08:00:00.000000Z"),
        true,
    )
    .await;
    app.get::<Notifier>().assert_nothing_shown();
}

#[tokio::test]
async fn someone_elses_last_visit_isnt_this_users() {
    let fake = with_reads(laravel(1));
    listing(&fake, three_since_eight(), 3, 1);
    let pusher = PusherFake::start().await;
    let (app, _heard) = app(
        &fake,
        Backend::new(BASE)
            .broadcasting(pusher.config())
            .notifications(),
        false,
    );
    app.get::<Store>().set(
        "elyra.notifications.seen:/api/notifications",
        json!({ "user": "2", "at": "2026-10-13T08:00:00.000000Z" }),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let notifications = app.get::<Notifications>();
    eventually("listed", || !notifications.state().items.is_empty()).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    app.get::<Notifier>().assert_nothing_shown();
}

#[tokio::test]
async fn the_same_one_heard_twice_is_listed_and_counted_once() {
    // No list to read: what's heard is all the app has.
    let (app, heard, pusher) = signed_in(|b| b).await;
    for _ in 0..2 {
        pusher.broadcast(
            "private-App.Models.User.1",
            BROADCAST_EVENT,
            invoice_paid("9b1"),
        );
    }
    eventually("both", || heard.notifications().len() == 2).await;
    let state = app.get::<Notifications>().state();
    assert_eq!(state.items.len(), 1);
    assert_eq!(state.unread, 1);
}

/// Offline (RFC 0007), marking one read waits in the outbox and counts down
/// at once.
#[cfg(feature = "database")]
#[tokio::test]
async fn read_with_no_network_waits_in_the_outbox() {
    let path = std::env::temp_dir().join(format!(
        "elyra-notifications-{}-{}.db",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&path);
    let db = elyra::Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    let fake = laravel(1).get(format!("{BASE}/up"), 200, json!("ok"));
    listing(
        &fake,
        vec![row("a", "First", "2026-10-13T08:00:00.000000Z", false)],
        1,
        1,
    );
    let pusher = PusherFake::start().await;
    let app = TestApp::new(
        App::new()
            .bind(db)
            .backend(
                Backend::new(BASE)
                    .broadcasting(pusher.config())
                    .notifications()
                    .offline(),
            )
            .token_store(Arc::new(MemoryTokens::new().with(KEY, "7|earlier")))
            .swap(Http::fake(fake.clone()))
            .swap(Notifier::fake())
            .swap(Store::fake()),
    );
    let notifications = app.get::<Notifications>();
    notifications.refresh().await.unwrap();
    assert_eq!(notifications.unread(), 1);

    fake.clone().unreachable(Method::Post, format!("{LIST}/*"));
    notifications.mark_read("a").await.unwrap();
    assert_eq!(notifications.unread(), 0, "at once");
    let waiting = app.get::<elyra::offline::Outbox>().items().await;
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0].path, "/api/notifications/a/read");
    drop(app);
    let _ = std::fs::remove_file(&path);
}
