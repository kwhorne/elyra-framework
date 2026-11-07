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

use crate::icon::{Icon, Pixels};

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
    /// The icon (RFC 0028). Without one, the app's [`App::icon`], else a
    /// blue square.
    ///
    /// [`App::icon`]: crate::App::icon
    pub icon: Option<Icon>,
    /// Drawn by macOS in the menu bar's own colour (RFC 0028).
    pub template: bool,
    /// Other icons, by name, for [`Tray::set_icon`] (RFC 0028).
    pub icons: Vec<(String, Icon)>,
    /// Leave the unread count off the tray (RFC 0028).
    pub no_unread: bool,
}

impl TrayConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn tooltip(mut self, tooltip: impl Into<String>) -> Self {
        self.tooltip = Some(tooltip.into());
        self
    }

    /// A short text title shown next to the icon (macOS/Linux). Change it
    /// while the app runs with [`Tray::set_title`].
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The tray's icon (RFC 0028): `Icon::png(include_bytes!(..))`. Square,
    /// and 32 px or more (44 px draws sharp in a Retina menu bar). Without
    /// one, the app's [`App::icon`], else a blue square.
    ///
    /// [`App::icon`]: crate::App::icon
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Let macOS draw the icon in the menu bar's colour: black on a light
    /// bar, white on a dark one, as the system's own icons are. Give it a
    /// black shape on transparency; only its alpha counts. Elsewhere the
    /// icon is drawn as it is.
    pub fn template(mut self) -> Self {
        self.template = true;
        self
    }

    /// Another icon, shown with [`Tray::set_icon`]`(name)` while the app
    /// runs: syncing, offline, recording. Drawn as a template too when the
    /// tray's icon is.
    pub fn icon_named(mut self, name: impl Into<String>, icon: Icon) -> Self {
        self.icons.push((name.into(), icon));
        self
    }

    /// Leave the unread count ([`Windows::set_badge`]) off the tray. By
    /// default it's beside the icon on macOS and Linux, a red dot on the
    /// icon on Windows, and in the tooltip, so an app in the tray with no
    /// window open still shows it.
    ///
    /// [`Windows::set_badge`]: crate::window::Windows::set_badge
    pub fn no_unread(mut self) -> Self {
        self.no_unread = true;
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

/// What the icons in a config get wrong, for [`App::problems`].
///
/// [`App::problems`]: crate::App::problems
pub(crate) fn problems(config: &TrayConfig) -> Vec<String> {
    let mut problems = Vec::new();
    if let Some(e) = config.icon.as_ref().and_then(Icon::problem) {
        problems.push(format!("TrayConfig::icon(..): {e}"));
    }
    let mut seen = std::collections::HashSet::new();
    for (name, icon) in &config.icons {
        if let Some(e) = icon.problem() {
            problems.push(format!("TrayConfig::icon_named(\"{name}\", ..): {e}"));
        }
        if !seen.insert(name) {
            problems.push(format!(
                "TrayConfig::icon_named(\"{name}\", ..) is given twice: the names must differ"
            ));
        }
    }
    problems
}

/// The tray while the app runs (RFC 0028), in the container:
/// `ctx.get::<Tray>()`. Its changes go to the main thread, like a window's.
///
/// ```ignore
/// let tray = ctx.get::<Tray>();
/// tray.set_title(Some("01:24"));
/// tray.set_icon("syncing");
/// // later
/// tray.reset_icon();
/// ```
#[derive(Clone)]
pub struct Tray {
    send: std::sync::Arc<dyn Fn(TrayChange) -> bool + Send + Sync>,
    names: std::sync::Arc<std::collections::HashSet<String>>,
}

/// A change to the tray, for the main thread.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TrayChange {
    Title(Option<String>),
    Tooltip(Option<String>),
    /// A name from [`TrayConfig::icon_named`], or `None` for the tray's own.
    Icon(Option<String>),
}

impl Tray {
    #[cfg_attr(not(feature = "tray"), allow(dead_code))]
    pub(crate) fn new(
        config: &TrayConfig,
        send: impl Fn(TrayChange) -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            send: std::sync::Arc::new(send),
            names: std::sync::Arc::new(config.icons.iter().map(|(n, _)| n.clone()).collect()),
        }
    }

    /// The text beside the icon (macOS, Linux): a timer's `01:24`, say.
    /// `None` takes it away. Windows' tray has no text.
    pub fn set_title(&self, title: Option<&str>) -> bool {
        (self.send)(TrayChange::Title(title.map(str::to_owned)))
    }

    /// The tooltip. `None` puts back the one in the [`TrayConfig`], else
    /// the app's name.
    pub fn set_tooltip(&self, tooltip: Option<&str>) -> bool {
        (self.send)(TrayChange::Tooltip(tooltip.map(str::to_owned)))
    }

    /// Show the icon given as [`TrayConfig::icon_named`]`(name, ..)`. A name
    /// it wasn't given is refused (`false`), and the icon stays.
    pub fn set_icon(&self, name: &str) -> bool {
        if !self.names.contains(name) {
            crate::warn!(
                target: "elyra::tray",
                "no tray icon named {name:?}: give it with TrayConfig::icon_named"
            );
            return false;
        }
        (self.send)(TrayChange::Icon(Some(name.to_owned())))
    }

    /// Show the tray's own icon again.
    pub fn reset_icon(&self) -> bool {
        (self.send)(TrayChange::Icon(None))
    }
}

/// What the tray shows now, kept by the event loop: its config's icons,
/// and what [`Tray`] and the unread count have changed.
#[cfg_attr(not(feature = "tray"), allow(dead_code))]
pub(crate) struct Live {
    own: std::sync::Arc<Pixels>,
    icons: std::collections::HashMap<String, Icon>,
    template: bool,
    no_unread: bool,
    tooltip_given: Option<String>,
    app_name: String,
    title: Option<String>,
    tooltip: Option<String>,
    icon: Option<String>,
    unread: u32,
}

#[cfg_attr(not(feature = "tray"), allow(dead_code))]
impl Live {
    pub(crate) fn new(config: &TrayConfig, app_icon: Option<&Icon>, app_name: &str) -> Self {
        let own = config
            .icon
            .as_ref()
            .or(app_icon)
            .and_then(Icon::shared)
            .unwrap_or_else(|| std::sync::Arc::new(crate::icon::fallback()));
        Self {
            own,
            icons: config.icons.iter().cloned().collect(),
            template: config.template,
            no_unread: config.no_unread,
            tooltip_given: config.tooltip.clone(),
            app_name: app_name.to_owned(),
            title: config.title.clone(),
            tooltip: None,
            icon: None,
            unread: 0,
        }
    }

    pub(crate) fn change(&mut self, change: TrayChange) {
        match change {
            TrayChange::Title(t) => self.title = t,
            TrayChange::Tooltip(t) => self.tooltip = t,
            TrayChange::Icon(name) => self.icon = name,
        }
    }

    pub(crate) fn set_unread(&mut self, count: u32) {
        self.unread = count;
    }

    fn unread(&self) -> Option<String> {
        if self.no_unread {
            None
        } else {
            crate::badge::label(self.unread)
        }
    }

    /// The text beside the icon on `os`: the title, and the count after it
    /// (on Windows, where there's none, the count is a dot on the icon).
    pub(crate) fn title(&self, os: &str) -> Option<String> {
        let unread = if os == "windows" { None } else { self.unread() };
        match (&self.title, unread) {
            (Some(t), Some(n)) => Some(format!("{t} · {n}")),
            (Some(t), None) => Some(t.clone()),
            (None, n) => n,
        }
    }

    pub(crate) fn tooltip(&self) -> String {
        let base = self
            .tooltip
            .clone()
            .or_else(|| self.tooltip_given.clone())
            .unwrap_or_else(|| self.app_name.clone());
        match (self.no_unread, self.unread) {
            (false, n) if n > 0 => format!("{base} — {n} unread"),
            _ => base,
        }
    }

    /// The icon's pixels on `os`: the named one, or its own; on Windows,
    /// with a dot while anything is unread.
    pub(crate) fn pixels(&self, os: &str) -> std::sync::Arc<Pixels> {
        let icon = self
            .icon
            .as_ref()
            .and_then(|name| self.icons.get(name))
            .and_then(Icon::shared)
            .unwrap_or_else(|| self.own.clone());
        if os == "windows" && self.unread().is_some() {
            std::sync::Arc::new(crate::icon::with_dot(&icon))
        } else {
            icon
        }
    }

    /// Show all of it on the native tray.
    #[cfg(feature = "tray")]
    pub(crate) fn apply(&self, tray: &tray_icon::TrayIcon) {
        let os = std::env::consts::OS;
        if let Ok(icon) = native_icon(&self.pixels(os)) {
            #[cfg(target_os = "macos")]
            let _ = tray.set_icon_with_as_template(Some(icon), self.template);
            #[cfg(not(target_os = "macos"))]
            let _ = tray.set_icon(Some(icon));
        }
        tray.set_title(self.title(os));
        let _ = tray.set_tooltip(Some(self.tooltip()));
    }

    #[cfg(feature = "tray")]
    pub(crate) fn template(&self) -> bool {
        self.template
    }
}

#[cfg(feature = "tray")]
fn native_icon(pixels: &Pixels) -> Result<tray_icon::Icon, String> {
    tray_icon::Icon::from_rgba(pixels.rgba.clone(), pixels.width, pixels.height)
        .map_err(|e| e.to_string())
}

/// Build the native tray icon from a config. Must be called on the main thread
/// after the event loop has initialized.
#[cfg(feature = "tray")]
pub(crate) fn build(config: &TrayConfig, live: &Live) -> Result<tray_icon::TrayIcon, String> {
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

    let os = std::env::consts::OS;
    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(native_icon(&live.pixels(os))?)
        .with_icon_as_template(live.template())
        .with_tooltip(live.tooltip());
    if let Some(title) = live.title(os) {
        builder = builder.with_title(title);
    }
    if config.popover.is_some() {
        // The left click is the popover's (macOS, Windows).
        builder = builder.with_menu_on_left_click(false);
    }
    builder.build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WindowConfig;

    fn square(rgb: [u8; 3]) -> Icon {
        Icon::rgba([rgb[0], rgb[1], rgb[2], 255].repeat(32 * 32), 32, 32)
    }

    fn first_pixel(p: &Pixels) -> [u8; 4] {
        p.rgba[..4].try_into().unwrap()
    }

    #[test]
    fn the_tray_takes_its_own_icon_then_the_apps_then_a_blue_square() {
        let own = TrayConfig::new().icon(square([1, 2, 3]));
        let app = square([9, 9, 9]);
        assert_eq!(
            first_pixel(&Live::new(&own, Some(&app), "A").pixels("macos")),
            [1, 2, 3, 255]
        );
        assert_eq!(
            first_pixel(&Live::new(&TrayConfig::new(), Some(&app), "A").pixels("macos")),
            [9, 9, 9, 255]
        );
        let fallback = Live::new(&TrayConfig::new(), None, "A").pixels("macos");
        assert_eq!(*fallback, crate::icon::fallback());
    }

    #[test]
    fn a_named_icon_shows_until_reset() {
        let config = TrayConfig::new()
            .icon(square([1, 1, 1]))
            .icon_named("syncing", square([2, 2, 2]));
        let mut live = Live::new(&config, None, "A");
        live.change(TrayChange::Icon(Some("syncing".into())));
        assert_eq!(first_pixel(&live.pixels("linux")), [2, 2, 2, 255]);
        live.change(TrayChange::Icon(None));
        assert_eq!(first_pixel(&live.pixels("linux")), [1, 1, 1, 255]);
    }

    #[test]
    fn the_unread_count_follows_the_title_where_the_tray_has_text() {
        let mut live = Live::new(&TrayConfig::new(), None, "Timer");
        assert_eq!(live.title("macos"), None);
        live.set_unread(3);
        assert_eq!(live.title("macos").as_deref(), Some("3"));
        live.change(TrayChange::Title(Some("01:24".into())));
        assert_eq!(live.title("macos").as_deref(), Some("01:24 · 3"));
        assert_eq!(live.title("linux").as_deref(), Some("01:24 · 3"));
        live.set_unread(12);
        assert_eq!(live.title("macos").as_deref(), Some("01:24 · 9+"));
        live.set_unread(0);
        assert_eq!(live.title("macos").as_deref(), Some("01:24"));
    }

    #[test]
    fn on_windows_the_unread_count_is_a_dot_on_the_icon() {
        let mut live = Live::new(&TrayConfig::new().icon(square([255, 255, 255])), None, "A");
        live.set_unread(2);
        assert_eq!(live.title("windows"), None, "Windows' tray has no text");
        let dotted = live.pixels("windows");
        let at = ((25 * 32 + 25) * 4) as usize;
        assert_eq!(&dotted.rgba[at..at + 4], &[0xE5, 0x39, 0x35, 0xFF]);
        assert_eq!(
            first_pixel(&live.pixels("macos")),
            [255, 255, 255, 255],
            "elsewhere, no dot"
        );
        live.set_unread(0);
        assert_eq!(&live.pixels("windows").rgba[at..at + 4], &[255; 4]);
    }

    #[test]
    fn the_tooltip_is_the_set_one_then_the_configs_then_the_apps_name_with_the_count() {
        let mut live = Live::new(&TrayConfig::new(), None, "Quick todos");
        assert_eq!(live.tooltip(), "Quick todos");
        let mut given = Live::new(&TrayConfig::new().tooltip("Todos"), None, "Quick todos");
        assert_eq!(given.tooltip(), "Todos");
        given.change(TrayChange::Tooltip(Some("Syncing…".into())));
        assert_eq!(given.tooltip(), "Syncing…");
        given.change(TrayChange::Tooltip(None));
        assert_eq!(given.tooltip(), "Todos");
        live.set_unread(4);
        assert_eq!(live.tooltip(), "Quick todos — 4 unread");
    }

    #[test]
    fn no_unread_leaves_the_count_off_the_tray() {
        let mut live = Live::new(&TrayConfig::new().title("01:24").no_unread(), None, "A");
        live.set_unread(5);
        assert_eq!(live.title("macos").as_deref(), Some("01:24"));
        assert_eq!(live.tooltip(), "A");
        assert_eq!(
            first_pixel(&live.pixels("windows")),
            first_pixel(&live.pixels("macos"))
        );
    }

    #[test]
    fn tray_sends_its_changes_and_refuses_a_name_it_wasnt_given() {
        let sent = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
        let log = sent.clone();
        let config = TrayConfig::new().icon_named("syncing", square([2, 2, 2]));
        let tray = Tray::new(&config, move |c| {
            log.lock().push(c);
            true
        });
        assert!(tray.set_title(Some("01:24")));
        assert!(tray.set_tooltip(None));
        assert!(tray.set_icon("syncing"));
        assert!(!tray.set_icon("sycning"), "a misspelt name is refused");
        assert!(tray.reset_icon());
        assert_eq!(
            *sent.lock(),
            [
                TrayChange::Title(Some("01:24".into())),
                TrayChange::Tooltip(None),
                TrayChange::Icon(Some("syncing".into())),
                TrayChange::Icon(None),
            ]
        );
    }

    #[test]
    fn unreadable_or_twice_named_icons_are_problems() {
        let config = TrayConfig::new()
            .icon(Icon::png(b"nope"))
            .icon_named("syncing", Icon::png(b"nope"))
            .icon_named("idle", square([0, 0, 0]))
            .icon_named("idle", square([0, 0, 0]));
        let problems = problems(&config);
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems[0].starts_with("TrayConfig::icon(..): not a PNG"));
        assert!(problems[1].starts_with("TrayConfig::icon_named(\"syncing\", ..): not a PNG"));
        assert!(problems[2].contains("\"idle\", ..) is given twice"));
        assert!(super::problems(&TrayConfig::new().icon(square([0, 0, 0]))).is_empty());
    }

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
