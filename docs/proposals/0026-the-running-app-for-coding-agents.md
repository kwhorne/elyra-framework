# RFC 0026 — The running app, for the agent working on it

**Status:** implemented (2026-11-06) · **Target:** 0.37.0 · builds on [RFC 0013](0013-assistant-sees-the-window.md) and [RFC 0014](0014-elyra-inspector.md) · see [Decisions](#decisions)

## Summary

A coding agent that builds an Elyra app can read its code, its schema and
its log through `rata mcp dev`, but not the app itself: it changes a view
and has to ask the developer what it looks like now. This gives
`rata mcp dev` the running debug build's windows: what's on them as a
tree it can read, as a picture, and a way to click and type into them.

```text
agent → window_snapshot { window: "main" }

  route: /customers?search=acme
  - navigation "Main"
    - link "Customers" [current] [ref=e3]
    - link "Invoices" [ref=e4]
  - main
    - heading "Customers" [level=1]
    - searchbox "Search" value="acme" [ref=e7]
    - table "Customers" (2 rows)
      - row "Acme AS · 3 open invoices" [ref=e9]
      - row "Acme Industrier · –" [ref=e10]
    - button "New customer" [ref=e12]

agent → window_act { window: "main", action: "click", ref: "e12" }
agent → window_screenshot { window: "main" }          → image/png
```

It's for development only: debug builds, as the inspector (RFC 0014), and
nothing of it is compiled into a release.

## Motivation

- **An agent can't see what it built.** It writes a Svelte view, runs
  `rata dev`, and is blind. The developer describes the screen, or pastes
  a screenshot, or the agent guesses from the code. Every round trip is
  the developer's time.
- **Elyra already knows the window.** RFC 0013 describes it to the in-app
  assistant (route, what's shown, what's selected); RFC 0014's inspector
  has the calls, the live queries and the window's scope; `rata mcp dev`
  is already in the agent's `.mcp.json` after `rata ai:install`. What's
  missing is the page itself, and a way to act on it.
- **GPU-drawn frameworks do this headlessly** (anomalyco/rune renders to
  PNG and dumps its element tree for agents). Elyra draws in the system's
  webview, and its accessibility tree is the browser's own: a richer tree
  than a custom renderer has, if the agent can read it.

## Current state (0.37)

- **`rata mcp dev`** (`ratatosk/src/mcp_dev.rs`): stdio MCP for the coding
  agent, read-only: `app_info`, `database_schema`, `migration_status`,
  `last_error`, `read_log_entries`, `list_commands`, `search_docs`. It
  reads the source and the log; it doesn't talk to a running app.
- **The app's MCP endpoint** (`framework/src/mcp/endpoint.rs`): a `0600`
  Unix socket (loopback on Windows) with a challenge-response on a
  per-install token, opened only when the app calls `.mcp(..)`. It serves
  the app's commands as tools: the app's own surface, not the developer's.
- **The inspector** (`/__inspector/*`, debug builds only): calls, live
  queries, presence, listening, scope, the ability mask, offline.
- **wry** gives each platform's webview: `WKWebView` (macOS,
  `takeSnapshot`), `ICoreWebView2` (Windows, `CapturePreview`),
  `webkit2gtk::WebView` (Linux, `get_snapshot`). Elyra uses none of them
  for pictures today.

## Design

### Tools

Added to `rata mcp dev`, answered by the running app:

| Tool | What it does |
|---|---|
| `app_windows` | The open windows: label, title, route, size, shown or hidden, focused. |
| `window_snapshot` | The page as an accessibility tree in text: role, name, value and state, a `ref` on what can be acted on, the route, and the console errors since the last snapshot. |
| `window_screenshot` | The window's webview as a PNG, at most 1280 CSS pixels wide. |
| `window_act` | `click`, `type` (into a field, replacing or appending), `press` (a key, with modifiers), `select` (an option), `scroll`, or `navigate` (to a route); then waits for the page to settle and returns a new snapshot. |
| `app_calls` | The inspector's call log: command, outcome, time, size (never arguments). |

`window` is a label (`main` by default). Without a running debug build,
each tool says so and how to start one (`rata dev`), and the rest of
`rata mcp dev` works as before.

### The snapshot

Built in the page by `@elyra/runtime` (debug builds only), from the DOM's
roles and accessible names, the way the browser's accessibility tree
computes them: `role` attributes and implicit roles, `aria-label`,
`aria-labelledby`, a label's text, `aria-selected`, `aria-current`,
`aria-expanded`, `aria-checked`, `disabled`, a field's value. What's
hidden (`aria-hidden`, `hidden`, `display: none`, `inert`) isn't in it.

- **Refs** (`e1`, `e2`, …) mark what can be acted on: links, buttons,
  fields, options, rows, tabs. A ref holds for the page until the next
  snapshot; acting on a stale one is an error that says to snapshot again.
- **Bounded:** at most 1,000 lines; a long list shows its first rows and
  how many more, and a long text is cut at 200 characters.
- **The note of RFC 0013 comes with it:** `showing` (the live queries, with
  their arguments) and `selected`, under the tree. It's what the window
  shows in the app's own words, next to what it shows on screen.
- **A password field's value is never in it** (`type="password"`, or
  `autocomplete` of a secret): `value=•••`.

### Acting

In-page, through the DOM: a click is `element.click()` after focusing it,
`type` sets the value and fires `input` and `change` as typing does,
`press` dispatches `keydown`/`keyup` to the focused element. Then it waits
for the page to settle (no command or live-query call in flight, and a
frame) for up to two seconds, and snapshots.

These are synthetic events: no OS input, no permission prompt, the user's
cursor and keyboard untouched, the same on every platform. A
`event.isTrusted` check in app code would see them as untrusted; Elyra's
own components don't check it.

### The picture

Native, per platform, from the webview wry made: `takeSnapshot` on macOS,
`CapturePreview` on Windows, `get_snapshot` on Linux. It's the page as
rendered, without the window's title bar. A hidden window has no picture
(`app_windows` says which are shown). Scaled to at most 1280 CSS pixels
wide, so an image costs the agent a bounded number of tokens.

### How rata reaches the app

The debug build opens a **dev endpoint**: the same private transport as the
MCP endpoint (a `0600` socket, a challenge-response on a per-install
token), always on in a debug build, never compiled into a release, and
separate from `.mcp(..)` (an app needn't serve MCP to be developed with
an agent). `rata mcp dev` finds it from the app's identifier, as it finds
the log. The app answers on the event loop: it evaluates the runtime's
snapshot or act function in the window, or takes the picture.

### Security

- **Not in a release:** `#[cfg(debug_assertions)]`, as the inspector's
  routes; a release-build test asserts the endpoint and the runtime's
  snapshot code aren't there (as `inspector_release` does).
- **Only the user who runs it:** a `0600` socket and the token in the
  app's own folder; another user on the machine can't connect.
- **What the agent sees, the developer sees:** the developer's own data
  in the dev build. Snapshots and pictures aren't written to disk or the
  log. Password values are masked in the snapshot; a picture shows what's
  on screen, as a screenshot would.
- **The agent can click anything,** a "Delete" too: it acts as the
  developer would in their dev build. The tools' descriptions say so.

### What it doesn't do

- **No headless rendering.** A window is the system's webview; there's no
  off-screen renderer to draw it without one. CI keeps the smoke harness's
  headless Chrome against a fake backend.
- **No OS-level input**, and no pixel coordinates: refs, not positions.
- **No release-build or production use.** An agent driving a shipped app
  for a user is the app's MCP server (RFC 0003) and its own commands.

### Testing

- The snapshot in `@elyra/runtime`: roles, names, states and values from
  `@elyra/ui`'s components; refs; what's hidden; bounds; masked passwords.
- Acting: each action against `@elyra/ui`'s components, the settle wait,
  a stale ref.
- The endpoint: the challenge-response, debug-only (a release-build test),
  a window that isn't there.
- `rata mcp dev` against a running example app: each tool, and each one's
  answer when no app is running.
- The picture: a PNG of the right size on macOS, locally; Windows and Linux
  compiled in CI, and checked by hand before the release says so.

## Implementation plan

1. The snapshot and act functions in `@elyra/runtime` (debug only), with
   tests in happy-dom against `@elyra/ui`.
2. The dev endpoint in the framework: windows, snapshot, act, calls.
3. `rata mcp dev`'s new tools, through the endpoint.
4. The picture, per platform.
5. Docs: `ai-agents.md`, `inspector.md`, `security.md`; CHANGELOG.

## As built

- **Shipped with RFC 0025 in 0.37.0,** not 0.38.0: it was done before 0.37
  was released.
- **The dev socket is in the app's folder,** beside `instance.token`, not in
  `$TMPDIR` as the MCP socket is: a coding agent's `rata` runs under the
  agent's environment, which a sandbox can give its own `TMPDIR`, and would
  then look in the wrong place. Both sides already know the app's folder. On Windows it's loopback on `elyra-dev/<app>`'s ports.
- **The endpoint is the MCP one, generalized:** its greeting (`ELYRA-DEV/1`)
  and place are parameters, and `serve_with` takes any handler. The two
  don't take each other's clients (tested).
- **A script's answer is fetched, not awaited:** a webview's script callback
  has the script's value, not a promise's, and wry queues a script sent
  before the page has loaded without its callback. So the shell starts a job
  in the page (`__elyraAgent.start`) and asks for it (`take`) every 40 ms,
  each question with a 5-second answer timeout and the job with 10.
- **The script is the framework's** (`framework/src/shell/agent.js`, added to
  each page's initialization scripts under `cfg(debug_assertions)`), so a
  release has none of it whatever runtime the app has. It's tested in
  `@elyra/ui`'s test setup (happy-dom) against the components themselves.
- **Names read like the browser's:** a `<label for>` names a button too
  (Select's combobox), a label's own words exclude its control's (a
  `<select>`'s options), a row's cells are read with a space between them,
  and a name that comes from content shown below it isn't said twice.
- **The picture** is decoded and averaged down to CSS pixels in Rust (`png`),
  so every platform's PNG is treated alike.
- **`app_calls` by window:** the call log is kept by client id; the shell
  maps a window to the client ids its pages used.
- **The RFC 0013 note (`showing`, `selected`) isn't in the snapshot yet:**
  the tree, the route and the errors are; `app_calls` has the live queries'
  names.
- **Checked on macOS** with the example app through `rata mcp dev`: the
  windows (and the hidden popover), an 81-line snapshot, a 560×760 picture
  from a Retina window, typing into a field, clicking a button that called
  `greet` and the answer in the next snapshot, `app_calls` with that call, a
  stale ref's error, `navigate`, `press`, and an unknown window's error.
  **Not seen:** the picture on Windows (compiled) and Linux (compiled in
  CI).

## Alternatives considered

- **A separate `rata mcp window` server:** a second entry in `.mcp.json`
  for what is the same agent working on the same app.
- **Through the app's MCP endpoint (`.mcp(..)`):** that's the app's
  surface for its users' agents, with their abilities and confirmations;
  the developer's tools don't belong there, and an app needn't have it.
- **The DOM as HTML instead of an accessibility tree:** larger, full of
  styling, and not what an agent is used to acting on.
- **OS-level input** (`CGEvent`, `SendInput`, XTest): trusted events and
  real focus, but a permission prompt on macOS, the user's cursor moving,
  and three implementations for what the DOM does in one.
- **The page in headless Chrome, as the smoke harness does:** no shell, a
  fake backend, a different engine from the one the user has.

## Open questions

1. **Where the tools live:** `rata mcp dev` (recommended: the server the
   agent already has from `rata ai:install`), or a server of their own?
2. **Input:** in-page synthetic events (recommended: no permissions, every
   platform alike, the user's input untouched), or OS-level?
3. **The snapshot's form:** an accessibility tree in text with refs
   (recommended), or HTML?
4. **The picture's size:** at most 1280 CSS pixels wide (recommended), or
   the window's full size?
5. **The picture on Windows and Linux:** in this release, compiled in CI
   and checked by hand before it's said to work (recommended), or macOS
   first and the others later?
6. **Destructive clicks:** allowed, as the developer's own action in a dev
   build (recommended), or asked for through the agent's client first?

## Decisions

Settled on 2026-11-06, all as recommended:

1. **The tools are in `rata mcp dev`.**
2. **Input is in-page synthetic events.**
3. **The snapshot is an accessibility tree in text, with refs.**
4. **A picture is at most 1280 CSS pixels wide.**
5. **The picture on Windows and Linux is in this release,** compiled in CI
   and checked by hand before it's said to work.
6. **Destructive clicks are allowed,** as the developer's own action in a
   dev build.
