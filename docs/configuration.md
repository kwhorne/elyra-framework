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
  (macOS `Info.plist`, `.deb` control file, Windows folder name).
- **`icon`** — source icon; becomes `AppIcon.icns` on macOS and a `hicolor` icon
  in the `.deb`.
- **`deep_link`** — a custom URL scheme. Written to `CFBundleURLTypes` on macOS and
  `MimeType=x-scheme-handler/<scheme>` in the Linux `.desktop` entry. Pair it with
  `App::deep_link("myapp")`; without the bundle entry the OS never routes the URL
  to your app.
- **`description`**, **`maintainer`** — Linux package metadata.

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

## Environment variables

| Variable | Read by | Purpose |
|---|---|---|
| `ELYRA_DEV_URL` | the app | Load the webview from this URL (set by `rata dev`); also the only origin that gets CORS |
| `ELYRA_CODEGEN_OUT` | the app | Write bindings and exit (set by `rata codegen`) |
| `ELYRA_MIGRATE` | the app | `up` / `down` — run Rust migrations and exit |
| `ELYRA_SEED` | the app | Run registered seeders and exit |
| `ELYRA_LOG` | the app | Log level: `error`/`warn`/`info`/`debug`/`trace`/`off` |
| `DATABASE_URL` | `rata migrate`, the app | Fallback DB URL when `[database].url` is unset |

`ELYRA_DEV_URL` and `ELYRA_CODEGEN_OUT` are normally set for you by the CLI; you
rarely set them by hand.

## Related

- [Logging](logging.md) — `ELYRA_LOG` and the file sink
- [Migrations](migrations.md) — `ELYRA_MIGRATE` and seeders
- [CLI](cli.md) · [Security](security.md)
