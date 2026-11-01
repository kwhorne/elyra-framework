# RFC 0016 — Capabilities per window

**Status:** implemented (2026-11-01) · **Target:** 0.28.0 · builds on [RFC 0010](0010-laravel-abilities.md), [RFC 0011](0011-in-app-assistant.md) and [RFC 0014](0014-elyra-inspector.md) · see [Decisions](#decisions)

## Summary

An app grants its frontend capabilities and abilities once, for every
window. A preview window, a report, a small widget from the tray all get
what the main window gets. This RFC lets a window be given less:

```rust
Windows::open(
    WindowConfig::new("preview")
        .path("/invoices/42")
        .only_abilities(["invoices.view"])   // no invoices.delete, no billing.*
        .deny(Capability::Assistant)         // no assistant panel here
        .deny(Capability::Sidecar),
)
```

In that window `delete_invoice` (`#[command(can = "invoices.delete")]`)
is refused at the shell's gate, `can("invoices.delete")` is false so its
buttons hide, the assistant routes answer `403`, and the inspector shows
the window's limits. The main window is unchanged.

A window can only be narrowed. It never gets more than the app grants,
and the user's own abilities (RFC 0010) still apply on top.

## Motivation

- **A smaller blast radius.** A window that shows a rendered invoice, a
  Markdown preview or a report shows content the app didn't write. An
  XSS there today reaches every command the main window can. Narrowing
  it to what it needs makes that hole small.
- **The UI follows.** `can` hides what the user may not do (RFC 0010).
  A read-only window hides its edit and delete buttons the same way,
  without the page knowing why.
- **It's on the roadmap.** "Capabilities per window — the policy is
  app-global today; per-window scoping is the next step."

## Current state (0.27)

- `Policy` holds one token, one set of capabilities and one list of
  abilities for the whole app. `guard::check` checks a request against
  them, and then against the user's abilities.
- A window says who it is with `x-elyra-client-id`. The runtime makes
  that id up itself (`crypto.randomUUID()`), so it says which queue the
  events go to, not who may do what. Any page can send another window's
  id, or a new one. The token is the same in every window.
- Each window is built in `shell::webview::build_window` with its **own**
  `with_asynchronous_custom_protocol` handler. Every `elyra://` request
  from that webview, its frames included, arrives through that handler.
  The shell knows which window sent it, and the page can't change that.
  Today the handler throws the knowledge away.
- `/__window/*` acts on the focused or primary window (`None` as the
  label), not on the window that asked. A background window's `close()`
  closes the focused one.
- The frontend can't open windows. Only Rust can, with `Windows::open`
  or `App::window`.

## Design

### Which window is asking

The window's protocol handler carries its label and its limits, taken from
its `WindowConfig` when it's built, and hands them to `route`. That's the
window's identity for everything in this RFC. It isn't a header, and it
isn't a secret a page could copy. `x-elyra-client-id` keeps its job (a
queue per document) and decides nothing.

The IPC token stays as it is: it proves the page is one the app loaded.
The window's identity says which one.

### What a window can be given

On `WindowConfig`, each narrowing only:

```rust
.deny(Capability::Assistant)                 // one capability less
.only_capabilities([Capability::Commands, Capability::Window])
.only_abilities(["invoices.view", "customers.*"])
.deny_abilities(["invoices.delete", "billing.*"])
```

- **Capabilities.** The window has the app's grant minus what it denies,
  or, with `only_capabilities`, only those the app also grants.
- **Abilities.** A command that declares `can = "…"` needs it granted by
  the app, allowed by the window, and (with `Backend::abilities()`) held
  by the user. Patterns use the same `*` rules as `allow_ability`.
- **A command without `can`** is reachable wherever `Capability::Commands`
  is, as today. A window that must not reach those denies
  `Capability::Commands`, or the app declares abilities on them. The
  security guide already says to declare abilities on anything that
  matters (see [question 2](#open-questions)).
- **No widening.** Nothing on `WindowConfig` adds a capability or an
  ability the app didn't grant. A window with no limits is the app's
  grant, so existing apps don't change.

### Where it's enforced

- **The gate.** `guard::check` gets the window's limits beside the policy
  and checks them after the app's grant: a capability the window lacks is
  `403 forbidden` ("not granted to this window"), and an ability the
  window lacks is `403 denied`, as a user without it would get. Live
  subscriptions (`/__live/*`) run the command, so they're checked too.
- **The assistant.** The tools offered in a window are the ones its
  limits allow, and a call is checked again when it runs. A window
  without `Capability::Assistant` has no panel and no routes.
- **`can`.** `/__auth/abilities` answers for the window that asks: the
  user's list narrowed by the window. A change of the user's abilities
  goes to each window as its own narrowed list (`emit_to`), not one list
  to all.
- **`/__window/*`** acts on the window that asked.
- **Commands** learn which window called them: `CommandRequest::window`
  (`Option<String>`, the label; `None` from Rust, MCP and tests), so a
  middleware can log it or refuse it.
- **The inspector** shows the window's label and limits in its Abilities
  section; its mask still only takes away.

### What it doesn't cover

- **Events.** `emit` still goes to every window. Data only one window
  should see goes with `emit_to`, as presence's does.
- **Windows a command opens.** A command called from a narrowed window can
  open a window with fewer limits. The app writes that command, so it
  decides; the guide says so.
- **MCP agents** aren't windows: their grant is separate, as before.
- **The user's rights.** Laravel still decides every request. A window's
  limits keep the desktop from offering, or sending, what that window
  shouldn't.

### Testing

`TestShell` gets `window(label, config)` to register a window with limits
and `handle_in(label, request)` to send a request from it. Tests cover
each gate (capability, ability, live subscription, assistant tools and
calls, `/__window/*`), the narrowed `can` list and its per-window events,
that a header can't change the window, and that an app without limits
answers exactly as before. The browser smoke test opens a second window
with `only_abilities` and checks that the delete button is gone and the
command refused.

## Implementation plan

1. `WindowScope` (capabilities and ability patterns, narrowing only) in
   `security`, built from `WindowConfig`; the protocol handler passes the
   label and scope to `route`; `guard::check` applies it.
2. The assistant, `/__auth/abilities` and its events per window,
   `/__window/*` on the asking window, `CommandRequest::window`.
3. `TestShell::window` / `handle_in`, the inspector's window section, the
   docs (security, windows, inspector), the smoke test.

## As built

As designed, with these details:

- **`WindowScope`** (in `security`) holds a window's limits; `WindowConfig`
  has a public `scope` and the four builders. `TestShell::window(config)` /
  `handle_in(label, request)` / `close(label)` stand in for the webview.
- **Patterns** are the grant's: an exact name or a trailing `*`. A leading
  wildcard (`*.delete`) isn't one.
- **A refused capability is checked before the rate limit,** so a narrowed
  window's refused calls don't use up the app's allowance.
- **Client ids belong to one window.** Found while building it: the runtime
  makes its `x-elyra-client-id` up, so a page could send another window's
  and read its events, open live queries in its name, or answer its
  assistant's questions. The shell now gives each id to the first window
  that uses it (sixteen per window, the oldest let go) and refuses it from
  another, `forbidden`; a window's ids are freed when it closes. A request
  the gate refuses claims nothing.
- **`can`'s limits** come in the answer to `/__auth/abilities` (`window:
  { only?, deny }`), for the window that asks. A change of the user's list
  still goes to every window on `elyra:abilities`, without them, and the
  runtime keeps the window's (they don't change), rather than each window
  getting its own event.
- **`/__window/*`** targets the native window the request came from; if it
  has closed meanwhile, nothing happens (it used to fall back to the
  focused one).
- **`CommandRequest::window`** is set for commands and live queries from a
  window, and kept for the live query's re-runs.
- **The inspector** has `scope` (`{ label, denied, abilities }`), a "This
  window" section shown only in a window with limits, and "not in this
  window" on an ability row.

## Alternatives

- **A token per window,** injected by each webview's init script, with the
  shell mapping token to window. It works, but it's a second secret that
  a page could leak to another window (`postMessage`, shared storage), and
  the protocol handler already knows the answer without one.
- **Trust `x-elyra-client-id`.** Any page can send any id: no boundary.
- **Separate processes per window.** The strongest isolation, and far more
  than a desktop app's preview window needs; the webview already isolates
  pages from the OS.

## Open questions

1. **Identity:** the window's own protocol handler (recommended: the page
   can't forge it and there's no new secret), or a token per window?
2. **Commands without `can`** in a window with `only_abilities`: reachable,
   as in the app's grant (recommended: one rule everywhere, and the window
   can deny `Commands` outright), or refused unless allowed by name
   (`only_commands([..])`)?
3. **`.read_only()`:** a shorthand now (only `#[command(live)]` commands),
   or later (recommended: later; a command that isn't live isn't always a
   write, so the shorthand would refuse harmless calls, and
   `only_abilities` says what's meant)?
4. **`/__window/*`** on the window that asked (recommended: yes, it's what
   a page means by "close", and it stops one window steering another), or
   left as it is?
5. **`CommandRequest::window`:** in this RFC (recommended: a middleware
   can audit or refuse per window; a new field, noted under Upgrading),
   or later?

## Decisions

Settled on 2026-11-01, all as recommended:

1. **The window is known by its own protocol handler,** not by a token
   per window and not by `x-elyra-client-id`.
2. **A command without `can`** is reachable in a window with
   `only_abilities`, as in the app's grant. A window that must not reach
   those denies `Capability::Commands`.
3. **No `.read_only()` yet;** `only_abilities` says what's meant.
4. **`/__window/*` acts on the window that asked.**
5. **`CommandRequest::window`** is in this RFC.
