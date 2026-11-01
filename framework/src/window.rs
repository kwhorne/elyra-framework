//! Window management: per-window configuration, the runtime [`Windows`] handle,
//! and the user-event the tao loop listens for.
//!
//! Multiple windows share one event loop and one protocol handler (same origin,
//! same commands, same event bus). Open more at runtime from anywhere —
//! including inside a command — via the container-bound [`Windows`] handle:
//!
//! ```ignore
//! #[command]
//! async fn open_settings(ctx: Ctx) {
//!     ctx.get::<Windows>().open(WindowConfig::new("settings").path("settings"));
//! }
//! ```

use parking_lot::Mutex;

use tao::event_loop::EventLoopProxy;

/// Per-window configuration.
#[derive(Clone, Debug)]
pub struct WindowConfig {
    pub label: String,
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub min_size: Option<(f64, f64)>,
    pub resizable: bool,
    pub decorations: bool,
    pub always_on_top: bool,
    /// Path appended to the app origin (e.g. `"/"`, `"settings"`). Lets a window
    /// deep-link into the SPA.
    pub path: String,
    /// What this window may use, narrowed from the app's grant (RFC 0016):
    /// set with [`deny`](Self::deny), [`only_abilities`](Self::only_abilities)
    /// and the others.
    pub scope: crate::security::WindowScope,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            label: "main".into(),
            title: "Elyra".into(),
            width: 900.0,
            height: 640.0,
            min_size: None,
            resizable: true,
            decorations: true,
            always_on_top: false,
            path: "/".into(),
            scope: crate::security::WindowScope::default(),
        }
    }
}

impl WindowConfig {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ..Default::default()
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.min_size = Some((width, height));
        self
    }

    pub fn resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    pub fn decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.always_on_top = always_on_top;
        self
    }

    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    /// Refuse `capability` in this window, though the app grants it
    /// (RFC 0016). A window can only be narrowed, never widened.
    ///
    /// ```ignore
    /// WindowConfig::new("preview").deny(Capability::Assistant)
    /// ```
    pub fn deny(mut self, capability: crate::security::Capability) -> Self {
        self.scope.deny(capability);
        self
    }

    /// Allow only `capabilities` in this window, of those the app grants
    /// (RFC 0016). A second call adds to them.
    pub fn only_capabilities(
        mut self,
        capabilities: impl IntoIterator<Item = crate::security::Capability>,
    ) -> Self {
        self.scope.only_capabilities(capabilities);
        self
    }

    /// Allow only the abilities `patterns` cover in this window
    /// (`invoices.view`, `customers.*`), of those the app grants and the user
    /// holds (RFC 0016). A command whose `can = "…"` isn't covered is refused
    /// here, and `can` says no. Commands without `can` aren't affected: deny
    /// `Capability::Commands` to refuse those too. A second call adds to them.
    pub fn only_abilities<S: Into<String>>(
        mut self,
        patterns: impl IntoIterator<Item = S>,
    ) -> Self {
        self.scope.only_abilities(patterns);
        self
    }

    /// Refuse the abilities `patterns` cover in this window (RFC 0016).
    pub fn deny_abilities<S: Into<String>>(
        mut self,
        patterns: impl IntoIterator<Item = S>,
    ) -> Self {
        self.scope.deny_abilities(patterns);
        self
    }
}

/// Events the tao loop listens for at runtime.
pub(crate) enum UserEvent {
    /// Boxed: with its limits (RFC 0016) it's much the largest event.
    OpenWindow(Box<WindowConfig>),
    /// A window-control request (from JS or a command), applied on the main thread.
    Window(WindowCommand),
    /// A menu item was clicked (carries its id). Covers the application/window
    /// menu on every platform (e.g. "About") and tray menu items.
    MenuClick(String),
    /// A registered global keyboard shortcut fired (carries its hotkey id).
    #[cfg(feature = "shortcuts")]
    Shortcut(u32),
    /// A second launch occurred (single-instance mode); carries its forwarded
    /// command-line payload (often a deep-link URL, possibly empty).
    SecondInstance(String),
    /// The unread count for the app's icon and tray (RFC 0008); `0` clears it.
    Badge(u32),
}

/// A window-control request targeting a window: the native one a page asked
/// from, else by label, else the focused / primary one.
pub(crate) struct WindowCommand {
    pub label: Option<String>,
    /// The window a page asked from (RFC 0016). When it's gone, nothing
    /// happens.
    pub native: Option<tao::window::WindowId>,
    pub action: WindowAction,
}

/// The window operations exposed to the frontend + [`Windows`].
pub(crate) enum WindowAction {
    Minimize,
    ToggleMaximize,
    ToggleFullscreen,
    Close,
    Focus,
    Show,
    Hide,
    Center,
    SetTitle(String),
    SetSize(f64, f64),
}

/// Whether one of the app's windows has focus — kept by the event loop, and
/// in the container (`ctx.get::<Focus>()`). Without windows (a test, headless
/// MCP), none has.
#[derive(Default)]
pub struct Focus(std::sync::atomic::AtomicBool);

impl Focus {
    pub fn is_focused(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Set by the event loop as focus moves; a test may set it too.
    pub fn set(&self, focused: bool) {
        self.0.store(focused, std::sync::atomic::Ordering::SeqCst);
    }
}

/// A runtime handle for opening windows. Bound in the container by [`App`],
/// resolvable via `ctx.get::<Windows>()`. `Send + Sync`, so it works from
/// commands, providers, and background tasks.
///
/// [`App`]: crate::App
pub struct Windows {
    proxy: Mutex<EventLoopProxy<UserEvent>>,
}

impl Windows {
    pub(crate) fn new(proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            proxy: Mutex::new(proxy),
        }
    }

    /// Open a new window. Returns `false` if the event loop has already exited.
    pub fn open(&self, config: WindowConfig) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::OpenWindow(Box::new(config)))
            .is_ok()
    }

    pub(crate) fn command_for(&self, label: Option<&str>, action: WindowAction) -> bool {
        self.command(label, action)
    }

    fn command(&self, label: Option<&str>, action: WindowAction) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::Window(WindowCommand {
                label: label.map(str::to_owned),
                native: None,
                action,
            }))
            .is_ok()
    }

    /// `action` on the native window `native`, the one a page asked from
    /// (RFC 0016).
    pub(crate) fn command_on(&self, native: tao::window::WindowId, action: WindowAction) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::Window(WindowCommand {
                label: None,
                native: Some(native),
                action,
            }))
            .is_ok()
    }

    /// Minimize a window (label, or the focused/primary one when `None`).
    pub fn minimize(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Minimize)
    }
    /// Toggle maximized.
    pub fn toggle_maximize(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::ToggleMaximize)
    }
    /// Toggle borderless fullscreen.
    pub fn toggle_fullscreen(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::ToggleFullscreen)
    }
    /// Close a window (exits the app when it's the last one).
    pub fn close(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Close)
    }
    /// Give a window keyboard focus (raising it).
    pub fn focus(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Focus)
    }
    /// Show a hidden window.
    pub fn show(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Show)
    }
    /// Hide a window (without closing it).
    pub fn hide(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Hide)
    }
    /// Center a window on its current monitor.
    pub fn center(&self, label: Option<&str>) -> bool {
        self.command(label, WindowAction::Center)
    }
    /// Set a window's title.
    pub fn set_title(&self, label: Option<&str>, title: impl Into<String>) -> bool {
        self.command(label, WindowAction::SetTitle(title.into()))
    }
    /// The number on the app's icon — the dock (macOS), the launcher
    /// (Linux), the taskbar button (Windows) — and in the tray's tooltip.
    /// `0` clears it.
    pub fn set_badge(&self, count: u32) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::Badge(count))
            .is_ok()
    }

    /// Resize a window's inner area (logical pixels).
    pub fn set_size(&self, label: Option<&str>, width: f64, height: f64) -> bool {
        self.command(label, WindowAction::SetSize(width, height))
    }
}
