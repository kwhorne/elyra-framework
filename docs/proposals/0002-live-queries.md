# RFC 0002 — Live queries

**Status:** accepted (2026-10-05) · **Target:** 0.9.0 · see [Decisions](#decisions)

## Summary

A command whose result the frontend can *subscribe* to: Elyra re-runs it when
the data it read changes, and pushes the new result to every window that's
watching. No polling, no manual "reload after save", no event wiring per view.

```rust
#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> Result<Page<Customer>> {
    // unchanged — an ordinary query
}
```

```svelte
<script>
  import { live } from "./bindings";
  let query = $state({ search: null, page: 1 });
  const customers = $derived(live.customers_index(query));
</script>

{#each $customers.value?.data ?? [] as c (c.id)}<p>{c.name}</p>{/each}
```

Save a customer in one window and the list in every other window updates —
because the store command wrote to `customers`, and the list read from it.

## Motivation

Rust owns the state in an Elyra app; the frontend renders it. Today that's
one-way at a single moment: a view calls `api.customers_index(...)` once and
holds a copy that starts going stale immediately.

- **Within a window**, every screen re-fetches by hand after a write. The
  `make:resource` list does it with a `version` counter it bumps after a
  delete — every view gets its own variant of this, and some forget.
- **Across windows** it's worse: a second window showing the same list has no
  idea anything changed. The tools exist (`App::broadcast` and a domain event
  per write), but each view has to subscribe to the right events and decide
  what to re-fetch — and nobody does that for every screen.
- **Background work** — the queue, the scheduler, a sync job — writes data
  the UI is showing, with no path to tell it.

What a desktop app has that a web app doesn't is that **every write happens
in the same process as every read.** The model layer sees each `insert`,
`update` and `delete`, and knows the table. Invalidation that needs websockets,
Redis and a cache-tagging scheme on the web is, here, a function call. Elyra
should use that.

## Current state

- **Reads** go through `Query<M>`'s execution methods (`get`, `first`, `find`,
  `count`, `paginate`, aggregates, `exists`, `chunk`) and the generated
  relation loaders — each knows `M::TABLE` and any joined tables.
- **Writes** go through the generated `insert` / `update` / `delete` / `save`,
  `Query::update` / `delete` / `soft_delete` / `restore`, factories (which use
  `insert`), and `Database::transaction`. Raw `sqlx` through `db.pool()` and
  migrations bypass the model layer.
- **`EventBus`** pushes MessagePack batches over one long-poll per window, and
  fans every emit out to *every* connected window; there's no per-window send.
- **`channel(name)`** in `@elyra/runtime` is already a Svelte store over that
  connection.
- **Codegen** emits a typed `api.*` per command.

## Design

### 1. Dependencies are recorded, not declared

While a live command runs, the model layer records the tables it reads into a
task-local *read set* — `Customer::query().paginate(..)` adds `customers`, a
`join("orders", ..)` adds `orders`, a `with_team()` adds `teams`. The command
needs no annotation; whatever it reads is what it depends on.

Reads the model layer can't see (raw `sqlx`) are declared:

```rust
ctx.depends_on("orders");          // a table
ctx.depends_on("settings:theme");  // any key — not only tables (see 4)
```

The read set lives in a task-local, so a query run on a *spawned* task isn't
recorded. That's documented, and `depends_on` covers it.

### 2. Writes announce what they changed

`Database` gains a change hub (an `Arc` inside it, so every clone shares it).
Each model-layer write reports its table once it's durable:

- outside a transaction — after the statement succeeds;
- inside `Database::transaction` — buffered, and reported **on commit**; a
  rollback reports nothing;
- raw SQL and other processes — not seen; `db.touch("orders")` reports by hand.

### 3. The live registry re-runs what's affected

`#[command(live)]` marks a command as subscribable. Subscribing goes through
the normal guard — `Capability::Commands`, the command's `can` ability, rate
limits — and the result comes back inline, so the first render doesn't wait
for an event:

```
POST elyra://localhost/__live/subscribe   [command, args…]  -> { id, value }
POST elyra://localhost/__live/unsubscribe [id]
```

The registry keeps `(id, window, command, args, read set)`. When the hub
reports a change:

1. Find the subscriptions whose read set contains the table.
2. **Coalesce**: a burst of writes (an import, a seeder, a bulk update) causes
   one re-run per subscription per batch window, not one per row.
3. Re-run the command through the full middleware pipeline, as the original
   call did, recording a fresh read set (it can change: a different branch,
   a different join).
4. **Skip unchanged results**: if the encoded result is byte-identical to the
   last one sent, nothing is pushed.
5. Push the result to *that subscription's window only*, on
   `elyra:live:<id>` — which needs a new `EventBus::emit_to(client, ..)`.

A window that closes or reloads drops its subscriptions (the bus already
notices disconnects).

### 4. Not only tables

The hub's keys are strings; a table is just `table:customers`. The same
mechanism serves anything the app owns:

```rust
ctx.invalidate("settings:theme");   // e.g. from a Store or Cache write
```

so a `#[command(live)] fn current_settings` can follow the settings store, and
a queue job can invalidate `"reports:monthly"` when it finishes.

### 5. Live commands don't write

A live command is re-run by Elyra, as often as its data changes, so it must be
a pure read: a write inside it would re-trigger itself. During a live run the
model layer rejects writes with an error naming the command and table, and the
generated tests exercise it. (A one-off `api.*` call of the same command is an
ordinary call and isn't restricted.)

### 6. The frontend

Codegen emits `live.*` next to `api.*` for every `#[command(live)]`:

```ts
live.customers_index(query: CustomerQuery): Readable<Live<Page<Customer>>>

interface Live<T> {
  value: T | undefined;          // the latest result
  error: CommandError | null;    // the latest re-run's failure, if it failed
  loading: boolean;              // true until the first result
}
```

The store subscribes on first use and unsubscribes when the last listener goes
(component destroyed). In Svelte 5, `$derived(live.customers_index(query))`
re-subscribes when `query` changes. A failed re-run keeps the last `value` and
sets `error`, so a transient error doesn't blank the screen.

### 7. Testing

```rust
let mut list = app.live::<Page<Customer>>("customers_index", (CustomerQuery::default(),)).await;
assert_eq!(list.value().total, 0);
app.invoke_ok::<Customer>("customers_store", (input(1),)).await;
assert_eq!(list.next().await.total, 1);   // pushed, not re-fetched
```

### 8. `make:resource`

The generated list becomes `live.<plural>_index(query)`; the `seq` / `version`
bookkeeping disappears; and the generated tests assert that a store shows up
in a live list. Every resource gets multi-window updates for free.

## Security

- Subscribing is a command call: the same capability, ability and rate-limit
  checks. Re-runs reuse the subscription's context, so a re-run never sees more
  than the original call could.
- **Bounded**: a cap on subscriptions per window (64 by default), coalescing,
  and the unchanged-result check keep a busy write path from turning into a
  push storm.
- A live command can't write (5), so a subscription can't be made into a loop.
- Subscription ids are random per window; another window can't unsubscribe,
  or listen to, someone else's.

## Performance

A change re-runs only the subscriptions that read that table, once per batch
window, and pushes only when the result differs. A `Page` of 25 rows is a few
KB of MessagePack. The cost that grows is *re-running queries*: N windows × M
subscriptions on a hot table. v1 accepts that (they're local SQLite queries)
and measures it; row-level invalidation or diffs come later if numbers say so.

## Alternatives considered

- **Polling** (`setInterval(load, 2000)`) — trivial, but stale between polls,
  wasteful when nothing changed, and every view does it differently.
- **Domain events + manual refetch** — possible today with `App::broadcast`,
  but each view must know which events affect it. Live queries derive that
  from what the command read, which is exact and can't drift.
- **Database-level change capture** — SQLite's `update_hook`, Postgres
  `LISTEN/NOTIFY`. Would catch raw SQL and other processes too, but is
  per-driver and not reachable through sqlx's `Any` driver. A possible later
  source for the same hub (see Open questions).
- **Declared dependencies only** (`#[command(live(tables = ["customers"]))]`)
  — explicit, but it drifts from the code the moment someone adds a join.
  Recording reads can't drift; `depends_on` remains for the gaps.
- **Local-first / CRDT sync** — a different problem (offline, many devices);
  live queries are the in-process half of it and would be its UI layer.

## Implementation plan

1. **Change hub + read tracking** in `elyra-db`: the task-local read set
   across `Query` and the relation loaders, write reporting incl. commit-time
   reporting for transactions, `touch`, write rejection in live runs. Unit
   tests per read and write path.
2. **Live registry** in the framework: `#[command(live)]`, `/__live/subscribe`
   and `/unsubscribe`, `EventBus::emit_to`, coalescing, unchanged-result
   skipping, the per-window cap, cleanup on disconnect, `ctx.depends_on` /
   `ctx.invalidate`.
3. **Frontend**: `live()` in `@elyra/runtime` and `live.*` in codegen.
4. **`TestApp::live`**.
5. **`make:resource`**: the list on `live.*`, the generated live test, the
   smoke harness extended (a write in one "window" updates another).
6. **Docs + the example app**: two windows on one list.

Each step is a PR; 1–2 are useful on their own (Rust-side invalidation).

## Open questions

1. **Opt-in per command (`#[command(live)]`), or every read command
   subscribable?** Recommended: opt-in. It's explicit, codegen only emits
   `live.*` where it makes sense, and the "live commands don't write" rule has
   a clear scope.
2. **The store's value: `Live<T>` (`{ value, error, loading }`) or plain `T`
   with errors elsewhere?** Recommended: `Live<T>` — loading and a failed
   re-run are states every screen needs to show.
3. **Granularity: table-level in v1?** Recommended: yes — simple, exact
   enough for local data, and measurable before anything finer.
4. **Batch window for coalescing:** reuse `App::batch_window` (default: none)
   or a live-specific default (~16 ms, one frame)? Recommended: a live default
   of one frame, configurable.
5. **Database change capture later?** SQLite `update_hook` / Postgres
   `LISTEN/NOTIFY` as an additional hub source, to catch raw SQL and other
   processes. Recommended: out of v1, revisit with the sync work.

## Decisions

Settled on 2026-10-05, all as recommended:

1. **Opt-in** with `#[command(live)]`; only those get `live.*`, and only those
   are held to "live commands don't write".
2. **`Live<T>`** — `{ value, error, loading }` — is the store's value.
3. **Table-level** invalidation in v1; finer granularity only if measurements
   ask for it.
4. **One frame (~16 ms)** of coalescing by default, configurable.
5. **No database change capture** in v1; revisit with the sync work.
