//! RFC 0015 against a real Laravel app and Reverb: Ada has a customer open in
//! the desktop app, Bob opens it in a browser tab (a plain Pusher client, as
//! Echo is), and each sees the other — joined through Laravel's own channel
//! rule, `customers.{id}` (scripts/laravel-backend.sh). A customer the rule
//! refuses leaves the window unjoined. Skipped unless `ELYRA_LARAVEL_URL`
//! and `ELYRA_PUSHER` (`<key>@<host>:<port>`) say where they are; CI starts
//! them.
#![cfg(feature = "backend")]

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, Broadcasting, State};
use elyra::event::EventBus;
use elyra::http::Http;
use elyra::presence::CHANNEL;
use elyra::testing::TestShell;
use elyra::App;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;
use wry::http::{Request, Response};

#[derive(Deserialize, Debug, Clone)]
struct Member {
    id: String,
    info: Value,
}

#[derive(Deserialize, Debug, Clone)]
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

async fn post(shell: &TestShell, path: &str, channel: &str) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "w1")
        .body(rmp_serde::to_vec_named(&One { channel }).unwrap())
        .unwrap();
    shell.handle(request).await
}

/// The window's next `elyra:presence` for `channel` that satisfies `check`.
async fn heard(bus: &EventBus, channel: &str, check: impl Fn(&Seen) -> bool) -> Seen {
    let wait = async {
        loop {
            let batch: Vec<(String, rmpv::Value)> =
                rmp_serde::from_slice(&bus.next_batch_for("w1").await).unwrap();
            for (name, value) in batch {
                if name != CHANNEL {
                    continue;
                }
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).unwrap();
                let state: Seen = rmp_serde::from_slice(&buf).unwrap();
                if state.channel == channel && check(&state) {
                    return state;
                }
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .expect("an elyra:presence event")
}

fn ids(state: &Seen) -> Vec<&str> {
    state.members.iter().map(|m| m.id.as_str()).collect()
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A browser tab on Reverb, joined the way Echo joins: the socket's id,
/// Laravel's signature for it, then `pusher:subscribe`.
struct Tab {
    socket: Socket,
    /// Who was there when it joined, by id.
    here: Vec<String>,
}

impl Tab {
    async fn open(url: &str, key: &str, address: &str, token: &str, channel: &str) -> Tab {
        let (mut socket, _) = tokio_tungstenite::connect_async(format!(
            "ws://{address}/app/{key}?protocol=7&client=js&version=8.4.0"
        ))
        .await
        .expect("Reverb answers");
        let established = next(&mut socket, "pusher:connection_established").await;
        let socket_id = established["socket_id"].as_str().unwrap().to_owned();
        let wire = format!("presence-{channel}");
        let auth: Value = Http::new()
            .post(format!("{url}/api/broadcasting/auth"))
            .token(token)
            .json(&json!({ "socket_id": socket_id, "channel_name": wire }))
            .send()
            .await
            .unwrap()
            .json()
            .expect("Laravel signs the tab in");
        let subscribe = json!({ "event": "pusher:subscribe", "data": {
            "channel": wire, "auth": auth["auth"], "channel_data": auth["channel_data"],
        }});
        socket
            .send(Message::Text(subscribe.to_string().into()))
            .await
            .unwrap();
        let joined = next(&mut socket, "pusher_internal:subscription_succeeded").await;
        let here = joined["presence"]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(id)
            .collect();
        Tab { socket, here }
    }

    async fn close(mut self) {
        self.socket.close(None).await.unwrap();
    }
}

fn id(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// The socket's next `event`, its data parsed (Pusher sends it as a string).
async fn next(socket: &mut Socket, event: &str) -> Value {
    let wait = async {
        while let Some(message) = socket.next().await {
            let Message::Text(text) = message.expect("the socket stays up") else {
                continue;
            };
            let frame: Value = serde_json::from_str(&text).unwrap();
            assert_ne!(frame["event"], "pusher:error", "{frame}");
            if frame["event"] == event {
                return match &frame["data"] {
                    Value::String(s) => serde_json::from_str(s).unwrap(),
                    other => other.clone(),
                };
            }
        }
        panic!("the socket closed before {event}");
    };
    tokio::time::timeout(Duration::from_secs(10), wait)
        .await
        .unwrap_or_else(|_| panic!("no {event}"))
}

async fn token(url: &str, email: &str) -> String {
    Http::new()
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": email, "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text()
        .trim()
        .to_owned()
}

async fn user_id(url: &str, token: &str) -> String {
    let user: Value = Http::new()
        .get(format!("{url}/api/user"))
        .token(token)
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();
    id(&user["id"])
}

#[tokio::test]
async fn the_desktop_and_a_browser_tab_see_each_other_on_a_customer() {
    let (Ok(url), Ok(pusher)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_PUSHER"),
    ) else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL and ELYRA_PUSHER=<key>@<host>:<port>");
        return;
    };
    let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
    let (host, port) = address.rsplit_once(':').expect("<host>:<port>");

    // A customer to have open, made by Ada on the web.
    let (ada_web, bob_web) = (
        token(&url, "ada@example.com").await,
        token(&url, "bob@example.com").await,
    );
    let (ada, bob) = (user_id(&url, &ada_web).await, user_id(&url, &bob_web).await);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let customer: Value = Http::new()
        .post(format!("{url}/api/customers"))
        .token(&ada_web)
        .json(&json!({ "name": format!("Open {stamp}"), "email": format!("open{stamp}@example.com") }))
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();
    let channel = format!("customers.{}", customer["id"]);

    // Ada, in the desktop app, opens it.
    let prepared = App::new()
        .backend(
            Backend::new(&url)
                .broadcasting(
                    Broadcasting::pusher(key)
                        .host(host)
                        .port(port.parse().expect("a port"))
                        .scheme("ws"),
                )
                .presence(["customers.*"]),
        )
        .token_store(Arc::new(MemoryTokens::new()))
        .prepare();
    let (ctx, bus) = (prepared.ctx.clone(), prepared.bus.clone());
    bus.register_client("w1");
    let shell = TestShell::new(prepared);
    ctx.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");
    let broadcaster = ctx.get::<Broadcaster>();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while broadcaster.state() != State::Connected {
        assert!(tokio::time::Instant::now() < deadline, "never connected");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    post(&shell, "/__presence/join", &channel).await;
    let state = heard(&bus, &channel, |s| s.joined).await;
    assert_eq!(ids(&state), [ada.as_str()], "Laravel let her in");
    assert_eq!(state.me.as_deref(), Some(ada.as_str()));
    assert_eq!(state.members[0].info["name"], "Ada", "the rule's info");

    // Bob opens it in a browser: he sees Ada, and she sees him.
    let tab = Tab::open(&url, key, address, &bob_web, &channel).await;
    assert!(tab.here.contains(&ada), "{:?}", tab.here);
    let state = heard(&bus, &channel, |s| s.members.len() == 2).await;
    let bob_there = state.members.iter().find(|m| m.id == bob).expect("Bob");
    assert_eq!(bob_there.info["name"], "Bob");

    // He closes the tab: she sees him go.
    tab.close().await;
    let state = heard(&bus, &channel, |s| s.members.len() == 1).await;
    assert_eq!(ids(&state), [ada.as_str()]);

    // He opens it again, and she closes it: he sees her go.
    let mut tab = Tab::open(&url, key, address, &bob_web, &channel).await;
    assert!(tab.here.contains(&ada), "{:?}", tab.here);
    post(&shell, "/__presence/leave", &channel).await;
    let gone = next(&mut tab.socket, "pusher_internal:member_removed").await;
    assert_eq!(id(&gone["user_id"]), ada);
    tab.close().await;

    // A customer that isn't there: Laravel's rule says no (a refusal is the
    // only event a join sends without joining), the window stays unjoined,
    // and the same connection takes the next channel.
    post(&shell, "/__presence/join", "customers.0").await;
    let refused = heard(&bus, "customers.0", |_| true).await;
    assert!(!refused.joined && refused.members.is_empty());
    post(&shell, "/__presence/join", &channel).await;
    let state = heard(&bus, &channel, |s| s.joined).await;
    assert_eq!(ids(&state), [ada.as_str()]);
    assert_eq!(broadcaster.state(), State::Connected);

    // Tidy up, and sign out.
    Http::new()
        .delete(format!("{url}/api/customers/{}", customer["id"]))
        .token(&ada_web)
        .send()
        .await
        .unwrap();
    ctx.get::<Auth>().sign_out().await.unwrap();
}
