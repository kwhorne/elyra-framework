# Application menu

Define custom submenus with [`Menu`](../framework/src/menu.rs) / `Submenu` and
pass them to `App::menu`. They're appended after the standard app + Edit menus,
and clicking an item emits the `elyra:menu` event carrying the item id.

```rust
use elyra::{App, Menu, Submenu};

App::new()
    .title("MyApp")
    .menu(
        Menu::new()
            .submenu(
                Submenu::new("File")
                    .item_accel("file.new", "New", "CmdOrCtrl+N")
                    .item_accel("file.save", "Save", "CmdOrCtrl+S")
                    .separator()
                    .item("file.export", "Export…"),
            )
            .submenu(
                Submenu::new("View")
                    .item_accel("view.reload", "Reload", "CmdOrCtrl+R"),
            ),
    )
    .run()
```

Handle clicks on the frontend:

```ts
import { onMenu } from "@elyra/runtime";

onMenu((id) => {
  switch (id) {
    case "file.new": /* … */ break;
    case "file.export": /* … */ break;
  }
});
```

Accelerators use the same syntax as [global shortcuts](shortcuts.md)
(`CmdOrCtrl+S`, `Shift+Alt+F`, …).

## Items that change

A document app's menu follows the document: Save greyed out with nothing to
save, a check mark on Track changes
([RFC 0055](proposals/0055-menus-that-change.md)).

```rust
use elyra::{Item, Menu, Platform, Submenu, SystemItem};

Menu::new()
    .submenu(
        Submenu::new("Edit")
            .system(SystemItem::Cut)
            .system(SystemItem::Copy)
            .system(SystemItem::Paste)
            .system(SystemItem::SelectAll),
    )
    .submenu(
        Submenu::new("Document")
            .entry(Item::new("doc.save", "Save").accelerator("CmdOrCtrl+S").enabled(false))
            .check("doc.track", "Track changes", false)
            .submenu(Submenu::new("Zoom").item("zoom.in", "Zoom In").item("zoom.out", "Zoom Out"))
            .separator()
            .entry(Item::new("doc.close", "Close").only(Platform::WINDOWS | Platform::LINUX)),
    )
```

```ts
import { menu, onMenu } from "@elyra/runtime";

onMenu((id) => run(id));
menu.set("doc.save", { enabled: dirty });
menu.set("doc.track", { checked: tracking });
menu.set("doc.save", { label: "Save Draft" });
await menu.state("doc.track"); // { enabled, checked, label }
```

- **Check items** (`.check(id, label, checked)`) say their id when
  clicked, as any item does. **The mark doesn't flip by itself**: the app
  sets it from its own state, so it never shows a change the app refused.
- **State at runtime**: `enabled`, `checked` and `label` by id, from the page
  (`menu.set`) or from Rust (`ctx.get::<AppMenu>().set(id, MenuState { .. })`),
  on every window's menu bar at once. A window opened later starts with
  the state as it is.
- **The platform's own items** (`SystemItem`): `Undo`, `Redo`, `Cut`,
  `Copy`, `Paste`, `SelectAll`, `Minimize`, `Fullscreen`, `CloseWindow`,
  and on macOS `Hide`, `HideOthers`, `ShowAll`, `Services`. Cut, Copy and
  Paste have to be these: a menu click isn't a user gesture, so the page
  can't copy on its behalf. `Undo` and `Redo` drive the webview's own undo,
  which knows nothing of an app's document model: an app with its own
  history declares its own items and enables them from its state.
- **`SystemItem::Print`** ("Print…", ⌘P / Ctrl+P) asks the window in
  front to print: the page's `onPrint` handler says what, else the window
  prints ([printing](printing.md)). Only in a menu that adds it.
- **Submenus nest**, and an item or a submenu can be for some platforms
  alone (`.only(Platform::MAC_OS)`).
- **An id used twice**, or an item or submenu with no label, stops the app
  at start with the other [configuration problems](configuration.md#the-apps-configuration-checked-at-start).
- **`Capability::Menu`** covers `/__menu/*`, granted by default: it changes
  how the app's own menu looks, and a click still only says an id. A
  [read-only window](windows.md#only-reading) can read the state, not
  change it.

## Platform support

| Platform | How it renders |
|---|---|
| macOS | the application menu bar, alongside the built-in app/Edit menus and the [About](about.md) item |
| Windows | a per-window menu bar (`init_for_hwnd`), plus a `Help ▸ About <App>` item |
| Linux | a per-window GTK menu bar, plus a `Help ▸ About <App>` item |

Item clicks arrive on `elyra:menu` on every platform. The macOS Edit menu (which is
what makes ⌘C/⌘V/⌘X reach the webview) is macOS-only by nature; elsewhere the
clipboard shortcuts are handled by the platform itself.

## Related

- [Global shortcuts](shortcuts.md) · [System tray](tray.md) · [Windows](windows.md)
