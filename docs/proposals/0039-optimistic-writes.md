# RFC 0039 — Shown at once: a write in every window before the server answers

**Status:** accepted (2026-11-20) · **Target:** 0.50.0 · builds on [RFC 0002](0002-live-queries.md), [RFC 0007](0007-offline-and-sync.md) and [RFC 0017](0017-record-invalidation.md) · see [Decisions](#decisions)

## Summary

Over a Laravel backend, a change takes two round trips to show: the write,
then the lists re-read from the server once it's done. Offline it doesn't
show at all: the write waits in the outbox, and the lists keep showing what
the server last said until it's sent, maybe hours later.

This RFC shows the change **at once, in every window**, while the write is
on its way or waiting, and takes it back if the server says no:

```rust
#[command(can = "customers.update")]
async fn customers_update(ctx: Ctx, id: i64, input: CustomerInput) -> elyra::Result<Customer> {
    Ok(ctx.get::<Backend>()
        .put(format!("/api/customers/{id}"))
        .body(&input)
        .optimistic()                  // shown in every list and view now
        .queue_offline()
        .resource()
        .await?)
}
```

```svelte
<Table page={customers} pending={$pending.customers} />   <!-- a dot on a row not yet sent -->
```

- **Every live list and view that read the record** shows the new values
  the moment the command starts, from the offline copy, without asking the
  server.
- **While it's on its way or waiting in the outbox**, every read of that
  record or its lists, from the server or from the copy, is shown with the
  change applied, so nothing flickers back.
- **When the server says yes**, the lists re-read as they do today; when it
  says no (`422`, `403`, `409`, `412`), the change is taken back in every
  window, and the error is where it is today: the command's, or the
  outbox's.
- **`$pending`** says which records have a change not yet sent, for a dot
  on the row.

## Motivation

- **A desktop app is expected to feel local.** On the web a spinner after
  Save is normal; in a desktop app the row is expected to change when the
  dialog closes. Today it changes after 100–300 ms online, and offline
  only once the network is back.
- **Offline is where it hurts.** RFC 0007 lets a write wait, and says
  "Saved, it will be sent when you're back online", but every list in the
  app still shows the old name. The user can't tell their change was kept,
  and the next edit starts from the old values.
- **The parts are there.** The offline copy (RFC 0007) keeps every read's
  answer; RFC 0017 knows which record and lists a write touches; the
  outbox knows which writes haven't been sent. What's missing is applying
  a waiting write to what's read.

## Current state (0.49)

- **A write** to `/api/customers/42` re-runs, once it succeeds, the live
  queries that read record 42 and customers' lists (RFC 0017's
  `write_keys`). Each re-run is a request to the server.
- **The offline copy** (`elyra_backend_cache`) keeps the last answer to each
  `GET`, per user and URL. Offline, reads answer from it at once; online,
  they go to the server.
- **The outbox** keeps writes made with `.queue_offline()` that got no
  answer, with their method, path and JSON body, until they're sent or
  refused. Refused ones move to `failed`, with the server's message.
- **Nothing applies a write to a read.** A list shows what the server or
  the copy said, whatever is waiting.

## Design

### Marking a write

`.optimistic()` on a `PUT`, `PATCH` or `DELETE` to a record
(`/api/customers/42`, or `/api/customers/42/notes/7`). Its JSON body says
what changes; for a body that isn't the record's fields,
`.optimistic_with(json!({ "name": "Acme AS" }))` says it instead.

A `POST` that creates a record isn't optimistic in this RFC (see the open
questions): where a new record goes in a sorted, filtered, paged list is
the server's to say.

### What a read shows

While an optimistic write is **pending**, from when the command sends it
until the server answers, or for as long as it waits in the outbox, every
`GET` of the backend has it applied to its JSON before the command that
read it sees it:

- **The record** (`GET /api/customers/42`): its fields, in `data` when it's
  wrapped as an API Resource is, are replaced by the write's, **only those
  the record already has**. A field the server doesn't send back
  (`password`, `password_confirmation`) isn't added, and one it computes
  (`updated_at`, `full_name`) stays as it was.
- **A list** (`GET /api/customers?…`): the item whose record key (`id`, or
  RFC 0017's `.record_key(..)`) is 42, in `data` (a page) or the array
  itself, is changed the same way. A `DELETE` takes it out (and `total`
  goes down by one, when there is one).
- **Nested resources** (`/api/customers/42/notes/7`) the same, under their
  own lists.
- Several pending writes to one record apply in the order they were made.

It's applied to the server's answer and to the copy's alike, so a list
re-read while the write is in flight doesn't flicker back to the old name.
Reads the app makes outside live queries see it too: the copy is what the
app believes.

### At once

When an optimistic write starts, the live queries that read its record and
lists (its `write_keys`) re-run **from the copy, with the write applied,
without asking the server**. That's what makes it "at once": a re-run is a
read of the local database, a fraction of a millisecond. A query whose
answer isn't in the copy (never read, or bigger than 2 MiB) re-runs from
the server as today, with the write applied.

So `.optimistic()` needs `.offline()`: the copy is where "at once" comes
from. Without it, `.optimistic()` still keeps a re-read from flickering
back, but the change shows only after a round trip.

### When the server answers

- **Yes**: the write is no longer pending; the lists re-run from the
  server, as they do today, and show what the server made of it (its
  `updated_at`, a computed field).
- **No** (`422`, `403`, `404`, `409`, `412`, or another refusal): no longer
  pending, and the lists re-run, so every window shows the record as the
  server has it. Online, the command gets its error as today; from the
  outbox, the write is in `failed` as today, and `OutboxFailed` is
  dispatched.
- **No answer**: with `.queue_offline()`, it waits in the outbox and stays
  pending, across restarts (the outbox row says it's optimistic). Without,
  it's an `offline` error and no longer pending.

### In the frontend

```ts
import { pending } from "@elyra/runtime";

$pending;                 // { customers: [42], "customers/42/notes": [7] }
$pending.customers;       // the customers with a change not yet sent
```

`@elyra/ui`'s `<Table pending={ids}>` draws a small dot at the start of
those rows, with "Not sent yet" for a screen reader and as a tooltip. Any
other component can use `$pending` the same way.

### Security

Nothing new crosses the IPC boundary: the page reads `$pending` (resource
names and record keys it already has) and nothing else. A pending write is
the app's own, made by a command; it changes what the app shows, never
what's sent. Signing out ends every pending write with the outbox it's in
(RFC 0007), and the copy with it.

### Testing

- **Applying**: unit tests over JSON shapes: a record, wrapped and not; a
  page; a plain array; a nested resource; a field the record doesn't have;
  a `DELETE` with a `total`; two writes to one record; a custom record key.
- **At once**: a live query over a fake server whose write answers after
  500 ms sees the new name before the answer, without a `GET` to the server.
- **Taken back**: the same with a `422`, a `412` and a `403`.
- **Offline**: a queued write is shown, survives a restart, and goes when
  it's sent or refused.
- **Against Laravel** in CI's backend job: update a customer with the plug
  pulled, see it in a live list, push the plug in, see the server's
  `updated_at`.

## Implementation plan

1. Applying a write to JSON (pure functions), and the pending set (the
   in-flight writes and the outbox's optimistic rows).
2. `.optimistic()`, `.optimistic_with(..)`; applied in `BackendRequest::send`
   to every `GET`'s answer; the outbox row's flag.
3. Re-running from the copy at once; the yes, no and no-answer paths.
4. `$pending` (`elyra:pending`) and `<Table pending>`.
5. Docs: `docs/backend.md` (a section), `docs/live-queries.md`,
   `docs/ui/table.md`; the generated commands of `make:resource --backend`
   (see the open questions).

## Not in this RFC

- **Creating optimistically.** See the open questions.
- **Optimism for the local database.** A local write is already a fraction
  of a millisecond, and the live queries follow it.
- **Merging** a waiting write with someone else's change: that's RFC 0007's
  conflicts, unchanged.
- **Undo.** A pending write can be discarded through the outbox, as today.

## Alternatives considered

- **The command patches the lists itself** (a closure over the live
  query's value). Each command would need to know every list's shape, and
  the copy and the outbox wouldn't know.
- **Writing the change into the copy** instead of applying it when read.
  The copy would then hold something the server never said, and a refused
  write would need undoing in every answer it touched. Applying on read
  keeps the copy the server's, and taking it back is just no longer
  applying it.
- **In the frontend**, as TanStack Query does. Each window would hold its
  own guess, the offline copy and the outbox are in the shell, and a
  restart would lose it.

## Decisions

Settled on 2026-11-20, all as recommended:

1. **Creates are left out**: a `POST` isn't optimistic in this RFC.
2. **Opt-in** with `.optimistic()`; `make:resource --backend` generates it
   on its update and delete commands.
3. **Fields the record doesn't have are never added**; a body that isn't
   the record's shape uses `.optimistic_with(..)`.
4. **Without `.offline()` it's allowed**: no flicker, but no "at once", and
   a warning in the log once.
5. **`$pending` is per resource path**, with the record keys under each.
6. **`<Table pending>` draws the dot** at the start of those rows whenever
   `pending` is given, labelled "Not sent yet" (in `labels`).
