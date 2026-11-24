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
    /// Shown when made; `false` makes it hidden, for `Windows::show`.
    pub visible: bool,
    /// Kept out of the taskbar (Windows, Linux). The tray's popover is.
    pub skip_taskbar: bool,
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
            visible: true,
            skip_taskbar: false,
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

    /// Made hidden with `false`, to be shown later (`Windows::show`).
    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Kept out of the taskbar (Windows, Linux).
    pub fn skip_taskbar(mut self, skip: bool) -> Self {
        self.skip_taskbar = skip;
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
    /// Only read in this window (RFC 0041): only `#[command(live)]`
    /// commands run, under the same guard as a live query's re-run (a model
    /// or backend write in them fails), `can` says no to an ability only
    /// commands that write need, and only the routes that read are there:
    /// no store writes, clipboard, dialogs, sign-out, sidecar, microphone…
    /// A preview of something someone else wrote, say. It only narrows.
    pub fn read_only(mut self) -> Self {
        self.scope.read_only();
        self
    }

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

/// A native window's id: tao's, with the window (RFC 0054). Without it
/// there's never a window, and this is a stand-in no value of which exists.
#[cfg(feature = "gui")]
pub(crate) type NativeId = tao::window::WindowId;
#[cfg(not(feature = "gui"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NativeId {}

/// The event loop's proxy: tao's, with the window. Without it there's no
/// loop, nothing makes one, and every event sent is refused.
#[cfg(feature = "gui")]
pub(crate) type Proxy = tao::event_loop::EventLoopProxy<UserEvent>;
#[cfg(not(feature = "gui"))]
#[derive(Clone)]
pub(crate) enum Proxy {}
#[cfg(not(feature = "gui"))]
impl Proxy {
    pub(crate) fn send_event(&self, _event: UserEvent) -> Result<(), UserEvent> {
        match *self {}
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
    /// The tray icon was clicked with the left button (macOS, Windows),
    /// at this rect: the popover's click (RFC 0019). Linux's icon sends none.
    #[cfg_attr(
        not(all(feature = "tray", any(target_os = "macos", target_os = "windows"))),
        allow(dead_code)
    )]
    TrayClick(crate::popover::Rect),
    /// Show, hide or toggle the tray's popover from code (RFC 0019).
    Popover(PopoverAction),
    /// The tray's title, tooltip or icon, from [`Tray`](crate::tray::Tray)
    /// (RFC 0028).
    #[cfg(feature = "tray")]
    Tray(crate::tray::TrayChange),
    /// End the app ([`Windows::quit`], `quit()` in the runtime).
    Quit,
    /// Show the Quick Look window at a route, beside the window `source`, or
    /// hide it with `None` (RFC 0041).
    QuickLook {
        source: Option<NativeId>,
        route: Option<String>,
    },
    /// A moment after the app's last window lost the focus: Quick Look hides
    /// if none has it back (RFC 0041), the app having gone to the background.
    QuickLookBlur,
    /// What the diagnostics say of the windows, the screens and the theme
    /// (RFC 0033).
    Seen(tokio::sync::oneshot::Sender<crate::diagnostics::Seen>),
    /// Open the matrix's frames by the window labelled `source`, or close
    /// them if any is open (RFC 0034): answers whether they're open now.
    /// Debug builds only.
    #[cfg(debug_assertions)]
    Matrix {
        source: Option<String>,
        frames: Vec<crate::matrix::Opening>,
        reply: tokio::sync::oneshot::Sender<bool>,
    },
    /// A coding agent's question, through the dev endpoint (RFC 0026): debug
    /// builds only.
    #[cfg(debug_assertions)]
    Dev(crate::dev::Ask),
}

/// What [`Windows::show_popover`] and the others ask of the tray's popover.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PopoverAction {
    Show,
    Hide,
    Toggle,
}

/// A window-control request targeting a window: the native one a page asked
/// from, else by label, else the focused / primary one.
pub(crate) struct WindowCommand {
    pub label: Option<String>,
    /// The window a page asked from (RFC 0016). When it's gone, nothing
    /// happens.
    pub native: Option<NativeId>,
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
    proxy: Mutex<Proxy>,
}

impl Windows {
    pub(crate) fn new(proxy: Proxy) -> Self {
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
    pub(crate) fn command_on(&self, native: NativeId, action: WindowAction) -> bool {
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
    /// Close a window (exits the app when it's the last one, unless it stays
    /// in the tray; there, closing the main window hides it).
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

    /// Show the tray's popover (RFC 0019): where it last opened, or by the
    /// cursor, or at the top of the main monitor. For a global shortcut,
    /// say. Without a popover, nothing happens.
    pub fn show_popover(&self) -> bool {
        self.popover(PopoverAction::Show)
    }
    /// Hide the tray's popover.
    pub fn hide_popover(&self) -> bool {
        self.popover(PopoverAction::Hide)
    }
    /// Show the tray's popover if it's hidden, else hide it.
    pub fn toggle_popover(&self) -> bool {
        self.popover(PopoverAction::Toggle)
    }
    fn popover(&self, action: PopoverAction) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::Popover(action))
            .is_ok()
    }

    /// Show the Quick Look window (RFC 0041) at `route`, one of the app's own
    /// (`/customers/42`), beside the focused window: made the first time,
    /// read-only, and moved there when it's already shown.
    pub fn show_quick_look(&self, route: &str) -> bool {
        match quick_look_route(route) {
            Some(route) => self.quick_look(None, Some(route)),
            None => false,
        }
    }

    /// Hide the Quick Look window.
    pub fn hide_quick_look(&self) -> bool {
        self.quick_look(None, None)
    }

    /// Ask, in 300 ms, whether the app is in the background (RFC 0041): a
    /// window losing the focus to another of the app's is followed by that
    /// one getting it, not always in the same turn of the loop.
    pub(crate) fn quick_look_blur_later(&self) {
        let proxy = self.proxy.lock().clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            let _ = proxy.send_event(UserEvent::QuickLookBlur);
        });
    }

    pub(crate) fn quick_look(&self, source: Option<NativeId>, route: Option<String>) -> bool {
        self.proxy
            .lock()
            .send_event(UserEvent::QuickLook { source, route })
            .is_ok()
    }

    /// End the app, as the tray's Quit item does: the main window's geometry
    /// is saved (with `persist_window_state`), then the loop exits. How an app
    /// that stays in the tray (RFC 0020) is quit from code.
    pub fn quit(&self) -> bool {
        self.proxy.lock().send_event(UserEvent::Quit).is_ok()
    }

    /// What the event loop sees of the windows, the screens and the theme,
    /// for the diagnostics (RFC 0033): `None` once it has ended, or when it
    /// doesn't answer in two seconds.
    pub(crate) async fn seen(&self) -> Option<crate::diagnostics::Seen> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.proxy.lock().send_event(UserEvent::Seen(reply)).ok()?;
        tokio::time::timeout(std::time::Duration::from_secs(2), answer)
            .await
            .ok()?
            .ok()
    }

    /// Open `frames` by the window `source`, or close the frames that are
    /// open (RFC 0034): whether they're open now, or `None` once the loop
    /// has ended.
    #[cfg(debug_assertions)]
    pub(crate) async fn toggle_matrix(
        &self,
        source: Option<String>,
        frames: Vec<crate::matrix::Opening>,
    ) -> Option<bool> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.proxy
            .lock()
            .send_event(UserEvent::Matrix {
                source,
                frames,
                reply,
            })
            .ok()?;
        tokio::time::timeout(std::time::Duration::from_secs(5), answer)
            .await
            .ok()?
            .ok()
    }

    /// Window `label`'s picture, at most `max_width` CSS pixels wide (RFC
    /// 0035's tasks): asked of the event loop, as `rata mcp dev` asks.
    #[cfg(debug_assertions)]
    pub(crate) async fn picture(
        &self,
        label: &str,
        max_width: f64,
    ) -> Result<crate::dev::Picture, String> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        self.proxy
            .lock()
            .send_event(UserEvent::Dev(crate::dev::Ask::Picture {
                window: Some(label.to_owned()),
                max_width,
                reply,
            }))
            .map_err(|_| "the app has ended".to_owned())?;
        tokio::time::timeout(std::time::Duration::from_secs(5), answer)
            .await
            .map_err(|_| "no picture in five seconds".to_owned())?
            .map_err(|_| "no picture".to_owned())?
    }

    /// Resize a window's inner area (logical pixels).
    pub fn set_size(&self, label: Option<&str>, width: f64, height: f64) -> bool {
        self.command(label, WindowAction::SetSize(width, height))
    }
}

/// The Quick Look window's label (RFC 0041).
pub const QUICK_LOOK: &str = "quick-look";

/// A route the Quick Look window may show: one of the app's own, as the
/// hash router has them (`/customers/42`, `#/customers/42`), never a URL.
/// `None` for anything else.
pub(crate) fn quick_look_route(route: &str) -> Option<String> {
    let route = route.trim();
    let route = route.strip_prefix('#').unwrap_or(route);
    let ok = route.starts_with('/')
        && !route.starts_with("//")
        && !route.contains(['\\', '\n', '\r'])
        && route.len() <= 2000;
    ok.then(|| route.to_owned())
}

#[cfg(test)]
mod quick_look_tests {
    use super::quick_look_route;

    #[test]
    fn only_the_app_s_own_routes() {
        assert_eq!(
            quick_look_route("/customers/42").as_deref(),
            Some("/customers/42")
        );
        assert_eq!(
            quick_look_route("#/customers/42?tab=a").as_deref(),
            Some("/customers/42?tab=a")
        );
        for bad in [
            "https://evil.test",
            "//evil.test",
            "javascript:alert(1)",
            "customers",
            "",
            "/a\\b",
        ] {
            assert_eq!(quick_look_route(bad), None, "{bad}");
        }
    }
}

/// Where the Quick Look window goes (RFC 0041), in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct QuickLookSpot {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl QuickLookSpot {
    /// Beside `window` (x, y, width, height): to its right when `screen`
    /// has room, else its left, else over its right part. As tall as the
    /// window, a third of its width, at least 360.
    pub(crate) fn beside(
        window: (f64, f64, f64, f64),
        screen: Option<(f64, f64, f64, f64)>,
    ) -> Self {
        let (x, y, w, h) = window;
        let width = (w / 3.0).max(360.0);
        let (sx, _, sw, _) = screen.unwrap_or((f64::MIN / 4.0, 0.0, f64::MAX / 2.0, 0.0));
        let right = x + w;
        let x = if right + width <= sx + sw {
            right
        } else if x - width >= sx {
            x - width
        } else {
            (right - width).max(sx)
        };
        Self {
            x,
            y,
            width,
            height: h,
        }
    }
}

#[cfg(test)]
mod quick_look_spot_tests {
    use super::QuickLookSpot;

    #[test]
    fn beside_its_window_where_there_is_room() {
        let screen = Some((0.0, 0.0, 1920.0, 1080.0));
        let right = QuickLookSpot::beside((100.0, 50.0, 1200.0, 800.0), screen);
        assert_eq!(
            right,
            QuickLookSpot {
                x: 1300.0,
                y: 50.0,
                width: 400.0,
                height: 800.0
            }
        );
        let left = QuickLookSpot::beside((700.0, 50.0, 1200.0, 800.0), screen);
        assert_eq!(left.x, 300.0);
        let over = QuickLookSpot::beside((0.0, 0.0, 1920.0, 1080.0), screen);
        assert_eq!(
            over,
            QuickLookSpot {
                x: 1280.0,
                y: 0.0,
                width: 640.0,
                height: 1080.0
            }
        );
        let narrow = QuickLookSpot::beside((0.0, 0.0, 600.0, 500.0), screen);
        assert_eq!(narrow.width, 360.0);
    }
}
