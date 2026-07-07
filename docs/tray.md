# System tray

Feature-gated behind `tray` (tao dropped its built-in tray in 0.35, so this
wraps the `tray-icon` crate).

```toml
elyra = { version = "0.1", features = ["tray"] }
```

## Configuring the tray

```rust
use elyra::TrayConfig;

App::new()
    .tray(
        TrayConfig::new()
            .tooltip("My App")
            .title("My App")            // short text next to the icon (macOS/Linux)
            .item("open", "Open")       // custom item -> emits "open" on the "tray" channel
            .separator()
            .quit("Quit"),              // closes the app
    )
    .run();
```

Builder: `new()`, `tooltip`, `title`, `item(id, label)`, `separator()`,
`quit(label)`. A simple solid-color icon is generated, so no image asset is
required.

## Handling clicks (frontend)

A custom item click emits its `id` on the `"tray"` [event channel](events.md); a
`Quit` item closes the app.

```svelte
<script>
  import { channel } from "@elyra/runtime";
  const tray = channel("tray");   // e.g. "open"
  $effect(() => { if ($tray === "open") { /* focus / navigate */ } });
</script>
```

## Notes

- The tray is created after the event loop initializes (required on macOS) and
  held for the program's lifetime.
- Menu clicks are routed through the event loop and re-emitted on the bus, so
  your frontend reacts the same way it does to any other event.

## Related

- [Events](events.md) · [Windows](windows.md)
