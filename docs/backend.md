# A Laravel backend

An Elyra app can be the desktop half of a product whose web half is Laravel,
served by [Askr](https://github.com/kwhorne/askr) or any other server. The app
signs the user in with a [Sanctum](https://laravel.com/docs/sanctum) token,
calls the same API as the web, and shows Laravel's validation messages in its
forms as if they were its own. Changes made on the web show in its windows,
it keeps working offline, and Laravel's notifications reach the desktop. The design is
[RFC 0005](proposals/0005-laravel-backend.md).

## In five lines

```toml
elyra = { …, features = ["backend"] }   # with "database" for make:resource
```

```rust
App::new().backend(Backend::new("https://crm.example.com"))

#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
}
```

```svelte
<script>
  import { auth } from "@elyra/runtime";
</script>
{#if $auth.signedIn}Hi {$auth.user.name}{:else}<SignIn />{/if}
```

Or generate the whole resource, with its screens:
`rata make:resource Customer --backend /api/customers --generate name:string email:email`.

## The Laravel side

Sanctum and three routes, in `routes/api.php` (Laravel prefixes them with
`/api`, as `Backend` expects):

```bash
php artisan install:api      # Sanctum; then add `HasApiTokens` to App\Models\User
```

```php
Route::post('/sanctum/token', function (Request $request) {
    $request->validate(['email' => 'required|email', 'password' => 'required', 'device_name' => 'required']);
    $user = User::where('email', $request->email)->first();
    if (! $user || ! Hash::check($request->password, $user->password)) {
        throw ValidationException::withMessages(['email' => ['The provided credentials are incorrect.']]);
    }
    return $user->createToken($request->device_name)->plainTextToken;
});

Route::middleware('auth:sanctum')->group(function () {
    Route::delete('/sanctum/token', fn (Request $r) => $r->user()->currentAccessToken()->delete());
    Route::get('/user', fn (Request $r) => $r->user());
    Route::apiResource('customers', CustomerController::class);
});
```

Different paths? `Backend::new(url).token_route(..).revoke_route(..).user_route(..)`.
[`scripts/laravel-backend.sh`](../scripts/laravel-backend.sh) builds a complete
app like this. CI runs Elyra against it.

The Composer package [`elyracode/elyra-backend`](../laravel/README.md), in
this repository, has these routes as `Route::elyraAuth()`, the
[idempotency middleware](#idempotency-on-the-laravel-side), and the
[notifications](#laravels-notifications) controller as
`Route::elyraNotifications()`. It also gives Laravel Boost guidelines and a
skill, so agents working on the Laravel app keep what the desktop relies on
([coding agents](ai-agents.md#the-laravel-half)):

```php
Route::elyraAuth();

Route::middleware(['auth:sanctum', Elyra\Backend\Http\Middleware\Idempotent::class])->group(function () {
    Route::elyraNotifications();
    Route::apiResource('customers', CustomerController::class);
});
```

## Calling the API

```rust
let backend = ctx.get::<Backend>();
let page: Page<Customer> = backend.get("/api/customers").query(&query).json().await?;
let customer: Customer   = backend.get(format!("/api/customers/{id}")).resource().await?;
let created: Customer    = backend.post("/api/customers").body(&input).resource().await?;
backend.delete(format!("/api/customers/{id}")).send().await?;
```

- **The token:** it's sent as `Bearer` when the user is signed in. Requests
  ask for JSON, which is what makes Laravel answer a failed validation with
  `422` JSON instead of a redirect.
- **`.json()` and `.resource()`:** `.json()` decodes the body as the type. A
  `204` decodes as `()`. `.resource()` also unwraps an API Resource's
  `{ "data": … }`, so a controller can return a model or a resource.
- **Pages:** `Page<T>` reads both of Laravel's paginators, `paginate()`'s flat
  one and an API Resource collection's `{ data, links, meta }`.
- **The rest of the request:** `.query`, `.header`, `.timeout` and `.retry`
  work as on [`Http`](http.md), which `Backend` is built on.
- **Updates send the whole record.** An `Option` that's `None` goes as
  `null`, which is what a form means. For a partial `PATCH`, mark the
  fields `#[serde(skip_serializing_if = "Option::is_none")]`.

## Laravel's answers

| Laravel | `BackendError` | The frontend sees |
| --- | --- | --- |
| `422 { errors }` | `Validation` | a `ValidationError`, per field, as from a local command |
| `401`, `419` | `Unauthenticated` | kind `unauthenticated`; the token is forgotten and `elyra:auth` says `expired` |
| `403 { message }` | `Forbidden` | kind `denied`, with Laravel's message |
| `404` | `NotFound` | kind `not-found` |
| `409`, `412` | `Conflict` | kind `conflict`, with Laravel's message (see [Conflicts](#conflicts)) |
| `429` + `Retry-After` | `TooManyRequests` | kind `too-many-requests`, saying when |
| `5xx` | `Server` | kind `server`, without the error page |
| no answer | `Unreachable` | kind `offline` |

`?` in a command turns each into the right error. The kind reaches the
frontend as `CommandError.kind`, from a command and from a live query's
re-run, so the UI can say "you're offline" rather than show an error.

[Working offline](#working-offline) adds two that Laravel never sends. A
write that waits in the outbox is `Queued`, kind `queued`, and signing out
with writes waiting is `PendingChanges`, kind `pending_changes`.

## Signing in

```rust
let auth = ctx.get::<Auth>();
auth.sign_in("ada@example.com", "secret").await?;   // a 422 for bad credentials
let user: User = auth.user().await?;                // GET /api/user, cached
auth.sign_out().await?;                             // revoke, forget
```

- **The token lives in the OS keychain only**, never in the webview or a
  file. It's restored when the app starts, so a signed-in user stays signed
  in. With no keychain (Linux without Secret Service), sign-in fails, and
  the token the server just issued is revoked.
- **The device is named `<host> · <app>`**, so the user recognizes it among
  their tokens on the web.
- **A `401` from any request signs the user out**, and `elyra:auth` carries
  `{ signedIn: false, reason: "expired" }`.

From the frontend, through `/__auth/*` (`Capability::Auth`, granted by
default):

```ts
import { auth, onSignedOut, ValidationError } from "@elyra/runtime";

try {
  await auth.signIn(email, password);
} catch (e) {
  if (e instanceof ValidationError) errors = e.errors;   // { email: ["…"] }
}
$auth;                                                    // { signedIn, user, reason }
onSignedOut((reason) => reason === "expired" && toast("Please sign in again"));
```

## Live queries

A live command that reads from the backend updates itself when this app
writes. Inside a live command, a `GET` depends on its **resource**: the path
without the query and without trailing ids (a number, UUID or ULID). So
`/api/customers?page=2` and `/api/customers/12` are both
`backend:/api/customers`. A read also depends on each path it's under, so
`/api/customers/12/notes` follows `/api/customers` too. A successful `POST`,
`PUT`, `PATCH` or `DELETE` to that resource re-runs them. `ctx.depends_on` and `ctx.invalidate` cover paths
that aren't shaped like an `apiResource`.

As with the database, a live command's re-run may not write.

Changes made elsewhere, on the web or in a colleague's app, come in through
broadcasting.

## Broadcasting: changes made on the web

Laravel announces its changes with
[broadcasting](https://laravel.com/docs/broadcasting), and the app listens
the way Laravel Echo does in a browser ([RFC 0006](proposals/0006-laravel-broadcasting.md)):

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))   // REVERB_APP_KEY
        .follow("customers", "/api/customers"),          // private-customers → re-run what read /api/customers
)
```

- **The connection:** the Pusher protocol, which [Askr](https://github.com/kwhorne/askr)
  (`--pusher`), Reverb and Pusher speak. It goes to the backend's own host,
  as Askr serves it; `.host()`, `.port()` and `.scheme("ws")` point it at a
  Reverb on its own port. It's `wss` only, except to loopback.
- **When:** once the user is signed in, and it closes on sign-out. A dropped
  connection reconnects, backing off from 1 s up to 30 s, and then re-runs
  everything followed, since events sent meanwhile are lost.
- **Private channels** are authorized by the backend, as the signed-in user,
  at `/api/broadcasting/auth`. The app secret stays on the server.
- **`->toOthers()`:** while connected, `Backend` sends `X-Socket-ID`, so
  Laravel can skip the app whose own write already re-ran its queries.

Each event on a followed channel does three things:

1. It **re-runs the live queries** that read the followed resource, coalesced,
   and pushed only if the result changed.
2. It's a **domain event**, `BackendEvent { channel, event, data }`:
   `App::listen(|e: BackendEvent, ctx| …)` for a notification or a log.
3. It **reaches the windows** on `elyra:backend`:

```js
import { onBackendEvent, broadcasting } from "@elyra/runtime";

onBackendEvent("customers", "CustomerUpdated", (data) => toast(`${data.model.name} was updated`));
// $broadcasting.state: "connecting" | "connected" | "offline"
```

The data is the server's, untrusted like any input. Re-running a query
doesn't read it, only the fact that something changed. `.follow_public(..)`
follows a public `Channel`. Presence channels aren't supported yet.

On the Laravel side, the model announces itself and a channel rule says who
may listen:

```bash
php artisan install:broadcasting --reverb   # or Askr: `askr serve --pusher`
```

```php
// app/Models/Customer.php
class Customer extends Model
{
    use BroadcastsEvents;

    public function broadcastOn(string $event): array
    {
        return [new PrivateChannel('customers')];
    }
}

// routes/channels.php
Broadcast::channel('customers', fn ($user) => true);
```

```php
// bootstrap/app.php — channel auth under /api, with the Sanctum token
->withBroadcasting(__DIR__.'/../routes/channels.php', [
    'prefix' => 'api', 'middleware' => ['api', 'auth:sanctum'],
])
```

Events are queued by default. Run a queue worker, or set
`QUEUE_CONNECTION=sync`. For Askr, point Laravel's `reverb` connection at it
(`REVERB_HOST`, `REVERB_PORT`) with the same app key and secret.

## Working offline

With no network, on a train or while the server is deployed, an app can keep
showing what it last read ([RFC 0007](proposals/0007-offline-and-sync.md)):

```rust
App::new()
    .database("sqlite://app.db?mode=rwc")        // where the copy lives
    .backend(Backend::new("https://crm.example.com").offline())
```

- **Reads:** each successful `GET` keeps its answer, per user, URL and query.
  A `GET` that gets no answer returns that copy instead of an `offline`
  error; one never read before is still `offline`. No command changes.
  `Response::stale_since()` says when the copy was made. `.no_offline()` on a
  request keeps it out: a one-time code, a big report.
- **The connection state:** no answer means offline. While offline, reads
  answer from the copy at once, and Laravel's health route (`/up`,
  `.health_route(..)`) is probed every 1 s, backing off to 30 s. Any answer,
  from the probe, any request or broadcasting reconnecting, means online
  again, and every live query that read from the backend re-runs. The state
  is on `elyra:connection` (`{ state: "online" | "offline", since }`) and
  `Backend::connection()`.
- **Whose copy:** it's kept in `elyra_backend_cache` in the app's database,
  created on first use, with the user and the URL hashed. It's kept per user,
  only while signed in, and cleared when the user signs out or someone else
  signs in. An expired sign-in keeps it for the same user. It's capped at
  100 MiB (`.offline_cache_limit(..)`, oldest first), and an answer over
  2 MiB isn't kept.

### Writes: the outbox

A write marked `.queue_offline()` that gets no answer waits in the outbox
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
        .await?;                     // no answer: Err(BackendError::Queued(id)), kind "queued"
    ctx.dispatch(CustomerCreated { customer: customer.clone() }).await?;
    Ok(customer)
}
```

- **`queued` is an error kind, not a value.** The command's types stay what
  they are, and the code after the write runs only when the write really
  happened. The frontend gets kind `queued` and says "Saved, it will be sent
  when you're back online".
- **Sent in order, once.** When the server is back, writes go one at a time,
  oldest first, each with the `Idempotency-Key` it was first tried with. While
  earlier writes wait, a new one waits behind them, even online. Without
  `.queue_offline()` a write fails with `offline` as before: a payment
  shouldn't go later by accident.
- **The server's answer**, when it's sent:
  - A `2xx` dispatches `OutboxSent { id, method, path, status, body }`, and
    the lists re-run.
  - A `422`, `403`, `404` or another refusal moves the write aside, with the
    server's message and errors per field, and dispatches `OutboxFailed`. The
    next writes go on. A `404` on a `DELETE` counts as sent.
  - A `401` waits, untried, until the same user signs in again.
  - No answer, a `5xx` or a `429` is tried again with backoff (1 s up to
    5 minutes, or the server's `Retry-After`), and the rest wait behind it.
- **`Outbox`** (`ctx.get::<Outbox>()`): `pending()`, `failed()`, `items()`,
  `retry(id, Some(fixed_body))`, `discard(id)`, `flush()`. The counts are on
  `elyra:connection` as `pending` and `failed`.
- **Signing out** with writes waiting or refused is refused with kind
  `pending_changes`, since they'd be lost. `Auth::sign_out_discarding()`
  signs out anyway.
- It's kept in `elyra_outbox` and `elyra_outbox_failed`, per user, created on
  first use. It survives a restart, and the token is never stored there.

### In the frontend

```svelte
<script>
  import { connection, outbox, auth } from "@elyra/runtime";
</script>

{#if $connection.state === "offline"}
  <p>Offline since {new Date($connection.since).toLocaleTimeString()}
     — {$connection.pending} change(s) waiting</p>
{/if}

{#each $outbox.filter((w) => w.status === "failed") as w}
  <p>{w.method} {w.path}: {w.error}</p>
  {#if w.reason === "conflict"}
    <button on:click={() => outbox.keepMine(w.id)}>Keep mine</button>
    <button on:click={() => outbox.takeTheirs(w.id)}>Take theirs</button>
  {:else}
    <button on:click={() => outbox.retry(w.id, fixedBody)}>Send again</button>
    <button on:click={() => outbox.discard(w.id)}>Discard</button>
  {/if}
{/each}
```

- `connection` is `{ state, since, pending, failed }`. It's always `online`
  for an app without `.offline()`.
- `outbox` lists the writes, oldest first: `{ id, method, path, body, status,
  attempts, error, errors, reason, createdAt }`. `errors` are per field, as a
  `ValidationError` has them.
- A save that answers kind `queued` worked: "Saved, it will be sent when
  you're back online".
- `auth.signOut()` rejects with kind `pending_changes` while writes wait.
  Ask the user, then `auth.signOutDiscarding()`.

The page goes through `/__auth/outbox` (`Capability::Auth`). It can list,
retry, resolve and discard writes, and a retry may change the body only. It
can't add a write, change one's method or path, or see the token.

### Conflicts

By default the last write wins, as in Laravel: Ada's update, sent once she's
back online, overwrites what Bob changed meanwhile. Where that's wrong, a
write carries a precondition:

```rust
backend.put(format!("/api/customers/{id}"))
    .body(&input)
    .unless_changed_since(&customer.updated_at)   // If-Unmodified-Since
    .queue_offline()
```

The server checks it and answers `412` when the record changed, and `409`
means the same. Online, that's `BackendError::Conflict`, kind `conflict`.
From the outbox, the write is refused with `reason: "conflict"`, and the user
chooses between two resolutions:

- `outbox.keep_mine(id)` sends it again without the precondition.
- `outbox.take_theirs(id)` drops it and re-reads the record.

On the Laravel side it's one line in the controller:

```php
abort_if($request->hasHeader('If-Unmodified-Since')
    && $customer->updated_at->gt(Carbon::parse($request->header('If-Unmodified-Since'))), 412,
    'Someone changed this customer meanwhile.');
```

### Idempotency on the Laravel side

A middleware answers a repeated key from its record, so a write that reached
the server before the connection dropped isn't made twice. It goes after
`auth:sanctum`, so it knows whose key it is, and it remembers only writes that
happened. A refused write sent again (fixed, or "keep mine") carries a new key
anyway:

```php
// routes/api.php
Route::middleware(['auth:sanctum', Idempotent::class])->group(function () { /* … */ });

// app/Http/Middleware/Idempotent.php: a write that happened, answered again for 24 h per key and user
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
    if ($response->isSuccessful()) {
        Cache::put($cacheKey, ['status' => $response->getStatusCode(), 'body' => $response->getContent()], now()->addDay());
    }
    return $response;
}
```

## Laravel's notifications

What `$user->notify(..)` sends through the `database` and `broadcast`
channels reaches the desktop: a notification in the OS, the unread count on
the app's icon, and a list in the app, read once wherever the user reads it
([RFC 0008](proposals/0008-laravel-notifications.md)):

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher(key))
        .notifications(),   // App.Models.User.{id}, and /api/notifications
)
```

```php
class InvoicePaid extends Notification
{
    public function via(object $notifiable): array
    {
        return ['database', 'broadcast'];
    }

    public function toArray(object $notifiable): array
    {
        return ['title' => 'Invoice paid', 'body' => "Invoice 7 was paid.", 'url' => '/invoices/7'];
    }
}
```

- **Heard** on the signed-in user's private channel, Laravel's
  `App.Models.User.{id}` (`.notifications_channel(..)` for another),
  authorized by the backend like any private channel. Signing in as someone
  else follows theirs. Each one is a `NotificationReceived` (a domain event
  with the `Notification` and how it's `Presented`) and is sent to the
  windows on `elyra:notification`. It needs `.broadcasting(..)`, and the app
  won't start without it.
- **Shown:** `title`, `body` and `url` from what `toArray` returned, by
  convention, or `.present_notifications(|n| Presented { .. })`. With no
  window of the app in front, the OS shows it (the `system` feature; on macOS
  a bundled app). With one in front, it's for the app to toast
  (`e.shown === "toast"`). `.notification_previews(false)` shows only "New
  notification", for a locked screen.
- **Clicked:** the app comes forward, the notification is read, and its
  `url` opens when it's a path in the app (`/invoices/7`). A full URL is
  never opened by a click. The app's own listener can open one: it's a
  `NotificationClicked`, also on `elyra:notification-clicked`.
- **Counted:** the unread count, the server's, is on the dock (macOS), the
  launcher (Linux), or the taskbar button (Windows), and in the tray's
  tooltip. `.notification_badge(false)` turns that off, and signing out
  clears it.
- **Listed and read:** `Notifications::refresh()` reads `GET
  /api/notifications`, Laravel's paginator with `unread` beside it, and
  `more()` reads the next page. `mark_read(id)` and `mark_all_read()` count
  down at once and post to `/{id}/read` and `/read-all`, through the outbox
  when offline. What was read here stays read until the server shows it
  read too. The list is read again when a window gets focus, and when the
  web broadcasts `NotificationsRead`.
- **Missed** while offline or closed: when the connection is back, or at the
  start, what came meanwhile is shown, one as itself, more as "4 new
  notifications" that opens `/notifications`. A user's first sign-in on a
  machine shows nothing. The newest one listed is kept in the `Store`.

The Laravel side is the `notifications` table (`php artisan
make:notifications-table`), a `NotificationController` with `index`, `read`
and `readAll`, its routes under `auth:sanctum`, and a `NotificationsRead`
event broadcast `->toOthers()` when the web reads them. `rata
make:notifications` prints all of it; `scripts/laravel-backend.sh` has it
working.

In the frontend:

```svelte
<script>
  import { notifications, onNotification, toast } from "@elyra/runtime";
  onNotification((e) => e.shown === "toast" && toast(e.presented.title));
</script>

<a href="#/notifications">Notifications {$notifications.unread}</a>
{#each $notifications.items as n (n.id)}
  <button onclick={() => notifications.markRead(n.id)}>{n.data.title}</button>
{/each}
```

The `notifications` store (`{ unread, items, hasMore }`) is read from the
server when first subscribed, then kept up to date, with `refresh()`,
`more()`, `markRead(id)` and `markAllRead()`. While it's subscribed, a click
on a notification in the OS opens its path in the app. `onNotificationClicked`
hands a click to the app. It all goes through `/__auth/notifications*`
(`Capability::Auth`): the page may read the list and mark it read, nothing
else.

### `rata make:notifications`

It writes `app/src/resources/notifications/` and lists it in the resource
registry:

- the page at `#/notifications`, with Mark all read, More, and the offline
  banner;
- a nav item with the unread count. `rata new` renders a nav item's `count`
  since 0.15, and rata prints the lines for an older `App.svelte`;
- toasts while the app is in front, and the click that opens a page, set up
  once for the whole app.

It prints the Laravel side, and the `.notifications()` hint until `main.rs`
has it.

## What the user may do

Laravel decides what each user may do: a policy, a Gate, or
spatie/laravel-permission's roles. `.abilities()` brings that list to the
desktop ([RFC 0010](proposals/0010-laravel-abilities.md)):

```rust
App::new().backend(Backend::new(url).abilities())   // GET /api/abilities

let abilities = ctx.get::<Abilities>();
if abilities.allows("customers.export") { /* … */ }
```

- After sign-in the app reads `GET /api/abilities` as the user: a list of
  strings (`["customers.view", "invoices.*"]`, a trailing `*` covering a
  namespace), or the same inside `{ "data": … }`. Signing out forgets it.
- Before the list has been read, and signed out, the user has none.
- With `.offline()`, the list is part of the user's offline copy, so it holds
  while the server can't be reached.

The list stays current. It's read again when a window gets focus and when
the connection comes back, and at once when Laravel broadcasts
`AbilitiesChanged` on the user's private channel (`App.Models.User.{id}`,
followed with `.broadcasting(..)` whether or not notifications are on):

```php
// elyracode/elyra-backend does this for spatie's role and permission events
// (with `permission.events_enabled`); for anything else:
broadcast(new \Elyra\Backend\Events\AbilitiesChanged($user));
```

A live command that asks `abilities.allows(..)` re-runs when the list
changes.

In the frontend, `$can(..)` hides what the user can't do:

```svelte
<script>
  import { can, abilities } from "@elyra/runtime";
</script>

{#if $can("customers.delete")}
  <button onclick={remove}>Delete</button>
{/if}
```

`$can` is `false` until the list is read and while signed out; `$abilities`
is the list itself (`{ loaded, abilities }`), and `abilities.refresh()` reads
it again. The page can read the list and ask for it again
(`/__auth/abilities`, `Capability::Auth`), never set it. An app without
`.abilities()` answers `["*"]`: its own grants are all there is, and the
shell enforces them.

A command with `#[command(can = "customers.delete")]` then runs from the
frontend only when the app grants the ability (`allow_ability`, as always)
**and** the user has it. Otherwise it's refused at once with kind `denied`,
the kind Laravel's `403` has, without a request, and, offline, without being
queued to fail later. The app's own missing grant stays `forbidden`. Rust
callers (jobs, providers, `TestApp::invoke`) aren't gated.

```php
// routes/api.php (elyracode/elyra-backend), inside auth:sanctum
Route::elyraAbilities();   // GET /api/abilities
```

`Route::elyraAbilities()` answers the user's spatie/laravel-permission
permissions (their roles' included) when the package is installed, and
otherwise every ability in `config('elyra.abilities')` the Gate allows them
(`php artisan vendor:publish --tag=elyra-config`). Any route that answers a
list of strings works too.

Per record, Laravel's policies can say more than a flat list: an API
Resource may add `can`, which the generated views prefer:

```php
'can' => [
    'update' => $request->user()->can('update', $this->resource),
    'delete' => $request->user()->can('delete', $this->resource),
],
```

## Files

Attach a file the user chose, and Laravel gets it as an upload
([RFC 0009](proposals/0009-files.md)):

```rust
use elyra::files::ChosenFile;

#[command(can = "customers.update")]
async fn customers_attach(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Attachment> {
    Ok(ctx.get::<Backend>()
        .post(format!("/api/customers/{id}/attachments"))
        .attach("file", &file)          // streamed from disk
        .field("note", "Signed copy")
        .max_size(20 * 1024 * 1024)     // optional: refused here, before sending
        .json()
        .await?)
}
```

```ts
const [file] = await dialog.open({ filters: [{ name: "PDF", extensions: ["pdf"] }] });
if (file) await commands.customersAttach({ id: 7, file });
```

```php
public function store(Request $request, Customer $customer)
{
    $request->validate(['file' => ['required', 'file', 'mimes:pdf', 'max:20480']]);

    return $customer->attachments()->create([
        'path' => $request->file('file')->store('attachments'),
        'name' => $request->file('file')->getClientOriginalName(),
    ]);
}
```

- **A `ChosenFile`** is what the frontend hands over from a dialog or a drop
  ([a file the user chose](system.md#a-file-the-user-chose)); Rust code can
  attach any path it knows, or bytes with `.attach_bytes(..)`.
- **Laravel's `422`** on the file (`mimes`, `max`) is a validation error on
  its field, like any other. **A `413`**, from nginx's `client_max_body_size`
  or PHP's `post_max_size` before Laravel sees the request, is one too: "The
  file is too large for the server."
- **`PUT`, `PATCH` and `DELETE` with a file** are sent as a `POST` with
  `_method`, because PHP reads files only from a `POST`. Laravel routes it as
  the method named.
- **Retries**: `.idempotent()` adds an `Idempotency-Key` (the
  [`Idempotent` middleware](#idempotency-on-the-laravel-side) honours it), so
  `.retry(..)` may send it again; the file is read again from the start.
- **The timeout** is the time without progress, 30 s unless `.timeout(..)`
  says otherwise: a large file on a slow line isn't cut off while it moves.

Downloads go to disk, never all into memory:

```rust
// To where the user chose in a save dialog (a ChosenFile of kind `save`):
let saved = backend.get(format!("/api/invoices/{id}/pdf")).download_to(&to).await?;

// Into a folder the user chose (kind `folder`), under the server's name:
let saved = backend.get("/api/exports/42").download_into(&folder).await?;

// Into the Downloads folder:
let saved = backend.get("/api/exports/42").download().await?;

// A signed URL the API returned (S3's temporaryUrl), fetched without the token:
let saved = backend.download_url(url).download_into(&folder).await?;
```

```php
Route::get('/invoices/{invoice}/pdf', fn (Invoice $invoice) =>
    Storage::download($invoice->pdf_path, "Invoice {$invoice->number}.pdf"));
```

- The name comes from `Content-Disposition` (what `Storage::download`'s second
  argument sets), cleaned: it can't leave the folder, isn't hidden, and is
  numbered when taken (`Invoice 7 (1).pdf`).
- A download is written to a `.part` file and renamed when complete. It's
  marked as downloaded from the internet (quarantine on macOS, the Mark of the
  Web on Windows), so the OS checks it before it's opened. Nothing is opened
  by itself.
- `download_to` needs a `ChosenFile` from a save dialog, and `download_into`
  one of a folder: a file the user picked to read isn't overwritten.
- A download isn't kept as the offline copy.

### Uploads while offline

`.queue_offline()` works for an upload too. With no answer, it waits in the
[outbox](#writes-the-outbox) with a **copy** of its files, in the app's data
folder (`elyra-outbox-files`), so what's sent later is what the user
attached, even if they move, change or delete the original meanwhile:

```rust
backend.post(format!("/api/customers/{id}/attachments"))
    .attach("file", &file)
    .queue_offline()
    .json()
    .await?;   // offline: Err, kind `queued`, and it's sent once the server is back
```

- It's sent once, with its `Idempotency-Key`, in order with the other
  writes, and its copy is removed when it's sent or discarded. A refused
  one keeps its copy until it's retried or discarded; it's retried as it
  was (attach the file again to change it).
- `Backend::offline_upload_limit(bytes)` caps the copies a user's queued
  uploads may keep (500 MB by default). Over it, the upload isn't kept: it's
  the `offline` error instead. `Backend::offline_upload_folder(path)` puts
  them elsewhere.
- The copies are per user, and go with the outbox when the user signs out
  discarding their changes.
- `transfers` lists a waiting upload as `queued`; cancelling it takes it out
  of the outbox. The outbox's list shows its fields and each file by name.

### Transfers

Every upload and download is in `Transfers` while it runs, and for a minute
after: its name (or `.label(..)`), how many bytes so far, the total, and
whether it's running, done, failed or cancelled. It's published on
`elyra:transfers`. `Transfers::cancel(id)` stops one: the command that
started it gets an error of kind `cancelled`, and a download leaves no
`.part` file behind.

```rust
let transfers = ctx.get::<Transfers>();
for t in transfers.list() {
    println!("{} {}/{:?} {:?}", t.name, t.sent, t.total, t.state);
}
```

In the frontend it's the `transfers` store:

```svelte
<script>
  import { transfers } from "@elyra/runtime";
</script>

{#each $transfers.filter((t) => t.state === "running" || t.state === "queued") as t (t.id)}
  <div class="transfer">
    {t.direction === "upload" ? "↑" : "↓"} {t.name}
    {#if t.state === "queued"}waiting for the network{:else if t.total}{Math.round((t.sent / t.total) * 100)} %{/if}
    <button onclick={() => transfers.cancel(t.id)}>Cancel</button>
  </div>
{/each}
```

The page can list transfers and cancel one (through `/__auth/transfers`, so
with `Capability::Auth`), nothing else.

## `make:resource --backend`

```bash
rata make:resource Customer --backend /api/customers --generate name:string email:email 'phone:string?'
```

It makes the same resource as `--generate`, with the same screens, types,
abilities and events. The difference is that its commands call the API:

- `index` sends `search`, `sort`, `direction`, `page` and `per_page` as the
  query.
- `show`, `store`, `update` and `destroy` are one request each.

The model is a plain serde struct, with Laravel's ISO timestamps. There's no
migration or seeder, because the table is the server's. Validation is the
server's too: a `422` lands in the form, per field. The tests run against an
`HttpFake`.

The resource follows its channel. Its `mod.rs` has
`FOLLOWS = &[("customers", commands::PATH)]`, the registry gathers them all,
and the app listens with
`.broadcasting(Broadcasting::pusher(key)).follow_all(resources::follows())` on
its `Backend`. A generated test broadcasts through a `PusherFake` and watches
the open list re-run.

It works offline once the app has `.offline()`, and rata's hint says so:

- Its reads need nothing.
- `store`, `update` and `destroy` are marked `.queue_offline()`.
- `outbox_sent` dispatches the resource's `Created`, `Updated` or `Deleted`
  event once a write saved offline reaches the server. The registry gathers
  them, and the app wires it once with `.listen(resources::outbox_sent)`.
- The list shows an offline banner and the writes waiting, and those the
  server refused, with Fix, Keep mine / Take theirs and Discard. The form
  opens a refused write with the server's errors, and says "Saved, it will be
  sent" for kind `queued`.
- A generated test saves with no network and checks it's sent once, with its
  key.

With abilities on (the default), `make:resource --backend` generates a list
and a detail view that hide New, Edit and Delete when `$can("<plural>.create")`
and the others say no, preferring a record's `can.update` / `can.delete`
when the API sends it. rata prints the permissions to create and
`Route::elyraAbilities()`.

### A file field

```bash
rata make:resource Customer --backend /api/customers --generate name:string 'contract:file?'
```

A `file` field is a file on the server ([files](#files)):

- The record has its name (`contract: Option<String>`), what the API answers.
- `store` and `update` take `contract: Option<ChosenFile>` beside the input.
  With a file, the input goes as a form with the file beside it (multipart);
  without, as JSON, as before. An update with a file is sent as a `POST` with
  `_method=PUT`. Offline, it waits in the outbox with a copy.
- `customers_contract(id, to)` downloads it to where the user chose in a save
  dialog.
- The form has a `FileField` (written once to
  `app/src/resources/FileField.svelte`, yours to restyle): a button that opens
  the file dialog, and the first one takes a file dropped on the window. The
  detail view's file name downloads it.
- rata prints the controller's lines (`$request->file('contract')->store(..)`,
  keeping the name and where it's stored) and the download route
  (`Storage::download(..)`).

The app needs elyra's `system` feature for the dialogs; rata says so.

rata prints what the Laravel side needs: `Route::apiResource`, the
`BroadcastsEvents` model and channel rule above, and the idempotency
middleware. The `CustomerController` in
`scripts/laravel-backend.sh` shows an `index` that reads that query. For now the project needs the `database` feature too,
because the resource registry also lists migrations.

## Testing

```rust
use elyra::auth::MemoryTokens;
use elyra::http::{Http, HttpFake, Method};

let fake = HttpFake::new()
    .get("https://crm.test/api/customers", 200, json!({ "data": [], "total": 0, "per_page": 25, "current_page": 1, "last_page": 1 }))
    .post("https://crm.test/api/customers", 422, json!({ "errors": { "email": ["Taken."] } }));
let app = TestApp::new(
    App::new()
        .backend(Backend::new("https://crm.test"))
        .token_store(Arc::new(MemoryTokens::new().with("backend-token:https://crm.test", "1|token")))  // signed in
        .commands(commands())
        .swap(Http::fake(fake.clone())),
);
let errors = app.invoke_validation_errors("customers_store", (input,)).await.unwrap();
fake.assert_sent("POST", "https://crm.test/api/customers");
```

`TestApp` keeps tokens in memory unless told otherwise, so a test never
touches the keychain. `MemoryTokens::failing()` behaves like a system without
one.

`PusherFake` stands in for Reverb or Askr. Point the backend at it with
`.broadcasting(pusher.config())`, then broadcast:

```rust
let pusher = PusherFake::start().await;
// …an app with `.broadcasting(pusher.config()).follow("customers", "/api/customers")`, signed in…
pusher.wait_for_subscriptions(1).await;
pusher.broadcast("private-customers", "CustomerCreated", json!({ "model": { "id": 4 } }));
list.next().await;   // re-ran
```

Offline, the backend needs a database for its copy and its outbox. A route
marked `unreachable` is a network that's gone, and a later route for the same
URL brings it back:

```rust
let fake = HttpFake::new()
    .post("https://crm.test/api/sanctum/token", 200, json!("1|token"))
    .get("https://crm.test/api/user", 200, json!({ "id": 1 }))
    .unreachable(Method::Post, "https://crm.test/api/customers")
    .unreachable(Method::Get, "https://crm.test/up");
// …an app with `.bind(db)`, `.backend(Backend::new("https://crm.test").offline())`
// and `.swap(Http::fake(fake.clone()))`, signed in with `Auth::sign_in`…
assert_eq!(app.invoke_err_kind("customers_store", (input,)).await.as_deref(), Some("queued"));

fake.clone().post("https://crm.test/api/customers", 201, json!({ "data": { "id": 7 } }));
app.get::<Outbox>().flush().await;
assert!(app.get::<Outbox>().items().await.is_empty());   // sent
```

`make:resource --backend` generates this test for each resource.

A notification is a broadcast on the user's channel. `Notifier::fake()`
stands in for the OS's notification center:

```rust
// …an app with `.broadcasting(pusher.config()).notifications()`, signed in as user 1,
// and `.swap(Notifier::fake()).swap(Store::fake())`…
pusher.broadcast(
    "private-App.Models.User.1",
    "Illuminate\\Notifications\\Events\\BroadcastNotificationCreated",
    json!({ "id": "9b1", "type": "invoice-paid", "title": "Invoice paid", "url": "/invoices/7" }),
);
app.get::<Notifier>().assert_shown(|n| n.title == "Invoice paid");
app.get::<Notifier>().click(0);   // as the user would
```

[`scripts/laravel-backend.sh`](../scripts/laravel-backend.sh) builds an app with
all of it. CI runs Elyra against it offline too, through a proxy the test
pulls the plug on.

Against a real Laravel app:

```bash
scripts/laravel-backend.sh /tmp/api
# Reverb too: the customers broadcast their changes, so writes need it
(cd /tmp/api && php artisan serve --port 8765 & php artisan reverb:start --port 8080 &)
ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend --test laravel_backend

# Broadcasting, through Reverb (or Askr's --pusher) with the app's key
ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="$(grep ^REVERB_APP_KEY= /tmp/api/.env | cut -d= -f2)@127.0.0.1:8080" \
  cargo test -p elyra --features backend,database --test laravel_broadcasting

# Working offline: a proxy the test pulls the plug on
ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend,database --test laravel_offline

# Notifications, through Reverb
ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="…@127.0.0.1:8080" \
  cargo test -p elyra --features backend --test laravel_notifications
```

CI runs all four, broadcasting through Reverb and then through Askr.

## Related

- [HTTP client](http.md) · [Live queries](live-queries.md) · [Events](events.md) ·
  [Resources](resources.md) · [Secrets](secrets.md) · [Security](security.md)
