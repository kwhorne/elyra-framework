# AI agents (MCP)

An Elyra app can be an [MCP](https://modelcontextprotocol.io) server: an AI
agent in Claude, Cursor or VS Code calls the commands you choose, as tools.
They are the same commands the frontend calls, through the same middleware and
validation. Nothing is exposed until you say so. The design is
[RFC 0003](proposals/0003-mcp-server.md).

## In five lines

```rust
App::new()
    .commands(resources::commands())
    .mcp(Mcp::new()
        .allow_abilities(["customers.view", "customers.create"])
        .allow_ability("customers.delete")
        .confirm("customers.delete"))
```

```sh
rata mcp inspect      # what the agent will see
rata mcp install      # the config that connects Claude to this app
```

The client launches `myapp --mcp`. If the app is open, the agent works in it:
the windows update as it adds a customer. If it isn't, the app runs headless,
without a window.

## What's a tool

A command is a tool when the ability its `#[command(can = "…")]` declares is
granted with `Mcp::allow_ability` / `allow_abilities`. An exact name or a
namespace (`customers.*`) works. A command without an ability is never a tool.
This grant is separate from the frontend's `App::allow_abilities`.

Each tool is a projection of its command:

| MCP | From |
| --- | --- |
| `name` | the command's name (`customers_store`) |
| `description` | its `///` doc comment |
| `inputSchema` | a JSON Schema of its arguments, built from the specta types, after serde's renames, tagging and flattening |
| `outputSchema` | the same, for its result |
| `annotations.readOnlyHint` | `true` for a `#[command(live)]` |
| `annotations.destructiveHint` | `true` when its ability is in `Mcp::confirm` |

Arguments arrive by name: `{ "input": { "name": "Ada" } }` for
`customers_store(ctx, input: CustomerInput)`. An `Option` argument, or an
`Option` field, may be left out.

When a call fails, the model gets an error it can act on (`isError`), not a
protocol error. That covers a validation bag (with the messages per field), a
command's `Err`, a panic, an unknown argument, and a type that doesn't decode.

Write the doc comments for the model: what the command does, and when to use
it. Only the first line is shown when the user is asked to confirm.

## Confirmation

`Mcp::confirm("customers.delete")` makes the server ask the user before such a
call runs. The question appears in the MCP client: who is asking, which tool,
the first line of its description, and the arguments.

- A 2026-07-28 client gets a multi round-trip: `input_required` with an
  `elicitation/create`, then a retry carrying the user's answer.
- A legacy client that declared elicitation gets a server-initiated
  `elicitation/create`.
- A client that can't ask gets an error, and the tool doesn't run. So does a
  decline, a dismissal, or no answer within 10 minutes.

The server enforces this; it isn't a hint the client may ignore. The round
trip's `requestState` is signed, bound to the tool, the arguments and the
client, and expires after 5 minutes. It also runs the call only once.

## Resources

A granted `#[command(live)]` that can run with no arguments given is also a
**resource**, `app://<command>`. That means every argument is an `Option`, or
a struct whose fields all are. `resources/read` runs it.

A client can subscribe (`subscriptions/listen`, or `resources/subscribe`
before 2026-07-28). It's then told whenever the live registry would push a new
result to a window: coalesced, only when the result changed, and only for
what the command read. See [live queries](live-queries.md). The agent watches
"today's orders" the way a window does.

## Seeing what it does

- **Logs:** every call and read is logged under `elyra::mcp`, with its outcome
  and duration. The arguments aren't logged, since they may be personal data.
- **An event, for an audit log:**

  ```rust
  App::new().listen(|e: elyra::mcp::AgentCalled, ctx: Ctx| async move {
      // e.tool, e.client, e.ok, e.millis
      Ok(())
  })
  ```

- **A channel, for the UI:** `elyra:mcp` carries a `started` and a
  `finished` for every call:

  ```ts
  import { onAgent, toast } from "@elyra/runtime";
  onAgent((a) => {
    if (a.phase === "started") toast(`${a.client} is running ${a.tool}…`);
  });
  ```

- **Who's calling, in middleware:** `CommandRequest::origin` is
  `Origin::Agent { client }` for an MCP call, and `Origin::Frontend`
  otherwise. Refuse an agent there, or audit it
  ([middleware](middleware.md)).

## Rate limits

Every tool takes at most 60 calls a minute by default, counted per tool across
all connections. That's plenty for an agent at work, and a stop for one caught
in a loop. Change it per ability, namespace, or for everything:

```rust
Mcp::new()
    .allow_abilities(["customers.*"])
    .rate_limit("*", 120, Duration::from_secs(60))
    .rate_limit("customers.create", 10, Duration::from_secs(60))
```

The last limit that matches wins. A throttled call is an error the model sees,
and it's refused before the user is asked to confirm it. Reads count;
re-runs for a subscription don't.

## Connecting a client

```sh
rata mcp install                  # Claude Code and Claude Desktop
rata mcp install --client cursor
rata mcp install --client vscode
rata mcp install --release        # the optimized build
```

`rata mcp install` builds the app and prints the config for that binary.
It edits nothing; you paste the config where the client reads it. For a
shipped app, point `command` at the installed binary instead
(`/Applications/CRM.app/Contents/MacOS/crm`), with `"args": ["--mcp"]`.

When the app has no `App::database(..)`, a headless run reads `DATABASE_URL`.
`rata mcp install` puts the one from `elyra.toml` in the config's `env`.

### The running app, or headless

`myapp --mcp` is a small shim:

- **When the app is open,** the shim connects to it over a private local
  endpoint, and the agent works inside the app. It shares the app's database
  connection, live queries and events, so the windows update, and one agent
  sees another's changes.
- **When it isn't,** the shim serves MCP itself, without a window.
  Everything works, but nothing shows it, and two headless agents don't hear
  about each other's writes.

The endpoint exists only when the app calls `.mcp(..)`. It's a `0600` Unix
socket, or loopback on Windows, and a connection is served only after a
challenge-response on a per-install token (see [security](security.md)).

## Testing

`TestApp::mcp()` is an in-process client, so a test checks what an agent
sees:

```rust
let app = TestApp::new(App::new().commands(resources::commands()).mcp(mcp()));
let agent = app.mcp();
assert!(agent.list_tools().await.iter().any(|t| t["name"] == "customers_store"));

let added = agent.call("customers_store", json!({ "input": { "name": "Ada" } })).await;
assert!(!added.is_error, "{}", added.text);

// Confirmation, answered as the user would.
let gone = app.mcp().confirming("accept").call("customers_destroy", json!({ "id": 1 })).await;
assert!(gone.confirmation.unwrap().contains("customers_destroy"));

// Resources and subscriptions.
let page = agent.read("app://customers_index").await.unwrap();
let (_id, watching) = agent.listen(&["app://customers_index"]).await;
```

## Protocol support

- **2026-07-28**, the stateless revision. It covers per-request `_meta`,
  `server/discover`, `tools/list` with `ttlMs` and `cacheScope`, multi
  round-trip confirmation, and `subscriptions/listen`.
- **2025-11-25, 2025-06-18 and 2025-03-26**, for clients that open with
  `initialize`. Elicitation came in 2025-06-18.

The transport is stdio only. There's no network listener.

CI drives the example app with the official
[MCP Inspector](https://github.com/modelcontextprotocol/inspector) CLI:
`scripts/mcp-inspector.sh`.

## Related

- [Commands](commands.md) · [Middleware](middleware.md) ·
  [Live queries](live-queries.md) · [Security](security.md) ·
  [Testing](testing.md)
