# Configuration

Two halves: **`elyra.toml`**, the project descriptor `rata` reads, and
**[`Config`](#config-at-runtime)**, the runtime facade your app reads.

## `elyra.toml`

Ratatosk reads `elyra.toml` from the current directory or any parent (walking
up). It describes where your app crate and frontend live, plus codegen, bundle,
and database settings.

```toml
[app]
crate = "myapp"                  # the cargo package containing your App

[frontend]
dir = "app"                      # directory with package.json / vite

[codegen]
out = "app/src/bindings.ts"      # where `rata codegen` writes bindings
                                 # (default: "<frontend.dir>/src/bindings.ts")

[bundle]                         # optional; used by `rata bundle`
identifier = "com.example.myapp" # default: "com.example.<app.crate>"
name = "My App"                  # default: <app.crate>
version = "0.1.0"                # default: "0.1.0"
icon = "assets/icon.svg"         # source icon (.svg or raster)
deep_link = "myapp"              # registers myapp:// (Info.plist / .desktop)
description = "A demo app"       # Linux package metadata
maintainer = "Dev <dev@x.io>"    # .deb Maintainer:
publisher = "Example AS"         # Windows: the .exe's company, the installer's publisher

[database]                       # optional; used by `rata migrate`
url = "sqlite://app.db?mode=rwc" # supports ${VAR} expansion; else $DATABASE_URL
migrations = "migrations"        # migrations directory (default: "migrations")
```

## Sections

### `[app]`
- **`crate`** (required) — the cargo package name of your binary. `rata` runs
  `cargo run/build -p <crate>`.

### `[frontend]`
- **`dir`** (required) — folder containing `package.json` / `vite.config.js`.

### `[codegen]`
- **`out`** — path for the generated `bindings.ts`.

### `[bundle]`
- **`identifier`**, **`name`**, **`version`** — bundle metadata for `rata bundle`
  (macOS `Info.plist`, `.deb` control file, the Windows `.exe`'s version and
  its installer; the identifier keys the installer's entry in Settings → Apps).
  Pair the identifier with `App::identifier("com.example.myapp")`: Windows
  shows the app's notifications under it ([system](system.md#notes--platform-caveats)).
- **`icon`** — source icon, an SVG or a PNG; becomes `AppIcon.icns` on macOS,
  the `.exe`'s icon and an `.ico` on Windows, and `hicolor` PNGs and the
  AppImage's icon on Linux. See [App icon](cli.md#app-icon).
- **`deep_link`** — a custom URL scheme. Written to `CFBundleURLTypes` on macOS,
  `MimeType=x-scheme-handler/<scheme>` in the Linux `.desktop` entry, and
  registered by the Windows installer. Pair it with
  `App::deep_link("myapp")`; without the bundle entry the OS never routes the URL
  to your app.
- **`description`**, **`maintainer`** — Linux package metadata.
- **`publisher`** — who publishes the app: the Windows `.exe`'s company name
  and the publisher Settings → Apps shows for its installer.

### `[database]`
- **`url`** — a connection URL; the scheme selects the driver (`sqlite:`,
  `mysql:`, `postgres:`). `${VAR}` occurrences are expanded from the
  environment. If omitted, `rata` falls back to the `DATABASE_URL` env var.
- **`migrations`** — the migrations directory, relative to the project root.

## `Config` at runtime

`elyra.toml` is a *build/tooling* descriptor; `Config` is what your app reads at
runtime. Bind it with `ConfigProvider` and resolve it like any service:

```rust
use elyra::config::ConfigProvider;

App::new()
    .provider(ConfigProvider::new().with_default("app.theme", "dark"))
    .commands(commands![..])
    .run()
```

```rust
#[command]
async fn settings(ctx: Ctx) -> Vec<String> {
    let config = ctx.get::<elyra::Config>();
    let url = config.string("database.url").unwrap_or_default();
    let debug = config.bool("app.debug").unwrap_or(false);
    let port: u16 = config.get_or("server.port", 8080);
    let db = config.section("database"); // BTreeMap<String, String>
    vec![url, debug.to_string(), port.to_string(), db.len().to_string()]
}
```

### Sources, lowest priority first

1. defaults registered with `ConfigProvider::with_default` / `Config::default_value`
2. `elyra.toml` (tables flattened to dotted keys: `[app] crate` → `app.crate`)
3. `config/*.toml` (the file stem prefixes its keys: `config/services.toml`
   `[mail] from` → `services.mail.from`)
4. `.env`
5. real process environment variables

A later source wins, so a shipped default can always be overridden by an env var
without rebuilding.

### Keys

Dotted and lowercase. Environment variables map in by lowercasing, dropping an
`ELYRA_` prefix and turning separators into dots:

| Env var | Key |
|---|---|
| `ELYRA_DATABASE__URL` | `database.url` |
| `DATABASE_URL` | `database.url` |
| `APP__DEBUG` | `app.debug` |

`${VAR}` (and `$VAR`) inside a value is expanded from the environment, matching
what `rata` already does for `[database] url`.

### Accessors

`raw` · `string` · `get::<T>()` · `get_or` · `bool` (`1/true/yes/on`) · `int` ·
`has` · `section(prefix)` · `all()` · `set` · `default_value`.

## The app's configuration, checked at start

Some calls on `App` need another one: `stay_in_tray()` needs a tray,
`Backend::notifications()`, `presence(..)` and `listen(..)` need
broadcasting, `UpdaterConfig::from_backend(..)` needs a backend, an updater
needs a way to verify what it downloads, a command's middleware names
must resolve, and the app's and the tray's icons must be readable PNGs,
each named icon named once ([RFC 0028](proposals/0028-tray-and-icons.md)). Elyra checks all of them before anything starts, and reports
them together (RFC 0025):

```text
Error: the app's configuration has 2 problems:
  - App::stay_in_tray() needs a tray: add .tray(TrayConfig::new()..), or the app would run with no way to reach it
  - Backend::listen(..) needs broadcasting to listen to the channels: add .broadcasting(Broadcasting::pusher(key)) to the Backend
```

`App::run()` returns them as `Error::Config` (so `fn main() -> elyra::Result<()>`
prints the list and exits non-zero), `App::prepare()` panics with them, and
`App::problems()` lists them without starting anything, for a
[test](testing.md#the-apps-configuration). What a provider binds (the
database for `Backend::offline()`, an assistant) is checked once the
providers have run.

## Environment variables

| Variable | Read by | Purpose |
|---|---|---|
| `ELYRA_DEV_URL` | the app | Load the webview from this URL (set by `rata dev`); also the only origin that gets CORS |
| `ELYRA_CODEGEN_OUT` | the app | Write bindings and exit (set by `rata codegen`) |
| `ELYRA_MIGRATE` | the app | `up` / `down` / `status` — migrate (Rust migrations and the SQL files) and exit; `rata migrate` sets it when the project has Rust migrations |
| `ELYRA_MIGRATIONS_DIR` | the app | The SQL migrations directory for `ELYRA_MIGRATE` (default `migrations`; set by `rata migrate`) |
| `ELYRA_SEED` | the app | Run registered seeders and exit |
| `ELYRA_LOG` | the app | Log level: `error`/`warn`/`info`/`debug`/`trace`/`off` |
| `DATABASE_URL` | `rata migrate`, the app | Fallback DB URL when `[database].url` is unset |

`ELYRA_DEV_URL` and `ELYRA_CODEGEN_OUT` are normally set for you by the CLI; you
rarely set them by hand.

## Related

- [Logging](logging.md) — `ELYRA_LOG` and the file sink
- [Migrations](migrations.md) — `ELYRA_MIGRATE` and seeders
- [CLI](cli.md) · [Security](security.md)
