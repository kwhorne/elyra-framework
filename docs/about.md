# About dialog

Elyra ships a built-in **About dialog** — the small window that shows your app's
name, version, description, and links. You provide the metadata once on the Rust
side; the framework serves it and `@elyra/runtime` renders a themed dialog.

## Set the metadata

Call [`App::about`] with an [`AboutInfo`]:

```rust
use elyra::{AboutInfo, App};

App::new()
    .title("BlogWriter")
    .about(
        AboutInfo::new("BlogWriter", env!("CARGO_PKG_VERSION"))
            .description("Generate and publish blog articles on a schedule.")
            .website("elyracode.com")
            .repository("github.com/kwhorne/blogwriter")
            .author("Knut W. Horne", "kwhorne.com")
            .icon("/icon.svg"),
    )
    .run()
```

Only `name` and `version` are required; unset rows are omitted from the dialog.
If you skip `.about(...)` entirely, `name` falls back to the primary window
title and the dialog still works with a built-in Elyra icon.

| Setter | Row |
|---|---|
| `.description(..)` | paragraph under the title |
| `.website(url)` | **Website** |
| `.repository(url)` | **GitHub** |
| `.author(name, url)` | **Developed by** |
| `.icon(path)` | icon at the top (defaults to a built-in mark) |

## Open it

**macOS** — the standard **About &lt;App&gt;** application-menu item opens the
dialog automatically. The shell replaces the system panel with a custom item
that emits an `elyra:about` event; `@elyra/runtime` listens for it as soon as it
is imported, so this works with no extra wiring.

**Any platform / from a button** — import `openAbout` and call it:

```svelte
<script>
  import { openAbout } from "@elyra/runtime";
</script>

<button onclick={() => openAbout()}>About</button>
```

`openAbout()` fetches the metadata from the Rust side (the private `/__about`
endpoint). You can also pass an `AboutInfo` object to render without a fetch.
`closeAbout()` dismisses it (also on `Escape` or an overlay click).

## Theming

The dialog reads CSS custom properties from your app when present, falling back
to a dark palette otherwise:

| Variable | Used for | Fallback |
|---|---|---|
| `--surface` / `--panel` | card background | `#1e2030` |
| `--bg` | row / button background | `#16161e` |
| `--text` | primary text | `#c0caf5` |
| `--muted` | labels, secondary text | `#787c99` |
| `--accent` | links, focus | `#7aa2f7` |
| `--border` | hairlines | `rgba(255,255,255,.08)` |

The scaffold's Grove theme defines these, so a new app's dialog matches its
window out of the box.

## How it works

- `App::about(..)` stores the [`AboutInfo`]; the shell serves it as MessagePack
  at `elyra://localhost/__about` (a named map → a JS object).
- On macOS the app menu's About item carries the id `__elyra_about`; clicking it
  routes through the event loop and the shell emits `elyra:about` on the
  [`EventBus`] with the metadata as the payload.
- `@elyra/runtime` subscribes to `elyra:about` on import and renders the dialog;
  `openAbout()` is the same renderer, callable directly.

[`App::about`]: ../framework/src/app.rs
[`AboutInfo`]: ../framework/src/about.rs
[`EventBus`]: events.md
