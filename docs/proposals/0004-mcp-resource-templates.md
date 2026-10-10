# RFC 0004 — Live commands with arguments as MCP resource templates

**Status:** implemented (2026-10-10) · **Target:** 0.11.0 · builds on [RFC 0003](0003-mcp-server.md) · see [Decisions](#decisions) · the guide: [docs/mcp.md](../mcp.md#resource-templates)

## Summary

Since 0.10, a `#[command(live)]` an agent is granted is an MCP resource, so the
agent can read it and subscribe to it. That only works when the command can run
with no arguments given. Most live commands take arguments: the generated
`customers_index(query)` takes a search and a sort, and `customers_show(id)`
takes an id. This RFC turns those commands into
[resource templates](https://modelcontextprotocol.io/specification/2026-07-28/server/resources#resource-templates),
URIs with variables:

```
app://customers_index{?direction,page,per_page,search,sort}
app://customers_show/{id}
```

An agent fills in the variables and reads or subscribes to the result, the way
a window does:

```
subscriptions/listen  { "resourceSubscriptions": ["app://customers_index?search=ada"] }
→ notifications/resources/updated whenever a write changes what that search returns
```

"Keep an eye on customer 12" becomes a subscription to `app://customers_show/12`.
Before, the agent had to poll the `customers_show` tool.

## Motivation

- **The live machinery is already there.** A live subscription stores a command
  and its arguments, and re-runs it when a table it read changes. The window's
  `live.customers_index(query)` already does this with arguments. Only the MCP
  side is missing a way to name a command *with its arguments* as a URI.
- **Agents want filtered views.** "Tell me when an order over 10 000 comes in"
  or "watch Ada's customer record" are the useful subscriptions. The whole
  unfiltered list, the one resource 0.10 can offer, rarely is.
- **It adds no reach.** Every command that would get a template is already a
  granted tool, which the agent can call with any arguments. A template adds
  *watching*, with the same arguments, through the same validation and
  middleware.

## Current state (0.10)

- `resources/list` lists `app://<command>` for each granted live command whose
  arguments can all be left out. That means every argument is an `Option`, or
  a struct whose fields all are (it's read with `{}`). A command that needs
  confirmation is never a resource.
- `resources/templates/list` answers with an empty list.
- `resources/read` and `subscriptions/listen` accept only those exact URIs.
- The live registry already stores arguments per subscription, as MessagePack.
  The live view of a window uses that today.

## Design

### Which commands get a template

A granted `#[command(live)]` that doesn't need confirmation gets a template
when its arguments **flatten** to scalar variables:

- A scalar argument (`id: i64`, `page: Option<i64>`, `status: Status` where
  `Status` is a unit-variant enum) is one variable, named after the argument.
- A struct argument contributes its fields, one level deep, when each field is
  a scalar (`CustomerQuery` → `direction`, `page`, `per_page`, `search`,
  `sort`). They go in name order. The schema's property order can differ
  between builds (serde_json's `preserve_order`), and the template mustn't.
- Anything else makes the command a tool only, with no template. That covers a
  nested struct, a list, a map, a tagged enum, `serde_json::Value`, or two
  variables with the same name. `rata mcp inspect` says why.

The JSON Schema that `tools/list` already publishes decides all of this. A
"scalar" is a schema of type `string`, `integer`, `number` or `boolean`, or an
enum of string constants, optionally nullable. That's the same projection as
the tool, so the template and the tool can't disagree.

A command whose variables are all optional keeps its plain resource
(`app://customers_index`, the defaults) and also gets a template.

### The URI

The template is a subset of [RFC 6570](https://datatracker.ietf.org/doc/html/rfc6570):

| Variable | Becomes | Example |
| --- | --- | --- |
| required | a path segment, `/{name}`, in argument order | `app://customers_show/{id}` |
| optional | a form-style query, `{?a,b}` | `app://customers_index{?search,page}` |

```
app://orders_between/{from}/{to}{?status}   ← orders_between(from: String, to: String, status: Option<Status>)
```

Reading the URI back is strict:

- Values are percent-decoded, and the path must have exactly the right number
  of segments.
- A query parameter the template doesn't name is an error, not ignored, so a
  typo can't quietly mean "no filter".
- A repeated parameter is an error too (arrays are out of v1).
- The whole URI is at most 2 KiB.

### From strings to typed arguments

A URI carries strings. Each one is converted using its variable's schema type,
before the command sees it:

| Schema | Accepts | Else |
| --- | --- | --- |
| `string` | anything, `""` included | — |
| `integer` | `-?[0-9]+`, within `i64` (and `minimum: 0` for unsigned) | error |
| `number` | a finite decimal | error |
| `boolean` | `true` / `false` | error |
| enum of constants | one of the constants | error |
| nullable, parameter absent | `null` | — |

The arguments are then rebuilt in the command's shape. Scalars stay
positional. A struct's fields go back into its object, and an absent field is
left out, so serde reads it as `None`. The command gets exactly what a
`tools/call` with the same values would give it. The command's own validation
runs as usual.

### Reading and watching

- `resources/read` with an expanded URI runs the command with those
  arguments, as the agent (`Origin::Agent`), through the middleware. It
  counts against the tool's rate limit, as a 0.10 read does.
- `subscriptions/listen` (and legacy `resources/subscribe`) with an expanded
  URI opens a live subscription with those arguments. Notifications carry the
  URI the client sent. Two URIs that spell the same arguments differently,
  such as reordered query parameters, are separate subscriptions; the client
  asked for each.
- The existing limits hold. The live registry caps subscriptions per
  connection (64), and re-runs for a subscription don't count against the rate
  limit (as in 0.10).

### Errors

| Case | Answer |
| --- | --- |
| no command / no template matches the URI | `-32602` "Resource not found", `data: { uri }` (as in 0.10) |
| a value doesn't convert, an unknown or repeated parameter | `-32602` "Invalid arguments", with the variable and why in `data` |
| the command's validation fails | `-32602` "Invalid arguments", with the field messages in `data.errors` (the validation bag) |
| the command fails otherwise | `-32603`, with its message (as in 0.10) |

A subscription to a URI that fails to convert isn't acknowledged in the
`resourceSubscriptions` list, as for an unknown URI today.

### Completion

The server declares the `completions` capability and answers
`completion/complete` for a template's variables, from what the schema knows:

- an enum variable: its constants, filtered by the typed prefix;
- a boolean: `true`, `false`;
- anything else: an empty list.

That lets a client offer `direction` → `asc` / `desc` without asking the app
anything. Completions an app computes, such as customer names, are an open
question.

### `resources/templates/list`

```json
{
  "uriTemplate": "app://customers_index{?direction,page,per_page,search,sort}",
  "name": "customers_index",
  "description": "A page of customers, searched and sorted.",
  "mimeType": "application/json"
}
```

It's in name order, with `ttlMs` and `cacheScope` on 2026-07-28, and it's
answered on the legacy era too, since `resources/templates/list` exists there.

### Testing and tooling

- `TestApp::mcp().read("app://customers_show/12")` and
  `.listen(&["app://customers_index?search=ada"])` work unchanged, because
  they take URIs.
- `rata mcp inspect` prints each template, and why a live command has none.
- The CI Inspector script reads an expanded URI from the example app.

## Security

- **No new reach.** A template exists only for a granted, live, unconfirmed
  command, which the agent can already call as a tool with any arguments.
- **The same path as a call.** The values are converted against the published
  schema, decoded by serde, validated by the command, and run through the
  middleware as `Origin::Agent`. Nothing is spliced into SQL that a tool call
  wouldn't splice.
- **Bounded.** The URI length cap, strict parsing, the subscription cap per
  connection, and the rate limit on reads.

## Alternatives considered

- **Everything in the query** (`app://customers_show{?id}`). Simpler, but a
  required argument then looks optional, and a client could expand the
  template without it. A path segment can't be left out.
- **A JSON blob in the URI** (`app://customers_index?args=%7B…%7D`). Takes any
  argument shape, but no client can present it to a user, and RFC 6570
  doesn't describe it.
- **Templates only by opt-in** (`#[command(live, template)]`). That's one more
  thing to remember. A live command is already declared read-only, and the
  grant already says who may use it.
- **Let resources take arguments a non-standard way.** Not interoperable; the
  spec has templates for exactly this.

## Implementation plan

1. **Templates in the catalog.** Derive variables from the input schema, place
   them in the path or the query, and add `Tool::template`.
   `resources/templates/list` answers with them, and `rata mcp inspect` shows
   them (and why a command has none).
2. **Expanded URIs.** Match an expanded URI to a template, convert the values,
   and rebuild the arguments. `resources/read`, `subscriptions/listen` and
   `resources/subscribe` accept these URIs, with the errors above.
3. **Completion:** `completion/complete` for enum and boolean variables, and
   the `completions` capability.
4. **Docs, the example app** (a live command with a filter), and the CI
   Inspector check.

## Open questions

1. **Which commands:** every granted live command whose arguments flatten
   (recommended: the grant and `live` already say enough), or only those
   marked for it?
2. **Required arguments in the path** (`app://customers_show/{id}`,
   recommended), or everything in the query?
3. **Completion:** from the schema only, enums and booleans (recommended for
   now), or also completers an app registers
   (`Mcp::complete("customers_index", "search", |prefix| …)`) in this
   release?
4. **Lists** (`{?tags*}`, `?tags=a&tags=b`): out of v1 (recommended, since no
   generated command takes one), or in?
5. **A validation failure on read:** `-32602` with the field messages
   (recommended: it's the arguments that are wrong), or a successful read
   whose content is the error?

## Decisions

Settled on 2026-10-10, all as recommended:

1. **Every granted live command whose arguments flatten** gets a template.
   There's no marker.
2. **Required arguments are path segments** (`app://customers_show/{id}`), and
   optional ones are the query.
3. **Completion from the schema only** (enums, booleans) in this release.
   Completers an app registers can come later.
4. **No lists in v1.** A command that takes one stays a tool without a
   template.
5. **A validation failure on read is `-32602`**, with the field messages in
   `data.errors`.
