# RFC 0017 — A write re-runs what showed that record, not every view of the resource

**Status:** accepted (2026-11-02) · **Target:** 0.30.0 · builds on [RFC 0002](0002-live-queries.md), [RFC 0005](0005-laravel-backend.md) and [RFC 0006](0006-laravel-broadcasting.md) · see [Decisions](#decisions)

## Summary

A live query over the Laravel backend depends on its whole resource. Ada
renames customer 40 on the web, and every desktop app with customers open
fetches every view of them again: the list, and customer 42, and customer 7.
None of those changed. This RFC keys a read of one record by that record:

```
GET /api/customers/42        depends on  backend:/api/customers/42  (and the resource)
GET /api/customers?page=1    depends on  backend:/api/customers     (the lists)
PUT /api/customers/40        re-runs     the lists, and what read customer 40
CustomerUpdated { model: { id: 40 } }    the same
```

Nothing changes in an app: `make:resource --backend` already reads
`/api/customers/{id}`, writes `PUT /api/customers/{id}`, and has Laravel
broadcast `CustomerUpdated` with the model.

## Motivation

The measurement ([`tests/live_reruns.rs`](../../framework/tests/live_reruns.rs),
0.30): a list and ten detail views open, a customer none of them shows renamed
ten times.

| | re-runs | pushed | unchanged |
|---|---|---|---|
| SQLite, a row no view shows | 110 | 0 | 110 |
| SQLite, a row the list and a view show | 110 | 20 | 90 |
| SQLite, a new row | 110 | 10 | 100 |
| **Backend, a row no view shows** | **110 requests** | 0 | 110 |

- **Over SQLite it's cheap:** a re-run is a local query, about 0.35 ms in a
  debug build. Eleven of them per write isn't felt.
- **Over the backend each is a request to the server,** from every app that
  has the resource open, for every change to any record of it. Twenty users
  with eleven views open: one edit on the web is 220 requests that find
  nothing new, and the server's load grows with users × views × edits.
- **The backend tells us which record.** The id is in the URL a view read
  and a write went to, and in the model Laravel broadcasts. SQLite's bulk
  `update`s and raw SQL don't say which rows; that's why this RFC starts
  with the backend.

## Current state (0.29)

- `Backend::get(path)` inside a live command records `resource_keys(path)`:
  the path without its query and trailing ids, and each path it's under.
  `/api/customers/42` and `/api/customers?page=2` are both
  `backend:/api/customers`; `/api/customers/12/notes` is
  `backend:/api/customers/12/notes`, `…/customers/12`, `…/customers` and
  `backend:/api`.
- A successful write invalidates `resource_key(path)`: `PUT
  /api/customers/40` invalidates `backend:/api/customers`. So does an
  outbox write once it's sent (RFC 0007).
- An event on a followed channel invalidates its follow's resource:
  `private-customers` → `backend:/api/customers`, whatever the event.
- `ctx.invalidate(..)` and `ctx.depends_on(..)` take any key; apps use
  `backend::resource_key(..)` to name a resource.
- The inspector lists each query's keys (`reads`), and since 0.30 how often
  it re-ran and was pushed.

## Design

### Three kinds of key

For a resource `/api/customers`:

| key | read by | invalidated by |
|---|---|---|
| `backend:/api/customers` — the resource | every read of it, lists and records | a change that doesn't say which record; `ctx.invalidate` of it |
| `backend:/api/customers/42` — a record | a read of `/api/customers/42…` | a write to `/api/customers/42…`; an event naming 42 |
| `backend:list:/api/customers` — its lists | a read of `/api/customers` (any query) | every write to the resource; every event on it |

- **A list** (no trailing id) depends on the resource and its lists: any
  change to any record may change a page, its order or its total.
- **A record** depends on the resource and itself, not on the lists: a
  change to customer 40 doesn't re-run customer 42.
- **A write** to `/api/customers/40` invalidates the lists and record 40; a
  `POST /api/customers` the lists (no view has read the new one yet).
- **An event** whose data names a record (`model.id`, see below)
  invalidates the lists and that record; one that doesn't, the resource:
  everything, as today.
- **Under a record:** `/api/customers/12/notes` depends on its own resource
  and lists, on record `backend:/api/customers/12`, and on the resources
  above it (`backend:/api/customers`, `backend:/api`), not on customers'
  lists. A change to customer 12 re-runs its notes, as today; a change to
  customer 99 doesn't, as it does today.

The resource key keeps its meaning, "anything here may have changed": an
app's `ctx.invalidate(&resource_key("/api/customers"))` re-runs every view
of customers, as it does today.

### Which record an event names

Laravel's model broadcasting (`BroadcastsEvents`, what `make:resource
--backend` sets up) sends `CustomerCreated`, `CustomerUpdated`,
`CustomerDeleted`, `CustomerTrashed` and `CustomerRestored` with `{ model:
{ …attributes } }`. The record is `model.id`.

- **The key's name:** `id` by default; for a model whose route key is
  another column, the resource says which:
  `Backend::new(url).follow("customers", "/api/customers").record_key("/api/customers", "uuid")`.
- **An event without one** (a custom `ShouldBroadcast` with its own data, a
  bulk import) invalidates the whole resource, as today. Nothing is missed;
  it's only not narrowed.
- **The value** must be an id as `is_id` knows them (a number, a UUID, a
  ULID), so a broadcast can't make a key out of arbitrary text.

### Writes

The write's own path names the record: `PUT /api/customers/40`, `PATCH
…/40`, `DELETE …/40`, and `POST /api/customers/40/archive` (a record's
action) all invalidate the lists and record 40. `POST /api/customers`
invalidates the lists. The outbox's writes, when sent, do the same with the
path they were queued with.

### Escape hatch

A record's endpoint that returns other records' data (customer 42 with
"related customers") would now miss a change to them. Such a read says so:

```rust
backend.get(format!("/api/customers/{id}")).with_lists().json().await?
```

`with_lists()` makes the read depend on the lists too, so it re-runs on
every change to the resource, as today. Or the command declares what it
needs with `ctx.depends_on(..)`, as now.

### What it doesn't change

- **SQLite.** A model-layer read stays keyed by its table. See open
  question 1.
- **Pushes:** a re-run whose result is unchanged still isn't pushed; this
  only re-runs fewer.
- **Offline (RFC 0007):** reads answered from the copy record the same
  keys; the copy itself is kept per URL, as now.
- **Coalescing:** a burst of events is one re-run per query per batch
  window, as now.

### Testing

- `tests/live_reruns.rs`'s backend case: ten writes to customer 40 re-run
  the list ten times and no detail view; the ten requests are the list's.
  A write to customer 5 re-runs the list and view 5.
- An event with `model.id`, one without, one with `record_key(.., "uuid")`,
  and one with an id that isn't one (the whole resource).
- Nested reads; `with_lists()`; the outbox's replay; an app's
  `ctx.invalidate(resource_key(..))` re-running everything.
- The Laravel CI job: an edit on the web re-runs the desktop's view of that
  customer and not another's, through Reverb.

## Security

Nothing new is reached: a key only decides which re-runs happen. The risk
is staleness: a view not re-run when it should be. The rules err the safe
way: no id, an id that isn't one, an unknown event, an app's own
`invalidate`: the whole resource, as today.

## Alternatives considered

- **Re-run fewer by caching responses** (ETags, `If-None-Match`): saves
  the transfer, not the request or the server's work, and needs Laravel
  to answer `304`s.
- **Push the broadcast's model into the views directly,** with no request:
  fastest, but the event's attributes aren't the API Resource's output
  (hidden fields, appended ones, `can`), so the view would show the wrong
  shape.
- **Only narrow writes made by this app,** not broadcasts: simpler, but
  most changes come from other users, which is the case that costs.
- **Do SQLite at the same time:** see open question 1.

## Implementation plan

1. `resource_keys` gains the record and list keys; `Backend` records them
   on a read and invalidates them on a write; `with_lists()`.
2. `Backend::record_key(..)`, and events naming a record; the outbox's
   replay.
3. `tests/live_reruns.rs` flipped to the new numbers; unit tests for the
   keys; the Laravel CI case through Reverb.
4. Docs: live queries, backend; the inspector shows the record keys
   already.

## Open questions

1. **SQLite:** do the same for the model layer (`find(42)` keyed
   `customers:42`, `save()` invalidating it, bulk writes and raw SQL the
   whole table), later, or not at all? Recommended: **not now**. The
   measurement says a local re-run costs a fraction of a millisecond, and
   bulk writes make it the harder half. Revisit if an app's
   `LiveRegistry::stats()` says otherwise.
2. **The key of the lists:** `backend:list:/api/customers`, as above, or
   change `backend:/api/customers` to mean the lists and add a new
   "everything" key? Recommended: **a new list key.** An app's existing
   `ctx.invalidate(resource_key(..))` keeps re-running everything.
3. **`ctx.depends_on(resource_key(..))`** in an app's own command re-ran on
   every write; with this RFC, only on changes that don't name a record.
   Recommended: **say so under Upgrading, and give `backend::list_key(..)`**
   for a command that wants every change. (A read through `Backend` needs
   nothing.)
4. **`with_lists()`:** needed at launch, or wait for an app that asks?
   Recommended: **at launch.** It's small, and it's the answer to "my view
   went stale".
5. **The event's record:** only `model.<key>` (Laravel's model
   broadcasting), or also a top-level `id`, or a path set per follow?
   Recommended: **`model.<key>` only,** plus `record_key(..)`. Anything else
   invalidates the resource, which is safe.

## Decisions

Settled on 2026-11-02, all as recommended:

1. **SQLite isn't in this RFC.** A model-layer read stays keyed by its
   table; revisit if an app's `LiveRegistry::stats()` asks for it.
2. **The lists get a key of their own,** `backend:list:<resource>`; the
   resource key keeps meaning "anything here", so an app's
   `ctx.invalidate(resource_key(..))` re-runs every view of it.
3. **`ctx.depends_on(resource_key(..))`** in an app's command re-runs only
   on changes that don't name a record: said under Upgrading, with
   `backend::list_key(..)` for every change.
4. **`with_lists()`** comes with it.
5. **An event names its record by `model.<key>` only,** `id` unless
   `Backend::record_key(..)` says otherwise; anything else invalidates the
   resource.
