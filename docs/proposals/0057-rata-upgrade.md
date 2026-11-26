# RFC 0057 — `rata upgrade`: one version, everywhere it's written

**Status:** implemented (2026-11-26) · **Target:** 0.58.0 · builds on [releasing](../releasing.md) and [`rata check`](0036-rata-check.md) · see [Decisions](#decisions)

## Summary

An Elyra app names the framework's version in four places, and none of them
knows about the others:

- `Cargo.toml`: `elyra = { git = "…", tag = "v0.57.0" }`, and `elyra-ai` or
  `elyra-db` beside it when the app uses them;
- `app/package.json`: the `@elyra/runtime` tarball,
  `…/releases/download/v0.57.0/elyra-runtime-0.57.0.tgz`;
- `app/package.json`: the `@elyra/ui` tarball, with the version written in it twice;
- the Laravel app: a clone of the repository at `v0.57.0`, which `composer`
  copies `laravel/` from.

Moving the app to a new release means editing them by hand, and then
running `cargo update`, `npm install` and `composer update`. If one is
missed, the app still builds. But then the page's runtime and the Rust
shell disagree about a route, or `@elyra/ui` calls a store that this runtime
doesn't have yet, and the bug only shows when the app runs.

This RFC moves all of them in one command, and lets `rata check` say when
they've drifted apart:

```bash
rata upgrade              # to this rata's version
rata upgrade 0.58.0       # or to a version you name
rata upgrade --dry-run    # what would change, and the Upgrading notes
```

```text
elyra 0.57.0 → 0.58.0
  Cargo.toml          elyra, elyra-ai     tag v0.57.0 → v0.58.0
  app/package.json    @elyra/runtime, @elyra/ui   0.57.0 → 0.58.0
  ../web (Laravel)    elyracode/elyra-backend     v0.57.0 → v0.58.0
  cargo update -p elyra · npm install · composer update elyracode/elyra-backend

Upgrading (0.58.0):
  - An app with an I18nProvider and a catalog in Norwegian now shows
    @elyra/ui's words in Norwegian …
```

```text
$ rata check
error    elyra-versions-differ: the Rust side is 0.58.0, @elyra/runtime 0.57.0.
         Run `rata upgrade 0.58.0`.
```

- **Each place is moved together**, and the lock files are refreshed by the
  tool that owns each one.
- **The CHANGELOG's Upgrading notes** are printed for each release between
  the old version and the new one, because they say what the app must
  change.
- **The Laravel app** is moved too when `[check] backend` points at its
  folder. Otherwise rata prints the steps.
- **`rata check`** reports `elyra-versions-differ` when the Rust side,
  `@elyra/runtime` and `@elyra/ui` aren't the same version (an error). It
  also reports it when the Laravel package is a different version from them
  (a warning).
- **Local checkouts are left as they are.** `path` and `file:` dependencies,
  used when developing the framework itself, are never touched.

## Motivation

- **Dogfood.** The releases come every few days. Every app the user ports has
  the same four edits to make per release, and a missed one is found at run
  time, not at build time.
- **The two halves of the IPC are one version.** The runtime and the shell
  share routes, payload shapes and capabilities. Nothing checks that
  they're the same release: a `@elyra/ui` from 0.57 on a runtime from 0.56
  calls `frameworkWords` and gets `undefined`.
- **The Upgrading notes are where the work is.** Today the app's developer
  reads the CHANGELOG by hand to find which releases they skipped.

## Current state (0.57)

- `rata new` (`ratatosk/src/scaffold.rs`) writes the tag and the two tarball
  URLs at the CLI's own version; `--elyra <path>` writes `path` and `file:`
  instead.
- `laravel/README.md`: a `git clone --branch v0.57.0` beside the Laravel
  app, a `path` repository with `symlink: false`, and `composer require
  "elyracode/elyra-backend:@dev"`. The package doesn't know its own version.
- `rata check` runs the app in its check mode (`ELYRA_CHECK`), and with
  `--backend` reads the Laravel app's `elyra:contract`. Neither says which
  release either side is.
- `rata` itself is installed with `cargo install --git … --tag vX ratatosk`.

## Design

### What it reads

- **Rust:** `Cargo.toml`, plus the workspace's members when it is one: each
  dependency on `elyra`, `elyra-ai`, `elyra-db` or `elyra-macros` with
  `git = "https://github.com/kwhorne/elyra-framework"` and a `tag`. It is
  edited with `toml_edit`, so comments and formatting survive.
- **Frontend:** `<frontend_dir>/package.json`: `@elyra/runtime` and
  `@elyra/ui` whose value is the repository's release tarball URL. The edit
  touches only those strings, so the file's order and indentation stay.
- **Laravel:** `[check] backend` in `elyra.toml`, when it's a folder. Its
  `composer.json` gives the `path` repository's `url`, which is the clone,
  and the clone's `git describe --tags` gives the version.

A dependency in any other form (a `branch`, a `rev`, a `path`, a `file:`) is
listed as left alone, with the reason.

### The version it moves to

By default, the version of the `rata` that is running, as `rata new` does.
That works offline, and it's the version whose codegen and checks the app
gets. `rata upgrade 0.58.0` names another one. rata checks that the tag
exists (`git ls-remote --tags`) before it edits anything.

When the target is newer than `rata` itself, it says to install that rata
first, and how:

```text
rata is 0.57.0; 0.58.0's codegen and checks come with rata 0.58.0:
  cargo install --git https://github.com/kwhorne/elyra-framework --tag v0.58.0 ratatosk --force
```

### What it runs

`cargo update -p elyra` (and each other crate it moved), `npm install` in
the frontend's folder, and, for the Laravel app,
`git -C <clone> fetch --depth 1 origin tag vX && git -C <clone> checkout vX`
followed by `composer update elyracode/elyra-backend` in its folder. Each
step's failure stops what comes after it, and names the step. The edits
made so far are kept, so running it again finishes the job.

### The Upgrading notes

The CHANGELOG at the target tag is fetched from
`raw.githubusercontent.com`, as the repository is the source. rata prints
the **Upgrading** paragraph of each release after the old version, up to
and including the new one. Offline, it prints the CHANGELOG's link instead.

### `rata check`: `elyra-versions-differ`

The app's check shape gets `"elyra": "0.58.0"` (the `elyra` crate's
`CARGO_PKG_VERSION`). rata reads the installed versions from
`node_modules/@elyra/{runtime,ui}/package.json` (else `package-lock.json`),
and from the contract's new `version` with `--backend`:

| Differs | Level | Why |
|---|---|---|
| `@elyra/runtime` or `@elyra/ui` from the Rust side | error | one IPC, one release |
| `@elyra/ui` from `@elyra/runtime` | error | the components call the runtime's stores |
| the Laravel package from the Rust side | warning | an HTTP contract, which `--backend` checks route by route |

### The Laravel package's version

`Elyra\Backend\ElyraBackend::VERSION`, bumped with the rest at a release
(`docs/releasing.md` lists it). `php artisan elyra:contract` writes it as
`version`, and `php artisan about` shows it.

### Testing

- Fixtures: a `Cargo.toml` with comments, git deps on two crates and a
  `path` one; a `package.json` with both tarballs and a `file:` one. The edit
  changes only the tags and versions, and leaves the `path` and `file:` ones.
- The Upgrading paragraphs between two versions, from a CHANGELOG fixture,
  including a skipped release.
- `elyra-versions-differ` from check shapes and installed versions that
  differ in each way.
- In CI, the scaffolded app (`rata new` at the previous tag) upgraded to the
  branch's version with `--dry-run`.

## Implementation plan

1. `ElyraBackend::VERSION`, `elyra:contract`'s `version`, `releasing.md`.
2. `"elyra"` in the check shape; `elyra-versions-differ` in `rata check`.
3. `ratatosk/src/upgrade.rs`: reading the places, `--dry-run`, editing,
   running the tools, the Upgrading notes.
4. Docs (`cli.md`, `getting-started.md`, `laravel/README.md`), CHANGELOG.

## Open questions

Answered in [Decisions](#decisions).

1. **What does `rata upgrade` move to by default: rata's own version, or
   the newest release?**
   *Recommendation:* rata's own. It works offline, and it's the version
   whose codegen and checks the app will get. When a newer release exists,
   rata says so in one line.
2. **Should it touch the Laravel app?**
   *Recommendation:* yes, when `[check] backend` names its folder, the
   setting `rata check --backend` already uses. Otherwise it prints the
   three commands to run there.
3. **Is a version difference an error or a warning in `rata check`?**
   *Recommendation:* an error between the Rust side, `@elyra/runtime` and
   `@elyra/ui`, because they're one IPC. A warning for the Laravel package,
   whose contract `--backend` already checks route by route.
4. **Should the Laravel package carry its own version?**
   *Recommendation:* yes: `ElyraBackend::VERSION`, reported by
   `elyra:contract`. A path repository installed with `@dev` has no other
   way to say which release it is.
5. **Should it also offer to install the newer `rata`?**
   *Recommendation:* no, it prints the command. Replacing the running
   binary is `cargo install`'s job, and the user should see it happen.

## Not in this RFC

- **Downgrading** past a release whose Upgrading notes changed the app's
  code. `rata upgrade 0.56.0` moves the versions, and the notes are the
  reader's to undo.
- **Changing the app's code for it** (codemods). The notes say what to do;
  `rata check` and the compiler find what was missed.
- **The runtime warning at startup** when the page's runtime and the shell
  differ. That's worth doing in debug builds, but separately.

## Alternatives considered

- **Publish to crates.io and npm and let their tools upgrade.** This is
  ruled out: the repository is, and stays, the only source.
- **A version file in the app (`elyra.toml`'s `version = "0.57.0"`), with
  the dependencies derived from it at build time.** That's one place to
  edit, but `cargo` and `npm` read their own files, so it would need a
  build step that rewrites them anyway, and editors would show the old
  ones.
- **Only the check, no upgrade.** It finds the drift, but leaves the four
  edits by hand.

## As built

- `ratatosk/src/upgrade.rs`. The manifests are every `Cargo.toml` of the
  workspace (three folders deep, past `target` and `node_modules`), edited
  with `toml_edit`: only the `tag`'s value changes, its comments and
  layout stay. In `package.json` only the two URLs' text is replaced.
- The current release is the oldest the places name, so an app half moved
  by hand is moved the rest of the way.
- The releases are read with `git ls-remote --tags`, and the CHANGELOG at
  the target's tag with `curl` from `raw.githubusercontent.com`. Offline,
  the tag isn't checked and the notes are a link.
- The Laravel app's clone is the `path` repository whose folder holds
  `elyracode/elyra-backend`'s `composer.json`, and its release is
  `git describe --tags --exact-match`.
- `rata check` reads `@elyra/runtime` and `@elyra/ui` from `node_modules`,
  else from `package-lock.json`. CI holds `ElyraBackend::VERSION` to
  `Cargo.toml`'s, so a release can't forget it.
- **Found on the way**: `toml_edit`'s `Item::get_mut` inserts what it
  doesn't find, so looking up `[workspace.dependencies]` in a crate
  without one wrote `workspace = {}` into it. Tables are now looked up as
  tables.

## Decisions

1. **The default is rata's own version**; a version can be named, and a
   newer release is mentioned in one line.
2. **The Laravel app is moved** when `[check] backend` names its folder;
   otherwise rata prints the steps.
3. **`elyra-versions-differ` is an error** between the Rust side,
   `@elyra/runtime` and `@elyra/ui`, and **a warning** for the Laravel
   package.
4. **The Laravel package carries its version**, `ElyraBackend::VERSION`,
   reported by `elyra:contract`.
5. **rata doesn't install itself**: it prints the `cargo install` command.
