# Configuration — `elyra.toml`

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
- **`identifier`**, **`name`**, **`version`** — macOS `Info.plist` values for
  `rata bundle`.

### `[database]`
- **`url`** — a connection URL; the scheme selects the driver (`sqlite:`,
  `mysql:`, `postgres:`). `${VAR}` occurrences are expanded from the
  environment. If omitted, `rata` falls back to the `DATABASE_URL` env var.
- **`migrations`** — the migrations directory, relative to the project root.

## Environment variables

| Variable | Read by | Purpose |
|---|---|---|
| `ELYRA_DEV_URL` | the app | Load the webview from this URL (set by `rata dev`) |
| `ELYRA_CODEGEN_OUT` | the app | Write bindings and exit (set by `rata codegen`) |
| `DATABASE_URL` | `rata migrate` | Fallback DB URL when `[database].url` is unset |

`ELYRA_DEV_URL` and `ELYRA_CODEGEN_OUT` are normally set for you by the CLI; you
rarely set them by hand.
