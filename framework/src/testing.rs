//! Test helpers — invoke commands and assert on events without opening a window.
//!
//! `App::prepare()` existed but was `#[doc(hidden)]`, so apps built on Elyra had
//! no supported way to test commands, middleware, providers or events. [`TestApp`]
//! is that way: it assembles the real container, provider and middleware stack,
//! then dispatches through the same pipeline the shell uses — only without
//! `tao`/`wry`.
//!
//! ```ignore
//! use elyra::testing::TestApp;
//!
//! #[tokio::test]
//! async fn greets() {
//!     let app = TestApp::new(App::new().commands(commands![greet]));
//!     let greeting: String = app.invoke("greet", ("World",)).await.unwrap();
//!     assert_eq!(greeting, "Hello, World!");
//! }
//! ```
//!
//! Events emitted while a command runs are collected, so you can assert on the
//! push side too:
//!
//! ```ignore
//! app.invoke::<()>("start_import", ()).await.unwrap();
//! app.assert_emitted("progress");
//! let payloads: Vec<Progress> = app.events_on("progress");
//! ```

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::app::{App, Prepared};
use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::event::EventBus;
use crate::security::Policy;

#[doc(inline)]
pub use crate::shell::TestShell;

/// An assembled app, ready to dispatch commands in a test.
pub struct TestApp {
    ctx: Ctx,
    registry: Arc<CommandRegistry>,
    bus: EventBus,
    policy: Policy,
    /// A distinct event-bus client per TestApp, so parallel tests don't share queues.
    client: String,
}

/// Why a test invocation failed.
#[derive(Debug)]
pub enum TestError {
    /// The command itself returned an error (the message the frontend would see).
    Command(String),
    /// Arguments couldn't be encoded, or the result couldn't be decoded as `T`.
    Codec(String),
}

impl std::fmt::Display for TestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestError::Command(m) => write!(f, "command error: {m}"),
            TestError::Codec(m) => write!(f, "codec error: {m}"),
        }
    }
}

impl std::error::Error for TestError {}

impl TestApp {
    /// Assemble `app` (running every provider's `register` + `boot`) without a window.
    pub fn new(app: App) -> Self {
        Self::from_prepared(app.prepare())
    }

    /// Build from an already-prepared app.
    pub fn from_prepared(prepared: Prepared) -> Self {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let client = format!(
            "test-{}",
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        Self {
            ctx: prepared.ctx,
            registry: prepared.registry,
            bus: prepared.bus,
            policy: prepared.policy,
            client,
        }
    }

    /// The container context, for resolving services in assertions.
    pub fn ctx(&self) -> &Ctx {
        &self.ctx
    }

    /// The app's event bus.
    pub fn events(&self) -> &EventBus {
        &self.bus
    }

    /// The app's IPC policy (capabilities, allowlists, token).
    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Resolve a bound service, like a command would. `T` may be a trait object.
    pub fn get<T: ?Sized + Send + Sync + 'static>(&self) -> Arc<T> {
        self.ctx.get::<T>()
    }

    /// Every registered command name.
    pub fn commands(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.registry.names().collect();
        names.sort_unstable();
        names
    }

    /// Invoke a command through the full middleware pipeline.
    ///
    /// `args` is a tuple matching the command's parameters (after `Ctx`), exactly
    /// like the frontend's `invoke("name", a, b)`:
    ///
    /// ```ignore
    /// let sum: i64 = app.invoke("add", (2, 3)).await.unwrap();
    /// let all: Vec<Todo> = app.invoke("todos", ()).await.unwrap();
    /// ```
    pub async fn invoke<T: DeserializeOwned>(
        &self,
        command: &str,
        args: impl Serialize,
    ) -> Result<T, TestError> {
        let bytes = self.invoke_raw(command, args).await?;
        // Zero-arg / unit returns encode as nil, which decodes into `()`.
        rmp_serde::from_slice(&bytes).map_err(|e| TestError::Codec(e.to_string()))
    }

    /// Invoke and return the raw MessagePack response body.
    pub async fn invoke_raw(
        &self,
        command: &str,
        args: impl Serialize,
    ) -> Result<Vec<u8>, TestError> {
        // Compact array, the same framing `@elyra/runtime` sends.
        let body = rmp_serde::to_vec(&args).map_err(|e| TestError::Codec(e.to_string()))?;
        self.registry
            .clone()
            .dispatch(self.ctx.clone(), command, &body)
            .await
            .map_err(|e| TestError::Command(e.to_string()))
    }

    /// Invoke, expecting success (panics with the command's error otherwise).
    pub async fn invoke_ok<T: DeserializeOwned>(&self, command: &str, args: impl Serialize) -> T {
        match self.invoke(command, args).await {
            Ok(value) => value,
            Err(e) => panic!("command `{command}` was expected to succeed: {e}"),
        }
    }

    /// Invoke, expecting failure, and return the error message.
    pub async fn invoke_err(&self, command: &str, args: impl Serialize) -> String {
        match self.invoke_raw(command, args).await {
            Ok(_) => panic!("command `{command}` was expected to fail"),
            Err(e) => match e {
                TestError::Command(m) => m,
                TestError::Codec(m) => m,
            },
        }
    }

    /// Validation errors from a failed command, if it returned a
    /// [`ValidationErrors`](crate::validation::ValidationErrors) bag.
    pub async fn invoke_validation_errors(
        &self,
        command: &str,
        args: impl Serialize,
    ) -> Option<std::collections::BTreeMap<String, Vec<String>>> {
        let message = self.invoke_err(command, args).await;
        serde_json::from_str(&message).ok()
    }

    /// Drain the events emitted so far as `(channel, payload)` pairs.
    ///
    /// The batch is decoded from the same wire format the frontend receives, so a
    /// test exercises the real encoding path.
    pub async fn drain_events(&self) -> Vec<(String, rmpv::Value)> {
        // Nothing pending? Return immediately instead of waiting for the keep-alive.
        if self.bus.pending_for(&self.client) == 0 {
            return Vec::new();
        }
        let batch = self.bus.next_batch_for(&self.client).await;
        rmp_serde::from_slice(&batch).unwrap_or_default()
    }

    /// Payloads emitted on `channel`, decoded as `T`.
    pub async fn events_on<T: DeserializeOwned>(&self, channel: &str) -> Vec<T> {
        self.drain_events()
            .await
            .into_iter()
            .filter(|(name, _)| name == channel)
            .filter_map(|(_, value)| {
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).ok()?;
                rmp_serde::from_slice::<T>(&buf).ok()
            })
            .collect()
    }

    /// Assert at least one event was emitted on `channel`.
    pub async fn assert_emitted(&self, channel: &str) {
        let seen = self.drain_events().await;
        assert!(
            seen.iter().any(|(name, _)| name == channel),
            "expected an event on `{channel}`, saw: {:?}",
            seen.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
    }

    /// Assert no event was emitted on `channel`.
    pub async fn assert_not_emitted(&self, channel: &str) {
        let seen = self.drain_events().await;
        assert!(
            !seen.iter().any(|(name, _)| name == channel),
            "did not expect an event on `{channel}`"
        );
    }

    /// Subscribe to a `#[command(live)]`, as a window would: the handle holds
    /// the first result, and [`LiveHandle::next`] waits for the next one the
    /// registry pushes after a write.
    ///
    /// ```ignore
    /// let mut list = app.live::<Page<Customer>>("customers_index", (query,)).await;
    /// assert_eq!(list.value().total, 0);
    /// app.invoke_ok::<Customer>("customers_store", (input,)).await;
    /// assert_eq!(list.next().await.total, 1);
    /// ```
    ///
    /// Each handle is its own window (client), so handles don't see each
    /// other's updates, and none of them consume this TestApp's events.
    #[cfg(feature = "database")]
    pub async fn live<T: DeserializeOwned>(
        &self,
        command: &str,
        args: impl Serialize,
    ) -> LiveHandle<T> {
        match self.try_live(command, args).await {
            Ok(handle) => handle,
            Err(e) => panic!("subscribing to `{command}` failed: {e}"),
        }
    }

    /// [`live`](Self::live), returning the error instead of panicking.
    #[cfg(feature = "database")]
    pub async fn try_live<T: DeserializeOwned>(
        &self,
        command: &str,
        args: impl Serialize,
    ) -> Result<LiveHandle<T>, TestError> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let client = format!(
            "{}-live-{}",
            self.client,
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        self.bus.register_client(&client);
        let body = rmp_serde::to_vec(&args).map_err(|e| TestError::Codec(e.to_string()))?;
        let live = self.get::<crate::live::LiveRegistry>();
        let subscribed = live
            .subscribe(&client, command, &body)
            .await
            .map_err(|e| TestError::Command(e.to_string()))?;
        let value = rmp_serde::from_slice(&subscribed.value)
            .map_err(|e| TestError::Codec(e.to_string()))?;
        Ok(LiveHandle {
            bus: self.bus.clone(),
            live: (*live).clone(),
            channel: crate::live::channel(&subscribed.id),
            id: subscribed.id,
            client,
            value,
        })
    }

    /// Register this TestApp as an event client, so emits are queued for it from
    /// now on. Call before the code under test emits (the bus only buffers
    /// pre-connection events for the *first* client).
    pub fn listen(&self) -> &Self {
        self.bus.register_client(&self.client);
        self
    }
}

/// A live subscription in a test — see [`TestApp::live`].
#[cfg(feature = "database")]
pub struct LiveHandle<T> {
    bus: EventBus,
    live: crate::live::LiveRegistry,
    channel: String,
    id: String,
    client: String,
    value: T,
}

#[cfg(feature = "database")]
impl<T: DeserializeOwned> LiveHandle<T> {
    /// The latest result.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// The subscription's id, as the frontend sees it.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Wait for the next pushed result (at most five seconds), and return it.
    ///
    /// # Panics
    /// If none arrives, or the re-run failed — use [`next_update`](Self::next_update)
    /// to see failures.
    pub async fn next(&mut self) -> &T {
        match self.next_update().await {
            Ok(()) => &self.value,
            Err(e) => panic!("the live query's re-run failed: {e}"),
        }
    }

    /// Wait for the next push: `Ok` with [`value`](Self::value) updated, or
    /// `Err` with the re-run's error message.
    pub async fn next_update(&mut self) -> Result<(), String> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let batch = tokio::time::timeout_at(deadline, self.bus.next_batch_for(&self.client))
                .await
                .unwrap_or_else(|_| panic!("no update on `{}` within 5s", self.channel));
            let events: Vec<(String, rmpv::Value)> =
                rmp_serde::from_slice(&batch).unwrap_or_default();
            for (channel, payload) in events {
                if channel != self.channel {
                    continue;
                }
                let field = |name: &str| {
                    payload.as_map().and_then(|m| {
                        m.iter()
                            .find(|(k, _)| k.as_str() == Some(name))
                            .map(|(_, v)| v.clone())
                    })
                };
                if let Some(error) = field("error") {
                    let message = error
                        .as_map()
                        .and_then(|m| m.iter().find(|(k, _)| k.as_str() == Some("message")))
                        .and_then(|(_, v)| v.as_str().map(str::to_owned))
                        .unwrap_or_default();
                    return Err(message);
                }
                let value = field("value").unwrap_or(rmpv::Value::Nil);
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, &value).map_err(|e| e.to_string())?;
                self.value = rmp_serde::from_slice(&buf).map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }

    /// Whether an update arrives within `within` — for asserting that a write
    /// to an unrelated table, or one that changed nothing, pushes nothing.
    pub async fn updated_within(&mut self, within: std::time::Duration) -> bool {
        tokio::time::timeout(within, self.next_update())
            .await
            .is_ok()
    }
}

#[cfg(feature = "database")]
impl<T> Drop for LiveHandle<T> {
    fn drop(&mut self) {
        self.live.unsubscribe(&self.client, &self.id);
    }
}
