//! Laravel's broadcasting, heard on the desktop (RFC 0006): the Pusher
//! protocol's WebSocket — what Askr (`--pusher`), Reverb and Pusher speak —
//! as Laravel Echo uses it in a browser.
//!
//! ```ignore
//! Backend::new("https://crm.example.com")
//!     .broadcasting(Broadcasting::pusher("app-key"))
//!     .follow("customers", "/api/customers")   // private-customers
//! ```
//!
//! - It connects once the user is signed in, and closes on sign-out.
//! - A private channel is authorized by the backend (`/api/broadcasting/auth`,
//!   as the signed-in user); the app secret never leaves the server.
//! - It answers pings, pings a quiet line, and reconnects with backoff.
//! - While connected, `Backend` sends `X-Socket-ID`, so Laravel's
//!   `->toOthers()` skips this app.
//! - Each event on a followed channel is a [`BackendEvent`] (a domain event)
//!   and goes to the windows on `elyra:backend`.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use crate::backend::Backend;
use crate::container::{Ctx, WeakCtx};
use crate::event::EventBus;

/// The channel followed channels' events reach the windows on.
pub const CHANNEL: &str = "elyra:backend";

/// The channel the connection's state reaches the windows on.
pub const STATE_CHANNEL: &str = "elyra:broadcasting";

/// The longest wait between reconnects.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// How long a ping may go unanswered before the line counts as dead.
const PONG_TIMEOUT: Duration = Duration::from_secs(30);

/// Where the Pusher-protocol server is, and how to reach it.
#[derive(Clone, Debug)]
pub struct Broadcasting {
    key: String,
    host: Option<String>,
    port: Option<u16>,
    scheme: Option<String>,
    auth_route: String,
}

impl Broadcasting {
    /// The Pusher protocol with app key `key`, on the backend's own host — as
    /// Askr serves it. `.host()` / `.port()` / `.scheme()` point elsewhere
    /// (Reverb's port, Pusher's cluster host).
    pub fn pusher(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            host: None,
            port: None,
            scheme: None,
            auth_route: "/api/broadcasting/auth".into(),
        }
    }

    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    pub fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// `wss` (the default) or `ws` — `ws` only to loopback.
    pub fn scheme(mut self, scheme: impl Into<String>) -> Self {
        self.scheme = Some(scheme.into());
        self
    }

    /// Where private channels are authorized; `/api/broadcasting/auth`, as
    /// Laravel puts it with Sanctum (`withBroadcasting(…, ['prefix' => 'api'])`).
    pub fn auth_route(mut self, path: impl Into<String>) -> Self {
        self.auth_route = path.into();
        self
    }

    /// The socket's URL, from the backend's when not given.
    fn url(&self, backend_base: &str) -> Result<String, String> {
        let base = reqwest::Url::parse(backend_base)
            .map_err(|_| format!("the backend's URL `{backend_base}` doesn't parse"))?;
        let host = self
            .host
            .clone()
            .or_else(|| base.host_str().map(str::to_owned))
            .ok_or("no host to connect to")?;
        let scheme = self.scheme.clone().unwrap_or_else(|| {
            if base.scheme() == "http" {
                "ws".into()
            } else {
                "wss".into()
            }
        });
        let loopback = host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if scheme != "wss" && !(scheme == "ws" && loopback) {
            return Err(format!(
                "refusing `{scheme}://{host}`: only wss, except to loopback"
            ));
        }
        let port = self
            .port
            .or_else(|| base.port())
            .map(|p| format!(":{p}"))
            .unwrap_or_default();
        Ok(format!(
            "{scheme}://{host}{port}/app/{}?protocol=7&client=elyra&version={}&flash=false",
            self.key,
            env!("CARGO_PKG_VERSION")
        ))
    }
}

/// A channel the app listens to, and the resource its events change.
#[derive(Clone, Debug, PartialEq)]
pub struct Follow {
    /// The name the app uses (`customers`), and on `elyra:backend`.
    pub name: String,
    /// The name on the wire (`private-customers`).
    pub wire: String,
    /// The backend path whose live queries its events re-run (RFC 0006 step 2).
    pub resource: Option<String>,
}

impl Follow {
    /// Laravel's `PrivateChannel('customers')` — `private-customers`.
    pub(crate) fn private(name: &str, resource: Option<String>) -> Self {
        let name = name.trim_start_matches("private-");
        Self {
            name: name.to_owned(),
            wire: format!("private-{name}"),
            resource,
        }
    }

    /// Laravel's `Channel('news')` — anyone may listen.
    pub(crate) fn public(name: &str, resource: Option<String>) -> Self {
        Self {
            name: name.to_owned(),
            wire: name.to_owned(),
            resource,
        }
    }

    fn is_private(&self) -> bool {
        self.wire.starts_with("private-")
    }
}

/// An event a followed channel carried — dispatched as a domain event:
/// `App::listen(|e: BackendEvent, ctx| …)`. The data is the server's, as sent:
/// untrusted, like any input.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
pub struct BackendEvent {
    /// The followed channel's name (`customers`, not `private-customers`).
    pub channel: String,
    /// Laravel's event name: `CustomerUpdated` (model broadcasting), the
    /// event's class, or its `broadcastAs`.
    pub event: String,
    pub data: Value,
}

/// The connection, as `elyra:broadcasting` reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Connecting,
    Connected,
    Offline,
}

/// The listener: one connection at a time, run while the user is signed in.
#[derive(Clone)]
pub struct Broadcaster {
    inner: Arc<Inner>,
}

struct Inner {
    config: Broadcasting,
    follows: Vec<Follow>,
    backend: Backend,
    bus: Option<EventBus>,
    ctx: OnceLock<WeakCtx>,
    task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    state: Mutex<State>,
    /// Whether a connection was ever up: the next one may have missed events.
    connected_before: std::sync::atomic::AtomicBool,
}

impl Broadcaster {
    pub(crate) fn new(
        config: Broadcasting,
        follows: Vec<Follow>,
        backend: Backend,
        bus: Option<EventBus>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                config,
                follows,
                backend,
                bus,
                ctx: OnceLock::new(),
                task: Mutex::new(None),
                state: Mutex::new(State::Offline),
                connected_before: std::sync::atomic::AtomicBool::new(false),
            }),
        }
    }

    /// Hand over the context events are dispatched in.
    pub(crate) fn attach(&self, ctx: &Ctx) {
        let _ = self.inner.ctx.set(ctx.downgrade());
    }

    /// The channels it follows.
    pub fn follows(&self) -> &[Follow] {
        &self.inner.follows
    }

    pub fn state(&self) -> State {
        *self.inner.state.lock()
    }

    /// Connect (again), on the current runtime. A no-op without one, or
    /// when already running.
    pub fn start(&self) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            crate::debug!(target: "elyra::broadcasting", "no runtime to listen on");
            return;
        };
        let mut task = self.inner.task.lock();
        if task.as_ref().is_some_and(|t| !t.is_finished()) {
            return;
        }
        let me = self.clone();
        *task = Some(handle.spawn(async move { me.run().await }));
    }

    /// Close the connection, and stay closed until [`start`](Self::start).
    pub fn stop(&self) {
        if let Some(task) = self.inner.task.lock().take() {
            task.abort();
        }
        self.inner.backend.use_socket_id(None);
        self.set_state(State::Offline);
    }

    async fn run(&self) {
        let mut backoff = Duration::from_secs(1);
        loop {
            self.set_state(State::Connecting);
            match self.session().await {
                Ok(()) => backoff = Duration::from_secs(1),
                Err(Fatal(why)) => {
                    crate::error!(target: "elyra::broadcasting", "giving up: {why}");
                    self.inner.backend.use_socket_id(None);
                    self.set_state(State::Offline);
                    return;
                }
                Err(Retry(why)) => {
                    crate::warn!(target: "elyra::broadcasting", "disconnected: {why}");
                }
            }
            self.inner.backend.use_socket_id(None);
            self.set_state(State::Offline);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    }

    /// One connection, until it drops. `Ok` when it had been up (the next
    /// try starts from the shortest backoff).
    async fn session(&self) -> Result<(), Outcome> {
        let inner = &self.inner;
        let url = inner.config.url(inner.backend.base_url()).map_err(Fatal)?;
        let (socket, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|e| Retry(e.to_string()))?;
        let (mut tx, mut rx) = socket.split();

        // The handshake: `pusher:connection_established`.
        let (socket_id, activity) = loop {
            let frame = next_frame(&mut rx).await?;
            match frame.event.as_str() {
                "pusher:connection_established" => {
                    let data = frame.data_json();
                    let id = data["socket_id"]
                        .as_str()
                        .ok_or_else(|| Retry("no socket_id".into()))?
                        .to_owned();
                    let activity = data["activity_timeout"].as_u64().unwrap_or(120);
                    break (id, Duration::from_secs(activity.clamp(1, 600)));
                }
                "pusher:error" => return Err(error_outcome(&frame)),
                _ => {}
            }
        };
        inner.backend.use_socket_id(Some(socket_id.clone()));

        for follow in &inner.follows {
            let mut data = json!({ "channel": follow.wire });
            if follow.is_private() {
                let auth: Value = inner
                    .backend
                    .post(inner.config.auth_route.clone())
                    .body(&json!({ "socket_id": socket_id, "channel_name": follow.wire }))
                    .json()
                    .await
                    .map_err(|e| Retry(format!("authorizing {}: {e}", follow.wire)))?;
                let signature = auth["auth"]
                    .as_str()
                    .ok_or_else(|| Retry(format!("no `auth` for {}", follow.wire)))?;
                data["auth"] = json!(signature);
            }
            send(
                &mut tx,
                json!({ "event": "pusher:subscribe", "data": data }),
            )
            .await?;
        }
        self.set_state(State::Connected);
        // Events sent while there was no connection are gone: everything
        // followed may have changed.
        if inner
            .connected_before
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            for follow in &inner.follows {
                self.invalidate(follow);
            }
        }
        crate::info!(
            target: "elyra::broadcasting",
            "listening on {} channel(s)",
            inner.follows.len()
        );

        let mut awaiting_pong: Option<tokio::time::Instant> = None;
        loop {
            let quiet = match awaiting_pong {
                Some(since) => {
                    (since + PONG_TIMEOUT).saturating_duration_since(tokio::time::Instant::now())
                }
                None => activity,
            };
            let frame = match tokio::time::timeout(quiet, next_frame(&mut rx)).await {
                Ok(frame) => frame?,
                Err(_) if awaiting_pong.is_some() => {
                    return Err(Retry("no pong: the line is dead".into()));
                }
                Err(_) => {
                    // Quiet past the server's activity timeout: ask.
                    send(&mut tx, json!({ "event": "pusher:ping", "data": {} })).await?;
                    awaiting_pong = Some(tokio::time::Instant::now());
                    continue;
                }
            };
            awaiting_pong = None;
            match frame.event.as_str() {
                "pusher:ping" => {
                    send(&mut tx, json!({ "event": "pusher:pong", "data": {} })).await?;
                }
                "pusher:pong" | "pusher_internal:subscription_succeeded" => {}
                "pusher:error" => {
                    let outcome = error_outcome(&frame);
                    if matches!(outcome, Fatal(_)) {
                        return Err(outcome);
                    }
                    crate::warn!(target: "elyra::broadcasting", "server error: {}", frame.data_json());
                }
                "pusher:subscription_error" | "pusher_internal:subscription_error" => {
                    crate::warn!(
                        target: "elyra::broadcasting",
                        "subscribing to {} was refused",
                        frame.channel.as_deref().unwrap_or("?")
                    );
                }
                event => {
                    let Some(follow) = frame
                        .channel
                        .as_deref()
                        .and_then(|c| inner.follows.iter().find(|f| f.wire == c))
                    else {
                        continue;
                    };
                    self.deliver(
                        follow,
                        BackendEvent {
                            channel: follow.name.clone(),
                            event: event.to_owned(),
                            data: frame.data_json(),
                        },
                    );
                }
            }
        }
    }

    /// Re-run the live queries that read `follow`'s resource.
    fn invalidate(&self, follow: &Follow) {
        #[cfg(feature = "database")]
        if let (Some(resource), Some(ctx)) = (
            &follow.resource,
            self.inner.ctx.get().and_then(WeakCtx::upgrade),
        ) {
            if let Some(live) = ctx.try_get::<crate::live::LiveRegistry>() {
                live.invalidate(&crate::backend::resource_key(resource));
            }
        }
        #[cfg(not(feature = "database"))]
        let _ = follow;
    }

    /// An event arrived: its resource changed, it's a domain event, and it
    /// goes to the windows.
    fn deliver(&self, follow: &Follow, event: BackendEvent) {
        self.invalidate(follow);
        crate::debug!(
            target: "elyra::broadcasting",
            "{} on {}",
            event.event,
            event.channel
        );
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CHANNEL, &event);
        }
        if let Some(ctx) = self.inner.ctx.get().and_then(WeakCtx::upgrade) {
            ctx.dispatch_background(event);
        }
    }

    fn set_state(&self, state: State) {
        let changed = {
            let mut current = self.inner.state.lock();
            let changed = *current != state;
            *current = state;
            changed
        };
        if changed {
            if let Some(bus) = &self.inner.bus {
                let _ = bus.emit(STATE_CHANNEL, &json!({ "state": state }));
            }
        }
    }
}

/// Why a session ended.
enum Outcome {
    /// Try again, after the backoff.
    Retry(String),
    /// Don't: the server says this will never work (a wrong app key).
    Fatal(String),
}
use Outcome::{Fatal, Retry};

/// A Pusher frame: `{ event, channel?, data }`, where `data` is usually a
/// JSON-encoded string.
struct Frame {
    event: String,
    channel: Option<String>,
    data: Value,
}

impl Frame {
    /// `data`, decoded when it's JSON in a string (as Pusher sends it).
    fn data_json(&self) -> Value {
        match &self.data {
            Value::String(text) => serde_json::from_str(text).unwrap_or_else(|_| self.data.clone()),
            other => other.clone(),
        }
    }
}

type Reader = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;
type Writer = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

async fn next_frame(rx: &mut Reader) -> Result<Frame, Outcome> {
    loop {
        let message = rx
            .next()
            .await
            .ok_or_else(|| Retry("the server closed the connection".into()))?
            .map_err(|e| Retry(e.to_string()))?;
        let text = match message {
            Message::Text(text) => text.to_string(),
            Message::Close(frame) => {
                let code = frame.as_ref().map(|f| u16::from(f.code)).unwrap_or(0);
                return Err(if (4000..4100).contains(&code) {
                    Fatal(format!("closed with {code}"))
                } else {
                    Retry(format!("closed ({code})"))
                });
            }
            _ => continue,
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(event) = value["event"].as_str() else {
            continue;
        };
        return Ok(Frame {
            event: event.to_owned(),
            channel: value["channel"].as_str().map(str::to_owned),
            data: value["data"].clone(),
        });
    }
}

async fn send(tx: &mut Writer, frame: Value) -> Result<(), Outcome> {
    tx.send(Message::Text(frame.to_string().into()))
        .await
        .map_err(|e| Retry(e.to_string()))
}

/// Pusher's error codes: 4000–4099 mean don't reconnect (the app key is
/// wrong, the app is disabled); the rest are worth another try.
fn error_outcome(frame: &Frame) -> Outcome {
    let data = frame.data_json();
    let code = data["code"].as_u64().unwrap_or(0);
    let message = data["message"].as_str().unwrap_or("").to_owned();
    if (4000..4100).contains(&code) {
        Fatal(format!("{code}: {message}"))
    } else {
        Retry(format!("{code}: {message}"))
    }
}

/// A Pusher-protocol server for tests: it accepts connections, records each
/// subscription with the `auth` it carried, answers pings, and broadcasts
/// what a test tells it to. No app secret, no signature check — CI checks
/// those against Askr and Reverb.
///
/// ```ignore
/// let pusher = PusherFake::start().await;
/// let app = TestApp::new(App::new().backend(backend.broadcasting(pusher.config())));
/// pusher.wait_for_subscriptions(1).await;
/// pusher.broadcast("private-customers", "CustomerUpdated", json!({ "model": { "id": 1 } }));
/// ```
#[derive(Clone)]
pub struct PusherFake {
    inner: Arc<FakeInner>,
}

struct FakeInner {
    port: u16,
    activity_timeout: u64,
    clients: Mutex<Vec<FakeClient>>,
    subscriptions: Mutex<Vec<Subscribed>>,
    received: Mutex<Vec<Value>>,
    connections: std::sync::atomic::AtomicUsize,
}

struct FakeClient {
    socket_id: String,
    channels: Vec<String>,
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
}

/// A subscription the fake accepted.
#[derive(Clone, Debug, PartialEq)]
pub struct Subscribed {
    pub socket_id: String,
    pub channel: String,
    /// The signature the client sent for a private channel.
    pub auth: Option<String>,
}

impl PusherFake {
    /// A fake on a free loopback port, with a 120 s activity timeout.
    pub async fn start() -> Self {
        Self::start_with_activity_timeout(120).await
    }

    /// A fake that tells clients to ping after `seconds` of quiet.
    pub async fn start_with_activity_timeout(seconds: u64) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("a loopback port");
        let port = listener.local_addr().expect("an address").port();
        let fake = Self {
            inner: Arc::new(FakeInner {
                port,
                activity_timeout: seconds,
                clients: Mutex::new(Vec::new()),
                subscriptions: Mutex::new(Vec::new()),
                received: Mutex::new(Vec::new()),
                connections: std::sync::atomic::AtomicUsize::new(0),
            }),
        };
        let accepting = fake.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let fake = accepting.clone();
                tokio::spawn(async move { fake.serve(stream).await });
            }
        });
        fake
    }

    /// What to give `Backend::broadcasting` to reach this fake.
    pub fn config(&self) -> Broadcasting {
        Broadcasting::pusher("fake-key")
            .host("127.0.0.1")
            .port(self.inner.port)
            .scheme("ws")
    }

    /// Send `event` on `channel` to every client subscribed to it — `data`
    /// as a JSON string, as Pusher does.
    pub fn broadcast(&self, channel: &str, event: &str, data: Value) {
        let frame = json!({ "event": event, "channel": channel, "data": data.to_string() });
        for client in self.inner.clients.lock().iter() {
            if client.channels.iter().any(|c| c == channel) {
                let _ = client.tx.send(Message::Text(frame.to_string().into()));
            }
        }
    }

    /// Send `event` on `channel` to every client, subscribed or not — what a
    /// misbehaving server might do; the client must ignore what it didn't
    /// follow.
    pub fn send_to_all(&self, channel: &str, event: &str, data: Value) {
        let frame = json!({ "event": event, "channel": channel, "data": data.to_string() });
        for client in self.inner.clients.lock().iter() {
            let _ = client.tx.send(Message::Text(frame.to_string().into()));
        }
    }

    /// Ping every client, as a server does.
    pub fn ping(&self) {
        let frame = json!({ "event": "pusher:ping", "data": {} }).to_string();
        for client in self.inner.clients.lock().iter() {
            let _ = client.tx.send(Message::Text(frame.clone().into()));
        }
    }

    /// Send a `pusher:error` to every client.
    pub fn error(&self, code: u16, message: &str) {
        let frame =
            json!({ "event": "pusher:error", "data": { "code": code, "message": message } })
                .to_string();
        for client in self.inner.clients.lock().iter() {
            let _ = client.tx.send(Message::Text(frame.clone().into()));
        }
    }

    /// Drop every connection, as a restarting server would.
    pub fn disconnect_all(&self) {
        for client in self.inner.clients.lock().drain(..) {
            let _ = client.tx.send(Message::Close(None));
        }
    }

    /// Every subscription so far, in order.
    pub fn subscriptions(&self) -> Vec<Subscribed> {
        self.inner.subscriptions.lock().clone()
    }

    /// Every frame clients sent, in order.
    pub fn received(&self) -> Vec<Value> {
        self.inner.received.lock().clone()
    }

    /// How many connections were accepted, all told.
    pub fn connections(&self) -> usize {
        self.inner
            .connections
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How many clients are connected now.
    pub fn connected(&self) -> usize {
        self.inner.clients.lock().len()
    }

    /// Wait (up to 5 s) until `n` subscriptions have been made.
    ///
    /// # Panics
    /// When they haven't.
    pub async fn wait_for_subscriptions(&self, n: usize) {
        self.wait_until(|f| f.subscriptions().len() >= n, "subscriptions")
            .await;
    }

    /// Wait (up to 5 s) until `check` holds.
    ///
    /// # Panics
    /// When it doesn't.
    pub async fn wait_until(&self, check: impl Fn(&Self) -> bool, what: &str) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while !check(self) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "PusherFake: no {what} within 5 s"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    async fn serve(&self, stream: tokio::net::TcpStream) {
        let Ok(socket) = tokio_tungstenite::accept_async(stream).await else {
            return;
        };
        let n = self
            .inner
            .connections
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let socket_id = format!("{n}.{n}");
        let (mut write, mut read) = socket.split();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                let close = matches!(message, Message::Close(_));
                if write.send(message).await.is_err() || close {
                    break;
                }
            }
        });
        let established = json!({
            "event": "pusher:connection_established",
            "data": json!({ "socket_id": socket_id, "activity_timeout": self.inner.activity_timeout }).to_string(),
        });
        let _ = tx.send(Message::Text(established.to_string().into()));
        self.inner.clients.lock().push(FakeClient {
            socket_id: socket_id.clone(),
            channels: Vec::new(),
            tx: tx.clone(),
        });

        while let Some(Ok(message)) = read.next().await {
            let Message::Text(text) = message else {
                continue;
            };
            let Ok(frame) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            self.inner.received.lock().push(frame.clone());
            match frame["event"].as_str() {
                Some("pusher:ping") => {
                    let pong = json!({ "event": "pusher:pong", "data": {} }).to_string();
                    let _ = tx.send(Message::Text(pong.into()));
                }
                Some("pusher:subscribe") => {
                    let channel = frame["data"]["channel"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned();
                    self.inner.subscriptions.lock().push(Subscribed {
                        socket_id: socket_id.clone(),
                        channel: channel.clone(),
                        auth: frame["data"]["auth"].as_str().map(str::to_owned),
                    });
                    if let Some(client) = self
                        .inner
                        .clients
                        .lock()
                        .iter_mut()
                        .find(|c| c.socket_id == socket_id)
                    {
                        client.channels.push(channel.clone());
                    }
                    let ok = json!({
                        "event": "pusher_internal:subscription_succeeded",
                        "channel": channel,
                        "data": "{}",
                    });
                    let _ = tx.send(Message::Text(ok.to_string().into()));
                }
                _ => {}
            }
        }
        self.inner
            .clients
            .lock()
            .retain(|c| c.socket_id != socket_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_is_on_the_backends_host_unless_told() {
        let pusher = Broadcasting::pusher("key");
        assert_eq!(
            pusher.url("https://crm.example.com").unwrap(),
            format!(
                "wss://crm.example.com/app/key?protocol=7&client=elyra&version={}&flash=false",
                env!("CARGO_PKG_VERSION")
            )
        );
        let reverb = Broadcasting::pusher("key")
            .host("ws.example.com")
            .port(8080);
        assert!(reverb
            .url("https://crm.example.com")
            .unwrap()
            .starts_with("wss://ws.example.com:8080/app/key?"));
        // A loopback backend over http gets ws.
        assert!(Broadcasting::pusher("k")
            .url("http://127.0.0.1:8765")
            .unwrap()
            .starts_with("ws://127.0.0.1:8765/app/k?"));
    }

    #[test]
    fn plain_ws_only_to_loopback() {
        let err = Broadcasting::pusher("k")
            .scheme("ws")
            .url("https://crm.example.com")
            .unwrap_err();
        assert!(err.contains("only wss"), "{err}");
    }

    #[test]
    fn private_and_public_channels() {
        let private = Follow::private("customers", None);
        assert_eq!(
            (private.name.as_str(), private.wire.as_str()),
            ("customers", "private-customers")
        );
        assert!(private.is_private());
        assert_eq!(
            Follow::private("private-customers", None),
            private,
            "either spelling"
        );
        assert!(!Follow::public("news", None).is_private());
    }

    #[test]
    fn data_is_json_in_a_string() {
        let frame = Frame {
            event: "CustomerUpdated".into(),
            channel: None,
            data: json!(r#"{"model":{"id":1}}"#),
        };
        assert_eq!(frame.data_json()["model"]["id"], 1);
        let raw = Frame {
            event: "x".into(),
            channel: None,
            data: json!({ "already": "an object" }),
        };
        assert_eq!(raw.data_json()["already"], "an object");
    }
}
