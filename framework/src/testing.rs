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

    /// Resolve a bound service, like a command would.
    pub fn get<T: std::any::Any + Send + Sync>(&self) -> Arc<T> {
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

    /// Register this TestApp as an event client, so emits are queued for it from
    /// now on. Call before the code under test emits (the bus only buffers
    /// pre-connection events for the *first* client).
    pub fn listen(&self) -> &Self {
        self.bus.register_client(&self.client);
        self
    }
}
