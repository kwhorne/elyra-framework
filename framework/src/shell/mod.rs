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
//! | `guard` | **Who may call what**: token, capabilities, rate limits, body limits, CORS, CSP |
//! | `protocol` | The MessagePack success/error shapes and the `x-elyra-*` headers |
//! | `router` | The path → handler dispatch table, plus events and command dispatch |
//! | `facades` | `/__store`, `/__cache`, `/__storage`, `/__queue`, `/__window`, `/__sys`, `/__sidecar`, `/__autostart` |
//! | `update` | The updater routes and the `elyra:update` phase stream |
//! | `assets` | The embedded frontend: `ETag`, conditional requests, byte ranges |
//! | `webview` | The tao event loop, window construction, the app menu |
//!
//! Access control is `guard` and nothing else. A handler in `facades` or
//! `router` never re-decides whether a caller is allowed in — it is reached only
//! after `guard::check` passed the request — which is the point of keeping it
//! in a file small enough to audit in one sitting.

mod assets;
mod facades;
mod guard;
#[cfg(debug_assertions)]
pub(crate) mod inspector;
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
/// `/__live/<command>` subscribes to a live command (RFC 0002).
const LIVE_PREFIX: &str = "/__live/";
/// Ends a live subscription; the body is its id.
const LIVE_STOP_PATH: &str = "/__live-stop";
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
    /// Which window each `x-elyra-client-id` belongs to (RFC 0016).
    clients: Clients,
}

/// The window a request came from (RFC 0016): known by the window's own
/// protocol handler, which the page can't change, never by a header.
pub(crate) struct WindowRef {
    /// Unique for this run, so two windows with one label stay apart.
    pub(crate) id: u64,
    pub(crate) label: String,
    pub(crate) scope: crate::security::WindowScope,
    /// The native window, for `/__window/*`; `None` in the test harness.
    pub(crate) native: Option<tao::window::WindowId>,
}

impl WindowRef {
    pub(crate) fn new(
        label: &str,
        scope: crate::security::WindowScope,
        native: Option<tao::window::WindowId>,
    ) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        Self {
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            label: label.to_owned(),
            scope,
            native,
        }
    }
}

/// The client ids each window has used. The runtime makes its id up, so
/// without this a page could send another window's id and read that
/// window's events or answer its assistant's questions. An id belongs to the
/// window that used it first.
#[derive(Default)]
struct Clients(parking_lot::Mutex<ClientsInner>);

#[derive(Default)]
struct ClientsInner {
    owner: std::collections::HashMap<String, u64>,
    /// Each window's ids, oldest first, so a window reloading many times
    /// doesn't grow the map without end.
    of: std::collections::HashMap<u64, std::collections::VecDeque<String>>,
    /// The native windows' ids, to release one when it closes.
    native: std::collections::HashMap<tao::window::WindowId, u64>,
}

/// How many client ids a window keeps (a reload makes a new one).
const CLIENTS_PER_WINDOW: usize = 16;

impl Clients {
    /// Whether `client` may be used by `window`: unused yet (it is now
    /// `window`'s), or already `window`'s.
    fn claim(&self, client: &str, window: u64) -> bool {
        let mut inner = self.0.lock();
        match inner.owner.get(client) {
            Some(owner) => *owner == window,
            None => {
                inner.owner.insert(client.to_owned(), window);
                let ids = inner.of.entry(window).or_default();
                ids.push_back(client.to_owned());
                if ids.len() > CLIENTS_PER_WINDOW {
                    if let Some(old) = ids.pop_front() {
                        inner.owner.remove(&old);
                    }
                }
                true
            }
        }
    }

    /// `window` closed: its ids are free again.
    fn release(&self, window: u64) {
        let mut inner = self.0.lock();
        for id in inner.of.remove(&window).unwrap_or_default() {
            inner.owner.remove(&id);
        }
    }

    /// Remember which native window `window` is.
    fn adopt(&self, native: tao::window::WindowId, window: u64) {
        self.0.lock().native.insert(native, window);
    }

    /// The native window `native` closed.
    fn release_native(&self, native: tao::window::WindowId) {
        let window = self.0.lock().native.remove(&native);
        if let Some(window) = window {
            self.release(window);
        }
    }
}

/// A window-less harness around the real `router::route` pipeline, for tests.
///
/// The IPC surface (token gating, capabilities, body limits, asset caching,
/// command dispatch) is the most security-sensitive code in the framework and had
/// no tests because it seemed to require `tao`/`wry`. It doesn't: `route` only
/// needs the shared state.
///
/// [`handle`](Self::handle) sends a request as the app's grant allows it;
/// [`window`](Self::window) and [`handle_in`](Self::handle_in) send one from
/// a window with its own limits (RFC 0016), as its protocol handler would.
#[doc(hidden)]
pub struct TestShell {
    runner: Arc<Runner>,
    windows: std::collections::HashMap<String, Arc<WindowRef>>,
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
                clients: Clients::default(),
            }),
            windows: std::collections::HashMap::new(),
        }
    }

    /// A window the requests of [`handle_in`](Self::handle_in) come from,
    /// with `config`'s label and limits.
    pub fn window(mut self, config: crate::window::WindowConfig) -> Self {
        let window = WindowRef::new(&config.label, config.scope, None);
        self.windows.insert(config.label, Arc::new(window));
        self
    }

    /// Close window `label`: the client ids it used are free again.
    pub fn close(&self, label: &str) {
        if let Some(window) = self.windows.get(label) {
            self.runner.clients.release(window.id);
        }
    }

    /// This run's IPC token (what the webview would be handed).
    pub fn token(&self) -> &str {
        self.runner.policy.token()
    }

    /// Run a request through the protocol handler.
    pub async fn handle(&self, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        router::route(&self.runner, None, request).await
    }

    /// Run a request through window `label`'s protocol handler (registered
    /// with [`window`](Self::window)).
    ///
    /// # Panics
    /// When no window is called `label`.
    pub async fn handle_in(
        &self,
        label: &str,
        request: Request<Vec<u8>>,
    ) -> Response<Cow<'static, [u8]>> {
        let window = self
            .windows
            .get(label)
            .unwrap_or_else(|| panic!("no window `{label}`: add it with TestShell::window"));
        router::route(&self.runner, Some(window), request).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_client_id_is_the_first_windows_until_it_closes() {
        let clients = Clients::default();
        assert!(clients.claim("a", 1));
        assert!(clients.claim("a", 1));
        assert!(!clients.claim("a", 2));
        clients.release(1);
        assert!(clients.claim("a", 2));
    }

    #[test]
    fn a_window_keeps_its_newest_ids() {
        let clients = Clients::default();
        for n in 0..CLIENTS_PER_WINDOW {
            assert!(clients.claim(&format!("doc-{n}"), 1));
        }
        // One more: the oldest is let go, the rest stay the window's.
        assert!(clients.claim("doc-new", 1));
        assert!(clients.claim("doc-0", 2));
        assert!(!clients.claim("doc-1", 2));
        assert!(!clients.claim("doc-new", 2));
    }
}
