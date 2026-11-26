# RFC 0055 — Menus that change: check items, state and the system's items

**Status:** implemented (2026-11-26) · **Target:** 0.57.0 · builds on [the application menu](../menu.md) · see [Decisions](#decisions)

## Summary

The app's menu is fixed when the app starts. Each item is a label, an id
and maybe an accelerator, and clicking it says the id on `elyra:menu`. A
document app's menu isn't like that. "Track changes" has a check mark that
follows the document, "Save" is greyed out with nothing to save, "View" has
a submenu, and Cut, Copy and Paste must be the system's own items, because
a menu click isn't a user gesture and the page can't copy on its behalf.
kver, porting from Tauri, has all of these. Elyra has none of them.

This RFC adds them, declared in Rust as the menu is now, and changed at
runtime from the page or from Rust:

```rust
App::new().menu(
    Menu::new()
        .submenu(
            Submenu::new("Edit")
                .system(SystemItem::Undo)
                .separator()
                .system(SystemItem::Cut)
                .system(SystemItem::Copy)
                .system(SystemItem::Paste)
                .system(SystemItem::SelectAll),
        )
        .submenu(
            Submenu::new("Document")
                .item(Item::new("doc.save", "Save").accelerator("CmdOrCtrl+S").enabled(false))
                .check("doc.track", "Track changes", false)
                .submenu(Submenu::new("Zoom").item(Item::new("zoom.in", "Zoom in")).item(Item::new("zoom.out", "Zoom out")))
                .separator()
                .item(Item::new("doc.close", "Close").only(Platform::Windows | Platform::Linux)),
        ),
)
```

```ts
import { menu, onMenu } from "@elyra/runtime";

onMenu((id) => run(id));                         // as today
menu.set("doc.save", { enabled: dirty });        // greyed out, or not
menu.set("doc.track", { checked: tracking });    // the check mark
menu.set("doc.save", { label: "Save Draft" });   // a new label
```

- **Check items**: `.check(id, label, checked)`. A click says its id as any
  item does; the mark is the app's to set, so it always shows the app's
  state, not a toggle the menu guessed.
- **State at runtime**: `enabled`, `checked` and `label` by id, from the
  page (`menu.set`) or from Rust (`ctx.get::<AppMenu>().set(..)`), on every
  window's menu bar at once.
- **The system's items**: `SystemItem::{Undo, Redo, Cut, Copy, Paste,
  SelectAll, Minimize, Fullscreen, CloseWindow, Hide, HideOthers, ShowAll,
  Services, Quit}`, through the platform's own responder chain.
- **Nested submenus**, and items for one platform alone (`.only(..)`).

## Motivation

- **Dogfood.** kver's menu (`src/lib/menu.svelte.ts`) has check items whose
  state follows the document, the system's Cut/Copy/Paste, nested submenus
  and entries for macOS or for the others. It's the next gap the port hit
  after opening documents (RFC 0052).
- **A greyed-out item is information**: what the app can do now. A menu
  that offers Save with nothing to save, or Undo with nothing to undo,
  sends the user to find out by trying.
- **The system's items can't be imitated**: the page can't copy from a menu
  click (it isn't a user gesture), so Cut/Copy/Paste only work as the
  platform's own items.

## Current state (0.56)

- `Menu`/`Submenu` (`framework/src/menu.rs`): items (`id`, `label`,
  optional accelerator) and separators, appended after the standard app
  menu and Edit menu (macOS) or as a per-window menu bar (Windows, Linux).
- `shell/webview.rs` builds muda menus from it: `MenuItem::with_id`,
  always enabled; no check items, no nesting, no system items besides the
  fixed Edit menu on macOS and the Help menu.
- A click goes out as `elyra:menu` with the id; `onMenu` in the runtime.
- Nothing changes an item after it's built.

## Design

### Declaring

```rust
pub struct Item { /* id, label, accelerator, enabled, only */ }
impl Item {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self;
    pub fn accelerator(self, a: impl Into<String>) -> Self;
    pub fn enabled(self, on: bool) -> Self;
    pub fn only(self, platforms: Platform) -> Self;
}

impl Submenu {
    pub fn item(self, item: impl Into<Item>) -> Self;        // `item("id", "Label")` still works
    pub fn item_accel(self, id, label, accelerator) -> Self; // as today
    pub fn check(self, id, label, checked: bool) -> Self;
    pub fn check_item(self, item: Item, checked: bool) -> Self;
    pub fn system(self, item: SystemItem) -> Self;
    pub fn submenu(self, submenu: Submenu) -> Self;
    pub fn separator(self) -> Self;
    pub fn only(self, platforms: Platform) -> Self;          // the whole submenu
}
```

`Platform` is a small set (`MacOs`, `Windows`, `Linux`, combinable). An
entry for another platform is left out when the menu is built. `(&str,
&str)` converts into an `Item`, so existing `.item("id", "Label")` calls
compile unchanged.

The ids are checked when the app starts, among its configuration problems
(RFC 0025): an id used twice, a check item with an accelerator another item
has, or an empty label is a problem.

### Changing

```rust
pub struct MenuState { pub enabled: Option<bool>, pub checked: Option<bool>, pub label: Option<String> }

impl AppMenu {                       // bound in the container
    pub fn set(&self, id: &str, state: MenuState) -> bool; // false: no such item
    pub fn state(&self, id: &str) -> Option<MenuStateNow>;
}
```

- `AppMenu` keeps the current state of each item by id. A change goes to
  the event loop (`UserEvent::Menu`), which applies it to every muda item
  with that id: the app menu on macOS, each window's bar elsewhere. A
  window opened later builds its bar from the current state.
- `checked` on an item that isn't a check item, or a `label` for a system
  item, is ignored and logged once.
- **From the page**: `menu.set(id, { enabled?, checked?, label? })` posts
  to `/__menu/set`, a native route under a new `Capability::Menu`, granted
  by default (it changes how the app's own menu looks, nothing else). A
  read-only window (Quick Look) can't. `menu.state(id)` reads it back.
- **A click on a check item** says its id and the state it was in; the
  mark doesn't flip by itself (open question 1).

### Testing

- **Unit**: the builder (nesting, `only`, system items, `(&str, &str)` into
  `Item`), the configuration problems, `AppMenu`'s state and its changes
  (an unknown id, `checked` on a plain item).
- **Route**: `TestShell` posting `/__menu/set`, its capability, and a
  read-only window refused.
- **The running app in CI**: the example app gets a check item and a
  disabled item; `rata mcp dev`'s snapshot doesn't see native menus, so the
  walk-throughs check `AppMenu::state` through a debug command after the
  page sets it. The menu's look is checked by hand on each platform.
- **Runtime**: `menu.set` and `menu.state` against the stub.

## Implementation plan

1. `menu.rs`: `Item`, `SystemItem`, `Platform`, nested submenus, check
   items; the configuration problems.
2. `shell/webview.rs`: build them in muda (`CheckMenuItem`,
   `PredefinedMenuItem`, nested `Submenu`), keep each window's items by id,
   apply `UserEvent::Menu`.
3. `AppMenu` in the container; `/__menu/set` and `/__menu/state`;
   `Capability::Menu`.
4. `menu` in `@elyra/runtime`; the example; docs (`menu.md`), CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **A check item clicked: does the mark flip by itself?**
   *Recommendation:* no. The click says the id (and `checked` as it was),
   and the app sets the mark from its own state. A mark that flips by
   itself is wrong whenever the app refuses the change (no document open,
   a read-only one), and kver already sets the mark from its state.
2. **The whole menu from the page** (`menu.define(tree)`), as kver builds
   it in TypeScript today?
   *Recommendation:* not in this RFC. The structure stays in Rust, where
   the app menu, the Help menu and `rata check` see it, and the page sets
   the state, which is what changes. A port moves its tree into Rust once;
   its labels can still come from translations.
3. **`Capability::Menu` granted by default?**
   *Recommendation:* yes. It only changes the app's own menu's look (what's
   greyed out, checked, how it's labelled); a click still only says an id
   to the page. An app can deny it like any capability.
4. **Undo and Redo as system items**: they drive WebKit's own undo manager,
   which knows nothing of an app's document model (kver's reason for not
   using them).
   *Recommendation:* offer them, and say in the docs when not to: an app
   with its own history declares its own Undo item and enables it from its
   state.

## Not in this RFC

- **Context menus**: the runtime has `contextMenu` already.
- **The tray's menu**: its own builder (`TrayConfig`).
- **Icons in menu items** and radio groups.
- **A menu built from the page** (open question 2).

## Alternatives considered

- **Rebuild the whole menu on every change**: simple, but each window's bar
  flickers and loses an open submenu, and muda can replace a menu only as a
  whole.
- **Only `enabled` and `checked` from Rust**: the page is where most of
  the state is (a document's dirty flag, a selection), so it would need a
  command per item to set it.

## As built

As designed, with these specifics:

- **`Submenu::item(id, label)` keeps its two arguments**, and a configured
  item goes in with **`Submenu::entry(Item)`**: a conversion from `(&str,
  &str)` would have needed every existing call written as a tuple. `check`
  and `check_item` likewise. `Platform` is a small set of consts
  (`MAC_OS`, `WINDOWS`, `LINUX`, `ALL`) combined with `|`.
- **`elyra:menu` still says the id alone**, not the mark as it was: a
  second field would have changed every `onMenu` handler. The page that
  wants the mark reads `menu.state(id)`, or knows its own state.
- **`AppMenu` is bound in `prepare`**, so `TestShell` and an app without
  the window have it; the event loop gives it a way to say each change
  (`UserEvent::Menu`) when it starts.
- **The native items are kept by id** in a thread-local on the event
  loop's thread (muda's items aren't `Send`), every window's copy under its
  id; a change sets `enabled`, the label and a check item's mark on all of
  them, and a window built later reads the state as it is. After a click
  on a check item, its mark is set again from the app's state, since the
  platform flips it on its own.
- **`SystemItem` is `#[non_exhaustive]`**; `Undo` and `Redo` come with the
  docs' note on when not to use them.
- **Tests**: the builder, `AppMenu`'s state and changes, the configuration
  problems (`menu.rs`); `/__menu/set` and `/__menu/state` through
  `TestShell`, a read-only window refused a change, `Capability::Menu`
  denied (`framework/tests/menu.rs`); `menu.set` and `menu.state` in the
  runtime. The example app's menu has a check item, an item greyed out
  until the page enables it, nested submenus and system items, and its page
  sets state on load: the walk-throughs build and run it on macOS, Linux
  and Windows. What the menu bar looks like after a change isn't read in
  CI (the walk-throughs see the page, not native menus). On macOS it was
  read through System Events: the page's `menu.set` relabelled and enabled
  Save, a click on the check item put the mark there through the page and
  a second took it away, and the nested submenu has its items. Windows' and
  Linux's menu bars haven't been looked at.

## Decisions

1. **A check item's mark doesn't flip by itself**: the click says the id
   and the mark as it was, and the app sets it.
2. **The structure stays in Rust**; the page sets the state. No
   `menu.define(tree)` in this RFC.
3. **`Capability::Menu` is granted by default**; a read-only window can't
   use it.
4. **Undo and Redo are offered as system items**, and the docs say when an
   app with its own history declares its own instead.
