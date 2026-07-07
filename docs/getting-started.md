# Getting started

## Prerequisites

- **Rust** (stable, 1.80+) and Cargo.
- **Node.js** + npm (for the Svelte frontend / Vite).
- macOS is the primary target today (tao/wry use system WebKit). Linux/Windows
  compile but are less exercised.

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

> `--elyra <path>` points the generated `Cargo.toml` at a local checkout of the
> framework **and** wires `@elyra/runtime` to its sibling `runtime/` via a
> `file:` dependency, so both `cargo` and `npm install` work offline (useful
> pre-publish). Without it, published versions are referenced.

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
memory over the `elyra://localhost` custom protocol. Before you've built the
frontend, the shell serves a built-in fallback page so `cargo run` works alone.

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
- [The CLI](cli.md) and [`elyra.toml`](configuration.md).
