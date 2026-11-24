# Contributing to Elyra

Thanks for your interest in improving Elyra! This document covers how to build,
test, and submit changes.

## Prerequisites

- **Rust** (stable, edition 2021; MSRV **1.94**, set by sqlx 0.9) — install via
  [rustup](https://rustup.rs).
- **Node.js** 20.19+ or 22.12+ (Vite 8's floor) and npm — for the frontend
  runtime, the UI and scaffolded apps.
- macOS, Linux, or Windows. The GUI shell (tao + wry) needs the platform
  webview; on Linux, install the packages listed in
  [getting started](docs/getting-started.md) (WebKitGTK, GTK 3, `libxdo`, …).

## Getting started

```bash
git clone https://github.com/kwhorne/elyra-framework
cd elyra-framework

# Build and test the whole workspace.
cargo test --workspace --all-features

# Run the demo app (serves a built-in fallback page, no npm needed).
cargo run -p elyra-example
```

The repository layout and architecture are documented in
[`docs/`](docs/README.md) — start with
[getting started](docs/getting-started.md) and
[architecture](docs/architecture.md).

## Before you open a pull request

Every change must be formatted, lint-clean, and tested. CI runs these, on
macOS, Linux and Windows, among its other jobs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

If you changed anything about `#[command]` signatures, regenerate the example
bindings so the typed `api.*` facade stays in sync:

```bash
cargo run -p ratatosk -- codegen   # or: rata codegen
```

For frontend changes, type-check and test the runtime, and test the UI:

```bash
(cd runtime && npm ci && npm run typecheck && npm test)
(cd ui && npm ci && npm test)
```

## Pull request guidelines

- Branch off `main`; `main` is protected, so open a PR rather than pushing to it.
- Keep PRs focused; one logical change per PR is easier to review.
- Write a clear description: what changed, why, and how you verified it.
- Update docs and [`CHANGELOG.md`](CHANGELOG.md) (under `[Unreleased]`) when
  behavior changes.
- Be honest in the PR about what is tested vs. what is only compile-checked or
  smoke-tested — Elyra's docs call this out and PRs should too.

## Commit messages

Use a short imperative summary line (≤ 72 chars), then a body explaining the
_why_. Reference issues with `#123` where relevant.

## Reporting bugs / requesting features

Use the [issue templates](https://github.com/kwhorne/elyra-framework/issues/new/choose).
For security issues, **do not** open a public issue — see [SECURITY.md](SECURITY.md).

## License

By contributing, you agree that your contributions will be licensed under the
[MIT License](LICENSE), without any additional terms or conditions.
