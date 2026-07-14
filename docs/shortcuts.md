# Global shortcuts

Feature-gated behind `shortcuts`. Register OS-level keyboard shortcuts that fire
even when the app isn't focused.

```toml
elyra = { version = "0.3", features = ["shortcuts"] }
```

```rust
use elyra::App;

App::new()
    .title("MyApp")
    .global_shortcut("CmdOrCtrl+Shift+P")
    .global_shortcut("CmdOrCtrl+Alt+K")
    .run()
```

When a shortcut fires, the shell emits the `elyra:shortcut` event carrying the
accelerator string; subscribe from the frontend:

```ts
import { onShortcut } from "@elyra/runtime";

const off = onShortcut((accelerator) => {
  if (accelerator === "CmdOrCtrl+Shift+P") openCommandPalette();
});
```

## Accelerator syntax

Modifiers joined with `+`, then a key:

- Modifiers: `CmdOrCtrl` / `CommandOrControl`, `Cmd`/`Super`, `Ctrl`/`Control`,
  `Alt`/`Option`, `Shift`.
- Keys: letters (`P`), digits (`1`), function keys (`F5`), and named keys
  (`Space`, `Enter`, `ArrowUp`, …).

Registration failures (an invalid or already-taken accelerator) are logged and
skipped; the rest still register.

## Related

- [UI components](components.md) — pair a shortcut with the ⌘K command palette.
- [System integration](system.md)
