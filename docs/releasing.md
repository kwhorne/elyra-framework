# Releasing

Elyra is released as a **tagged GitHub release**. The crates are not published to
crates.io, so the source tree (at a tag) is the distribution.

## 1. Pre-flight

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
(cd runtime && npm ci && npm run typecheck && npm test && npm run build)
rustup run 1.94.0 cargo check --workspace --all-features   # the declared MSRV
```

Bump the version in **two** places, so a checkout of the tag is self-consistent:

- `Cargo.toml` `[workspace.package] version` (all crates inherit it)
- `runtime/package.json` `version`

Then `cargo update -w` so `Cargo.lock` follows, move the `[Unreleased]` changelog
section under the new version + date, and update the README status block.

## 2. Wait for green CI

Eight jobs: clippy + test on macOS/Linux/Windows, MSRV, `cargo deny`, the
MySQL/Postgres model tests, the `rata new` smoke test, and the runtime job.

**Don't tag before they're green.** The matrix is the only thing that compiles the
per-platform code (app menu, deep-link registration, autostart, config paths) —
when it was introduced it immediately found six latent cross-platform bugs.

## 3. Tag and release

```bash
git tag -a v0.5.7 -m "Elyra Framework v0.5.7 …"
git push origin v0.5.7

gh release create v0.5.7 --title "v0.5.7 — …" --notes-file <(…changelog section…)
```

Point the release notes at the changelog section for the version, and call out
breaking changes explicitly — pre-1.0 minors may contain them.

## Consuming a release

Since nothing is published to a registry, consumers depend on the repository:

```toml
[dependencies]
elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.5.7" }
```

This is what `rata new` scaffolds by default. For the frontend it points
`@elyra/runtime` at the tarball attached to the release:

```json
"@elyra/runtime": "https://github.com/kwhorne/elyra-framework/releases/download/v0.5.7/elyra-runtime-0.5.7.tgz"
```

npm accepts a remote tarball URL; it cannot install a subdirectory of a git
repository (which is what `runtime/` is), so **the release must carry that asset**:

```bash
(cd runtime && npm ci && npm run build && npm pack)
gh release upload v0.5.7 runtime/elyra-runtime-0.5.7.tgz
```

`rata new --elyra <path-to-framework>` instead wires the project to a local
checkout (path + `file:` dependencies) for framework development.

## Registries (not used)

If that ever changes, note that two crate names are already taken on crates.io by
unrelated projects: **`substrate-core`** (a hexagonal-architecture crate) and
**`ratatosk`** (a CLI for debugging Unleash SDKs). Free at the time of writing:
`elyra`, `elyra-db`, `elyra-macros`, `elyra-ai`, plus `elyra-substrate` /
`substrate-contracts` and `rata` / `elyra-cli` as alternatives for the two
collisions. The crates already carry the metadata (`description`, `license`,
`keywords`, `categories`, `homepage`, `readme`) and internal dependencies carry a
`version` alongside `path`, so they are publish-ready apart from the names.

`.github/workflows/publish-runtime.yml` can publish `@elyra/runtime` to npm, but
it is **manual only** (`workflow_dispatch`) and needs an `NPM_TOKEN` secret — a
tag does not trigger it.

## Not in scope

Code signing, Apple Developer ID, notarization, MSI/NSIS installers and
AppImage/Flatpak belong to each application's own pipeline. `rata bundle`
produces a local ad-hoc-signed `.app`, a `.deb`, and portable archives.
