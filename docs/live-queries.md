# Live queries

A live query is a command the frontend *subscribes* to. Elyra records what it
reads, re-runs it when that data changes, and pushes the new result to every
window that's watching. Nothing on the frontend polls or reloads after a save,
and two windows showing the same list stay in step.

It works because a desktop app's writes all happen in the same process as its
reads: the model layer sees every `insert`, `update` and `delete`, and knows the
table. The design is [RFC 0002](proposals/0002-live-queries.md).

## In five lines

```rust
#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    // an ordinary query — nothing live-specific in here
}
```

```svelte
<script>
  import { live } from "./bindings";
  const customers = $derived(live.customers_index(query));
</script>
{#each $customers.value?.data ?? [] as c (c.id)}<p>{c.name}</p>{/each}
```

Save a customer anywhere — this window, another one, a queue job — and the
list updates. `rata make:resource` generates its list and detail view this way.

## How it decides what to re-run

**Reads are recorded.** While a live command runs, every model-layer read adds
its table to the command's read set: `Customer::query()…` adds `customers`, a
`join("orders", …)` adds `orders`, a relation adds the related table, a
`belongs_to_many` adds its pivot. Nothing to declare.

**Writes are reported.** Every model-layer write reports its table once it's
durable: the generated `insert` / `update` / `delete` / `save`, `Query`'s bulk
`update` / `delete` / `soft_delete` / `restore` (only when a row changed),
factories, and pivot `attach` / `detach` / `sync`. Inside
`Database::transaction` the reports go out on commit; a rollback sends none.

**What changed is matched against what was read.** Only subscriptions whose
read set contains the table re-run. Then:

- changes are **coalesced** — a burst of writes is one re-run per subscription
  per batch window (one frame, 16 ms, by default:
  `App::live_batch_window(duration)`);
- a result **byte-identical** to the last one pushed isn't pushed;
- the push goes to the subscribing **window only** (`elyra:live:<id>`);
- re-runs go one round at a time, so an older result never arrives after a
  newer one.

## What the model layer can't see

Raw SQL, another process, a file, a cache, a setting — declare and report those
yourself. Keys are strings: a table's key is its name, anything else is yours
(`settings:theme`).

```rust
#[command(live)]
async fn report(ctx: Ctx) -> elyra::Result<Report> {
    ctx.depends_on("orders");          // read with raw SQL below
    ctx.depends_on("settings:currency");
    // …
}

// elsewhere
sqlx::query("UPDATE orders SET …").execute(db.pool()).await?;
db.touch("orders");                    // or ctx.invalidate("orders")
ctx.invalidate("settings:currency");
```

`ctx.depends_on` records into the running command's read set, so a read on a
task the command *spawns* isn't recorded — declare it.

## The frontend

`rata codegen` gives every live command a store in `live.*` next to its
`api.*` call. Its value is a `Live<T>`:

| Field | |
|---|---|
| `value` | the latest result (`undefined` until the first one) |
| `error` | the latest re-run's failure — `CommandError`, or `ValidationError` — with the previous `value` kept |
| `loading` | `true` until the first result |

The subscription opens with the store's first listener and closes with its
last, so `$store` in a component is the whole lifecycle. With `$derived`, a
changed argument opens a new subscription and closes the old one. Without
codegen, `live<T>(command, ...args)` from `@elyra/runtime` does the same,
untyped. See [the frontend runtime](frontend-runtime.md#live-queries--live).

## Rules

- **A live command only reads.** Elyra re-runs it whenever its data changes, so
  a write inside it would re-trigger itself: during a live run, a model-layer
  write fails with "`x` is a live command and must only read". The same
  command called once through `api.*` is an ordinary call.
- **Opt-in:** only `#[command(live)]` can be subscribed to.
- **The same guard as any call:** subscribing (`POST /__live/<command>`)
  needs `Capability::Commands` and the command's `can` ability.
- **Bounded:** a window holds at most 64 subscriptions; a window that goes
  away takes its subscriptions with it.
- **Needs the `database` feature** (the change tracking lives in the database
  layer); `ctx.invalidate` keys work with or without a `Database` bound.

## Testing

```rust
let mut list = app.live::<Page<Customer>>("customers_index", (CustomerQuery::default(),)).await;
assert_eq!(list.value().total, 0);

app.invoke_ok::<Customer>("customers_store", (input,)).await;
assert_eq!(list.next().await.total, 1);          // pushed, not re-fetched

assert!(!list.updated_within(Duration::from_millis(100)).await);  // and nothing else
```

`next()` waits (up to five seconds) for the next push; `next_update()` returns
a failed re-run as `Err`; dropping the handle unsubscribes. Each handle is its
own window, so handles don't see each other's pushes.

## When not to

A live query re-runs the whole command. That's cheap for the local queries a
desktop app shows — a page of rows, a count — and it's what you want for lists,
details, dashboards and badges. For a stream of fine-grained events (progress,
a cursor), emit on a [channel](events.md) instead; for an expensive report,
keep it an `api.*` call the user refreshes.

## Related

- [Commands](commands.md#live-commands-live) · [Database — changes](database.md#changes)
- [Frontend runtime](frontend-runtime.md#live-queries--live) · [Resources](resources.md)
- [RFC 0002](proposals/0002-live-queries.md)
