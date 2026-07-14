//! Native application menu.
//!
//! Describe custom submenus with [`Menu`] / [`Submenu`] and pass them to
//! [`App::menu`]. They're appended after the standard app + Edit menus. Clicking
//! an item emits the `elyra:menu` event carrying the item's id, which
//! `@elyra/runtime` surfaces via `onMenu`.
//!
//! This is data-only (no platform types), so it compiles everywhere; the menu is
//! rendered on **macOS** (the app menu bar). Menu bars on Windows/Linux are a
//! later addition.
//!
//! ```ignore
//! App::new().menu(
//!     Menu::new().submenu(
//!         Submenu::new("File")
//!             .item_accel("file.new", "New", "CmdOrCtrl+N")
//!             .item_accel("file.save", "Save", "CmdOrCtrl+S")
//!             .separator()
//!             .item("file.export", "Export…"),
//!     ),
//! )
//! ```
//!
//! [`App::menu`]: crate::App::menu

/// A native menu: an ordered list of [`Submenu`]s.
#[derive(Default, Clone)]
pub struct Menu {
    pub(crate) submenus: Vec<Submenu>,
}

/// A titled submenu of [entries](MenuEntry).
#[derive(Clone)]
pub struct Submenu {
    pub(crate) title: String,
    pub(crate) items: Vec<MenuEntry>,
}

#[derive(Clone)]
pub(crate) enum MenuEntry {
    Item {
        id: String,
        label: String,
        accelerator: Option<String>,
    },
    Separator,
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
}

impl Submenu {
    /// A submenu with the given title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            items: Vec::new(),
        }
    }

    /// Append a clickable item (emits `elyra:menu` with `id`).
    pub fn item(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        self.items.push(MenuEntry::Item {
            id: id.into(),
            label: label.into(),
            accelerator: None,
        });
        self
    }

    /// Append a clickable item with a keyboard accelerator (e.g. `"CmdOrCtrl+S"`).
    pub fn item_accel(
        mut self,
        id: impl Into<String>,
        label: impl Into<String>,
        accelerator: impl Into<String>,
    ) -> Self {
        self.items.push(MenuEntry::Item {
            id: id.into(),
            label: label.into(),
            accelerator: Some(accelerator.into()),
        });
        self
    }

    /// Append a separator line.
    pub fn separator(mut self) -> Self {
        self.items.push(MenuEntry::Separator);
        self
    }
}
