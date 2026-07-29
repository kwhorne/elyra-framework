# Ratatosk — the `rata` CLI

Ratatosk is Elyra's Artisan: the squirrel that carries messages between the Rust
root and the Svelte crown. Most commands read [`elyra.toml`](configuration.md)
from the current directory or any parent.

```
rata <command>
```

| Command | What it does |
|---|---|
| `new <name>` | Scaffold a new workspace + Svelte app |
| `dev` | Start Vite + launch the app against it (HMR) |
| `codegen` | specta → TypeScript types + the typed `api.*` facade |
| `build` | Vite build → embedded assets → release binary |
| `bundle` | Package the release binary (`.app` / `.deb` / portable folder) |
| `migrate` | Apply pending database migrations |
| `migrate:rollback` | Roll back the most recent batch |
| `migrate:status` | Show applied/pending migrations |
| `make:migration <name>` | Scaffold `up`/`down` `.sql` files (`--rust` for a schema-builder migration) |
| `help` | Show usage |

## `rata new`

```bash
rata new myapp [--elyra <path-to-framework-crate>] [--dir <parent>]
```

- `--elyra <path>` — depend on a local framework checkout (`elyra = { path = .. }`)
  instead of a published version, **and** wire the frontend's `@elyra/runtime`
  to the sibling `runtime/` via a `file:` dependency, so `npm install` + build
  work offline. Handy pre-publish / for contributing. Without it, published
  versions are referenced.
- `--dir <parent>` — where to create the project (default: current directory).

The generated project is its own `[workspace]`, so it builds anywhere.

## `rata dev`

Spawns `npm run dev` in the frontend directory, waits for `:5173`, then runs the
app with `ELYRA_DEV_URL=http://localhost:5173` so the webview loads from Vite for
hot reloading. IPC still targets `elyra://localhost`; CORS headers are added for
that exact origin **only while `ELYRA_DEV_URL` is set** (a production build sends
none — see [security](security.md)). Vite is torn down when the app exits.

## `rata codegen`

Runs the app in codegen mode (`ELYRA_CODEGEN_OUT`), which writes the bindings and
exits before opening a window. Output path comes from `[codegen].out`. See
[codegen](codegen.md).

## `rata build`

1. `npm run build` in the frontend dir (emits `dist/`).
2. `cargo build --release -p <app crate>` (embeds `dist/`).

## `rata bundle`

Builds release, then packages it for the **host platform**. Metadata comes from
`[bundle]` in [`elyra.toml`](configuration.md).

| Host | Output |
|---|---|
| macOS | `target/release/bundle/<Name>.app` with `Info.plist` + `PkgInfo`, ad-hoc code-signed (`codesign -s -`) so it launches locally |
| Linux | `<package>_<version>.deb` (built without `dpkg`) + a portable `.tar.gz`, including the `.desktop` entry and `hicolor` icon |
| Windows | a portable folder with the `.exe`, icon and a `README.txt` |

With `[bundle].deep_link = "myapp"` the macOS `Info.plist` gets
`CFBundleURLTypes` and the Linux `.desktop` entry gets
`MimeType=x-scheme-handler/myapp` — without that, the scheme registered by
`App::deep_link` never reaches the app.

Out of scope (they need per-project certificates and external toolchains): real
Developer ID signing + notarization, MSI (WiX) / NSIS installers, and
AppImage/Flatpak.

### App icon

The bundle generates the native dock/Finder icon: it renders the source image
into `Contents/Resources/AppIcon.icns` (via `sips` + `iconutil`; SVGs are
rasterized at 1024 with `qlmanage`) and sets `CFBundleIconFile`. Point at a
source with `[bundle].icon`, or drop one at a conventional path:

```toml
[bundle]
name = "My App"
icon = "app/public/icon.svg"   # .svg or a raster image (png, …)
```

Auto-detected if `icon` is omitted: `app/public/icon.svg`, `app/public/icon.png`,
`assets/icon.png`, `assets/icon.svg`, `icon.png`, `icon.svg` (scaffolded apps
ship `app/public/icon.svg`, so this works out of the box). Icon generation is
best-effort — if the tooling or a source image is missing, the bundle still
builds with the default icon.

## Migrations

`migrate`, `migrate:rollback`, `migrate:status`, and `make:migration` connect
directly to the database (no app binary needed), reading `[database]` from
`elyra.toml`. See [migrations](migrations.md).

```bash
rata make:migration create_users            # up/down .sql files
rata make:migration create_users --rust     # a RustMigration using the schema builder
rata migrate
rata migrate:status
rata migrate:rollback
```

Rust migrations and seeders live in the app, so they run through the app binary:

```bash
ELYRA_MIGRATE=up   cargo run    # apply Rust migrations
ELYRA_MIGRATE=down cargo run    # roll back the last batch
ELYRA_SEED=1       cargo run    # run registered seeders
```

## Generators (`make:*`)

Artisan-style scaffolding. Each writes one source file under `src/` and prints
the wiring step Rust needs (a `mod` line + registration) — `rata` never rewrites
`main.rs` for you.

```bash
rata make:command greet_user   # -> src/greet_user.rs   (#[command] handler)
rata make:provider Payments    # -> src/payments.rs     (PaymentsProvider)
rata make:model BlogPost       # -> src/blog_post.rs    (#[derive(Model)], table "blog_posts")
```

Names are normalized: `BlogPost`/`blog post` → file `blog_post.rs`, struct
`BlogPost`; model table names are pluralized (`Category` → `categories`). Existing
files are never overwritten.
