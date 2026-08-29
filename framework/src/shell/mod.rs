//! The native shell: tao window + wry webview + the `elyra://` custom protocol.
//!
//! Everything the frontend touches lives under a single origin,
//! `elyra://localhost` — the app is served from `/`, commands from `/__cmd/*`,
//! and the event stream from `/__events`. Same origin means no CORS, no
//! preflight, no `data:`-URL games.
//!
//! ## The map
//!
//! | Module | Concern |
//! |---|---|
//! | [`guard`] | **Who may call what**: token, capabilities, rate limits, body limits, CORS, CSP |
//! | [`protocol`] | The MessagePack success/error shapes and the `x-elyra-*` headers |
//! | [`router`] | The path → handler dispatch table, plus events and command dispatch |
//! | [`facades`] | `/__store`, `/__cache`, `/__storage`, `/__queue`, `/__window`, `/__sys`, `/__sidecar`, `/__autostart` |
//! | [`update`] | The updater routes and the `elyra:update` phase stream |
//! | [`assets`] | The embedded frontend: `ETag`, conditional requests, byte ranges |
//! | [`webview`] | The tao event loop, window construction, the app menu |
//!
//! Access control is [`guard`] and nothing else. A handler in `facades` or
//! `router` never re-decides whether a caller is allowed in — it is reached only
//! after [`guard::check`] passed the request — which is the point of keeping it
//! in a file small enough to audit in one sitting.

mod assets;
mod facades;
mod guard;
mod protocol;
mod router;
#[cfg(feature = "updater")]
mod update;
mod webview;

use std::borrow::Cow;
use std::sync::Arc;

use wry::http::{Request, Response};

use crate::about::AboutInfo;
use crate::assets::AssetResolver;
use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::event::EventBus;
use crate::security::Policy;

pub(crate) use webview::run;

const SCHEME: &str = "elyra";
const CMD_PREFIX: &str = "/__cmd/";
const EVENTS_PATH: &str = "/__events";
const ABOUT_PATH: &str = "/__about";

/// Menu id of the built-in "About <App>" item; clicking it opens the dialog.
/// Not feature-gated: the app menu is rendered on every platform now, so this is
/// reachable without `tray` and outside macOS.
const ABOUT_MENU_ID: &str = "__elyra_about";

/// Shared state captured by the protocol handler.
struct Runner {
    registry: Arc<CommandRegistry>,
    ctx: Ctx,
    bus: EventBus,
    assets: Option<AssetResolver>,
    /// The IPC runtime. `None` in the test harness, which already runs inside one.
    rt: Option<tokio::runtime::Runtime>,
    about: AboutInfo,
    /// The registered deep-link scheme (e.g. "myapp"), if any.
    deep_link: Option<String>,
    /// Optional Content-Security-Policy for HTML responses.
    csp: Option<String>,
    /// Origin/token gating + the `shell.open` and sidecar allowlists.
    policy: Policy,
    /// The app-provided menu (installed per window outside macOS).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    menu: Option<crate::menu::Menu>,
    /// Abort handles for in-flight commands that carried a request id, so the
    /// frontend can cancel a slow/long-running command.
    cancellations: parking_lot::Mutex<std::collections::HashMap<String, tokio::task::AbortHandle>>,
}

/// A window-less harness around the real [`router::route`] pipeline, for tests.
///
/// The IPC surface (token gating, capabilities, body limits, asset caching,
/// command dispatch) is the most security-sensitive code in the framework and had
/// no tests because it seemed to require `tao`/`wry`. It doesn't: `route` only
/// needs the shared state.
#[doc(hidden)]
pub struct TestShell {
    runner: Arc<Runner>,
}

impl TestShell {
    /// Build a harness from a prepared app.
    pub fn new(prepared: crate::app::Prepared) -> Self {
        Self {
            runner: Arc::new(Runner {
                registry: prepared.registry,
                ctx: prepared.ctx,
                bus: prepared.bus,
                assets: prepared.assets,
                rt: None,
                about: prepared.about,
                deep_link: prepared.deep_link,
                csp: prepared.csp,
                policy: prepared.policy,
                menu: prepared.menu.clone(),
                cancellations: parking_lot::Mutex::new(std::collections::HashMap::new()),
            }),
        }
    }

    /// This run's IPC token (what the webview would be handed).
    pub fn token(&self) -> &str {
        self.runner.policy.token()
    }

    /// Run a request through the protocol handler.
    pub async fn handle(&self, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        router::route(&self.runner, request).await
    }
}

/// The runtime handle IPC work is spawned onto: the shell's own runtime in a real
/// app, or the ambient one under the test harness.
fn ipc_handle(runner: &Arc<Runner>) -> tokio::runtime::Handle {
    match &runner.rt {
        Some(rt) => rt.handle().clone(),
        None => tokio::runtime::Handle::current(),
    }
}
