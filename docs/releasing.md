# Releasing

Elyra is released as a **tagged GitHub release**. The crates are not published to
crates.io, so the source tree (at a tag) is the distribution.

## 1. Pre-flight

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo test --workspace --all-features
(cd runtime && npm ci && npm run typecheck && npm test && npm run build)
rustup run 1.94.0 cargo check --workspace --all-features   # the declared MSRV
```

Bump the version everywhere it's written, so a checkout of the tag is
self-consistent (`git grep` the old one to be sure):

- `Cargo.toml`: `[workspace.package] version` (all crates inherit it), and the
  `version` beside each internal `path` dependency
- `runtime/package.json`, and `runtime/package-lock.json` (twice)
- `example/app/package-lock.json` (the linked `@elyra/runtime`)
- the version in the README's install snippet, and the tag in
  `docs/getting-started.md`
- the examples in this file

Then `cargo update -w` so `Cargo.lock` follows, and move the `[Unreleased]`
changelog section under the new version + date, with an intro, an
**Upgrading** note and the compare links at the bottom. Update the README
status block and its test counts (`cargo test --workspace --all-features`,
`npm test` in `runtime/`), and the roadmap's shipped list.

## 2. Wait for green CI

Ten jobs: clippy + test on macOS/Linux/Windows, MSRV, `cargo deny`, the
MySQL/Postgres model tests, the `rata new` smoke test, the runtime job, the
MCP Inspector against the example app, and the backend against a real Laravel
app.

**Don't tag before they're green.** The matrix is the only thing that compiles the
per-platform code (app menu, deep-link registration, autostart, config paths) —
when it was introduced it immediately found six latent cross-platform bugs.

## 3. Tag and release

```bash
git tag -a v0.16.0 -m "Elyra Framework v0.16.0 …"
git push origin v0.16.0

gh release create v0.16.0 --title "v0.16.0 — …" --notes-file <(…changelog section…)
```

Point the release notes at the changelog section for the version, and call out
breaking changes explicitly — pre-1.0 minors may contain them. Attach the
runtime tarball (see [below](#consuming-a-release)) in the same command by
naming it after the flags.

## 4. Verify from the tag

What a user gets from the release, scaffolded from the tag itself:

```bash
git worktree add /tmp/at-tag v0.16.0
(cd /tmp/at-tag && cargo build -p ratatosk --release)

# The default: Elyra from the git tag, the runtime from the release asset
/tmp/at-tag/target/release/rata new plain --dir /tmp/check   # creates /tmp/check/plain
(cd /tmp/check/plain/app && npm install && npm run build) && (cd /tmp/check/plain && cargo check)

# The whole resource workflow, against the tag's own checkout
(cd /tmp/at-tag/runtime && npm ci && npm run build)          # `--elyra` links it with `file:`
/tmp/at-tag/target/release/rata new fromtag --elyra /tmp/at-tag/framework --dir /tmp/check
/tmp/at-tag/scripts/smoke-resource.sh /tmp/check/fromtag /tmp/at-tag/target/release/rata

git worktree remove --force /tmp/at-tag
```

`--dir` is the parent: the project is the folder named after the app inside
it.

## Consuming a release

Since nothing is published to a registry, consumers depend on the repository:

```toml
[dependencies]
elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.16.0" }
```

This is what `rata new` scaffolds by default. For the frontend it points
`@elyra/runtime` at the tarball attached to the release:

```json
"@elyra/runtime": "https://github.com/kwhorne/elyra-framework/releases/download/v0.16.0/elyra-runtime-0.16.0.tgz"
```

npm accepts a remote tarball URL; it cannot install a subdirectory of a git
repository (which is what `runtime/` is), so **the release must carry that asset**:

```bash
(cd runtime && npm ci && npm run build && npm pack)
gh release upload v0.16.0 runtime/elyra-runtime-0.16.0.tgz
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
