# Releasing

Elyra ships as six crates plus one npm package. This is the order and the
checklist; the framework itself deliberately doesn't automate signing or
distribution (see [roadmap](roadmap.md)).

## 1. Pre-flight

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
(cd runtime && npm ci && npm run typecheck && npm test && npm run build)
rustup run 1.94.0 cargo check --workspace --all-features   # the declared MSRV
```

Bump the version in **two** places — they must match, and the npm publish
workflow verifies the tag against `package.json`:

- `Cargo.toml` `[workspace.package] version` (all crates inherit it)
- `runtime/package.json` `version`

Then `cargo update -w` so `Cargo.lock` follows, move the `[Unreleased]` changelog
section to the new version, and update the README status block.

## 2. Wait for green CI

Eight jobs: clippy+test on macOS/Linux/Windows, MSRV, `cargo deny`, the
MySQL/Postgres model tests, the `rata new` smoke test, and the runtime job.
**Don't tag before they're green** — the matrix is the only thing that compiles
the per-platform code (app menu, deep-link registration, autostart, paths).

## 3. Tag

```bash
git tag -a v0.5.7 -m "Elyra Framework v0.5.7 …"
git push origin v0.5.7
```

The tag triggers `.github/workflows/publish-runtime.yml`, which publishes
`@elyra/runtime` to npm with provenance. It needs an `NPM_TOKEN` repository
secret (an npm automation token with publish rights on the `@elyra` scope).

## 4. crates.io, in dependency order

Each crate must be on the registry before anything that depends on it, because
`cargo publish` verifies dependencies against the index:

```bash
cargo publish -p substrate-core
cargo publish -p elyra-db
cargo publish -p elyra-macros
cargo publish -p elyra-ai
cargo publish -p elyra
cargo publish -p ratatosk
```

`elyra-example` is `publish = false`.

Dry-run first (`cargo publish -p <crate> --dry-run`). Note that the dry run for
`elyra` and `ratatosk` fails with *"no matching package named …"* until their
Elyra dependencies are actually published — that's expected, not a problem with
the package.

### Name availability

Two of the current crate names are **taken on crates.io by unrelated projects**
and must be renamed before that crate can be published:

| Crate | Status | Free alternatives |
|---|---|---|
| `substrate-core` | taken (`0.3.10`, "Hexagonal core contracts for substrate") | `elyra-substrate`, `substrate-contracts` |
| `ratatosk` | taken (`0.1.0`, "CLI for debugging Unleash SDKs") | `rata` (matches the binary name), `elyra-cli` |

`elyra`, `elyra-db`, `elyra-macros` and `elyra-ai` are free.

A rename touches: the crate's `Cargo.toml` `name`, the workspace dependency entry,
`use` paths (`substrate_core::` in the framework's cache/storage/queue), the
`pub use substrate_core as substrate` re-export, `docs/substrate.md`, the crate
list in `docs/architecture.md`, and the README layout block. The binary name
(`rata`) is set by `[[bin]]` and doesn't have to match the crate name.

## 5. After publishing

- Verify a scaffolded project builds against the *published* versions:
  `rata new smoke && cd smoke && cargo check` (without `--elyra`).
- Check the docs.rs build for `elyra` (all features are documented via
  `docs.rs` metadata; a failure there is usually a missing system library).
- Draft the GitHub release from the changelog section.

## Not in scope

Code signing, Apple Developer ID, notarization, MSI/NSIS installers and
AppImage/Flatpak belong to each application's own pipeline. `rata bundle`
produces a local ad-hoc-signed `.app`, a `.deb`, and portable archives.
