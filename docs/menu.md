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
