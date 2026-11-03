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
//!
//! A [popover](TrayConfig::popover) (RFC 0019) is a window that opens under
//! the icon on a left click and hides on a click elsewhere.

/// Menu id emitted for the built-in Quit item.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) const QUIT_ID: &str = "__elyra_tray_quit";

/// Menu id of the item that shows the popover on Linux, where the icon
/// sends no clicks (RFC 0019).
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) const POPOVER_ID: &str = "__elyra_tray_popover";

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
    /// The window that opens from the icon (RFC 0019).
    pub popover: Option<crate::WindowConfig>,
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

    /// A window that opens under the icon on a left click, and hides on a
    /// click elsewhere, Esc, or another click on the icon (RFC 0019). The
    /// menu moves to the right click. It's made hidden at start, without
    /// decorations, on top and out of the taskbar; its scope is its own,
    /// so give it only what it needs:
    ///
    /// ```ignore
    /// TrayConfig::new().popover(
    ///     WindowConfig::new("quick")
    ///         .path("quick")
    ///         .size(320.0, 420.0)
    ///         .only_abilities(["time.*"]),
    /// )
    /// ```
    ///
    /// On Linux, where the icon sends no clicks, the menu gets an item at
    /// the top, the window's title, that shows it.
    pub fn popover(mut self, window: crate::WindowConfig) -> Self {
        self.popover = Some(
            window
                .decorations(false)
                .resizable(false)
                .always_on_top(true)
                .visible(false)
                .skip_taskbar(true),
        );
        self
    }

    /// The menu as built on `os`: on Linux, with a popover, its item first.
    #[cfg_attr(not(feature = "tray"), allow(dead_code))]
    pub(crate) fn menu_items(&self, os: &str) -> Vec<TrayItem> {
        let mut items = Vec::with_capacity(self.items.len() + 2);
        if let (Some(popover), "linux") = (&self.popover, os) {
            items.push(TrayItem::Button {
                id: POPOVER_ID.into(),
                label: popover.title.clone(),
            });
            if !self.items.is_empty() {
                items.push(TrayItem::Separator);
            }
        }
        items.extend(self.items.iter().cloned());
        items
    }
}

/// Build the native tray icon from a config. Must be called on the main thread
/// after the event loop has initialized.
#[cfg(feature = "tray")]
pub(crate) fn build(config: &TrayConfig) -> Result<tray_icon::TrayIcon, String> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::TrayIconBuilder;

    let menu = Menu::new();
    for item in &config.menu_items(std::env::consts::OS) {
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
    if config.popover.is_some() {
        // The left click is the popover's (macOS, Windows).
        builder = builder.with_menu_on_left_click(false);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowConfig;

    #[test]
    fn a_popover_is_hidden_undecorated_on_top_and_out_of_the_taskbar() {
        let tray = TrayConfig::new().popover(WindowConfig::new("quick").size(320.0, 420.0));
        let window = tray.popover.unwrap();
        assert_eq!(window.label, "quick");
        assert_eq!((window.width, window.height), (320.0, 420.0));
        assert!(!window.visible);
        assert!(!window.decorations);
        assert!(!window.resizable);
        assert!(window.always_on_top);
        assert!(window.skip_taskbar);
    }

    #[test]
    fn on_linux_the_menu_opens_the_popover() {
        let tray = TrayConfig::new()
            .popover(WindowConfig::new("quick").title("Timer"))
            .item("open", "Open")
            .quit("Quit");
        let ids = |items: Vec<TrayItem>| {
            items
                .into_iter()
                .map(|i| match i {
                    TrayItem::Button { id, label } => format!("{id}:{label}"),
                    TrayItem::Separator => "-".into(),
                    TrayItem::Quit { label } => format!("quit:{label}"),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(tray.menu_items("linux")),
            [
                format!("{POPOVER_ID}:Timer"),
                "-".into(),
                "open:Open".into(),
                "quit:Quit".into()
            ]
        );
        // Elsewhere the left click opens it; the menu is as given.
        assert_eq!(ids(tray.menu_items("macos")), ["open:Open", "quit:Quit"]);
        assert_eq!(ids(tray.menu_items("windows")), ["open:Open", "quit:Quit"]);
        // Without other items, no separator after it.
        let only = TrayConfig::new().popover(WindowConfig::new("quick").title("Timer"));
        assert_eq!(
            ids(only.menu_items("linux")),
            [format!("{POPOVER_ID}:Timer")]
        );
        // Without a popover, nothing is added.
        let plain = TrayConfig::new().item("open", "Open");
        assert_eq!(ids(plain.menu_items("linux")), ["open:Open"]);
    }
}
