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

## A server chosen at sign-in

Some products aren't one server. Each company runs its own, and the user
types its address on the sign-in screen. `Backend::chosen_at_sign_in()`
is a backend with no server until then
([RFC 0021](proposals/0021-backend-chosen-at-sign-in.md)):

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .user_route("/api/v1/me")
        .broadcasting_from("/api/desktop/broadcasting"),
)
```

- **Every builder method works** as for `Backend::new(url)`. The routes are
  paths, and the broadcaster's socket goes to the chosen server's host
  unless the settings say otherwise.
- **Before a server is chosen, a request fails** with
  `BackendError::NoServer`, kind `unauthenticated`, as for a user who isn't
  signed in. Nothing is sent anywhere. The UI shows its sign-in screen, as
  it would anyway.
- **`Auth::sign_in_with_token` chooses it** (below). The address is
  normalised (`https://acme.example.com/` is `https://acme.example.com`)
  and must be an `http` or `https` URL.
- **It's remembered while the user is signed in.** It's in the keychain
  under `backend-server`, beside the token, which is under
  `backend-token:<server>` as for any backend. At the next start the app is
  signed in to the same server.
- **Signing out forgets it**, and so does a `401`. The next start shows the
  sign-in screen. If the app wants to fill the address in for the user, it
  keeps it itself (it isn't a secret: the [store](store.md) will do).
- **The broadcasting settings come from the server** with
  `.broadcasting_from(path)`, since each server has its own Reverb or
  Pusher and the app can't know its key when it's built. After each
  sign-in or switch, and at a start that restores one, the app sends a
  `GET` to the path as the user, before it connects. The answer looks like
  this:

  ```json
  { "key": "…", "host": "ws.acme.example.com", "port": 443, "scheme": "wss" }
  ```

  `host`, `port` and `scheme` may be left out. They default as for
  `Broadcasting::pusher(key)`, to the server's own host. A `{ "data": … }`
  wrapper is fine. Nothing of it is kept: the key is public, and it's read
  again each time. No answer (or a `5xx`, a `429`) is tried again with the
  connection's backoff. A `404`, a `403`, or settings without a key leave
  broadcasting off for this sign-in, with a warning in the log. Everything
  else works. The follows and the auth route are still the app's. For an
  auth route other than `/api/broadcasting/auth`, add
  `.broadcasting(Broadcasting::pusher("").auth_route(..))`; its key is
  replaced by the server's.
- `backend.base_url()` is the server, `None` until it's chosen.
  `Auth::sign_in(email, password)` needs one too: with none, it fails with
  `NoServer` as any request does.

## A token from elsewhere

`Auth::sign_in` posts an email and password to Sanctum's token route. An app
that signs in another way, such as OAuth's device authorization grant
through the browser (SSO, 2FA), ends up holding a token and the user's
profile. `sign_in_with_token` adopts them:

```rust
let me: Value = /* the profile the device grant's last step answered */;
ctx.get::<Auth>()
    .sign_in_with_token("https://acme.example.com", &access_token, me)
    .await?;
```

From then on it's as after `sign_in`. Requests carry the token, the
broadcaster connects, the abilities are read, notifications arrive, and
`elyra:auth` says `{ signedIn: true, user }`.

- **The server:** for `Backend::chosen_at_sign_in()`, the first argument
  chooses it. For `Backend::new(url)`, it must be that server, written any
  way that normalises to it. Another one is an error: an app with a fixed
  server can't be pointed elsewhere.
- **The token is kept in the keychain** as `sign_in` keeps one. With no
  keychain, it fails with `BackendError::Keychain` and nothing changes. It
  isn't revoked: the app got it, and the app decides.
- **The user value is what `auth.user()` returns** and what `elyra:auth`
  carries, without asking the server. It isn't kept across restarts: at
  the next start it's read from `user_route` the first time it's needed. A
  keychain entry is the wrong place for a profile, which can be larger
  than Windows allows (2.5 KB) and goes stale. So point `user_route` at a
  route that answers the same shape.
- **The user's id** names the user's channel, the user among presence
  members, and whose offline copy it is. It's at `id` unless
  `.user_id_path("user.id")` says otherwise, for a route that answers
  `{ "user": { "id": 7 }, "workspace": { … } }`.
- **Signing out** works as before. `sign_out()` revokes the token at
  `revoke_route` (a `DELETE`, as the user) and forgets it, so point that
  route at whatever revokes this kind of token.

## Switching

A user in several workspaces of the same product may hold one token per
workspace, each scoped to its workspace. The app keeps those in its own
keychain entries, and lets the user switch between them as Slack does.
Switching means calling `sign_in_with_token` again while signed in, with the
other token and its profile:

```rust
auth.sign_in_with_token(&server, &tokens.get(workspace)?, me).await?;
```

- **No sign-out between.** `elyra:auth` is sent once, `{ signedIn: true }`
  with the new user, so the `$auth` store goes from one user to the other.
  `onSignedOut` isn't called.
- **The broadcaster reconnects** with the new token. It subscribes to the
  app's channels again, and to [the channels named from the
  user](#channels-from-the-user) as the new user names them.
- **The abilities are read again.** They're forgotten first, so there's no
  moment with the last workspace's.
- **Everything live that read the backend re-runs.** Every backend read
  depends on `backend::ALL_KEY` as well as its resource.
- **Notifications** are counted again from the new user's list, once the
  broadcaster has reconnected.
- **The server's offline copy of reads is dropped.** Another token's
  answers to the same URL can differ (another workspace), so none is ever
  answered from the last one's. Other servers' copies stay. The outbox
  stays: each write waits for its own user's turn. A switch that keeps the
  user's id but changes the token would send the waiting writes with the
  new token, so it's refused with `PendingChanges` (kind `pending_changes`)
  until they're sent or discarded.
- **Another server** needs a sign-out first, unless the backend is signed in
  to [several at once](#signed-in-to-more-than-one-server).

## Signed in to more than one server

Some people work in more than one company's server: their employer's, and a
customer's. `.multiple_servers()` keeps them signed in to each, one of them
active, and moves between them without signing out
([RFC 0022](proposals/0022-multiple-servers.md)):

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .multiple_servers()
        .user_route("/api/v1/me")
        .broadcasting_from("/api/desktop/broadcasting")
        .offline(),
)
```

```rust
let auth = ctx.get::<Auth>();
auth.sign_in_with_token("https://acme.example.com", &acme, acme_me).await?;   // Acme active
auth.sign_in_with_token("https://kunde.example.com", &kunde, kunde_me).await?; // added, Kunde active
for account in auth.accounts() {             // Account { server, user_id, user, active }
    println!("{} {}", account.server, account.active);
}
auth.switch_to("https://acme.example.com").await?;     // nothing revoked
auth.sign_out_of("https://kunde.example.com").await?;  // Kunde's token revoked, Acme goes on
auth.sign_out_all().await?;                            // each revoked on its own server
```

- **One account per server.** Each has its own token
  (`backend-token:<server>` in the keychain), the user's id beside it
  (`backend-user:<server>`), its own offline copy and its own outbox. The
  list is under `backend-servers`, the active one under `backend-server`,
  and both outlive a restart. The user value is known per server for the
  run (`Account.user`), as given to `sign_in_with_token` or read while that
  server was active; at a start it's `None` until the server is active and
  `auth.user()` reads it.
- **Signing in to another server adds it** and makes it active. The others
  stay signed in, untouched. Signing in again to one already in the list
  replaces its token (another workspace there: the same rules as a
  [switch](#switching)).
- **`switch_to(server)`** makes another one active. Requests go there with
  its token; the broadcaster reconnects there (its settings, its key, the
  channels named from its user, and the presence channels windows want);
  abilities are read again; everything live that read the backend re-runs;
  its outbox carries on. `elyra:auth` is sent once, `{ signedIn: true,
  user, server }`. Nothing is revoked, and no copy is dropped.
- **`sign_out_of(server)`** revokes that server's token, there, and forgets
  it and its copy. The active one: the next in the list becomes active
  first, so the UI never shows "signed out". The last one is a sign-out as
  always. `sign_out()` is `sign_out_of` the active server;
  `sign_out_all()` leaves every one, the active one last.
- **A `401`** signs out of that server alone, and the next one becomes
  active.
- **`elyra:accounts`** carries `{ accounts, active, signedOutOf, reason }`
  after each change: `signedOutOf` and `reason` (`"signed-out"`,
  `"expired"`) name a server just left.
- **Without `.multiple_servers()`** nothing changes: another server needs a
  sign-out first, and `accounts()` lists the one signed in to, if any.

From the frontend:

```ts
import { accounts, auth, onSignedOutOf } from "@elyra/runtime";

$accounts.accounts;                      // [{ server, userId, user, active }]
await auth.switchTo("https://acme.example.com");
await auth.signOutOf("https://kunde.example.com");   // kind pending_changes while its writes wait
onSignedOutOf((server, reason) => toast(`Signed out of ${server}`));
```

Signing *in* to a server stays the app's Rust code: a page can move between
the servers the user signed in to, and leave them, never point a token at a
new host.

**Offline, per server.** Each server's reads are answered only from its own
copy, kept across switches. A write queued on one server is tagged with that
server and its user, and is sent only there, with that server's token: it
waits, untried, while another server is active, and goes when its own is
active again. The outbox, `$outbox` and `$connection.pending` are the active
server's. Signing out of a server with writes waiting is refused with
`PendingChanges`; `sign_out_of_discarding(server)` drops them, returns how
many, and dispatches `OutboxDropped { server, count }`.

## Live queries

A live command that reads from the backend updates itself when this app
writes, and re-runs only what the write could have changed
([RFC 0017](proposals/0017-record-invalidation.md)):

| a `GET` of | depends on |
|---|---|
| `/api/customers?page=2` (a list) | the resource's **lists**, `backend:list:/api/customers` |
| `/api/customers/count` (a part of the list) | the same lists |
| `/api/customers/42` (a record) | **that record**, `backend:/api/customers/42` |
| `/api/customers/12/notes` (under a record) | its own lists, and record 12 |

Every read also depends on its **resource**, `backend:/api/customers`, and
each path it's under. The resource is the path without its query and
trailing ids (a number, UUID or ULID).

- **A write** (`POST`, `PUT`, `PATCH`, `DELETE`) re-runs the lists, and the
  record its path names. `PUT /api/customers/40` re-runs the lists and the
  view of customer 40, not customer 42's. `POST /api/customers` re-runs the
  lists. `POST /api/customers/40/archive` is a change to customer 40.
- **`.with_lists()`** on a record's `GET` makes it re-run on every change
  to the resource. Use it for an endpoint that shows other records too.
- **`ctx.invalidate(&backend::resource_key("/api/customers"))`** re-runs
  every view of the resource. `ctx.depends_on(&backend::list_key(..))`
  re-runs on every change to it. `ctx.depends_on` and `ctx.invalidate`
  cover paths that aren't shaped like an `apiResource`.
- `LiveRegistry::stats()` and the inspector say how often each query
  re-ran and how often its result changed.

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

1. It **re-runs the live queries** it could have changed, coalesced, and
   pushed only if the result changed. An event whose `model` names its
   record (Laravel's model broadcasting: `CustomerUpdated { model: { id: 40,
   … } }`) re-runs the lists and the views of customer 40. Any other event
   re-runs everything that read the resource. A model whose route key isn't
   `id` says so: `.record_key("/api/invoices", "uuid")`.
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
follows a public `Channel`; presence channels have [their own section](#who-else-is-here-presence).

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

## Channels from the user

Laravel often broadcasts on channels named after the user, or the
workspace their token is for: `private-user.7`, `private-workspace.3`.
`.follow(..)` takes a fixed name. `.follow_user(..)` names the channel from
the signed-in user:

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .user_route("/api/v1/me")              // { "user": { "id": 7 }, "workspace": { "id": 3 }, … }
        .revoke_route("/api/v1/token")
        .user_id_path("user.id")
        .broadcasting_from("/api/desktop/broadcasting")   // { "key": "…", "host": "…", "port": 443 }
        .follow_user("chat", "user.{user.id}", None)                                  // private-user.7
        .follow_user("workspace", "workspace.{workspace.id}", Some("/api/v1/issues")), // private-workspace.3
)
```

- **`{dotted.path}`** is filled from the user value, as `sign_in_with_token`
  was given it or `user_route` answered it: a string or a number. A
  placeholder the user doesn't have skips that channel, with a warning in
  the log. The others are still followed.
- **It's a private channel**, `private-` on the wire, authorized at the
  auth route as any other (`Broadcasting::auth_route`,
  `/api/broadcasting/auth` by default).
- **It's named again at each connection,** so after a switch the app
  listens on the new user's and workspace's channels, not the old ones.
- **Its events are `BackendEvent`s on the follow's name**, on
  `elyra:backend`, as for `.follow(..)`. With a resource, they re-run what
  read it too.

```js
import { onBackendEvent } from "@elyra/runtime";

onBackendEvent("chat", "MessagePosted", (data) => append(data.message));
onBackendEvent("workspace", null, () => refreshSidebar());
```

## Who else is here: presence

Two people open the same customer, one in the browser and one in the
desktop app. With Laravel's presence channels, each sees the other before
either saves ([RFC 0015](proposals/0015-presence.md)):

```rust
Backend::new("https://crm.example.com")
    .broadcasting(Broadcasting::pusher("app-key"))
    .presence(["customers.*"])              // the channels a window may join
```

```php
// routes/channels.php: the same rule Echo.join('customers.42') uses on the web
Broadcast::channel('customers.{id}', fn ($user, $id) =>
    $user->can('view', Customer::find($id))
        ? ['id' => $user->id, 'name' => $user->name, 'avatar' => $user->avatar_url]
        : false);
```

```svelte
<Card title={record.name}>
  {#snippet actions()}<Presence channel={`customers.${id}`} />{/snippet}
</Card>
```

- **A window joins** while something follows the channel: `<Presence>`, or
  the runtime's `presence(channel)` store (`$here.members`, `$here.me`,
  `$here.joined`). The app joins each channel once for all its windows,
  and leaves when the last one leaves, or closes.
- **The members** are Laravel's: each one's `id` and the `info` its channel
  rule returned. A user with two tabs is one member. `<Presence>` leaves
  the user out (`self` shows them).
- **Only listed channels.** A name `Backend::presence(..)` doesn't cover is
  refused (`denied`) before anything is sent. Laravel's rule still decides
  who gets in; a `false` leaves it unjoined, and the connection goes on.
- **Without a connection** (signed out, offline, unplugged in the
  inspector), nothing is joined and nobody is shown. The channels are
  joined again when it's back.
- **In Rust**, `ctx.get::<Presence>()` has `members(channel)` and
  `channels()`.
- **`make:resource --backend .. --presence`** puts a `<Presence>` on the
  detail view and the edit form, and prints both lines above.

### Who's editing

An edit form can say it's being edited, and show who else is:

```svelte
<Presence channel={`customers.${id}`} editing={changed} />
```

Ada changes a field in the desktop app; Grete, with the same customer open
in the browser, sees "Ada is editing" next to her avatar, and the other way
round. It's a whisper on the presence channel, as Echo sends one, so the web
half takes part with Echo alone:

```js
// The browser: say it, and hear it.
const channel = Echo.join(`customers.${id}`);
channel.whisper("editing", { editing: true });
channel.listenForWhisper("editing", (e, { user_id }) => markEditing(user_id, e.editing));
```

- **Information, not a lock.** Laravel never sees a whisper. Two people can
  still both save; the conflict check at save time
  ([RFC 0007](proposals/0007-offline-and-sync.md)) is what stops a lost
  update.
- **When it's said.** The app tells the channel when its first window
  starts editing and when its last one stops (`editing` back to `false`,
  or the form closing), not on each keystroke. Whispers aren't kept, so it
  says it again to someone who joins later, and after a reconnect.
- **Who said it.** A member counts as editing only when the server names
  them: Reverb adds the sender's `user_id` to a whisper on a presence
  channel, and only from a member, with its default
  `REVERB_APP_ACCEPT_CLIENT_EVENTS_FROM=members`. With `all`, it doesn't
  name the sender, and nobody shows as editing; with `none`, whispers are
  refused. On Pusher, enable client events for the app.
- **Askr's `--pusher`** doesn't pass whispers on (nor presence members).
- **The runtime** has it as `presence(channel).editing(true)`, while the
  store is subscribed; each member has `editing`. In Rust,
  `Presence::editing(window, channel, on)` and `editing_here(channel)`.
- **`make:resource --presence`'s edit form** passes `editing={changed}`:
  changed from the record as it was loaded.

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
- **Trying it:** in a debug build, the [inspector](inspector.md#trying-the-app-offline)'s
  **Simulate offline** unplugs the backend without stopping Laravel; the
  state then carries `simulated: true`.
- **Whose copy:** it's kept in `elyra_backend_cache` in the app's database,
  created on first use, with the server, the user and the URL hashed. It's
  kept per user and per server, only while signed in, and cleared when the
  user signs out or someone else signs in. An expired sign-in keeps it for
  the same user. Signed in to [several servers](#signed-in-to-more-than-one-server),
  each keeps its own. It's capped at
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
  Ask the user, then `auth.signOutDiscarding()`. Signed in to several
  servers, `auth.signOutOf(server)` and `auth.signOutOfDiscarding(server)`
  do the same for one.

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

## The app's assistant

An app with an assistant ([RFC 0011](proposals/0011-in-app-assistant.md))
reaches Claude through Laravel, so no key is in the app. Laravel adds its key
and decides what may be asked:

```rust
App::new()
    .backend(Backend::new(url).abilities().assistant())   // POST /api/elyra/assistant/v1/messages
    .assistant(Assistant::new().allow_abilities(["customers.*"]))
```

```php
// routes/api.php (elyracode/elyra-backend), inside auth:sanctum
Route::elyraAssistant();
```

```dotenv
ANTHROPIC_API_KEY=sk-ant-…
# optional
ELYRA_ASSISTANT_MAX_TOKENS=4096
ELYRA_ASSISTANT_PER_MINUTE=30
ELYRA_ASSISTANT_STREAM=true
```

- The desktop sends the Messages API's body with the user's Sanctum token.
  Signed out, the assistant can't be used ("Sign in to use the assistant.").
- Laravel answers only for the models in `config('elyra.assistant.models')`
  (`claude-sonnet-5-5` by default), lowers `max_tokens` to its cap, refuses
  Claude's own server tools, and allows `per_minute` requests per user. A
  refusal is the Messages API's error shape, which the desktop shows. The
  tool loop stays on the desktop: its tools are the app's commands.
- **Streaming** ([RFC 0018](proposals/0018-assistant-streaming.md)): a
  request with `stream: true` is passed through as Claude writes it, as
  `text/event-stream` with `X-Accel-Buffering: no`, so nginx doesn't hold
  it. The usage is read off the stream for `AssistantUsed`. A server that
  buffers anyway (a proxy in front, a PHP server that doesn't flush) can
  set `ELYRA_ASSISTANT_STREAM=false`: the desktop then asks for each answer
  whole. **Askr** (1.9.2) holds a streamed response until it's done: the
  answer arrives right, but at once. Set `ELYRA_ASSISTANT_STREAM=false` there
  until it passes streams on.
- Each answered request dispatches `Elyra\Backend\Events\AssistantUsed`
  (the user, the model and the tokens, never the prompt), to keep a record
  of who used how much.
- `.assistant_route(..)` moves the route.

Without `.assistant()`, the assistant uses the user's own key from the
keychain (`/__assistant/key` keeps it, write-only, with `SecretsProvider`),
and then the container's `elyra::ai::Ai` (`AiProvider`, from
`ANTHROPIC_API_KEY`). `/__assistant/source` says which is in use.

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
- The form has `@elyra/ui`'s [`FileField`](ui/file-field.md) in a `Field`:
  a button that opens the file dialog, and the first one takes a file
  dropped on the window. Laravel's message for the file shows under it. The
  detail view's file name downloads it. (Before 0.23, rata wrote an
  `app/src/resources/FileField.svelte`; a form generated then keeps using
  it.)
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
one. `Secrets` is a token store too: `.token_store(Arc::new(secrets.clone()))`
with `Secrets::fake()` keeps each server's token where a test can read it.
One `HttpFake` answers any number of servers, by their full URLs, so two
servers signed in at once are tested as one
(`framework/tests/multiple_servers.rs`).

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
