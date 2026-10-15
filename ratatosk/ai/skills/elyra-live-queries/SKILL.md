---
name: elyra-live-queries
description: Elyra Framework live queries — #[command(live)] results that re-run and push themselves when the data they read changes, live.* stores in Svelte, depends_on / invalidate for what the model layer can't see, and testing them. Use when a list, detail, count or dashboard in an Elyra app should update without a reload.
---

# Elyra live queries

```rust
#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    let db = ctx.get::<Database>();
    Ok(Customer::query().order_by("name").paginate(&db, query.page, 25).await?)
}
```

```svelte
<script>
  import { live } from "./bindings";
  const customers = $derived(live.customers_index(query));
</script>
{#each $customers.value?.data ?? [] as c (c.id)}<p>{c.name}</p>{/each}
```

- Reads through the model layer record their tables; writes through it
  report theirs. A write re-runs only the subscriptions that read that table,
  coalesced per frame, pushed only when the result changed, to the
  subscribing window.
- **A live command only reads.** A write inside one fails. The same command
  called once through `api.*` is an ordinary call.
- Raw SQL, files, settings, another process: `ctx.depends_on("orders")` /
  `ctx.depends_on("settings:currency")` in the live command, and
  `db.touch("orders")` or `ctx.invalidate("settings:currency")` where it
  changes. Reads on a task the command spawns aren't recorded: declare them.
- A `Live<T>` store has `value`, `error` (the last re-run's, keeping the old
  value) and `loading`. Run `rata codegen` after adding a live command.
- Needs the `database` feature. A window holds at most 64 subscriptions.
- For fine-grained streams (progress) use a channel; for an expensive report
  keep a plain `api.*` call.

## Testing

```rust
let mut list = app.live::<Page<Customer>>("customers_index", (CustomerQuery::default(),)).await;
assert_eq!(list.value().total, 0);
app.invoke_ok::<Customer>("customers_store", (input,)).await;
assert_eq!(list.next().await.total, 1);
assert!(!list.updated_within(Duration::from_millis(100)).await);
```

A granted live command is also an MCP resource (`app://customers_index`).
`search_docs` ("live queries") has the rest.
