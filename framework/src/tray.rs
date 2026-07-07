//! System tray icon + menu (behind the `tray` feature).
//!
//! tao dropped its built-in tray in 0.35, so this wraps the `tray-icon` crate.
//! Menu clicks are routed to the [`EventBus`](crate::EventBus): a custom item
//! with id `"open"` emits `"open"` on the `"tray"` channel, and a `Quit` item
//! closes the app. Subscribe on the frontend with `channel("tray")`.
//!
//! ```ignore
//! App::new()
//!     .tray(
//!         TrayConfig::new()
//!             .tooltip("My App")
//!             .item("open", "Open")
//!             .separator()
//!             .quit("Quit"),
//!     )
//!     .run()
//! ```
//!
//! The tray is created after the event loop initializes (required on macOS) and
//! shares the one running event loop.

/// Menu id emitted for the built-in Quit item.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) const QUIT_ID: &str = "__elyra_tray_quit";

/// A single tray menu entry.
#[derive(Clone, Debug)]
pub enum TrayItem {
    /// A clickable item; clicking emits `id` on the `"tray"` event channel.
    Button { id: String, label: String },
    /// A horizontal separator.
    Separator,
    /// Quits the application.
    Quit { label: String },
}

/// Configuration for the system tray.
#[derive(Clone, Debug, Default)]
pub struct TrayConfig {
    pub tooltip: Option<String>,
    pub title: Option<String>,
    pub items: Vec<TrayItem>,
}

impl TrayConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// A short text title shown next to the icon (macOS/Linux).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Add a clickable item that emits `id` on the `"tray"` channel.
    pub fn item(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.items.push(TrayItem::Button {
            id: id.into(),
            label: label.into(),
        });
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(TrayItem::Separator);
        self
    }

    pub fn quit(mut self, label: impl Into<String>) -> Self {
        self.items.push(TrayItem::Quit {
            label: label.into(),
        });
        self
    }
}

/// Build the native tray icon from a config. Must be called on the main thread
/// after the event loop has initialized.
#[cfg(feature = "tray")]
pub(crate) fn build(config: &TrayConfig) -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::TrayIconBuilder;

    let menu = Menu::new();
    for item in &config.items {
        let stringify = |e: tray_icon::menu::Error| e.to_string();
        match item {
            TrayItem::Separator => menu
                .append(&PredefinedMenuItem::separator())
                .map_err(stringify)?,
            TrayItem::Button { id, label } => menu
                .append(&MenuItem::with_id(id.clone(), label, true, None))
                .map_err(stringify)?,
            TrayItem::Quit { label } => menu
                .append(&MenuItem::with_id(QUIT_ID, label, true, None))
                .map_err(stringify)?,
        }
    }

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(default_icon()?);
    if let Some(tooltip) = &config.tooltip {
        builder = builder.with_tooltip(tooltip);
    }
    if let Some(title) = &config.title {
        builder = builder.with_title(title);
    }
    builder.build().map_err(|e| e.to_string())
}

/// A simple solid-color 32×32 icon, so no image asset is required.
#[cfg(feature = "tray")]
fn default_icon() -> Result<tray_icon::Icon, String> {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.extend_from_slice(&[59, 130, 246, 255]); // Elyra blue
    }
    tray_icon::Icon::from_rgba(rgba, SIZE, SIZE).map_err(|e| e.to_string())
}
