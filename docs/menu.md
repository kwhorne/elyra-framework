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

## Platform support

The menu is rendered on **macOS** (the application menu bar), alongside the
built-in app/Edit menus and the [About](about.md) item. Windows/Linux menu bars
attached per-window are a later addition; `App::menu` is accepted on all
platforms and simply not shown elsewhere for now.

## Related

- [Global shortcuts](shortcuts.md) · [System tray](tray.md) · [Windows](windows.md)
