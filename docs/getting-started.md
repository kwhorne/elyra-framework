# Getting started

## Prerequisites

- **Rust** (stable, 1.94+) and Cargo — the floor comes from sqlx 0.9.
- **Node.js** 20.19+ or 22.12+ (Vite 8's floor) and npm — for the Svelte frontend.
- macOS is the primary target today (tao/wry use system WebKit). Linux and
  Windows compile and are covered by CI, but are less exercised in practice.
- **Linux build dependencies**: WebKitGTK, GTK 3, and `libxdo` (the last one for
  the application menu). On Debian/Ubuntu:

  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev pkg-config
  # plus libayatana-appindicator3-dev for the `tray` feature
  # and libdbus-1-dev for `system` (notifications)
  ```

## Scaffold a project

```bash
rata new myapp                 # or: rata new myapp --elyra ./path/to/framework
cd myapp
```

This generates a self-contained workspace:

```
myapp/
├── Cargo.toml          # bin crate `myapp`, its own [workspace]
├── elyra.toml          # Ratatosk project descriptor
├── src/main.rs         # commands + App
└── app/                # Vite + Svelte 5 frontend
    ├── package.json
    ├── vite.config.js
    ├── index.html
    └── src/
        ├── main.js
        ├── App.svelte
        ├── app.css     # default theme (Tokyo Night palette)
        └── theme.js    # auto / light / dark switching
```

### Default theme

New projects ship a **default theme** — the Tokyo Night palette (the same colors
as Grove / Elyra Conductor), as CSS variables in `app.css`. It's **dark by
default**, with a light variant via `[data-theme="light"]`, and `theme.js`
provides `auto` / `light` / `dark` switching (persisted to `localStorage`;
`auto` follows the OS). The starter `App.svelte` includes a theme toggle — edit
`app.css` to make it your own.

> By default the generated project depends on the matching **tagged GitHub
> release** — `elyra = { git = "…", tag = "v0.5.8" }` for Rust, and the
> `@elyra/runtime` tarball attached to that release for the frontend. Elyra isn't
> published to crates.io or npm; see [releasing](releasing.md).
>
> `--elyra <path>` instead points the generated `Cargo.toml` at a local checkout
> and wires `@elyra/runtime` to its sibling `runtime/` via a `file:` dependency,
> so both `cargo` and `npm install` work offline. Use it when developing the
> framework itself.

## Run it

```bash
cd app && npm install && npm run build && cd ..   # build the frontend once
rata codegen                                       # generate typed bindings
cargo run                                          # launch the window
```

Or during development, with Vite HMR:

```bash
rata dev
```

The Rust binary embeds the built frontend (`rust-embed`) and serves it from
memory over the `elyra://localhost` custom protocol, with an `ETag` per asset so
reloads are cheap. Before you've built the frontend, the shell serves a built-in
fallback page so `cargo run` works alone.

Everything under `elyra://localhost/__*` (commands, events, the facades) is gated
by a per-run IPC token that the shell injects into the webview.
`@elyra/runtime` attaches it for you — if you hand-roll a `fetch`, read
[security](security.md) first.

## A first command

`src/main.rs`:

```rust
use elyra::{command, commands, App, Ctx};

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

fn main() -> elyra::Result<()> {
    App::new()
        .title("myapp")
        .commands(commands![greet])
        .assets(elyra::asset_resolver::<Assets>())
        .run()
}
```

Frontend (`app/src/App.svelte`):

```svelte
<script>
  import { invoke } from "@elyra/runtime";
  let name = $state("world");
  let out = $state("");
</script>
<input bind:value={name} />
<button onclick={async () => (out = await invoke("greet", name))}>greet</button>
<p>{out}</p>
```

After `rata codegen` you also get a typed facade:

```ts
import { api } from "./bindings";
const out = await api.greet(name); // (name: string) => Promise<string>
```

## Next

- [Commands](commands.md) and [the container](container-and-providers.md).
- [Database](database.md) + [migrations](migrations.md) + [models](models.md).
- [The CLI](cli.md) and [configuration](configuration.md) (`elyra.toml` + `Config`).
- [Testing](testing.md) — `TestApp` runs commands without a window.
- [Security](security.md) — capabilities, CSP, and what the frontend may reach.
