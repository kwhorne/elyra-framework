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

use std::sync::Mutex;

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
}

/// Events the tao loop listens for at runtime.
pub(crate) enum UserEvent {
    OpenWindow(WindowConfig),
    /// A tray menu item was clicked (carries its id).
    #[cfg(feature = "tray")]
    MenuClick(String),
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
            .unwrap()
            .send_event(UserEvent::OpenWindow(config))
            .is_ok()
    }
}
