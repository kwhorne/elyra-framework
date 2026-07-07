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
| `bundle` | Package the release binary into a macOS `.app` |
| `migrate` | Apply pending database migrations |
| `migrate:rollback` | Roll back the most recent batch |
| `migrate:status` | Show applied/pending migrations |
| `make:migration <name>` | Scaffold `up`/`down` migration files |
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
hot reloading. IPC still targets `elyra://localhost` (CORS headers are added for
the cross-origin dev case). Vite is torn down when the app exits.

## `rata codegen`

Runs the app in codegen mode (`ELYRA_CODEGEN_OUT`), which writes the bindings and
exits before opening a window. Output path comes from `[codegen].out`. See
[codegen](codegen.md).

## `rata build`

1. `npm run build` in the frontend dir (emits `dist/`).
2. `cargo build --release -p <app crate>` (embeds `dist/`).

## `rata bundle` (macOS)

Builds release, then assembles `target/release/bundle/<Name>.app` with an
`Info.plist` + `PkgInfo`, and ad-hoc code-signs it (`codesign -s -`) so it
launches locally. Metadata comes from `[bundle]` in `elyra.toml`. Real
Developer ID signing + notarization is left to CI with your certificate.

## Migrations

`migrate`, `migrate:rollback`, `migrate:status`, and `make:migration` connect
directly to the database (no app binary needed), reading `[database]` from
`elyra.toml`. See [migrations](migrations.md).

```bash
rata make:migration create_users
rata migrate
rata migrate:status
rata migrate:rollback
```
