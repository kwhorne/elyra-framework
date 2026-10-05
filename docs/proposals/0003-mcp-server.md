# RFC 0003 — Every Elyra app is an MCP server

**Status:** draft (2026-10-05) · **Target:** 0.10.0 · see [Open questions](#open-questions)

## Summary

An Elyra app's commands are already typed (specta), described, gated by
abilities and validated. That is what an AI agent needs from a tool. This RFC
lets an app expose a chosen set of its commands as tools over the
[Model Context Protocol](https://modelcontextprotocol.io/specification/2026-07-28),
so Claude Desktop or any other MCP client can work *in the running app* — and
the user watches it happen, because the app's live queries update as the agent
writes.

```rust
App::new()
    .commands(resources::commands())
    .mcp(Mcp::new()
        .allow_abilities(["customers.view", "customers.create"])
        .confirm("customers.delete"))
```

```jsonc
// Claude Desktop's config — or `rata mcp install` prints it
{ "mcpServers": { "crm": { "command": "/Applications/CRM.app/Contents/MacOS/crm", "args": ["--mcp"] } } }
```

"Add a customer for Ada, ada@example.com" → the agent calls `customers_store`
→ the app validates it like any call → the customer list in the open window
updates. Nothing reaches the agent that the app didn't grant it.

## Motivation

- **Laravel + MCP.** Laravel ships MCP servers for web apps; desktop apps have
  nothing comparable, and a desktop app is where a user's own data lives.
- **It's mostly there.** A `#[command]` has a name, typed arguments and result
  (the same specta types codegen already exports), an ability, a validation
  layer and a middleware pipeline. A tool definition is a projection of that.
- **The running app is the right place.** An agent editing the database
  behind the app's back bypasses validation, abilities and events, and the
  user doesn't see it. Going through the app's own commands keeps every rule,
  dispatches every domain event, and — with live queries — shows the change in
  the UI as it happens.

## Protocol

The target is MCP revision **2026-07-28**, which changed a lot from 2025-11-25:

- **Stateless:** no `initialize` handshake; every request carries its protocol
  version and client capabilities in `_meta`
  (`io.modelcontextprotocol/protocolVersion`, `…/clientCapabilities`), and the
  server implements `server/discover`.
- **`subscriptions/listen`** replaces `resources/subscribe`: one long-lived
  request whose stream carries the notifications the client opted into
  (`toolsListChanged`, `resourceSubscriptions: [uris]`).
- **Multi round-trip requests:** a server asks for more input by returning
  `resultType: "input_required"` with `inputRequests` (e.g. an
  `elicitation/create`), and the client retries with `inputResponses`.
- **`tools/list`** results carry `ttlMs` and `cacheScope`, in deterministic
  order; every result carries `resultType`.

Clients on 2025-11-25 still send `initialize`. The stdio transport defines the
compatibility path (a modern client probes with `server/discover`; a legacy one
doesn't), so the server can answer both — see [Open questions](#open-questions).

The transport is **stdio** only: the client launches the app's binary with
`--mcp`. No network listener, no port, nothing reachable from another machine.

## Design

### 1. Which commands are tools

Deny by default, in the vocabulary the app already has — **abilities**:

```rust
App::new().mcp(Mcp::new().allow_abilities(["customers.view", "customers.create"]))
```

A command is a tool when its `can = "…"` ability is granted to the agent —
the same `customers.*` wildcards as `App::allow_abilities`, but a **separate
grant**: what the frontend may call and what an agent may call are different
decisions. A command without a `can` is never a tool (see Open question 1).
`make:resource` commands all have abilities, so a resource is one line away.

### 2. The tool definition

| Tool field | From |
|---|---|
| `name` | the command name (`customers_store`) |
| `description` | the command's doc comment — `#[command]` starts capturing `///` |
| `inputSchema` | an object with one property per argument, built from the argument types' specta definitions (field docs become `description`s), JSON Schema 2020-12 |
| `outputSchema` | the return type's schema (the `Ok` side of a `Result`) |
| `annotations.readOnlyHint` | `true` for a `#[command(live)]` (a live command can't write) |
| `annotations.destructiveHint` | `true` when the ability is in `Mcp::confirm(..)` |

The schema converter applies serde's attributes first (specta-serde, as codegen
does), and follows Elyra's number policy (`i64` → `integer`).

### 3. A call

`tools/call { name, arguments }` → the arguments, by name, become the
positional MessagePack body every command takes → `CommandRegistry::dispatch`
through the **full middleware pipeline**, with validation. Then:

- `Ok(value)` → `structuredContent` = the value as JSON, plus a text block of
  the same JSON (the spec's backwards-compatibility rule);
- a `ValidationErrors` bag → a tool *execution* error (`isError: true`) with
  the field messages, so the model can correct itself;
- any other command error → `isError: true` with its message;
- unknown tool, malformed arguments → JSON-RPC protocol errors.

Middleware can tell an agent's call from the frontend's: the request carries
its origin (`Origin::Agent { client }`), so an `audit` middleware can log it
and an `auth` one can refuse it.

### 4. Confirmation for what can't be undone

`Mcp::confirm("customers.delete")` makes the call a multi round-trip: the
first `tools/call` returns `input_required` with an `elicitation/create`
("Delete customer #12, Ada Lovelace?" — the command's description and the
arguments), and only a retry with an accepting `inputResponses` runs it. A
client that can't elicit gets an error saying the tool needs confirmation —
never a silent run. (Option: confirm in the app itself instead — Open
question 3.)

### 5. Live commands are resources

A `#[command(live)]` whose arguments all have defaults is also an MCP
**resource**, `app://<command>`: `resources/read` runs it, and a client that
lists the URI in `subscriptions/listen`'s `resourceSubscriptions` gets
`notifications/resources/updated` whenever the live registry would push it to
a window. The agent can watch "today's orders" the way a window does.

### 6. Running app or not

`myapp --mcp` is a small **stdio shim**:

- **The app is running:** the shim connects to it over a private local
  endpoint, built like single-instance's (a `0600` Unix socket; on Windows,
  loopback with the per-install token), and pipes MCP lines both ways. The
  MCP server runs *inside the app*: same database, same live registry, same
  domain events — the open windows update as the agent works.
- **It isn't:** the shim runs the app headless (no window, like
  `ELYRA_MIGRATE`) and serves MCP itself. Everything works except that no
  window shows it. (Option: refuse instead — Open question 4.)

The endpoint is separate from single-instance's, and exists only when the app
calls `.mcp(..)`.

### 7. Seeing what the agent does

- Every call is logged under `elyra::mcp` (tool, outcome, duration — not the
  arguments, which may be personal data).
- It's dispatched as a domain event, `AgentCalled { tool, client, ok }`, for an
  audit log.
- It's emitted on the `elyra:mcp` channel, so the frontend can show "Claude is
  adding a customer…" — the spec asks for visible indicators.

### 8. Tooling and tests

- `rata mcp inspect` — the tools, schemas and resources the app exposes, from
  the same codegen pass.
- `rata mcp install [--client claude]` — prints the client config for this
  app's binary (it doesn't edit another app's config).
- `TestApp::mcp()` — an in-process client: `list_tools()`,
  `call("customers_store", json!({...}))`, `read(uri)`, so an app tests what
  an agent sees.
- CI: the official MCP inspector's CLI against the example app.

## Security

An agent is an untrusted caller whose input can come from anything it read —
including text a tool returned (prompt injection). So:

- **Nothing is exposed by default**; the agent's grant is separate from the
  frontend's, and only commands with an ability can be granted.
- **The same rules as any call:** every call goes through the middleware
  pipeline and the command's validation; an agent can't reach anything a
  command doesn't do — no filesystem, shell, clipboard or window routes.
- **Confirmation** for the abilities the app names, enforced by the server,
  not left to the client.
- **Local only:** stdio, launched by the user's MCP client; the running-app
  endpoint is user-only (`0600` / per-install token), like single-instance's.
- **Rate limited** per tool, reusing `RateLimiter`.
- **Visible:** logs, an event, a channel for an in-app indicator.

## Implementation plan

1. **Tool catalog:** `#[command]` captures doc comments; specta → JSON Schema
   (inputs and outputs, serde-aware); `Mcp::new().allow_abilities(..)`;
   `rata mcp inspect`.
2. **MCP server core:** JSON-RPC over newline-delimited stdio, `server/discover`,
   `_meta` versioning, `tools/list` (`ttlMs`, `cacheScope`, deterministic
   order), `tools/call` → dispatch with `Origin::Agent`, errors;
   `TestApp::mcp()`.
3. **The shim:** `--mcp`, the private endpoint in the running app, the
   headless fallback.
4. **Confirmation:** `Mcp::confirm`, the `input_required` round trip.
5. **Resources:** live commands as resources, `resources/read`,
   `subscriptions/listen` driven by the live registry.
6. **Visibility, `rata mcp install`, docs, the example app, CI with the
   inspector.**

## Alternatives considered

- **An HTTP MCP server inside the app** (Streamable HTTP on localhost). Works
  with remote clients, but it's a network listener with its own auth, CORS and
  port — the same reasons an HTTP transport was dropped for the frontend.
  stdio is what desktop MCP clients launch.
- **A separate MCP server over the database.** Simpler, and wrong: it bypasses
  validation, abilities and events, and the app never knows.
- **Expose every command.** Convenient for a demo, dangerous in an app with a
  `delete_account` command. Abilities make the choice explicit.
- **An SDK (`rmcp`).** Worth evaluating in step 2; the surface Elyra needs
  (stdio, tools, resources, one subscription type) is small, and the 2026-07-28
  revision's stateless model changes what an SDK even does.

## Open questions

1. **Commands without an ability:** never tools (recommended — forces the
   explicit choice), or allowed with a `#[command(tool)]` marker?
2. **Protocol versions:** serve both 2026-07-28 and the legacy 2025-11-25
   `initialize` flow (recommended, since clients are moving at different
   speeds), or only 2026-07-28?
3. **Confirmation:** in the MCP client via elicitation (recommended — it's
   where the user is talking to the agent), in the app with a native dialog,
   or either, chosen by the app?
4. **App not running:** run headless (recommended), or refuse and tell the
   agent to have the user open the app?
5. **Origin in middleware:** a new field on `CommandRequest` (breaking for code
   that builds one by hand — rare), or an accessor on `Ctx`? Recommended: a
   field, with the 0.10 release note.
