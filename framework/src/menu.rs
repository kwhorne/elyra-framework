//! Native application menu.
//!
//! Describe custom submenus with [`Menu`] / [`Submenu`] and pass them to
//! [`App::menu`]. They're appended after the standard app + Edit menus (macOS)
//! or make each window's menu bar (Windows, Linux). Clicking an item emits the
//! `elyra:menu` event carrying the item's id, which `@elyra/runtime` surfaces
//! via `onMenu`.
//!
//! Items can be check items, greyed out, relabelled at runtime (RFC 0055):
//! [`AppMenu::set`] from Rust, `menu.set(id, { .. })` from the page, on every
//! window's menu at once. The platform's own items ([`SystemItem`]: Cut,
//! Copy, Paste, …) go through its responder chain, which the page can't.
//!
//! This is data-only (no platform types), so it compiles everywhere.
//!
//! ```ignore
//! App::new().menu(
//!     Menu::new().submenu(
//!         Submenu::new("File")
//!             .item_accel("file.new", "New", "CmdOrCtrl+N")
//!             .entry(Item::new("file.save", "Save").accelerator("CmdOrCtrl+S").enabled(false))
//!             .check("file.autosave", "Autosave", true)
//!             .separator()
//!             .system(SystemItem::CloseWindow),
//!     ),
//! )
//! ```
//!
//! [`App::menu`]: crate::App::menu

use std::collections::HashMap;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

/// A native menu: an ordered list of [`Submenu`]s.
#[derive(Default, Clone, Debug)]
pub struct Menu {
    pub(crate) submenus: Vec<Submenu>,
}

/// A titled submenu of entries.
#[derive(Clone, Debug)]
pub struct Submenu {
    pub(crate) title: String,
    pub(crate) items: Vec<MenuEntry>,
    pub(crate) only: Platform,
}

#[derive(Clone, Debug)]
pub(crate) enum MenuEntry {
    Item(Item),
    /// A check item, and its mark when the app starts.
    Check(Item, bool),
    System(SystemItem),
    Submenu(Submenu),
    Separator,
}

/// A clickable item: an id, a label, and what it starts as.
#[derive(Clone, Debug)]
pub struct Item {
    pub(crate) id: String,
    pub(crate) label: String,
    pub(crate) accelerator: Option<String>,
    pub(crate) enabled: bool,
    pub(crate) only: Platform,
}

impl Item {
    /// An item that says `id` on `elyra:menu` when clicked.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            accelerator: None,
            enabled: true,
            only: Platform::ALL,
        }
    }

    /// A keyboard accelerator (`"CmdOrCtrl+S"`, the syntax of global shortcuts).
    pub fn accelerator(mut self, accelerator: impl Into<String>) -> Self {
        self.accelerator = Some(accelerator.into());
        self
    }

    /// Whether it starts clickable (it can change: [`AppMenu::set`]).
    pub fn enabled(mut self, on: bool) -> Self {
        self.enabled = on;
        self
    }

    /// Only on these platforms: left out of the menu elsewhere.
    pub fn only(mut self, platforms: Platform) -> Self {
        self.only = platforms;
        self
    }
}

/// The platform's own items, which do what the platform does: copying from
/// the focused field, closing the window, quitting. A platform that has no
/// such item leaves it out.
///
/// `Undo` and `Redo` drive the webview's own undo, which knows nothing of an
/// app's document model: an app with its own history declares its own items
/// instead, and enables them from its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SystemItem {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Minimize,
    Fullscreen,
    CloseWindow,
    /// macOS: hide the app.
    Hide,
    /// macOS: hide the other apps.
    HideOthers,
    /// macOS: show all apps.
    ShowAll,
    /// macOS: the Services submenu.
    Services,
    Quit,
}

/// A label the framework puts in the menu itself (RFC 0056): the Edit and
/// Help menus, About, Save Diagnostics…, and the system's items. Each is
/// `elyra.menu.<word>` in the app's catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub(crate) enum MenuWord {
    Edit,
    Help,
    About,
    Diagnostics,
    System(SystemItem),
}

#[cfg_attr(not(feature = "gui"), allow(dead_code))]
impl MenuWord {
    /// Its key and its English, with `:app` for the app's name.
    fn word(self) -> (&'static str, &'static str) {
        match self {
            Self::Edit => ("edit", "Edit"),
            Self::Help => ("help", "Help"),
            Self::About => ("about", "About :app"),
            Self::Diagnostics => ("diagnostics", "Save Diagnostics…"),
            Self::System(item) => match item {
                SystemItem::Undo => ("undo", "Undo"),
                SystemItem::Redo => ("redo", "Redo"),
                SystemItem::Cut => ("cut", "Cut"),
                SystemItem::Copy => ("copy", "Copy"),
                SystemItem::Paste => ("paste", "Paste"),
                SystemItem::SelectAll => ("selectAll", "Select All"),
                SystemItem::Minimize => ("minimize", "Minimize"),
                SystemItem::Fullscreen => ("fullscreen", "Toggle Full Screen"),
                SystemItem::CloseWindow => ("closeWindow", "Close Window"),
                SystemItem::Hide => ("hide", "Hide :app"),
                SystemItem::HideOthers => ("hideOthers", "Hide Others"),
                SystemItem::ShowAll => ("showAll", "Show All"),
                SystemItem::Services => ("services", "Services"),
                SystemItem::Quit => ("quit", "Quit :app"),
            },
        }
    }

    /// The label in the app's language, or `None` while that's the
    /// framework's English, so the platform's own label stays as it was.
    pub(crate) fn translated(
        self,
        t: Option<&crate::i18n::Translator>,
        app: &str,
    ) -> Option<String> {
        let (key, _) = self.word();
        let message = t?.translated(&format!("elyra.menu.{key}"))?;
        Some(message.replace(":app", app))
    }

    /// The label to show: the app's language's, else the English.
    pub(crate) fn label(self, t: Option<&crate::i18n::Translator>, app: &str) -> String {
        self.translated(t, app)
            .unwrap_or_else(|| self.word().1.replace(":app", app))
    }
}

/// The platforms an entry is for; combine them with `|`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform(u8);

impl Platform {
    pub const MAC_OS: Platform = Platform(1);
    pub const WINDOWS: Platform = Platform(2);
    pub const LINUX: Platform = Platform(4);
    pub const ALL: Platform = Platform(7);

    /// The platform this runs on.
    pub fn current() -> Platform {
        if cfg!(target_os = "macos") {
            Platform::MAC_OS
        } else if cfg!(target_os = "windows") {
            Platform::WINDOWS
        } else {
            Platform::LINUX
        }
    }

    /// Whether `self` includes `other`.
    pub fn includes(self, other: Platform) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Platform {
    type Output = Platform;
    fn bitor(self, rhs: Platform) -> Platform {
        Platform(self.0 | rhs.0)
    }
}

impl Menu {
    /// An empty menu.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a submenu.
    pub fn submenu(mut self, submenu: Submenu) -> Self {
        self.submenus.push(submenu);
        self
    }

    /// Every item and check item, however deep, with its mark if it's a
    /// check item: what [`AppMenu`] starts from.
    pub(crate) fn items(&self) -> Vec<(&Item, Option<bool>)> {
        fn walk<'a>(entries: &'a [MenuEntry], out: &mut Vec<(&'a Item, Option<bool>)>) {
            for entry in entries {
                match entry {
                    MenuEntry::Item(item) => out.push((item, None)),
                    MenuEntry::Check(item, checked) => out.push((item, Some(*checked))),
                    MenuEntry::Submenu(sub) => walk(&sub.items, out),
                    MenuEntry::System(_) | MenuEntry::Separator => {}
                }
            }
        }
        let mut out = Vec::new();
        for sub in &self.submenus {
            walk(&sub.items, &mut out);
        }
        out
    }

    /// What's wrong with it, said when the app starts (RFC 0025): an id used
    /// twice, an empty label or title.
    pub(crate) fn problems(&self) -> Vec<String> {
        fn titles(entries: &[MenuEntry], out: &mut Vec<String>) {
            for entry in entries {
                if let MenuEntry::Submenu(sub) = entry {
                    if sub.title.trim().is_empty() {
                        out.push("App::menu: a submenu has no title".into());
                    }
                    titles(&sub.items, out);
                }
            }
        }
        let mut problems = Vec::new();
        for sub in &self.submenus {
            if sub.title.trim().is_empty() {
                problems.push("App::menu: a submenu has no title".into());
            }
            titles(&sub.items, &mut problems);
        }
        let mut seen: Vec<&str> = Vec::new();
        for (item, _) in self.items() {
            if item.label.trim().is_empty() {
                problems.push(format!("App::menu: the item `{}` has no label", item.id));
            }
            if seen.contains(&item.id.as_str()) {
                problems.push(format!(
                    "App::menu: the id `{}` is used twice; each item needs its own, to say which was clicked",
                    item.id
                ));
            } else {
                seen.push(&item.id);
            }
        }
        problems.dedup();
        problems
    }
}

impl Submenu {
    /// A submenu with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
            only: Platform::ALL,
        }
    }

    /// Append a clickable item (emits `elyra:menu` with `id`).
    pub fn item(self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.entry(Item::new(id, label))
    }

    /// Append a clickable item with a keyboard accelerator (e.g. `"CmdOrCtrl+S"`).
    pub fn item_accel(
        self,
        id: impl Into<String>,
        label: impl Into<String>,
        accelerator: impl Into<String>,
    ) -> Self {
        self.entry(Item::new(id, label).accelerator(accelerator))
    }

    /// Append an [`Item`] as configured: an accelerator, greyed out at start,
    /// for one platform.
    pub fn entry(mut self, item: Item) -> Self {
        self.items.push(MenuEntry::Item(item));
        self
    }

    /// Append a check item, marked or not at start. A click says its id; the
    /// mark is the app's to set ([`AppMenu::set`], `menu.set(..)`), so it
    /// shows the app's state, never a toggle the menu guessed.
    pub fn check(self, id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        self.check_item(Item::new(id, label), checked)
    }

    /// [`check`](Self::check), from a configured [`Item`].
    pub fn check_item(mut self, item: Item, checked: bool) -> Self {
        self.items.push(MenuEntry::Check(item, checked));
        self
    }

    /// Append one of the platform's own items.
    pub fn system(mut self, item: SystemItem) -> Self {
        self.items.push(MenuEntry::System(item));
        self
    }

    /// Append a submenu inside this one.
    pub fn submenu(mut self, submenu: Submenu) -> Self {
        self.items.push(MenuEntry::Submenu(submenu));
        self
    }

    /// Append a separator line.
    pub fn separator(mut self) -> Self {
        self.items.push(MenuEntry::Separator);
        self
    }

    /// Only on these platforms: the whole submenu is left out elsewhere.
    pub fn only(mut self, platforms: Platform) -> Self {
        self.only = platforms;
        self
    }
}

/// A change to an item's state: what's `Some` changes, the rest stays.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MenuState {
    #[serde(default)]
    pub enabled: Option<bool>,
    /// A check item's mark; ignored on an item that isn't one.
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub label: Option<String>,
}

/// An item's state now, as [`AppMenu::state`] gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MenuItemState {
    pub enabled: bool,
    /// `None` for an item that isn't a check item.
    pub checked: Option<bool>,
    pub label: String,
}

/// Who [`AppMenu`] tells of a change: the id of the item.
type Changed = Box<dyn Fn(&str) + Send + Sync>;

/// The menu's items as they are now, bound in the container: change one with
/// [`set`](Self::set), and every window's menu follows (RFC 0055).
pub struct AppMenu {
    items: Mutex<HashMap<String, MenuItemState>>,
    /// Told of each change; the window's loop, once there is one.
    changed: Mutex<Option<Changed>>,
}

impl AppMenu {
    pub(crate) fn new(menu: Option<&Menu>) -> Self {
        let items = menu
            .map(|m| {
                m.items()
                    .into_iter()
                    .map(|(item, checked)| {
                        (
                            item.id.clone(),
                            MenuItemState {
                                enabled: item.enabled,
                                checked,
                                label: item.label.clone(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            items: Mutex::new(items),
            changed: Mutex::new(None),
        }
    }

    /// Who's told of each change: the event loop, which applies it to every
    /// window's menu.
    pub(crate) fn on_change(&self, tell: impl Fn(&str) + Send + Sync + 'static) {
        *self.changed.lock() = Some(Box::new(tell));
    }

    /// Change the item `id`: greyed out or not, its mark (a check item's),
    /// its label. `false` when the menu has no such item.
    pub fn set(&self, id: &str, state: MenuState) -> bool {
        {
            let mut items = self.items.lock();
            let Some(item) = items.get_mut(id) else {
                return false;
            };
            if let Some(enabled) = state.enabled {
                item.enabled = enabled;
            }
            if let (Some(checked), Some(mark)) = (state.checked, item.checked.as_mut()) {
                *mark = checked;
            }
            if let Some(label) = state.label {
                item.label = label;
            }
        }
        if let Some(tell) = self.changed.lock().as_ref() {
            tell(id);
        }
        true
    }

    /// The item `id` as it is now.
    pub fn state(&self, id: &str) -> Option<MenuItemState> {
        self.items.lock().get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_WORDS: [MenuWord; 18] = [
        MenuWord::Edit,
        MenuWord::Help,
        MenuWord::About,
        MenuWord::Diagnostics,
        MenuWord::System(SystemItem::Undo),
        MenuWord::System(SystemItem::Redo),
        MenuWord::System(SystemItem::Cut),
        MenuWord::System(SystemItem::Copy),
        MenuWord::System(SystemItem::Paste),
        MenuWord::System(SystemItem::SelectAll),
        MenuWord::System(SystemItem::Minimize),
        MenuWord::System(SystemItem::Fullscreen),
        MenuWord::System(SystemItem::CloseWindow),
        MenuWord::System(SystemItem::Hide),
        MenuWord::System(SystemItem::HideOthers),
        MenuWord::System(SystemItem::ShowAll),
        MenuWord::System(SystemItem::Services),
        MenuWord::System(SystemItem::Quit),
    ];

    #[test]
    fn the_menus_words_are_the_catalogs() {
        let en: serde_json::Value = serde_json::from_str(include_str!("../lang/en.json")).unwrap();
        let menu = en["elyra"]["menu"].as_object().unwrap();
        assert_eq!(menu.len(), ALL_WORDS.len());
        for word in ALL_WORDS {
            let (key, english) = word.word();
            assert_eq!(menu[key], english, "{key}");
        }
    }

    #[test]
    fn the_menus_follow_the_apps_language_and_keep_the_platforms_english() {
        use crate::i18n::Translator;
        let t = Translator::new("en")
            .add_json("en", "{}")
            .add_json("nb", "{}");
        let copy = MenuWord::System(SystemItem::Copy);
        assert_eq!(
            copy.translated(Some(&t), "Kver"),
            None,
            "English: the platform's own"
        );
        assert_eq!(MenuWord::About.label(Some(&t), "Kver"), "About Kver");
        assert_eq!(MenuWord::About.label(None, "Kver"), "About Kver");
        t.set_locale("nb");
        assert_eq!(copy.translated(Some(&t), "Kver").as_deref(), Some("Kopier"));
        assert_eq!(
            MenuWord::System(SystemItem::Quit).label(Some(&t), "Kver"),
            "Avslutt Kver"
        );
        assert_eq!(MenuWord::Edit.label(Some(&t), "Kver"), "Rediger");
    }

    fn menu() -> Menu {
        Menu::new()
            .submenu(
                Submenu::new("Document")
                    .item("doc.open", "Open")
                    .entry(
                        Item::new("doc.save", "Save")
                            .accelerator("CmdOrCtrl+S")
                            .enabled(false),
                    )
                    .check("doc.track", "Track changes", true)
                    .system(SystemItem::Copy)
                    .submenu(Submenu::new("Zoom").item("zoom.in", "Zoom in"))
                    .separator(),
            )
            .submenu(
                Submenu::new("Mac")
                    .only(Platform::MAC_OS)
                    .item("mac.only", "Only here"),
            )
    }

    #[test]
    fn every_item_however_deep_starts_as_declared() {
        let state = AppMenu::new(Some(&menu()));
        assert_eq!(
            state.state("doc.save"),
            Some(MenuItemState {
                enabled: false,
                checked: None,
                label: "Save".into()
            })
        );
        assert_eq!(state.state("doc.track").unwrap().checked, Some(true));
        assert!(state.state("zoom.in").is_some(), "a nested submenu's item");
        assert!(state.state("nope").is_none());
    }

    #[test]
    fn a_change_is_kept_and_told_and_a_mark_only_on_a_check_item() {
        let state = AppMenu::new(Some(&menu()));
        let told = std::sync::Arc::new(Mutex::new(Vec::new()));
        let heard = told.clone();
        state.on_change(move |id| heard.lock().push(id.to_owned()));
        assert!(state.set(
            "doc.save",
            MenuState {
                enabled: Some(true),
                label: Some("Save Draft".into()),
                ..Default::default()
            }
        ));
        assert_eq!(
            state.state("doc.save"),
            Some(MenuItemState {
                enabled: true,
                checked: None,
                label: "Save Draft".into()
            })
        );
        // A mark on an item that isn't a check item: ignored.
        state.set(
            "doc.open",
            MenuState {
                checked: Some(true),
                ..Default::default()
            },
        );
        assert_eq!(state.state("doc.open").unwrap().checked, None);
        state.set(
            "doc.track",
            MenuState {
                checked: Some(false),
                ..Default::default()
            },
        );
        assert_eq!(state.state("doc.track").unwrap().checked, Some(false));
        assert!(!state.set("nope", MenuState::default()));
        assert_eq!(*told.lock(), ["doc.save", "doc.open", "doc.track"]);
    }

    #[test]
    fn a_menu_that_would_mislead_is_a_problem() {
        assert!(menu().problems().is_empty());
        let twice = Menu::new().submenu(
            Submenu::new("A")
                .item("x", "One")
                .submenu(Submenu::new("B").check("x", "Two", false)),
        );
        assert!(twice.problems()[0].contains("`x` is used twice"));
        let blank = Menu::new().submenu(Submenu::new(" ").item("y", ""));
        let problems = blank.problems();
        assert!(problems.iter().any(|p| p.contains("no title")));
        assert!(problems.iter().any(|p| p.contains("`y` has no label")));
    }

    #[test]
    fn platforms_combine() {
        let elsewhere = Platform::WINDOWS | Platform::LINUX;
        assert!(elsewhere.includes(Platform::LINUX));
        assert!(!elsewhere.includes(Platform::MAC_OS));
        assert!(Platform::ALL.includes(Platform::current()));
    }
}
