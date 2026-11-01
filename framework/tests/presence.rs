//! Presence (RFC 0015): windows join Laravel presence channels through the
//! shell, the app joins each once on the open connection, and the members
//! (from the server, and from a browser tab joining elsewhere) reach the
//! windows that joined. A `PusherFake` stands in for Reverb, an `HttpFake`
//! for Laravel's channel auth.
#![cfg(feature = "backend")]

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, PusherFake, State};
use elyra::event::EventBus;
use elyra::http::{Http, HttpFake};
use elyra::presence::{Presence, CHANNEL};
use elyra::testing::TestShell;
use elyra::{App, Ctx};
use serde::Deserialize;
use serde_json::{json, Value};
use wry::http::{Request, Response};

const BASE: &str = "https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

/// Laravel: Grete (7) signs in, and the channel rule lets her in.
fn laravel() -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 7, "name": "Grete" }))
        .delete(url("/api/sanctum/token"), 204, Value::Null)
        .post(
            url("/api/broadcasting/auth"),
            200,
            json!({
                "auth": "fake-key:signed-by-laravel",
                "channel_data": json!({ "user_id": 7, "user_info": { "name": "Grete" } }).to_string(),
            }),
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
    rig_with(&laravel(), |b| b.presence(["customers.*"])).await
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

#[derive(Deserialize, Debug, Clone, PartialEq)]
struct Member {
    id: String,
    info: Value,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
struct Seen {
    channel: String,
    joined: bool,
    members: Vec<Member>,
    me: Option<String>,
}

#[derive(serde::Serialize)]
struct One<'a> {
    channel: &'a str,
}

async fn join(rig: &Rig, window: &str, channel: &str) -> Response<Cow<'static, [u8]>> {
    let body = rmp_serde::to_vec_named(&One { channel }).unwrap();
    post(&rig.shell, window, "/__presence/join", body).await
}

async fn leave(rig: &Rig, window: &str, channel: &str) -> bool {
    let body = rmp_serde::to_vec_named(&One { channel }).unwrap();
    let res = post(&rig.shell, window, "/__presence/leave", body).await;
    rmp_serde::from_slice(res.body()).unwrap()
}

fn kind(res: &Response<Cow<'static, [u8]>>) -> Option<String> {
    res.headers()
        .get("x-elyra-error-kind")
        .map(|v| v.to_str().unwrap().to_owned())
}

/// Wait for the window's next `elyra:presence` that satisfies `check`.
async fn heard(bus: &EventBus, window: &str, check: impl Fn(&Seen) -> bool) -> Seen {
    let wait = async {
        loop {
            let batch: Vec<(String, rmpv::Value)> =
                rmp_serde::from_slice(&bus.next_batch_for(window).await).unwrap();
            for (channel, value) in batch {
                if channel != CHANNEL {
                    continue;
                }
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).unwrap();
                let state: Seen = rmp_serde::from_slice(&buf).unwrap();
                if check(&state) {
                    return state;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(5), wait)
        .await
        .expect("an elyra:presence event")
}

fn ids(state: &Seen) -> Vec<&str> {
    state.members.iter().map(|m| m.id.as_str()).collect()
}

fn presence_subscriptions(pusher: &PusherFake, channel: &str) -> usize {
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
async fn a_window_joins_signed_by_laravel_and_hears_who_is_there() {
    let rig = rig().await;
    let res = join(&rig, "w1", "customers.42").await;
    let first: Seen = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(first.channel, "customers.42");

    let state = heard(&rig.bus, "w1", |s| s.joined).await;
    assert_eq!(ids(&state), ["7"]);
    assert_eq!(state.members[0].info, json!({ "name": "Grete" }));
    assert_eq!(state.me.as_deref(), Some("7"));

    // Signed by Laravel, for this socket, with what Laravel said she is.
    let sub = rig
        .pusher
        .subscriptions()
        .into_iter()
        .find(|s| s.channel == "presence-customers.42")
        .unwrap();
    assert_eq!(sub.auth.as_deref(), Some("fake-key:signed-by-laravel"));
    let data: Value = serde_json::from_str(sub.channel_data.as_deref().unwrap()).unwrap();
    assert_eq!(data["user_id"], 7);
}

#[tokio::test]
async fn the_channel_auth_names_the_presence_channel_and_the_socket() {
    let fake = laravel();
    let rig = rig_with(&fake, |b| b.presence(["customers.*"])).await;
    join(&rig, "w1", "customers.42").await;
    heard(&rig.bus, "w1", |s| s.joined).await;
    let sent = fake.assert_sent("POST", &url("/api/broadcasting/auth"));
    let body = sent.body.as_ref().unwrap();
    assert_eq!(body["channel_name"], "presence-customers.42");
    let socket = &rig.pusher.subscriptions()[0].socket_id;
    assert_eq!(body["socket_id"], socket.as_str());
    assert_eq!(sent.header("authorization"), Some("Bearer 7|s3cr3t"));
}

#[tokio::test]
async fn someone_on_the_web_comes_and_goes() {
    let rig = rig().await;
    join(&rig, "w1", "customers.42").await;
    heard(&rig.bus, "w1", |s| s.joined).await;

    rig.pusher
        .join_as("presence-customers.42", "9", json!({ "name": "Ada" }));
    let state = heard(&rig.bus, "w1", |s| s.members.len() == 2).await;
    assert_eq!(ids(&state), ["7", "9"]);
    assert_eq!(state.members[1].info, json!({ "name": "Ada" }));

    rig.pusher.leave_as("presence-customers.42", "9");
    let state = heard(&rig.bus, "w1", |s| s.members.len() == 1).await;
    assert_eq!(ids(&state), ["7"]);
    assert_eq!(rig.ctx.get::<Presence>().members("customers.42")[0].id, "7");
}

#[tokio::test]
async fn a_channel_is_joined_once_and_left_by_the_last_window() {
    let rig = rig().await;
    join(&rig, "w1", "customers.42").await;
    join(&rig, "w2", "customers.42").await;
    heard(&rig.bus, "w2", |s| s.joined).await;
    assert_eq!(
        presence_subscriptions(&rig.pusher, "presence-customers.42"),
        1
    );
    // Both windows hear it.
    heard(&rig.bus, "w1", |s| s.joined).await;

    assert!(leave(&rig, "w1", "customers.42").await);
    assert!(!leave(&rig, "w1", "customers.42").await, "already left");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(unsubscribes(&rig.pusher, "presence-customers.42"), 0);

    assert!(leave(&rig, "w2", "customers.42").await);
    rig.pusher
        .wait_until(
            |p| unsubscribes(p, "presence-customers.42") == 1,
            "the unsubscribe",
        )
        .await;
    rig.pusher
        .wait_until(
            |p| p.members("presence-customers.42").is_empty(),
            "nobody there",
        )
        .await;
}

#[tokio::test]
async fn a_channel_the_app_doesnt_list_is_refused_before_anything_is_sent() {
    let rig = rig().await;
    for channel in ["invoices.1", "customers", "customers.4 2"] {
        let res = join(&rig, "w1", channel).await;
        assert_eq!(kind(&res).as_deref(), Some("denied"), "{channel}");
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(rig
        .pusher
        .subscriptions()
        .iter()
        .all(|s| !s.channel.starts_with("presence-")));
}

#[tokio::test]
async fn a_lost_connection_forgets_the_members_and_the_next_one_joins_again() {
    let rig = rig().await;
    join(&rig, "w1", "customers.42").await;
    heard(&rig.bus, "w1", |s| s.joined).await;

    rig.pusher.disconnect_all();
    let state = heard(&rig.bus, "w1", |s| !s.joined).await;
    assert!(
        state.members.is_empty(),
        "nobody shown rather than someone stale"
    );

    let state = heard(&rig.bus, "w1", |s| s.joined).await;
    assert_eq!(ids(&state), ["7"]);
    assert_eq!(
        presence_subscriptions(&rig.pusher, "presence-customers.42"),
        2
    );
}

#[tokio::test]
async fn a_join_while_signed_out_waits_for_the_sign_in() {
    let rig = rig().await;
    rig.ctx.get::<Auth>().sign_out().await.unwrap();
    join(&rig, "w1", "customers.42").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        presence_subscriptions(&rig.pusher, "presence-customers.42"),
        0
    );

    rig.ctx
        .get::<Auth>()
        .sign_in("grete@example.com", "secret")
        .await
        .unwrap();
    let state = heard(&rig.bus, "w1", |s| s.joined).await;
    assert_eq!(ids(&state), ["7"]);
}

#[tokio::test]
async fn laravel_saying_no_leaves_it_unjoined_and_the_connection_up() {
    let fake = laravel().post(
        url("/api/broadcasting/auth"),
        403,
        json!({ "message": "This action is unauthorized." }),
    );
    let rig = rig_with(&fake, |b| b.presence(["customers.*"])).await;
    join(&rig, "w1", "customers.42").await;
    // Refused: the window hears it isn't joined.
    let refused = heard(&rig.bus, "w1", |s| s.channel == "customers.42").await;
    assert!(!refused.joined);
    fake.clone().post(
        url("/api/broadcasting/auth"),
        200,
        json!({
            "auth": "fake-key:signed-by-laravel",
            "channel_data": json!({ "user_id": 7, "user_info": { "name": "Grete" } }).to_string(),
        }),
    );
    // Another channel, allowed: the same connection takes it.
    join(&rig, "w1", "customers.43").await;
    let state = heard(&rig.bus, "w1", |s| s.channel == "customers.43" && s.joined).await;
    assert_eq!(ids(&state), ["7"]);
    let refused = rig.ctx.get::<Presence>().state("customers.42");
    assert!(!refused.joined && refused.members.is_empty());
    assert_eq!(
        presence_subscriptions(&rig.pusher, "presence-customers.42"),
        0
    );
    assert_eq!(rig.ctx.get::<Broadcaster>().state(), State::Connected);
}

#[tokio::test]
async fn a_window_that_stopped_polling_is_let_go_of() {
    let rig = rig().await;
    // "gone" never polls: its window closed.
    join(&rig, "gone", "customers.7").await;
    join(&rig, "w1", "customers.42").await;
    heard(&rig.bus, "w1", |s| s.joined).await;
    rig.pusher
        .wait_until(
            |p| presence_subscriptions(p, "presence-customers.7") == 1,
            "gone's join",
        )
        .await;

    let presence = rig.ctx.get::<Presence>();
    presence.sweep(Duration::from_secs(60));
    assert_eq!(
        presence.channels(),
        ["customers.42", "customers.7"],
        "within the grace"
    );
    presence.sweep(Duration::ZERO);
    assert_eq!(presence.channels(), ["customers.42"]);
    rig.pusher
        .wait_until(
            |p| unsubscribes(p, "presence-customers.7") == 1,
            "gone's channel left",
        )
        .await;
}

#[tokio::test]
async fn an_app_without_presence_says_so() {
    let fake = laravel();
    let rig = rig_with(&fake, |b| b).await;
    let res = join(&rig, "w1", "customers.42").await;
    assert!(
        String::from_utf8_lossy(res.body()).contains("Backend::presence"),
        "{}",
        String::from_utf8_lossy(res.body())
    );
}

#[test]
#[should_panic(expected = "needs broadcasting")]
fn presence_without_broadcasting_is_a_mistake_found_at_start() {
    App::new()
        .backend(Backend::new(BASE).presence(["customers.*"]))
        .prepare();
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn unplugged_by_the_inspector_nobody_is_shown_and_plugged_in_it_rejoins() {
    let rig = rig().await;
    join(&rig, "w1", "customers.42").await;
    heard(&rig.bus, "w1", |s| s.joined).await;

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
    heard(&rig.bus, "w1", |s| !s.joined).await;
    unplug(false).await;
    heard(&rig.bus, "w1", |s| s.joined).await;
}
