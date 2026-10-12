# RFC 0007 — Working offline: the last answer to read, an outbox to write, and syncing back

**Status:** implemented (2026-10-12) · **Target:** 0.14.0 · the guide: [docs/backend.md](../backend.md#working-offline) · builds on [RFC 0005](0005-laravel-backend.md) and [RFC 0006](0006-laravel-broadcasting.md) · see [Decisions](#decisions)

## Summary

Since 0.12, an Elyra app works against its Laravel backend. Since 0.13, a
change made on the web shows in its open windows. But with no network, on a
train, at a customer's site, or while the server is being deployed, every
screen is an `offline` error. A desktop app is expected to do better than a
browser tab there. This RFC lets it keep working:

```rust
App::new()
    .database("sqlite://app.db?mode=rwc")        // where the offline copy lives
    .backend(Backend::new("https://crm.example.com").offline())
```

- **Reads** answer from the last copy when the server can't be reached. The
  window shows the customers it showed an hour ago, marked as such, instead
  of an error.
- **Writes** marked for it go into a durable **outbox** when the server can't
  be reached. They're sent in order once it's back, each with an
  `Idempotency-Key`, so a write that did reach the server isn't made twice.
- **Back online,** the outbox empties and every open backend query re-runs.
  Broadcasting reconnects as before, and the windows catch up.
- **What went wrong** stays visible: a write the server refused (a `422`, a
  conflict) waits in the outbox for the user, with the server's message, to
  fix, retry or discard.

This isn't replication. The server stays the source of truth, and the app
keeps a copy of what it last read and a list of what it still has to say.

## Motivation

- **The point of a desktop app.** A field sales app, a warehouse, a clinic:
  the network isn't always there, and the work doesn't stop for it.
- **Today's failure is honest but useless.** RFC 0005 made "no answer" its own
  kind, `offline`, so the UI can tell it apart. This RFC gives the UI
  something to show and lets the user keep going.
- **The pieces are in place.** The app's database (the durable queue already
  keeps its jobs there), the backend's resource keys (a re-run of everything
  under `/api`), the auth state, and broadcasting's reconnect. What's missing
  is the copy, the outbox and the state between them.

## Current state (0.13)

- `Backend` requests fail with `BackendError::Unreachable`, kind `offline`,
  when there's no answer.
- Live queries re-run on the app's own writes and on broadcast events. A
  broadcasting reconnect re-runs everything followed.
- The durable queue (`QueueProvider::durable()`) shows the storage pattern:
  tables in the app's database, created on first use, surviving a restart.
- Nothing remembers an answer, and nothing holds a write for later.

## Design

### Turning it on

`Backend::offline()` needs a database: the app's own, through `.database(..)`,
as the durable queue uses. Without one, the app doesn't start, with an error
that says so. Three tables are created on first use, with no migration to run:

| Table | Holds |
|---|---|
| `elyra_backend_cache` | the last answer to each `GET`, per user |
| `elyra_outbox` | writes waiting to be sent, in order, per user |
| `elyra_outbox_failed` | writes the server refused, until retried or discarded |

### Online, offline, and back

The backend has a **connection state**: `online`, or `offline` with the time it
went offline. It goes offline when a request gets no answer
(`Unreachable`). While offline it probes Laravel's health route, `/up` by
default (`.health_route(..)`), backing off from 1 s to 30 s. It's also back
when any request succeeds or broadcasting connects.

Coming back:

1. The outbox is sent, in order (below).
2. Every live query that read from the backend re-runs: `backend:/api` is an
   ancestor of every resource key, so invalidating it reaches them all.
3. Broadcasting, if any, has reconnected on its own and re-run what it
   follows.

The state is on `elyra:connection`, `{ state, since, pending, failed }`, and in
`@elyra/runtime`'s `connection` store. A window can show "Offline since 10:42
— 3 changes waiting".

### Reads: the last answer

With `.offline()`, every successful `GET` through `Backend` stores its answer,
keyed by the user, the path and the query. A `GET` that gets no answer returns
the stored one instead of `Unreachable`. If nothing is stored, it's still
`Unreachable`, kind `offline`, as today.

- **No code changes.** A command that reads `/api/customers?page=2` reads the
  copy of `/api/customers?page=2` when offline. That's the page the user saw,
  not a query run locally.
- **Marked as old.** `Response::stale_since()` says when the copy was made,
  for a command that wants to pass it on. The connection state tells every
  window anyway.
- **Not everything.** `.no_offline()` on a request keeps it out of the copy:
  a one-time code, a report too big to keep.
- **Size:** answers are capped (2 MiB each by default), and the oldest go first
  past a total (100 MiB by default, `.offline_cache_limit(..)`).
- **Live queries offline** show the copy. They re-run when the connection
  comes back, and push only if the answer changed.

### Writes: the outbox

A write marked `.queue_offline()` that gets no answer goes into the outbox
instead of failing:

```rust
#[command]
async fn customers_store(ctx: Ctx, input: CustomerInput) -> Result<Customer> {
    let customer: Customer = ctx
        .get::<Backend>()
        .post("/api/customers")
        .body(&input)
        .queue_offline()
        .resource()
        .await?;                     // offline: Err(BackendError::Queued(id)), kind "queued"
    ctx.dispatch(CustomerCreated { customer: customer.clone() });
    Ok(customer)
}
```

- **"Queued" is an error kind, not a value.** The command's types stay what
  they are, and the code after the write runs only when the write really
  happened: no `CustomerCreated` for a customer the server hasn't seen. The
  frontend gets kind `queued` and says "Saved — it will be sent when you're
  back online".
- **When it's sent,** the outbox dispatches `OutboxSent { id, method, path,
  status, body }`, a domain event. Its resource is invalidated as for any
  write, so the lists re-run. A generated resource listens for its own
  `OutboxSent` and dispatches `CustomerCreated` then (`make:resource`, below).
- **In order, one at a time,** per user. A later write never overtakes an
  earlier one, because it may depend on it.
- **Exactly once, as far as HTTP allows.** Every queued write carries an
  `Idempotency-Key` that it keeps across retries. A write that timed out after
  the server got it is sent again with the same key, and the server answers
  from its record instead of creating a second customer. That needs the
  middleware below on the Laravel side. Without it, a retry after a timeout
  can duplicate a create.
- **What the server says, when it's sent:**
  - `2xx`: done. `OutboxSent`, and it's removed.
  - `422`: refused. It moves to `elyra_outbox_failed` with the server's
    errors, and the outbox goes on with the next. The user sees it, can open
    it in its form with the messages per field, fix it and send it again, or
    discard it. The write's own resource is re-run, so the user sees the
    server's version.
  - `409` / `412`: a conflict (below). Failed, as for a `422`.
  - `401`: the sign-in expired. The outbox pauses until the same user
    signs in again.
  - `403`, `404`: refused, failed as for a `422`. A `404` on a `DELETE` counts
    as done, since the record is gone either way.
  - `429`, `5xx`, no answer: kept, retried with backoff (1 s to 5 min, using
    `Retry-After` if there is one), and the outbox waits behind it.
- **Records made offline** aren't in the lists until they're sent, because the
  lists are the server's answers. The `outbox` store lists what's waiting,
  so a window can show it next to the list (see question 3).
- **Only what's marked.** A write without `.queue_offline()` fails with
  `offline`, as today. Not every write makes sense later: a payment, a
  sign-in, anything whose answer the user is waiting for.

On the Laravel side, the idempotency middleware is a few lines that rata
prints, and `scripts/laravel-backend.sh` uses:

```php
// app/Http/Middleware/Idempotent.php — remembers an answer for 24 h per key and user
public function handle(Request $request, Closure $next)
{
    $key = $request->header('Idempotency-Key');
    if (! $key || $request->isMethodSafe()) {
        return $next($request);
    }
    $cacheKey = 'idempotency:'.$request->user()?->id.':'.$key;
    if ($seen = Cache::get($cacheKey)) {
        return response($seen['body'], $seen['status'], ['Content-Type' => 'application/json']);
    }
    $response = $next($request);
    if ($response->getStatusCode() < 500) {
        Cache::put($cacheKey, ['status' => $response->getStatusCode(), 'body' => $response->getContent()], now()->addDay());
    }
    return $response;
}
```

### Conflicts

Ada edits Acme offline. Meanwhile Bob changes Acme's phone number on the web.
When Ada's update is sent, by default **the last write wins**: Ada's form
overwrites the record, as two browser tabs would. That's Laravel's default,
and right for most fields.

Where it isn't, a write can carry a **precondition**:

```rust
backend.put(format!("/api/customers/{id}"))
    .body(&input)
    .unless_changed_since(&customer.updated_at)    // If-Unmodified-Since
    .queue_offline()
```

The server checks it and answers `412` if the record changed. A few lines in
the controller (`abort_if($customer->updated_at->gt($since), 412)`), or a
trait rata prints. The write fails with kind `conflict`. Offline, it lands in
`elyra_outbox_failed`, and the user chooses: **keep mine** (sent again without
the precondition) or **take theirs** (discarded, and the record re-read).

There's no field-by-field merge. That needs the server to say what changed
and from what, and Laravel has no standard way to say it.

### Signing out with changes waiting

The outbox is the user's, and their token goes with sign-out. So
`Auth::sign_out` refuses while the outbox isn't empty, with kind
`pending_changes` and the count. The UI asks: wait, or sign out and discard
them (`sign_out_discarding()`). Signing out clears the user's copy of the
answers too: the next person at this machine sees nothing of theirs.

A sign-in that expires while offline isn't a sign-out. The outbox and the copy
stay, and are used again once the same user signs in.

### The frontend

```js
import { connection, outbox } from "@elyra/runtime";

// $connection: { state: "online" | "offline", since, pending, failed }
// $outbox: [{ id, method, path, status: "pending" | "failed", error?, errors? }]
await outbox.retry(id);       // a failed one, maybe after editing its body
await outbox.discard(id);
```

The outbox's operations go through `/__auth/outbox` (`Capability::Auth`): the
page sees what's waiting, never the token.

### `make:resource --backend`

The generated resource works offline once the app has `.offline()`:

- Its reads need nothing.
- `store`, `update` and `destroy` are marked `.queue_offline()`.
- Its `mod.rs` listens for `OutboxSent` on its path and dispatches its
  `Created` / `Updated` / `Deleted` events then.
- The views show the connection banner and "Saved — it will be sent when
  you're back online" for kind `queued`. A failed write opens in its form
  with the server's messages.
- A generated test goes offline with `HttpFake::unreachable`, saves, comes
  back and checks that the write was sent once, with its key.

rata prints the Laravel side: the `Idempotent` middleware and where it goes.

### Testing

```rust
let fake = HttpFake::new().get(url("/api/customers"), 200, page(1));
let app = TestApp::new(app().backend(Backend::new(BASE).offline()).swap(Http::fake(fake.clone())));
app.invoke_ok::<Page<Customer>>("customers_index", (q.clone(),)).await;    // stored

// The network goes.
fake.clone()
    .unreachable(Method::Get, url("/api/customers"))
    .unreachable(Method::Post, url("/api/customers"));
let copy: Page<Customer> = app.invoke_ok("customers_index", (q,)).await;  // the copy
assert_eq!(app.invoke_err_kind("customers_store", (input,)).await, "queued");   // new: the kind

// And comes back.
fake.clone().post(url("/api/customers"), 201, record(2));
app.get::<Outbox>().flush().await;                                        // new: send now
let sent = fake.sent_to("POST", &url("/api/customers"));
assert!(sent.last().unwrap().header("idempotency-key").is_some());
```

`TestApp::invoke_err_kind` is new: today a test sees an error's message, not
its kind.

CI runs it against the real Laravel app: stop `artisan serve` mid-test,
write, start it again, and check that the customer exists once.

## Security

- **The copy is on disk.** Answers are stored in the app's database, the
  same place as the app's own data, and per user. They're cleared on sign-out,
  and `.no_offline()` keeps a request out. An app that needs them encrypted
  at rest gives `.database(..)` an encrypted SQLite (SQLCipher); that's the
  database's concern, not this RFC's.
- **The token never goes in the outbox.** A queued write stores the method,
  path, body, its own headers and its key. The token is the one in the
  keychain at the time of sending, and it must be the same user's.
- **The page doesn't send.** The frontend lists, retries and discards
  through `/__auth/outbox`. It can't add a write to the outbox or change one's
  path or method. A retry may change the body only, which is the same as
  saving the form again.
- **Replays are safe to repeat.** The `Idempotency-Key` makes a resend answer
  from the server's record, and the key is random (UUID v4), per write.

## Alternatives considered

- **A full local replica** (the server's tables mirrored in SQLite and synced
  both ways, as PowerSync or ElectricSQL do). It's the strongest offline
  story, but it needs a sync protocol on the server, which Laravel doesn't
  have. Every query would also have to run twice, once in SQL locally and
  once in the API. Askr could offer such a protocol one day. This RFC doesn't
  rule it out.
- **Optimistic writes everywhere** (every write applied to the copy at once,
  rolled back if the server refuses). That's the nicest feel, but it means
  guessing what the server would answer: ids, computed fields, validation.
  The lists would be partly invented. The outbox shows what's waiting
  instead, honestly.
- **A typed result instead of the `queued` kind** (`Saved<T> = Now(T) |
  Later(id)`). That's more precise, but every generated command's signature
  and the TypeScript types would change. The code after a write would then
  run for a write that hasn't happened.
- **The service worker / browser cache.** The data lives in Rust, behind the
  backend, not in the webview. A cache there wouldn't serve commands, live
  queries or MCP.

## Implementation plan

1. **The connection state and the copy.**
   - `Backend::offline()`, `.health_route(..)` and the probe.
   - `elyra_backend_cache` with its limits, and `.no_offline()`.
   - `Response::stale_since()`.
   - Re-running everything when the connection is back.
   - `elyra:connection`.
2. **The outbox.**
   - `.queue_offline()`, `BackendError::Queued`, `elyra_outbox` and
     `elyra_outbox_failed`.
   - In-order sending with idempotency keys and the answers above.
   - `OutboxSent` / `OutboxFailed`, the `Outbox` API (pending, failed,
     retry, discard, flush).
   - The sign-out guard.
3. **Conflicts.** `.unless_changed_since(..)`, `412` → `conflict`, keep mine
   and take theirs.
4. **The frontend.** The `connection` and `outbox` stores, `/__auth/outbox`.
5. **`make:resource --backend`, its views' banner and failed-write form, the
   docs, and CI** against the real Laravel app with the idempotency
   middleware, taken offline mid-test.

As built, two details differ from the text above. A refused write that is
sent again gets a new `Idempotency-Key`, since it never happened. The
middleware remembers only writes that succeeded, and sits after
`auth:sanctum`.

## Open questions

1. **Reads:** keep a copy of every `GET` once `.offline()` is on, with
   `.no_offline()` to leave one out (recommended: no code changes), or only
   the requests marked `.offline_copy()`?
2. **Writes:** queue only writes marked `.queue_offline()` (recommended: a
   payment shouldn't go later by accident; generated resources mark theirs),
   or every write by default?
3. **Records made offline:** keep them out of the lists until they're sent,
   with the outbox shown beside them (recommended for now: honest, and no
   invented ids), or show them in the lists with a temporary id and a
   "pending" mark?
4. **Conflicts:** last write wins by default, with an opt-in precondition and a
   `412` (recommended: Laravel's own default), or a precondition on every
   update?
5. **Idempotency:** an `Idempotency-Key` on every queued write, with the
   Laravel middleware that rata prints (recommended), or none, accepting a
   possible duplicate after a timeout?
6. **Sign-out with changes waiting:** refuse with `pending_changes` until the
   user discards them (recommended), or keep them for that user's next
   sign-in?

## Decisions

Settled on 2026-10-12, all as recommended:

1. **A copy of every `GET`** once `.offline()` is on; `.no_offline()` leaves
   one out.
2. **Only writes marked `.queue_offline()`** are queued; generated resources
   mark their `store`, `update` and `destroy`.
3. **Records made offline stay out of the lists** until they're sent; the
   outbox is shown beside them.
4. **Last write wins** by default, with an opt-in precondition
   (`.unless_changed_since(..)`) and a `412`.
5. **An `Idempotency-Key` on every queued write**, with the Laravel middleware
   rata prints.
6. **Sign-out refuses with `pending_changes`** while the outbox isn't empty,
   until the user discards them (`sign_out_discarding()`).
