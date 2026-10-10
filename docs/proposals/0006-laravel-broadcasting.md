# RFC 0006 — Laravel's broadcasting in: a change on the web updates the open windows

**Status:** draft (2026-10-10) · **Target:** 0.13.0 · builds on [RFC 0005](0005-laravel-backend.md)

## Summary

Since 0.12, an Elyra app works against its Laravel backend, and its live
queries follow *its own* writes to the API. A customer edited on the web, or
by a colleague's desktop app, isn't seen until something re-reads it. Laravel
already announces those changes with [broadcasting](https://laravel.com/docs/broadcasting),
over the Pusher protocol, which [Askr](https://github.com/kwhorne/askr)
(`--pusher`), Reverb and Pusher all speak. This RFC has the desktop app listen
the way Laravel Echo does in a browser:

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))   // the site's own host, as with Askr
        .follow("customers", "/api/customers"),          // private-customers → re-run what read /api/customers
)
```

```php
// Laravel: the model announces its changes (model broadcasting)
class Customer extends Model
{
    use BroadcastsEvents;

    public function broadcastOn(string $event): array
    {
        return [new PrivateChannel('customers')];
    }
}
```

Ada changes a customer in the browser. Laravel broadcasts `CustomerUpdated`
on `private-customers`. Elyra gets it and re-runs the live queries that read
`/api/customers`, and the open window shows the change, typically within a
frame plus a request.

## Motivation

- **The point of desktop and web together.** Two halves of one product have to
  agree on what's true. Without this, the desktop app shows stale data until
  the user does something.
- **Laravel already says what changed.** Model broadcasting is one trait and
  one method, and Echo is the standard way to listen. The desktop app should
  be a client like any browser tab, not a reason to build a second mechanism.
- **The pieces are in place.** The token for private channels
  (`/broadcasting/auth` takes Sanctum), resource keys for live queries (RFC
  0005 step 4), the event bus to the windows, and domain events.

## Current state (0.12)

- `Backend` + `Auth`: a signed-in user, a token, Laravel's errors.
- A write through `Backend` invalidates `backend:<resource>`, and live
  commands that read it re-run.
- Nothing listens to the server. `Http` is request/response; there's no
  WebSocket client in the framework.

## Design

### The connection

`Broadcasting::pusher(key)` opens the Pusher protocol's WebSocket,
`wss://<host>/app/<key>?protocol=7`. The host is the backend's own by default,
as Askr serves it, and `.host()` / `.port()` / `.scheme()` change that for
Reverb or Pusher's cluster hosts.

- **When:** it connects once the user is signed in, and closes on sign-out.
  Private channels need the user's token, and there's no one to listen for
  before that.
- **The protocol:** it waits for `pusher:connection_established` and keeps
  the `socket_id`. It subscribes to each followed channel, answers
  `pusher:ping` with `pusher:pong`, and pings when the line is quiet past the
  server's `activity_timeout`.
- **Private channels** (`private-…`): it asks the backend to authorize them.
  That's `POST /api/broadcasting/auth` with `socket_id` and `channel_name`, as
  the signed-in user, which is where Laravel puts it with Sanctum
  (`withBroadcasting(…, ['prefix' => 'api', 'middleware' => ['api', 'auth:sanctum']])`).
  The answer's `auth` signature goes into `pusher:subscribe`. The app secret
  never leaves the server.
- **Reconnecting:** a dropped connection reconnects with backoff (1 s, 2 s, 4 s
  … capped at 30 s) and subscribes again. Events sent while it was away are
  lost, so a reconnect re-runs every followed resource.
- **`toOthers()`:** while connected, `Backend` sends `X-Socket-ID: <socket_id>`
  on its requests. Laravel's `broadcast(…)->toOthers()` then skips this app,
  whose own write already re-ran its queries.

### What an event does

Each event on a followed channel does three things:

1. **Re-runs live queries.** With `.follow("customers", "/api/customers")`,
   any event on `private-customers` invalidates `backend:/api/customers`.
   Live queries that read it re-run, coalesced and pushed only if changed,
   exactly as for a local write.
2. **It's a domain event**, `BackendEvent { channel, event, data }`, for
   `App::listen`: a notification, a sound, a log.
3. **It reaches the frontend** on `elyra:backend`: `onBackendEvent("customers",
   "CustomerUpdated", (data) => …)` in `@elyra/runtime`, for a toast ("Ada
   updated Acme").

The payload is the server's, untrusted like any input. It's JSON in, typed
by whoever reads it. Re-running a query doesn't use it at all, only the fact
that something changed.

### The connection state

`elyra:broadcasting` carries `{ state: "connecting" | "connected" | "offline" }`,
and the runtime's `broadcasting` store holds it. A window can show a "live"
dot, or "changes from others will show when you're back online".

### `make:resource --backend`

The generated resource follows its channel: `private-<plural>` for its
resource path. rata prints the Laravel side, the `BroadcastsEvents` snippet
above and the channel's rule in `routes/channels.php`.

### Testing

A `PusherFake` stands in for the server in tests. It accepts the connection,
checks subscriptions, and lets a test broadcast:

```rust
let pusher = PusherFake::start().await;
let app = TestApp::new(app().backend(backend.broadcasting(pusher.config())));
// …sign in, open a live list…
pusher.broadcast("private-customers", "CustomerUpdated", json!({ "model": { "id": 1 } }));
list.next().await;   // re-ran
```

CI also runs against a real Laravel app that broadcasts, to keep the auth
signature and the frames honest.

## Security

- **The server decides who hears what.** Private channels are authorized per
  user in `routes/channels.php`, with the same token as the API. The app
  subscribes only to the channels it declares.
- **`wss` only**, except to loopback, as for `Http`.
- **No secret on the desktop.** The Pusher app key is public by design; the
  secret signs on the server.
- **Untrusted payloads.** Events are data. They're never evaluated, and
  re-running a query doesn't even read them.

## Alternatives considered

- **Polling the API.** Easy and wasteful. It's slow to show a change and busy
  when nothing changes; broadcasting is how Laravel already does this.
- **Askr's SSE stream** (`/askr/events`). It's simpler, but public channels
  only, Askr only, and not what a Laravel app's Echo uses.
- **Laravel Echo in the webview.** It would work, but the token would have to
  reach the page, and events wouldn't drive live queries or domain events
  without a round trip through commands.

## Implementation plan

1. **The Pusher client** (part of the `backend` feature, adding
   `tokio-tungstenite`).
   - It connects, subscribes, authorizes private channels through the
     backend, pings and reconnects.
   - It sends `X-Socket-ID`, dispatches `BackendEvent`, and connects on sign-in
     and closes on sign-out.
   - `PusherFake` for tests.
2. **Live queries:** `.follow(channel, resource)`, and re-running everything
   followed after a reconnect.
3. **The frontend:** `elyra:backend` and `onBackendEvent`, and the
   `broadcasting` store.
4. **`make:resource --backend`, the docs, and CI** against a real Laravel app
   that broadcasts.

## Open questions

1. **Transport:** the Pusher protocol only (recommended: Askr, Reverb, Pusher
   and Soketi all speak it), or also Askr's SSE for public channels?
2. **Events to live queries:** explicit, `.follow("customers", "/api/customers")`
   (recommended), or by convention (`private-<x>` re-runs `/api/<x>`)?
3. **The frontend:** forward every followed channel's events on
   `elyra:backend` (recommended; it's the app's own UI), or only the channels
   marked for it?
4. **Presence channels** (who else is looking at this customer): later
   (recommended), or in this RFC?
5. **CI:** against Askr's `--pusher` (it's your web half) *and* Laravel's
   default Reverb (recommended, both), or one of them?
