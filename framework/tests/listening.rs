//! Private channels a window listens to (RFC 0023): windows listen to
//! Laravel private channels through the shell, the app subscribes to each
//! once on the open connection, signed by the backend's broadcasting auth,
//! and the events reach only the windows listening. A `PusherFake` stands
//! in for Reverb, an `HttpFake` for Laravel's channel auth.
#![cfg(feature = "backend")]

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, PusherFake, State};
use elyra::event::EventBus;
use elyra::http::{Http, HttpFake};
use elyra::listening::{Listening, CHANNEL};
use elyra::testing::TestShell;
use elyra::{App, Ctx};
use serde::Deserialize;
use serde_json::{json, Value};
use wry::http::{Request, Response};

const BASE: &str = "https://felagi.test";
const RUN: &str = "task.01JD8X3QZ7";
const RUN_WIRE: &str = "private-task.01JD8X3QZ7";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

/// Laravel: Grete (7) signs in, and the channel rule lets her listen.
fn laravel() -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 7, "name": "Grete" }))
        .delete(url("/api/sanctum/token"), 204, Value::Null)
        .post(
            url("/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:signed-by-laravel" }),
        )
}

struct Rig {
    shell: TestShell,
    ctx: Ctx,
    bus: EventBus,
    pusher: PusherFake,
}

async fn rig_with(fake: &HttpFake, backend: impl FnOnce(Backend) -> Backend) -> Rig {
    let pusher = PusherFake::start().await;
    let prepared = App::new()
        .backend(backend(Backend::new(BASE).broadcasting(pusher.config())))
        .token_store(Arc::new(MemoryTokens::new()))
        .swap(Http::fake(fake.clone()))
        .prepare();
    let (ctx, bus) = (prepared.ctx.clone(), prepared.bus.clone());
    for window in ["w1", "w2"] {
        bus.register_client(window);
    }
    let shell = TestShell::new(prepared);
    ctx.get::<Auth>()
        .sign_in("grete@example.com", "secret")
        .await
        .unwrap();
    let broadcaster = ctx.get::<Broadcaster>();
    pusher
        .wait_until(|_| broadcaster.state() == State::Connected, "a connection")
        .await;
    Rig {
        shell,
        ctx,
        bus,
        pusher,
    }
}

async fn rig() -> Rig {
    rig_with(&laravel(), |b| b.listen(["task.*"])).await
}

async fn post(
    shell: &TestShell,
    window: &str,
    path: &str,
    body: Vec<u8>,
) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", window)
        .body(body)
        .unwrap();
    shell.handle(request).await
}

#[derive(serde::Serialize)]
struct One<'a> {
    channel: &'a str,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
struct Joined {
    channel: String,
    joined: bool,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
struct Heard {
    channel: String,
    event: String,
    data: Value,
}

async fn listen(rig: &Rig, window: &str, channel: &str) -> Response<Cow<'static, [u8]>> {
    let body = rmp_serde::to_vec_named(&One { channel }).unwrap();
    post(&rig.shell, window, "/__listen/join", body).await
}

async fn unlisten(rig: &Rig, window: &str, channel: &str) -> bool {
    let body = rmp_serde::to_vec_named(&One { channel }).unwrap();
    let res = post(&rig.shell, window, "/__listen/leave", body).await;
    rmp_serde::from_slice(res.body()).unwrap()
}

fn kind(res: &Response<Cow<'static, [u8]>>) -> Option<String> {
    res.headers()
        .get("x-elyra-error-kind")
        .map(|v| v.to_str().unwrap().to_owned())
}

/// Wait for the window's next value on `channel` that satisfies `check`.
async fn next_on<T: serde::de::DeserializeOwned>(
    bus: &EventBus,
    window: &str,
    channel: &str,
    check: impl Fn(&T) -> bool,
) -> T {
    let wait = async {
        loop {
            let batch: Vec<(String, rmpv::Value)> =
                rmp_serde::from_slice(&bus.next_batch_for(window).await).unwrap();
            for (name, value) in batch {
                if name != channel {
                    continue;
                }
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).unwrap();
                let Ok(seen) = rmp_serde::from_slice::<T>(&buf) else {
                    continue;
                };
                if check(&seen) {
                    return seen;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .unwrap_or_else(|_| panic!("a value on {channel} for {window}"))
}

async fn joined(rig: &Rig, window: &str, check: impl Fn(&Joined) -> bool) -> Joined {
    next_on(&rig.bus, window, CHANNEL, check).await
}

async fn heard(rig: &Rig, window: &str, check: impl Fn(&Heard) -> bool) -> Heard {
    next_on(&rig.bus, window, "elyra:backend", check).await
}

/// The channels of everything waiting for `window` now.
async fn waiting_for(bus: &EventBus, window: &str) -> Vec<String> {
    let mut names = Vec::new();
    while bus.pending_for(window) > 0 {
        let batch: Vec<(String, rmpv::Value)> =
            rmp_serde::from_slice(&bus.next_batch_for(window).await).unwrap();
        names.extend(batch.into_iter().map(|(name, _)| name));
    }
    names
}

fn subscriptions(pusher: &PusherFake, channel: &str) -> usize {
    pusher
        .subscriptions()
        .iter()
        .filter(|s| s.channel == channel)
        .count()
}

fn unsubscribes(pusher: &PusherFake, channel: &str) -> usize {
    pusher
        .received()
        .iter()
        .filter(|f| f["event"] == "pusher:unsubscribe" && f["data"]["channel"] == channel)
        .count()
}

#[tokio::test]
async fn a_window_listens_signed_by_laravel_and_hears_the_runs_events() {
    let fake = laravel();
    let rig = rig_with(&fake, |b| b.listen(["task.*"])).await;
    let res = listen(&rig, "w1", RUN).await;
    let first: Joined = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(first.channel, RUN);
    let state = joined(&rig, "w1", |s| s.joined).await;
    assert_eq!(state.channel, RUN);

    // Signed by Laravel's broadcasting auth, as the user, for this socket.
    let sub = rig
        .pusher
        .subscriptions()
        .into_iter()
        .find(|s| s.channel == RUN_WIRE)
        .unwrap();
    assert_eq!(sub.auth.as_deref(), Some("fake-key:signed-by-laravel"));
    assert_eq!(sub.channel_data, None);
    let sent = fake.assert_sent("POST", &url("/api/broadcasting/auth"));
    let body = sent.body.as_ref().unwrap();
    assert_eq!(body["channel_name"], RUN_WIRE);
    assert_eq!(body["socket_id"], sub.socket_id.as_str());
    assert_eq!(sent.header("authorization"), Some("Bearer 7|s3cr3t"));

    rig.pusher.broadcast(
        RUN_WIRE,
        "RunOutput",
        json!({ "chunk": "Reading the issue…", "seq": 1 }),
    );
    let event = heard(&rig, "w1", |e| e.event == "RunOutput").await;
    assert_eq!(
        event,
        Heard {
            channel: RUN.into(),
            event: "RunOutput".into(),
            data: json!({ "chunk": "Reading the issue…", "seq": 1 }),
        }
    );
    // Only the window listening hears it.
    let other = waiting_for(&rig.bus, "w2").await;
    assert!(
        !other.iter().any(|c| c == "elyra:backend" || c == CHANNEL),
        "{other:?}"
    );
}

#[tokio::test]
async fn a_channel_the_app_doesnt_list_is_refused_before_anything_is_sent() {
    let rig = rig().await;
    for channel in ["invoices.1", "task", "task.4 2", "private-task.1"] {
        let res = listen(&rig, "w1", channel).await;
        assert_eq!(kind(&res).as_deref(), Some("denied"), "{channel}");
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(rig.pusher.subscriptions().is_empty());
    assert!(rig.ctx.get::<Listening>().channels().is_empty());
}

#[tokio::test]
async fn two_windows_share_one_subscription_and_the_last_one_unsubscribes() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    listen(&rig, "w2", RUN).await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w2", |s| s.joined).await;
    joined(&rig, "w1", |s| s.joined).await;
    assert_eq!(subscriptions(&rig.pusher, RUN_WIRE), 1);

    // Both hear an event.
    rig.pusher
        .broadcast(RUN_WIRE, "RunStatusChanged", json!({ "status": "running" }));
    heard(&rig, "w1", |e| e.event == "RunStatusChanged").await;
    heard(&rig, "w2", |e| e.event == "RunStatusChanged").await;

    assert!(unlisten(&rig, "w1", RUN).await);
    assert!(!unlisten(&rig, "w1", RUN).await, "already stopped");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(unsubscribes(&rig.pusher, RUN_WIRE), 0, "w2 still listens");

    assert!(unlisten(&rig, "w2", RUN).await);
    rig.pusher
        .wait_until(|p| unsubscribes(p, RUN_WIRE) == 1, "the unsubscribe")
        .await;
    assert!(rig.ctx.get::<Listening>().channels().is_empty());
}

#[tokio::test]
async fn a_lost_connection_unjoins_and_the_next_one_subscribes_again() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;

    rig.pusher.disconnect_all();
    joined(&rig, "w1", |s| !s.joined).await;
    joined(&rig, "w1", |s| s.joined).await;
    assert_eq!(subscriptions(&rig.pusher, RUN_WIRE), 2);

    // Heard on the new connection.
    rig.pusher
        .broadcast(RUN_WIRE, "RunStatusChanged", json!({ "status": "done" }));
    let event = heard(&rig, "w1", |e| e.event == "RunStatusChanged").await;
    assert_eq!(event.data["status"], "done");
}

#[tokio::test]
async fn nothing_is_joined_while_signed_out_and_a_sign_in_joins_it() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;

    rig.ctx.get::<Auth>().sign_out().await.unwrap();
    joined(&rig, "w1", |s| !s.joined).await;
    listen(&rig, "w1", "task.2").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(subscriptions(&rig.pusher, "private-task.2"), 0);
    assert!(!rig.ctx.get::<Listening>().joined("task.2"));

    rig.ctx
        .get::<Auth>()
        .sign_in("grete@example.com", "secret")
        .await
        .unwrap();
    joined(&rig, "w1", |s| s.channel == "task.2" && s.joined).await;
    assert_eq!(subscriptions(&rig.pusher, RUN_WIRE), 2);
}

#[tokio::test]
async fn another_workspaces_token_subscribes_again_with_it() {
    let fake = laravel();
    let rig = rig_with(&fake, |b| b.listen(["task.*"])).await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;

    // The same user, another workspace's token: a switch.
    rig.ctx
        .get::<Auth>()
        .sign_in_with_token(BASE, "8|w0rkspace", json!({ "id": 7, "name": "Grete" }))
        .await
        .unwrap();
    rig.pusher
        .wait_until(
            |p| subscriptions(p, RUN_WIRE) == 2,
            "the second subscription",
        )
        .await;
    let authorized = fake.sent_to("POST", &url("/api/broadcasting/auth"));
    assert_eq!(
        authorized.last().unwrap().header("authorization"),
        Some("Bearer 8|w0rkspace")
    );
    rig.pusher
        .broadcast(RUN_WIRE, "RunOutput", json!({ "chunk": "again" }));
    heard(&rig, "w1", |e| e.data["chunk"] == "again").await;
}

#[tokio::test]
async fn laravel_saying_no_leaves_it_unjoined_and_the_connection_up() {
    let fake = laravel().post(
        url("/api/broadcasting/auth"),
        403,
        json!({ "message": "This action is unauthorized." }),
    );
    let rig = rig_with(&fake, |b| b.listen(["task.*"])).await;
    listen(&rig, "w1", RUN).await;
    let refused = joined(&rig, "w1", |s| s.channel == RUN).await;
    assert!(!refused.joined);
    assert_eq!(subscriptions(&rig.pusher, RUN_WIRE), 0);

    // Another channel, allowed: the same connection takes it.
    fake.clone().post(
        url("/api/broadcasting/auth"),
        200,
        json!({ "auth": "fake-key:signed-by-laravel" }),
    );
    listen(&rig, "w1", "task.2").await;
    joined(&rig, "w1", |s| s.channel == "task.2" && s.joined).await;
    assert!(!rig.ctx.get::<Listening>().joined(RUN));
    assert_eq!(rig.ctx.get::<Broadcaster>().state(), State::Connected);
    assert_eq!(rig.pusher.connections(), 1);
}

#[tokio::test]
async fn an_event_on_a_channel_nobody_listens_to_reaches_nobody() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;
    // A misbehaving server sends another run's events to everyone.
    rig.pusher
        .send_to_all("private-task.other", "RunOutput", json!({ "chunk": "no" }));
    rig.pusher
        .broadcast(RUN_WIRE, "RunOutput", json!({ "chunk": "yes" }));
    let event = heard(&rig, "w1", |e| e.event == "RunOutput").await;
    assert_eq!(event.data["chunk"], "yes");
    let other = waiting_for(&rig.bus, "w2").await;
    assert!(!other.iter().any(|c| c == "elyra:backend"), "{other:?}");
}

#[tokio::test]
async fn a_window_that_stopped_polling_is_let_go_of() {
    let rig = rig().await;
    // "gone" never polls: its window closed.
    listen(&rig, "gone", "task.7").await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;
    rig.pusher
        .wait_until(
            |p| subscriptions(p, "private-task.7") == 1,
            "gone's subscription",
        )
        .await;

    let listening = rig.ctx.get::<Listening>();
    listening.sweep(Duration::from_secs(60));
    assert_eq!(listening.channels(), [RUN, "task.7"], "within the grace");
    listening.sweep(Duration::ZERO);
    assert_eq!(listening.channels(), [RUN]);
    rig.pusher
        .wait_until(
            |p| unsubscribes(p, "private-task.7") == 1,
            "gone's channel left",
        )
        .await;
}

#[tokio::test]
async fn a_channel_the_app_follows_isnt_subscribed_twice_nor_left() {
    let rig = rig_with(&laravel(), |b| {
        b.follow("task.1", "/api/tasks").listen(["task.*"])
    })
    .await;
    rig.pusher
        .wait_until(|p| subscriptions(p, "private-task.1") == 1, "the follow")
        .await;
    listen(&rig, "w1", "task.1").await;
    joined(&rig, "w1", |s| s.joined).await;
    rig.pusher
        .broadcast("private-task.1", "RunOutput", json!({ "chunk": "once" }));
    heard(&rig, "w1", |e| e.event == "RunOutput").await;
    unlisten(&rig, "w1", "task.1").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(subscriptions(&rig.pusher, "private-task.1"), 1);
    assert_eq!(unsubscribes(&rig.pusher, "private-task.1"), 0);
}

#[tokio::test]
async fn an_app_without_listen_says_so() {
    let rig = rig_with(&laravel(), |b| b).await;
    let res = listen(&rig, "w1", RUN).await;
    assert!(
        String::from_utf8_lossy(res.body()).contains("Backend::listen"),
        "{}",
        String::from_utf8_lossy(res.body())
    );
}

#[test]
#[should_panic(expected = "needs broadcasting")]
fn listen_without_broadcasting_is_a_mistake_found_at_start() {
    App::new()
        .backend(Backend::new(BASE).listen(["task.*"]))
        .prepare();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn unplugged_by_the_inspector_nothing_is_joined_and_plugged_in_it_rejoins() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    joined(&rig, "w1", |s| s.joined).await;

    #[derive(serde::Serialize)]
    struct Unplug {
        on: bool,
    }
    let unplug = |on| {
        post(
            &rig.shell,
            "w1",
            "/__inspector/offline",
            rmp_serde::to_vec_named(&Unplug { on }).unwrap(),
        )
    };
    unplug(true).await;
    joined(&rig, "w1", |s| !s.joined).await;
    unplug(false).await;
    joined(&rig, "w1", |s| s.joined).await;
}

#[cfg(debug_assertions)]
#[derive(Deserialize, Debug)]
struct Inspected {
    enabled: bool,
    channels: Vec<InspectedChannel>,
}

#[cfg(debug_assertions)]
#[derive(Deserialize, Debug, PartialEq)]
struct InspectedChannel {
    channel: String,
    joined: bool,
    windows: usize,
    here: bool,
}

#[cfg(debug_assertions)]
async fn inspect(rig: &Rig, window: &str) -> Inspected {
    let res = post(&rig.shell, window, "/__inspector/listening", Vec::new()).await;
    rmp_serde::from_slice(res.body())
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(res.body())))
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn the_inspector_lists_each_listened_channel_and_whose_it_is() {
    let rig = rig().await;
    listen(&rig, "w1", RUN).await;
    listen(&rig, "w2", RUN).await;
    listen(&rig, "w2", "task.2").await;
    joined(&rig, "w2", |s| s.channel == "task.2" && s.joined).await;
    joined(&rig, "w1", |s| s.channel == RUN && s.joined).await;

    let seen = inspect(&rig, "w1").await;
    assert!(seen.enabled);
    assert_eq!(
        seen.channels,
        [
            InspectedChannel {
                channel: RUN.into(),
                joined: true,
                windows: 2,
                here: true,
            },
            InspectedChannel {
                channel: "task.2".into(),
                joined: true,
                windows: 1,
                here: false,
            },
        ]
    );
    assert!(inspect(&rig, "w2").await.channels.iter().all(|c| c.here));

    let without = rig_with(&laravel(), |b| b).await;
    let seen = inspect(&without, "w1").await;
    assert!(!seen.enabled);
    assert!(seen.channels.is_empty());
}
