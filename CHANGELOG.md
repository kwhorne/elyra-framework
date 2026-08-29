# Changelog

All notable changes to Elyra Framework are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While Elyra is pre-1.0, minor versions may contain breaking changes; these are
called out under **Changed** with a migration note.

## [Unreleased]

### Security

- **Per-command abilities — `#[command(can = "…")]`.** `Capability::Commands` is
  one grant covering all of `/__cmd/*`, so a single XSS reached *every* command an
  app registered. A command can now declare an ability, which the app must grant
  explicitly:

  ```rust
  #[command(can = "posts.delete")]
  async fn delete_post(ctx: Ctx, id: i64) -> elyra::Result<()> { /* … */ }

  App::new().allow_ability("posts.delete");   // or .allow_abilities([..]) / "posts.*"
  ```

  Deny-by-default: without the grant the call answers `403` naming the missing
  ability. Commands without `can = ..` are unchanged, so this is opt-in per
  command. Rust-side dispatch (providers, queue jobs, `TestApp`) is never gated —
  the same split as `Sidecar::spawn` vs. frontend sidecar spawn. See
  [docs/security.md](docs/security.md#4-per-command-abilities).

- **`#[command]` validates its attribute.** The attribute token stream used to be
  ignored outright, so `#[command(anything)]` compiled silently. Unknown options
  and malformed abilities are now compile errors.

- **`ForbiddenError` now reports which gate refused.** A 403 always read as
  "missing or invalid IPC token", so an ungranted capability — and now an
  ungranted ability — sent you looking in the wrong place. The runtime reads the
  shell's explanation into the message and exposes it as `error.detail`.

- **`Secrets::get` returns a `Secret`, not a `String`** (feature `secrets`). The
  value wipes its bytes on drop via `zeroize` instead of leaving the plaintext in
  a freed allocation, and its `Debug` renders `Secret(***)` so a stray log line
  can't leak it.

  **Upgrading:** `Secret` derefs to `&str`, so `secrets.get(k)?.as_deref()`,
  `&*secret` and `secret.expose()` all work; only code that bound the result as an
  owned `String` needs `.to_string()` (which escapes the wipe — prefer passing the
  `&str`). `get_or_migrate_env` changes the same way.

### Changed

- **`shell.rs` split into a `shell/` module.** 1 887 lines that mixed protocol,
  routing, access control and the webview lifecycle are now `guard` (token,
  capabilities, rate limits, body limits, CORS, CSP), `protocol` (the wire
  shapes), `router` (dispatch), `facades`, `update`, `assets` and `webview`.
  Purely internal — `shell` is a private module, so no public API moved — but the
  access-control decision is now auditable on its own, with 8 new unit tests over
  `guard::check` that the old layout made awkward to write.

### Added

- **`elyra::prelude`** — `App`, `Ctx`, `Container`, `Provider`, `Result`,
  `EventBus`, the middleware types, `command` + `commands!`, and (behind
  `database`) `Model`/`Query`/`Database`, in one glob import.
- **`rata make:middleware <name>`** — scaffolds a `Middleware` impl and prints the
  `.middleware(..)` wiring step, completing the `make:*` set.
- `ai` is documented in the README feature table (the feature itself shipped in
  0.4.0).

### Testing

- **The frontend runtime went from 9 tests to 56.** `src/index.test.ts` is split
  into `invoke`, `channel`, `system` and `token` suites over a shared
  `test-support.ts`, covering the command framing and header contract, every
  error kind (`command`/`validation`/`panic`/`forbidden`/unknown), the
  cancellable path, the event pump's reconnect, backoff and give-up-on-403
  behaviour, the snake_case `/__sys/*` payloads, and the no-token document.

### Changed

- **Frontend toolchain moved to the current major line.** The `rata new` template
  and the example app now scaffold Vite 8, `@sveltejs/vite-plugin-svelte` 7 and
  Svelte 5.56 (from Vite 6 / plugin 5 / Svelte 5.20). The runtime package builds
  on TypeScript 7 and tests on Vitest 4.

  **Upgrading:** these raise the Node floor to `^20.19 || >=22.12`, declared in
  both `@elyra/runtime` and generated apps. Existing projects keep working on
  their pinned versions; to move, bump the three devDependencies together —
  plugin-svelte 7 requires Vite 8 and Svelte >=5.46 as peers.

- Dependency refresh across the workspace lockfile (tokio 1.53, thiserror 2.0.20,
  ureq 3.4, tray-icon 0.24.2 and friends). No API changes.

## [0.5.7] — 2026-07-29

A **hardening + Laravel-parity** release. The IPC surface is now gated by a
per-run token, an origin rule and a capability model; the event bus, the settings
store, the cache and the queue got the durability they were missing; and the
Laravel-shaped pieces that were absent — `Log`, `Config`, `Secrets`, `TestApp`, a
schema builder, pagination/aggregates/joins/soft deletes/transactions — landed.

**Upgrading:** four behaviour changes need attention, all listed under
**Changed** below (IPC token for hand-written frontends, opt-in capabilities for
destructive routes, `sidecar_allow` for frontend spawn, and the new `Asset`
shape). See [docs/security.md](docs/security.md) for the new model.

### Security

- **IPC token + origin-scoped CORS.** `Access-Control-Allow-Origin: *` was sent
  on *every* response, including `/__cmd/*`, `/__sidecar/*`, `/__sys/*` and
  `/__storage/*` — any origin able to reach the protocol had full access to
  commands, the filesystem, the clipboard and process spawning. Production builds
  now send **no** CORS headers at all, `rata dev` gets CORS for the exact
  `ELYRA_DEV_URL` origin only, and every `/__*` request must carry this run's
  random **IPC token** (injected into the webview as `globalThis.__ELYRA__.token`,
  attached automatically by `@elyra/runtime` as `x-elyra-token`). Rejected
  requests get `403` and surface as `ForbiddenError` on the frontend.
- **Capabilities for the frontend.** Each `/__*` route now maps to a
  `security::Capability`. The everyday ones are granted by default; `StoreClear`,
  `CacheFlush`, `StorageDelete` and `UpdaterInstall` are **opt-in** via
  `App::allow_frontend(..)`, and `deny_frontend(..)` revokes any of them. The
  expensive ones are also rate-limited per window.
- **Sidecar spawn is deny-by-default.** `/__sidecar/spawn` accepted any program +
  args from the frontend (arbitrary code execution from a single XSS). The
  frontend may now only spawn programs named via `App::sidecar_allow(..)`;
  Rust-side `Sidecar::spawn` is unchanged.
- **`shell.open` is policy-gated.** Only `http`/`https`/`mailto` (plus schemes
  added with `App::allow_open_schemes(..)`) are handed to the OS; `file:` URLs,
  relative/non-existent paths, and anything executable (`.exe`, `.sh`, `.app`,
  `.desktop`, …) are refused. `App::deny_open_paths()` blocks local paths too.
- **A strict CSP by default.** `security::DEFAULT_CSP` is served with every HTML
  response unless overridden with `App::csp(..)` or disabled with
  `App::csp_disabled()`.
- **Request bodies are bounded and depth-checked.** 16 MiB by default
  (`App::max_request_body`), and MessagePack nested deeper than 64 levels is
  rejected before it reaches serde's recursive deserializer (a ~10 KB body could
  overflow the stack and abort the process).
- **Single-instance handshake is authenticated.** The rendezvous moved from a
  loopback TCP port with a guessable magic string to a `0600` Unix socket (TCP on
  Windows) gated by a random per-install token, and forwarded deep links are now
  *parsed and validated* instead of prefix-matched.
- **Updater hardening.** HTTPS is required for the manifest and the artifact
  (loopback excepted, `allow_insecure` to opt out), the download streams to disk
  with a `max_artifact_bytes` cap instead of buffering in memory, and a failed
  signature check leaves nothing staged.
- **Secrets.** New `secrets` feature: `Secrets` stores tokens in the OS keychain
  (macOS Keychain, Windows Credential Manager, Linux Secret Service) instead of
  the clear-text settings file, with `get_or_migrate_env` to move off env vars.
- New `docs/security.md`: the IPC surface, the three gating mechanisms, dangerous
  operations, and a release checklist.

### Added

- **`Log` facade.** Levels, targets, ISO-8601 timestamps, a rotating file sink
  (`LogProvider::to_app_dir`), `ELYRA_LOG` for runtime control, and `log_path()`
  for a "send us your log" button. Every command dispatch is traced with its
  duration; the framework's `eprintln!`s are gone.
- **`Config` layer.** `elyra.toml`, `config/*.toml`, `.env` and process env
  merged into one dotted-key map with `${VAR}` expansion, typed accessors and
  `section()`. Bound via `ConfigProvider`.
- **Test facilities.** `elyra::testing::TestApp` invokes commands through the real
  middleware pipeline without a window (`invoke`, `invoke_ok`, `invoke_err`,
  `invoke_validation_errors`, `events_on`, `assert_emitted`), and `TestShell`
  drives the actual `/__*` routing for IPC-level tests.
- **Schema builder.** `elyra_db::Schema::create/table/drop/rename` renders DDL per
  driver (SQLite/MySQL/Postgres), with `RustMigration` for migrations written in
  Rust and `rata make:migration --rust` to scaffold them.
- **Query-builder parity.** `where_like` / `where_null` / `where_not_null` /
  `where_between` / `or_where_eq`, chained `order_by`, `offset`, `join` /
  `left_join`, `count` / `sum` / `avg` / `min` / `max` / `exists`, `paginate`
  (returning `Page`), `chunk`, bulk `update` / `delete`, soft deletes
  (`#[model(soft_deletes)]` + `with_trashed` / `only_trashed` / `restore`), and
  `Database::transaction` / `begin`.
- **Seeders.** `App::seeder(..)` + `ELYRA_SEED=1`; Rust migrations run with
  `ELYRA_MIGRATE=up|down`.
- **Queue maturity.** Retries with exponential backoff, per-job `JobOptions`
  (attempts / backoff / timeout), a failed-jobs list with `retry_failed`,
  `push_later`, typed `dispatch` / `on_typed`, bounded capacity with
  backpressure, and multiple workers (`QueueProvider::with_workers`).
- **Typed event channels + bigint codegen.** `App::event::<T>("channel")` makes
  `rata codegen` emit an `ElyraEvents` map and a narrowed `channel()`;
  `App::codegen_bigint()` exports 64-bit integers as `bigint`.
- **Asset caching + ranges.** Embedded assets are served borrowed (no per-request
  copy) with `ETag`, `304` handling, `immutable` caching for fingerprinted files
  and `Range`/`206` support for media.
- **Cross-platform app menu.** `App::menu(..)` now renders on Windows and Linux
  (per-window menu bar via muda), not just macOS.
- **Bundling beyond macOS.** `rata bundle` builds a `.deb` (no `dpkg` needed) plus
  a portable `.tar.gz` on Linux and a portable folder on Windows, and the macOS
  `Info.plist` now registers `CFBundleURLTypes` from `[bundle].deep_link`.
- **`@elyra/runtime` is publishable.** Built to `dist/` with `.d.ts`, an `exports`
  map, `files`, 9 vitest tests, and a provenance publish workflow.
- **Pool tuning.** `DatabaseOptions` (max connections, acquire timeout, idle /
  lifetime) and SQLite `WAL` + `busy_timeout` + `foreign_keys` by default.
- **CI.** Rust matrix across macOS/Linux/Windows, an MSRV (1.80) job, `cargo deny`
  (advisories / licenses / bans / sources), a `rata new` build smoke test, and
  runtime typecheck + tests + build.

### Fixed

- **Events reached only one window.** The event bus kept a single shared queue, so
  with several windows whichever one polled first took the batch and the others
  silently lost the events. Each webview now identifies itself
  (`x-elyra-client-id`) and gets its own queue; an `emit` fans out to all of them.
  New: `EventBus::next_batch_for`, `disconnect`, `client_count`.
- **A panicking command hung the frontend forever.** Non-cancellable commands were
  dispatched inline in the task owning the protocol responder, so a panic (or a
  missing container binding, which panics by design) dropped the responder without
  a reply and `await invoke(..)` never settled. Commands now always run on their
  own task; a panic becomes a `500` with `x-elyra-error-kind: panic`.
- **Structured errors were unusable.** `Error::Command` prefixed every message with
  `"command failed: "`, so a `ValidationErrors` bag arrived as
  `command failed: {"email":[…]}` and could not be parsed — the documented
  validation flow was broken end to end. Messages are now verbatim, the response
  carries `x-elyra-error-kind`, and the runtime throws a typed `ValidationError`
  with a parsed `errors` bag.
- **`rata new` produced a project that couldn't build.** The scaffold pinned
  `elyra = "0.1"` (against a 0.5.x API) and `@elyra/runtime` `^0.0.0`; both now
  use the CLI's own version.
- **Store writes were neither atomic nor coalesced.** `settings.json` is now
  written to a temp file and renamed (with a `.bak` fallback on read), and bursts
  of `set()` are debounced off the IPC thread instead of writing per call.
- **Cache was unbounded.** Entry-count and byte budgets with LRU eviction
  (`Cache::with_limits`), and TTLs now use the wall clock as well as the monotonic
  one, so a deadline no longer freezes while the machine sleeps.
- **RateLimiter could overshoot its limit.** `attempt()` was a check followed by a
  separate increment; it now uses one atomic `Cache::increment_if_below`.
- **`where_in` and joins.** Qualified `table.column` identifiers are accepted (and
  validated) so joined columns can be filtered and ordered.

### Fixed (post-tag)

- **The cross-platform app menu broke non-`tray` builds off macOS.** `ABOUT_MENU_ID`
  and `UserEvent::MenuClick` were gated on `any(target_os = "macos", feature =
  "tray")`, but the new per-window menu references them unconditionally — so
  `cargo build --features database` failed to compile on Linux and Windows. Both
  are now unconditional. Caught by the new CI matrix and the `rata new` smoke test.
- Internal crates now carry a `version` alongside their `path`, which `cargo
  publish` requires and `cargo deny`'s wildcard check flagged.
- **SQLite URLs built from a filesystem path were broken on Windows.** The obvious
  `format!("sqlite://{}", path.display())` produces a drive colon and backslashes
  where the URL grammar expects an authority and forward slashes, so SQLite
  answered `unable to open database file`. New `elyra_db::sqlite_url(path)` (and
  `sqlite_url_opts`) build it portably; the model tests now use it. This affected
  any app deriving its database path at runtime, not just the tests.
- Linux builds now need `libxdo` (the cross-platform app menu links muda);
  documented in the getting-started prerequisites.
- **`rata new` scaffolded a project that still couldn't build.** Correcting the
  version strings wasn't enough: Elyra isn't published to crates.io or npm, so
  `elyra = "0.5.7"` and `@elyra/runtime@^0.5.7` named packages that don't exist.
  The scaffold now depends on the matching tagged GitHub release — a git
  dependency for the crate, and the `@elyra/runtime` tarball attached to the
  release for the frontend (npm accepts a tarball URL but cannot install a git
  subdirectory).

### Changed

- `Asset` carries `Cow<'static, [u8]>` plus an `etag` (was `Vec<u8>` + mime only).
- `CacheProvider` and `QueueProvider` are now constructed
  (`CacheProvider::new()` / `::with_limits(..)`, `QueueProvider::new()` /
  `::with_workers(..)`) instead of being unit structs.
- `Queue::push` returns `bool` (`false` when the queue is full) and the frontend's
  `queue.push` reports a full queue as an error instead of silently dropping.
- **Frontends that talk to the bridge without `@elyra/runtime`** must now send
  `x-elyra-token` (and `x-elyra-client-id` for `/__events`); the token is exposed
  to page scripts as `globalThis.__ELYRA__.token`.
- **`@elyra/runtime` ships built output.** The package now exports `dist/` with
  `.d.ts` files instead of raw `src/*.ts`; bundlers other than Vite work as a
  result, and the version tracks the crate (`0.5.7`).
- `elyra` and `@elyra/runtime` are both at **0.5.7**; `rata new` pins the CLI's own
  version instead of a hard-coded one.
- **MSRV is now declared as 1.94** (was 1.80). The old value was never verified —
  the new CI job showed it can't build at all, since sqlx 0.9 requires 1.94 and
  several dependencies need `edition2024` (Cargo 1.85+). Nothing regressed; the
  declaration was simply wrong.

## [0.5.6] — 2026-07-29

### Added

- **Rate limiter.** A cache-backed `RateLimiter` (Laravel-`RateLimiter`-style) —
  `too_many_attempts` / `hit` / `attempts` / `remaining` / `clear` / `attempt`,
  with self-expiring per-key counters. Get one via `Cache::limiter()`.
- **Task scheduler.** A Laravel-`Schedule`-style `Scheduler` for recurring
  background jobs — `every` / `every_minutes` / `hourly` / `daily`, async
  closures, bound via `SchedulerProvider`. Interval-based (from app start),
  in-process; registration works before or after start.
- **Artisan-style generators.** `rata make:command`, `make:provider`, and
  `make:model` scaffold a source file under `src/` (with normalized snake/Pascal
  names and pluralized model tables) and print the `mod` + registration wiring
  step. Existing files are never overwritten.
- **Validation.** A Laravel-style validator (`elyra::validation`): check command
  input against a rule string (`"required|email|min:18"`) and get a per-field
  `ValidationErrors` bag. Return it via `?` from a command and the frontend reads
  the structured errors with `validationErrors(err)` from `@elyra/runtime`.
  Rules: `required`, `nullable`, `string`, `integer`, `numeric`, `boolean`,
  `email`, `url`, `min`, `max`, `size`, `in`, `same`, `confirmed`. Core, no deps.

## [0.5.5] — 2026-07-25

### Fixed

- **The bundle updater rejected every update.** 0.5.4 verified the downloaded
  bundle with `codesign --verify --strict --quiet` — but `codesign` has no
  `--quiet` flag, so it exited 2 with *"unrecognized option"* on **every** run and
  the updater concluded that no update was correctly signed. Auto-update was dead
  on arrival for bundled apps: safe (nothing was installed) but useless.

  The verification now lives in its own function, carries codesign's own reason
  into the error message, and is covered by a **positive** test: a correctly signed
  bundle must be *accepted*, both freshly signed and after the `ditto` round-trip
  the updater performs. Rejection tests alone could not catch this — a broken
  invocation rejects everything, which looks exactly like a working guard.

## [0.5.4] — 2026-07-25

### Fixed

- **The auto-updater no longer breaks a signed macOS `.app`.**
  `Updater::apply_and_relaunch` replaced only the running executable. Inside a
  code-signed bundle that is fatal: the signature seals `Info.plist` and every
  file under `Contents/`, so dropping a new binary in — and leaving the
  `.old` backup beside it — broke the seal, and macOS then refused to launch the
  app at all with *"The application can't be opened."* The app had to be
  reinstalled by hand.

  The updater now detects that it is running inside `Foo.app/Contents/MacOS/` and
  switches to **whole-bundle replacement**: the artifact must be a zip of the
  signed `.app`, which is expanded with `ditto` (preserving extended attributes
  and the signature), verified with `codesign --verify --strict`, checked to carry
  the same `CFBundleIdentifier`, and only then swapped in with a rename inside the
  bundle's own directory — with the outgoing copy moved *outside* the bundle and a
  roll-back if the swap fails. Relaunch goes through `open` so LaunchServices
  re-registers the new bundle.

  A bare-binary artifact offered to a bundled app is now **refused with an
  explanation** instead of applied. Loose (unbundled) executables keep the
  previous in-place swap.

  **Releasing note:** apps distributed as a signed `.app` must publish a zip of
  the bundle as the update artifact. A release that keeps publishing the bare
  executable will now be rejected by the client rather than installed.

## [0.5.3] — 2026-07-24

### Added

- **`rata bundle` app icon.** The macOS bundle now generates the native
  dock/Finder icon: it renders the source image to
  `Contents/Resources/AppIcon.icns` (`sips` + `iconutil`; SVGs rasterized at
  1024 via `qlmanage`, `sips` fallback) and sets `CFBundleIconFile`. Configure
  with `[bundle].icon`, or it auto-detects `app/public/icon.svg` (shipped by
  `rata new`), `icon.png`, etc. Best-effort — the bundle still builds without it.

## [0.5.2] — 2026-07-22

### Added

- **Command cancellation.** `invokeCancellable(command, ...args)` in
  `@elyra/runtime` returns `{ id, result, cancel }`; `cancel()` aborts the
  in-flight command on the Rust side (via a request-id header + a `/__cancel`
  route that aborts the command's task). Progress is done with the event bus
  (documented pattern) — no new API needed.
- **AI rate limiting + token budget.** `AiBuilder::rate_limit(per_minute)`
  throttles every provider call (waits, doesn't error); `token_budget(max)`
  refuses new prompts once cumulative tokens hit the cap (`Error::Budget`);
  `Ai::tokens_used()` reports the running total.
- **Opt-in CSP.** `App::csp(policy)` sets a `Content-Security-Policy` header on
  HTML responses served over `elyra://` (off by default — a too-strict policy
  can break the webview).

### Changed

- **Locks no longer poison.** Switched internal `std::sync::Mutex` to
  `parking_lot::Mutex` across the cache, event bus, sidecar, store, queue,
  windows, and the AI client — a panic while holding a lock can no longer
  cascade into a poisoned-lock crash.

### Fixed

- **Sidecar CPU spin.** If every command sender dropped while a child was still
  running, the owning task's `select!` busy-looped on a closed channel at 100%
  CPU. The command arm is now disabled once the channel closes; the task only
  waits on the child to exit.
- **Unbounded EventBus growth.** Emitted events buffered without limit when the
  frontend was gone/reloading/slow. The queue is now capped (`MAX_QUEUED`); when
  full the oldest half is dropped so a reconnecting frontend still gets recent state.
- **Cache TTL leak.** Expired entries were only reclaimed when the same key was
  read again. `CacheProvider` now starts a background sweeper (`Cache::sweep`)
  that drops expired entries periodically; it holds a `Weak` ref and stops when
  the cache is dropped.
- **Predictable updater temp files.** Downloaded updates were written to a static
  path in the temp dir. They now use an unpredictable filename, refuse to open a
  pre-existing path (`O_EXCL`), and are created `0600` on Unix — mitigating
  symlink attacks and collisions on shared machines.

### Changed

- **Migrations run in a transaction.** Each migration (and its history row) now
  commits atomically where the driver supports transactional DDL (SQLite,
  Postgres); a mid-file failure rolls back cleanly. MySQL auto-commits DDL, so
  partial state there remains possible — documented.

## [0.5.1] — 2026-07-17

### Added

- **`substrate-core` crate.** A tiny, dependency-free crate defining the shared,
  backend-agnostic `Cache` / `Storage` / `Queue` contracts behind the "one
  ecosystem" facades — the same traits the Askr/Laravel side can implement.
  Elyra's `Cache`, `Storage`, and `Queue` now implement them (re-exported as
  `elyra::substrate`); conformance is verified in `tests/substrate.rs`. `Cache`
  is byte-internal, so `substrate` `get`/`put` round-trip losslessly.

### Changed

- `Cache` stores values as bytes internally (the JSON/typed API is unchanged
  sugar on top). No behavior change for existing callers.

## [0.5.0] — 2026-07-17

### Added

- **Queue facade.** An in-process background job queue with the same surface as
  Laravel's `Queue::` — `push` a named job, register an async handler with `on`.
  Jobs run in order on a background task; status is emitted on `elyra:queue`
  (`onQueue`). Bind with `QueueProvider`; push from Rust (`ctx.get::<Queue>()`)
  or the frontend (`queue` in `@elyra/runtime`). In-process / non-durable by
  design (durable, cross-process queues are Askr's domain).
- **Storage facade.** A filesystem disk with the same surface as Laravel's
  `Storage::` — `put` / `get` / `append` / `exists` / `delete` / `size` /
  `files` / `url`, every path jailed to the disk root. Bind with
  `StorageProvider::at(root)`; use from Rust (`ctx.get::<Storage>()`) or the
  frontend (`storage` in `@elyra/runtime`).
- **Cache facade.** An ergonomic in-process, TTL-aware key-value cache with the
  same surface as Laravel's `Cache::` (and Askr's shared cache) — `get` / `put` /
  `add` / `remember` / `increment` / `forget` / `flush`, typed helpers, arbitrary
  JSON values. Bind with `CacheProvider`; use from Rust (`ctx.get::<Cache>()`) or
  the frontend (`cache` in `@elyra/runtime`). First of the shared "one ecosystem"
  facades that mirror the Askr/Laravel side over a local backend.

## [0.4.0] — 2026-07-15

### Added

- **AI reliability (`ai`).** Automatic **retries** with exponential backoff on
  transient failures / retryable statuses (`AiBuilder::retries` / `retry_backoff`),
  provider **failover** (`Chat::failover([...])`), and in-memory response
  **caching** for plain prompts (`AiBuilder::cache` / `cache_ttl`, `clear_cache`).
- **AI provider tools (`ai`).** Native, server-executed **web search** and
  **web fetch** (`web_search` / `web_fetch` on `Chat`, `WebSearch` / `WebFetch`
  / `UserLocation`). Anthropic-native; OpenAI returns `Unsupported` (Responses
  API not used yet).
- **AI audio (`ai`).** Text-to-speech (`ai.speech(...).generate()` →
  `GeneratedAudio`) and transcription (`ai.transcribe(bytes, name).generate()`),
  over OpenAI (`gpt-4o-mini-tts` / `whisper-1` defaults).

## [0.3.1] — 2026-07-14

### Added

- **AI SDK (`ai` feature).** A new Laravel-inspired `elyra-ai` crate, re-exported
  as `elyra::ai`: anonymous + named **agents** (`Agent`), **tools** with an
  automatic tool-use loop (`Tool`), **sub-agents** (an `Agent` used as a tool via
  `sub_agent` / `AgentTool`), **structured output** (`prompt_as::<T>` via
  `serde` + `schemars`), **streaming** (`stream` → `StreamChunk`, ideal for the
  event bus), **images**, **embeddings**, and an in-memory **vector store** for
  RAG (`VectorStore` + `cosine_similarity`) over Anthropic + OpenAI.
  `AiProvider` binds an env-configured `Ai` client into the container. Default
  text model `claude-sonnet-5`; images `gpt-image-1`.
- **Single-instance** (`App::single_instance`). Later launches focus the running
  window and forward their command line on `elyra:second-instance` (`onSecondInstance`),
  then exit. Portable loopback rendezvous with a per-app handshake.
- **Deep-linking** (`App::deep_link("myapp")`). Launch URL via `deepLink.initial()`,
  later URLs on `elyra:deep-link` (`onDeepLink`) — macOS open-URL event + Windows/Linux
  scheme registration; pairs with single-instance for while-running delivery.

## [0.3.0] — 2026-07-14

### Added

- **Sidecar processes (`sidecar` feature).** Spawn and manage child processes
  via `sidecar` in `@elyra/runtime` (`spawn` / `write` / `kill`) or the
  `elyra::sidecar::Sidecar` handle; `stdout`/`stderr` lines and exit stream on
  the `elyra:sidecar` channel (`onSidecar`). No extra crate — uses `tokio`.
- **Autostart (`autostart` feature).** Launch the app at login via `autostart`
  in `@elyra/runtime` (`enable` / `disable` / `isEnabled`) or the `elyra::autostart`
  module. Backed by `auto-launch` (LaunchAgents / registry / `.desktop`).
- **Settings store.** A persistent key-value store (`store` in `@elyra/runtime`,
  `Store` in the container) backed by `settings.json` in the OS config dir —
  `get` / `set` / `delete` / `all` / `clear`, arbitrary JSON values. Core, no
  feature flag.

- **Native application menu.** `App::menu(Menu::new().submenu(Submenu::new("File")…))`
  adds custom submenus (with accelerators) after the standard app + Edit menus;
  clicks emit `elyra:menu` (subscribe with `onMenu`). Rendered on macOS.
- **Global shortcuts (`shortcuts` feature).** `App::global_shortcut("CmdOrCtrl+Shift+P")`
  registers OS-level keyboard shortcuts; firing one emits the `elyra:shortcut`
  event (subscribe with `onShortcut`). Backed by `global-hotkey`.
- **Window-state persistence.** `App::persist_window_state()` remembers the
  primary window's size, position, and maximized state between runs (stored under
  the OS config directory, keyed by the About name). Dependency-free.
- **Window control + file drop.** `@elyra/runtime` exports `appWindow`
  (minimize / maximize / fullscreen / close / focus / show / hide / center /
  setTitle / setSize) with live state via `appWindow.onState`, and `onFileDrop`
  for native file drops. Backed by new `Windows` methods on the Rust side
  (usable from commands, with an optional target-window label). Core — no
  feature flag.
- **UI components in `@elyra/runtime`.** Themed, dependency-free primitives:
  `alert` / `confirm` / `prompt` dialogs, `toast()` notifications, a ⌘K
  **command palette** (`registerCommands` / `openCommandPalette`), and
  `contextMenu()`. They read the app's CSS variables, matching the About /
  update components.
- **System integration (`system` feature).** Native desktop essentials exposed
  through `@elyra/runtime`: file dialogs (`dialog.open` / `dialog.save`),
  opening URLs/files in the OS (`shell.openExternal`), the clipboard
  (`clipboard.readText` / `writeText`), OS notifications (`notify`), and
  standard paths (`paths`). Backed by `rfd`, `open`, `arboard`, `notify-rust`,
  and `dirs`; also usable from Rust via the `elyra::system` module.

## [0.2.0] — 2026-07-13

### Added

- **Models — relation auto-hydration.** Declare a relation on a *field*
  (`#[model(has_many(Book, fk = "author_id"))] books: Vec<Book>`) and the derive
  skips it as a column, defaults it to empty, and generates a `with_<field>`
  batch hydrator that fills it in one query — no more joining a `HashMap` by
  hand. Works for `has_many` (`Vec<T>`), `has_one` / `belongs_to` (`Option<T>`;
  `belongs_to` targets must be `Clone`).
- **Models — non-`i64` primary keys.** A single-column primary key may now be any
  type (e.g. `String`), marked with `#[model(id)]`. The value is app-supplied and
  included in the `INSERT` (no key read-back), and `find` takes that key type.
  The default `i64` autoincrement behaviour is unchanged. Composite keys remain
  unsupported.
- **Models — column-aware `belongs_to`.** The owning row is looked up against the
  related model's actual primary-key column (`<T>::PK`) instead of a hardcoded
  `id`, and the child's foreign key is read by column name — so `belongs_to`
  works even when the owner's PK column is renamed via `#[model(column = "..")]`.

- **Codegen:** serde container attributes are now reflected in the generated
  TypeScript via `specta-serde` — `rename` / `rename_all`, tagged and untagged
  enums (as discriminated unions), `flatten` (as intersections), and `skip`.
  Elyra's numeric policy (64-bit ints and floats render as `number`) is applied
  on top.
- **Database tests:** model CRUD now runs against real **MySQL** and
  **Postgres** servers in CI (`model_servers.rs`), exercising per-driver
  placeholders (`?` vs `$n`) and key retrieval (`last_insert_id` vs
  `RETURNING`). The tests are opt-in via `ELYRA_TEST_MYSQL_URL` /
  `ELYRA_TEST_POSTGRES_URL` and skip cleanly when unset.

### Changed

- **Updater:** `UpdaterConfig::auto_check` now defaults to `false`. The silent
  startup check (and its toast) is opt-in via `.auto_check(true)`, so apps no
  longer notify about updates on launch unless they ask to.
- **Dependencies:** updated to their latest releases — `sqlx` 0.9 (dynamic SQL
  is now wrapped in `AssertSqlSafe`), `ureq` 3, `ed25519-dalek` 3, `tray-icon`
  0.24 + `muda` 0.19, and a `cargo update` across the tree. No public API
  changes.
- **Tooling:** CI typechecks the runtime with **TypeScript 7** and runs on
  **Node 24**; `@msgpack/msgpack` bumped to `^3.1.3`.
- Docs: clarified that code signing, Apple ID / Developer ID, notarization, and
  binary distribution are the application's responsibility — not the framework's.
  Removed them from the roadmap and added an explicit "Out of scope" section.

## [0.1.0] — 2026-07-13

First public release. Everything below is compiled, `clippy`-clean, and tested
(SQLite for the database layer; GUI/OS integrations are launch-smoked, with
visual or side-effecting steps called out as unverified in the docs).

### Added

#### Core

- **`App` builder** — fluent assembly of a desktop app: window options,
  container bindings, providers, middleware, commands, and assets.
- **Container + `Ctx`** — a type-keyed service container resolvable from any
  command, provider, or background task (`ctx.get::<T>()`).
- **Providers** — two-phase `register` / `boot` wiring, like Laravel service
  providers.
- **Middleware pipeline** — outermost-first command middleware around dispatch.

#### IPC bridge

- **`elyra://localhost` custom protocol** — the whole app lives under one
  origin: assets, commands (`/__cmd/*`), and the event stream (`/__events`).
- **MessagePack wire format** — compact argument arrays in, named maps out
  (structs decode to JS objects); no JSON in the hot path.
- **`#[command] async fn`** — typed commands dispatched on a multi-thread tokio
  runtime; the UI thread never blocks. `Result` commands surface `Err` as a
  rejected promise (`CommandError`).

#### Events

- **`EventBus` + `channel()`** — Rust→frontend push over a multiplexed
  long-poll, batched per flush; a Svelte-readable store on the frontend.

#### Windows, tray, updater

- **Multi-window** — additional windows at startup or at runtime via the
  container-bound `Windows` handle.
- **System tray** (`tray` feature) — icon + menu; clicks arrive on the `tray`
  event channel.
- **Auto-updater** (`updater` feature) — ed25519-verified update manifest,
  semver comparison, HTTP fetch, and signature-checked staged download.
- **macOS application menu** — an Edit menu (so ⌘C/⌘V/⌘X reach the webview) and
  a custom About item.

#### Data (`database` feature)

- **`Database`** — one handle over SQLite / MySQL / Postgres via sqlx's `Any`
  driver, with per-driver placeholder rendering.
- **Migrations** — `rata migrate` with batches, reversible `down`, and status.
- **`#[derive(Model)]`** — Active Record with CRUD, a typed query builder
  (`where_*`, `where_in`, `order_by`, `limit`, `get`/`first`), `bool`↔INTEGER
  mapping, `#[model(column)]`, `#[model(timestamps)]`, relations
  (`has_many` / `has_one` / `belongs_to`), and N+1-avoiding eager loading
  (`load_<name>`).

#### Codegen & runtime

- **`rata codegen`** — specta types → TypeScript definitions and a typed
  `api.*` facade that mirrors every `#[command]`.
- **`@elyra/runtime`** — `invoke()`, `channel()`, and the generated `api.*`.

#### Tooling

- **Ratatosk (`rata`)** — `new` (scaffold with the Grove theme),
  `dev` (Vite HMR + `elyra://` IPC), `codegen`, `build`, `bundle`
  (macOS `.app` + ad-hoc signing), and `migrate`.

#### UI components

- **About dialog** — set metadata once with `App::about(AboutInfo::new(..))`;
  the shell serves it at `/__about` and `@elyra/runtime` renders a themed
  dialog. On macOS the standard **About &lt;App&gt;** menu item opens it; from
  the frontend, call `openAbout()`.
- **Update component** — `App::updater(UpdaterConfig::new(..))` adds a silent
  startup check, `/__update/check` + `/__update/install` endpoints, progress on
  the `elyra:update` channel, and a themed **update toast** in
  `@elyra/runtime` (available → install → download → restart).
  `Updater::apply_and_relaunch` replaces the running binary and re-execs.

[Unreleased]: https://github.com/kwhorne/elyra-framework/compare/v0.5.7...HEAD
[0.5.7]: https://github.com/kwhorne/elyra-framework/compare/v0.5.6...v0.5.7
[0.5.6]: https://github.com/kwhorne/elyra-framework/compare/v0.5.5...v0.5.6
[0.5.5]: https://github.com/kwhorne/elyra-framework/compare/v0.5.4...v0.5.5
[0.5.4]: https://github.com/kwhorne/elyra-framework/compare/v0.5.3...v0.5.4
[0.5.3]: https://github.com/kwhorne/elyra-framework/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/kwhorne/elyra-framework/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/kwhorne/elyra-framework/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/kwhorne/elyra-framework/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/kwhorne/elyra-framework/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/kwhorne/elyra-framework/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/kwhorne/elyra-framework/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/kwhorne/elyra-framework/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/kwhorne/elyra-framework/releases/tag/v0.1.0
