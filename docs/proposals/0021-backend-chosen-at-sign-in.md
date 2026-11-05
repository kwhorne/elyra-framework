# RFC 0021 — A server chosen at sign-in, a token from elsewhere, switching

**Status:** implemented (2026-11-05) · **Target:** 0.34.0 · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0006](0006-laravel-broadcasting.md), [RFC 0008](0008-laravel-notifications.md) · see [Open questions](#open-questions)

## Summary

A Laravel backend whose server the user picks on the sign-in screen, a
sign-in that adopts a token the app got some other way, a switch between
identities on that server without signing out, and broadcast channels named
from the signed-in user:

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .user_route("/api/v1/me")              // { "user": { "id": 7 }, "workspace": { "id": 3 }, … }
        .revoke_route("/api/v1/token")
        .user_id_path("user.id")
        .broadcasting_from("/api/desktop/broadcasting")                               // the key, from the server
        .follow_user("chat", "user.{user.id}", None)                                  // private-user.7
        .follow_user("workspace", "workspace.{workspace.id}", Some("/api/v1/issues")), // private-workspace.3
)
```

```rust
// After OAuth's device grant in the browser:
auth.sign_in_with_token("https://acme.felagi.no", &token, me).await?;
// Later, the user picks another workspace in the sidebar:
auth.sign_in_with_token("https://acme.felagi.no", &other_token, other_me).await?;
```

## Motivation

Félagi Desktop is a Slack-like client for Félagi, a Laravel app that each
company runs on its own server. Four things it needs are missing:

- **The server is chosen at sign-in.** The user types their company's
  address on the sign-in screen. `Backend::new(base_url)` fixes the server
  when the app is built, and `Auth` keys its stored token by that URL.
- **The token comes from somewhere else.** Félagi signs in with OAuth's
  device authorization grant (RFC 8628) through the browser, for SSO and
  2FA, not with an email and password posted to a Sanctum token route. The
  app ends up holding a token and the user's profile. `Auth` has no way to
  take them, so broadcasting, abilities and notifications never start.
- **Switching identity without signing out.** Félagi's tokens are scoped to
  one workspace (tenant). A user in several workspaces holds a token for
  each, kept by the app in its own keychain entries, and switches between
  them as in Slack. A switch has to reconnect the broadcaster with the new
  token, read the abilities again, re-run what read the backend and update
  the frontend's `auth` store, and the UI must never see "signed out" in
  between.
- **The broadcasting settings are the server's.** Each company's server
  has its own Reverb, with its own app key, host and port. The app can't
  know them when it's built, and `Broadcasting::pusher(key)` wants the key
  then.
- **Channels named from the user.** Félagi broadcasts chat on
  `private-user.{id}` and workspace events on `private-workspace.{id}`.
  `Backend::follow(name, resource)` takes a fixed name. Only the
  notifications' channel (`App.Models.User.{id}`) is filled in from the user.

## Current state (0.33)

- **`Backend`** holds `base: String`, copied into each clone. The
  broadcaster, `Auth`, `Abilities` and the offline copy hold clones made at
  build time, so a base that changed later would reach none of them.
  `Offline` even takes its own `String` copy, for the copy's owner hash and
  the health probe.
- **`Auth::new`** names the token `backend-token:<base>` and the user's id
  `backend-user:<base>`, fixed at construction. It tells watchers `true` on
  sign-in and `false` on sign-out: the broadcaster starts and stops,
  abilities are read or forgotten, notifications are reset.
- **The user's id** is `user["id"]`, read in four places: the offline
  identity (`auth.rs`), the notifications channel (`channel_for`), the
  notifications' "seen" marker, and the presence member (`user_id`).
- **The broadcaster** works out the user's channel per connection
  (`follows_now`), so a new sign-in gets the new user's. Other follows are
  fixed `Follow`s.
- **Live queries** that read the backend depend on their resource's keys.
  Offline's "back online" re-runs the top keys it has seen read.

## Design

### The base is shared and read at use time

`Backend.base` becomes `Arc<RwLock<Option<String>>>`, shared by every clone.
Everything reads it when it uses it: a request when it's made, the
broadcaster when it connects, the offline copy when it hashes an owner or
probes `/up`, the assistant when it builds its client. `Backend::base_url()`
returns `Option<String>`.

### `Backend::chosen_at_sign_in()`

A backend with no server yet. Every builder method works. A request made
before a server is chosen fails with a new `BackendError::NoServer`, kind
`unauthenticated`. That isn't a panic or an `InvalidUrl`, and it isn't a
sign-out either, since nobody was signed in. The UI shows its sign-in screen,
as it would for a signed-out user.

### `Auth::sign_in_with_token(base_url, token, user)`

```rust
pub async fn sign_in_with_token(&self, base_url: &str, token: &str, user: Value) -> Result<(), BackendError>
```

It's `async` because a switch touches the offline copy and the outbox, both
in the database.

1. **The server.** `base_url` is normalised (trimmed, without the trailing
   `/`) and must be an `http(s)` URL with a host. For a `chosen_at_sign_in()`
   backend it chooses the server. While signed in to another one, it's
   refused: sign out first. For `Backend::new(url)` it must equal that URL,
   or it's an error. An app with a fixed server can't be pointed elsewhere.
2. **Kept first.** The token goes to the `TokenStore` under
   `backend-token:<server>`. For a chosen server, the server goes under the
   fixed key `backend-server`, and the user's id under `backend-user:<server>`.
   If the keychain refuses, nothing has changed (`BackendError::Keychain`).
   The token isn't revoked: the app got it, and the app decides.
3. **Adopted.** The server is chosen, the token used, the user value cached
   (what `auth.user()` returns and `elyra:auth` carries), and the offline
   identity set from it.
4. **Signed in before, or not.** If nobody was signed in, it's a sign-in:
   other users' offline copies are forgotten, this user's outbox resumes,
   `elyra:auth` says signed in, and watchers hear `SignedIn`. If someone
   was, it's a switch (below).

**At the next start**, `Auth::new` reads `backend-server`, chooses it, and
restores the token beside it. A server remembered without its token is
forgotten. The user value isn't kept. It's read again from `user_route` the
first time it's needed, as for a user who signed in with `sign_in` on an
earlier run (`/__auth/state` does this for the frontend). There are two
reasons. A keychain entry is the wrong place for a profile: Windows'
Credential Manager caps a secret at 2.5 KB, and a `/me` with a workspace and
its settings can pass that. And a profile goes stale (names, roles, the
workspace's plan), so it's better read fresh. The cost is that the app sets
`user_route` to a route that answers the same shape, which Félagi has
(`/api/v1/me`).

### The user's id: `Backend::user_id_path`

`.user_id_path("user.id")`, `id` by default, is where the user's id is in
the user value, as a dotted path. The four places that read `user["id"]`
read it there: the offline identity (the id, else `email`, as before), `{id}`
in the notifications channel, the notifications' "seen" marker, and the
presence member.

### Switching

`sign_in_with_token` while signed in, on the same server:

- **`elyra:auth` once**, `{ signedIn: true, user: <new> }`. No
  `signedIn: false` before it, so `onSignedOut` isn't called and the `$auth`
  store goes straight from one user to the other. The runtime needs no
  change: its store takes each `elyra:auth` state as it comes.
- **Watchers hear `Switched`**, a third value beside `SignedIn` and
  `SignedOut` (watchers were told a `bool`):
  - the broadcaster **restarts**: it closes the connection and connects again
    with the new token. `follows_now` reads the cached user, the new one, so
    the channels named from the user are named again. Presence members are
    forgotten with the old connection, and the windows' channels are joined
    again on the new one, as after any reconnect.
  - the abilities are **forgotten and read again**, so nothing is allowed on
    the old workspace's say-so for a moment. The frontend sees
    `loaded: false` briefly, as after a sign-in.
  - notifications are **reset**. The new connection's catch-up reads the new
    user's list.
  - live queries: **`backend::ALL_KEY`** (`backend:*`) is invalidated.
    Every read of the backend depends on it, beside its resource keys, so
    everything that read the backend re-runs once, as the new identity.
    Offline's "back online" re-runs only the top keys it has seen read, and
    only with `offline()`. That's no help here, since live queries need a
    database but not the offline copy.
- **The offline copy of reads is dropped**, all of it. The copy is keyed by
  owner (the server and the user's id) and URL, so a switch to another
  user id would never be answered from the last one's copy anyway. But the
  same user in another workspace has the same id and the same URLs, with
  other answers. Dropping the reads (a cache, refilled online) is the one
  rule that's right however the identity is defined.
- **The outbox stays,** keyed by owner as before. A write waits for its
  user, and is sent when that user is current again. One case would go
  wrong: the same user id with another token, where the waiting writes
  would be sent with the new token, so to the new workspace. That switch
  is refused with `PendingChanges` while writes wait, as `sign_out` is.
  A switch to the same token (a re-adoption) is allowed.

### Signing out

`sign_out()` revokes at `revoke_route` and forgets, as before. For a chosen
server it also forgets `backend-server` and unchooses the server, so
requests fail with `NoServer` again and the next start shows the sign-in
screen. A `401` (`expired`) does the same. An app that wants to fill in the
address next time keeps it itself. It isn't a secret.

### `Backend::broadcasting_from(path)`

The Pusher settings, read from the chosen server:

```rust
Backend::chosen_at_sign_in().broadcasting_from("/api/desktop/broadcasting")
```

- **When:** each time the broadcaster starts, which is after a sign-in, a
  switch, and at a start that restores a sign-in. It's a `GET` to the
  path as the user, before the connection. Félagi serves it at
  `GET /api/desktop/broadcasting`.
- **The answer** is `{ "key": "…", "host"?: "…", "port"?: 443, "scheme"?:
  "wss" | "ws" }`, or the same in `{ "data": … }`. What it leaves out is as
  built, else as `Broadcasting::pusher(key)` derives it from the server's
  URL. `ws` is still allowed only to loopback.
- **Nothing is kept.** The key is public, and the next start reads it
  again. With `offline()`, the answer is in the offline copy like any read,
  so a start without the network has it. That doesn't matter much, since
  there's nothing to connect to then.
- **Failures:** no answer, a `5xx` or a `429` is retried with the
  connection's backoff. Anything else (a `404` from a server that doesn't
  serve it, a `403`, an answer without a key) leaves broadcasting off for
  that start, with a warning. The rest of the app works as without
  broadcasting.
- **The follows and the auth route are the app's,** declared at build time.
  `broadcasting_from` keeps a `Broadcasting` given before or after it, for
  its auth route, and replaces its key with the server's.
- It works for a fixed `Backend::new(url)` too, for a product whose key
  differs per environment.

The alternative, an optional `Broadcasting` argument to
`sign_in_with_token`, would leave a restart without one: the app would
have to keep the settings itself and hand them over at start.

### `Backend::follow_user(name, template, resource)`

```rust
pub fn follow_user(self, name: &str, template: &str, resource: Option<&str>) -> Self
```

A private channel whose name has `{dotted.path}` placeholders, filled from
the signed-in user's value (a string or a number) at each connection. A
placeholder that doesn't resolve skips that follow with a warning, as the
notifications channel does when there's no `id`. The others are still
followed. On the wire it's `private-<filled>`, authorized at the auth route
with the bearer token, the socket id and the channel name, as any private
channel. `Broadcasting::auth_route(..)` sets the route
(`/api/broadcasting/auth` by default, which is Félagi's).

Its events are `BackendEvent { channel: name, … }`, dispatched and sent on
`elyra:backend` as for `follow`, so the frontend uses
`onBackendEvent("chat", "MessagePosted", cb)`. With a resource, they re-run
what read it, record by record as RFC 0017 has it. If the user's own channel
(notifications, abilities) has the same wire name as a `follow_user`, it's
subscribed once. Events reach both: notifications and ability changes as
before, everything else under the follow's name.

### Security

- The token and the server are in the keychain only, as before. The user
  value stays in memory.
- A fixed server can't be changed from the app. A chosen one can't be
  changed while signed in, so a page can't move a signed-in user's token to
  another host. `sign_in_with_token` isn't exposed through `/__auth`: it's
  Rust, called by the app's own sign-in code.
- Channel names come from the server's answer, and the server authorizes
  each, as for any private channel.
- A switch never answers from another token's offline copy, and never sends
  a write with a token it wasn't queued under.

### Testing

`framework/tests/backend_chosen_at_sign_in.rs`, with `HttpFake` and
`PusherFake`, covers:

- a request before a server is chosen;
- adopting a token, which chooses and persists the server, then a restart
  with the same token store;
- the wrong server on a fixed backend;
- a switch, with one `elyra:auth` and the new user;
- the broadcaster reconnecting with the new token to the re-named channels;
- an unresolved placeholder;
- the broadcasting settings read from the server after a sign-in and again
  at a restart (the fake records the app key each client connects with),
  and a `404` leaving broadcasting off with everything else working;
- sign-out and `401` forgetting the server;
- `follow_user` events on `elyra:backend`;
- with a database, live queries re-running and the offline copy across a
  switch.

## Alternatives considered

- **One `Backend` per tenant**, a container of them, picked per request.
  Each would need its own `Auth`, broadcaster, abilities and offline copy,
  and every `ctx.get::<Backend>()` in the app's commands would need to know
  which. The frontend's stores (`auth`, `can`, `broadcasting`) are single.
  The user is in one workspace at a time, so one backend whose identity
  changes matches the UI.
- **A tenant header (`X-Tenant: 3`) on one token.** It works for an API
  designed that way, but Félagi's tokens are scoped to a workspace by the
  server. A header the client sets is weaker than a token the server scoped,
  so a framework feature shouldn't push apps toward it. An app that wants a
  header can still add one per request.
- **Rebuilding the app (or its container) on a switch.** It's a full
  restart in all but name: windows reload, live queries drop. And it can't
  avoid the "signed out" between.
- **`Backend::set_base_url(..)` alone**, leaving the rest to the app. The
  base alone isn't enough: the keychain key, the broadcaster's connection,
  the offline owner and the auth state all follow from it, and an app would
  get some of them wrong.
- **Keeping the user value in the keychain** so that a restart needs no
  `user_route`. See above: the size limits on Windows, and staleness.
- **A template for the offline identity** (`{user.id}@{workspace.id}`), so
  each workspace keeps its own copy across switches. It's more to explain,
  and the outbox guard and dropping the reads make it unnecessary for
  correctness. It's left for later, if keeping the copy across switches
  turns out to matter.

## Implementation plan

1. `Backend`: the shared base, `chosen_at_sign_in()`, `NoServer`,
   `user_id_path`, `follow_user`, `ALL_KEY`. `Offline` reads the shared base.
2. `Auth`: keys from the base at use time, `backend-server`,
   `sign_in_with_token`, `Change { SignedIn, Switched, SignedOut }`, and
   forgetting the server on sign-out.
3. `App`: the watchers for a switch (broadcaster restart, abilities,
   notifications, live).
4. The broadcaster: the base at connect time, `follow_user` resolved per
   connection, `restart()`, the settings from `broadcasting_from` read per
   start.
5. Tests, `docs/backend.md`, the changelog.

## As built

As designed. Two notes:

- **The broadcaster's URL** was already worked out per connection
  (`session()` calls `Broadcasting::url(base)`). Only the base it was given,
  a build-time copy, had to become the shared one.
- **Presence, transfers, abilities and notifications** held no copy of the
  base. They go through `Backend` clones (abilities, the broadcaster) or the
  container's `Backend` (notifications), which now share it. Only `Offline`
  (owner hash, health probe) and the assistant's client (built per use, from
  `base_url()`) read it directly, and both now read it at use time.

- **`broadcasting_from`** came after the first draft, from the server side:
  the key, too, is the chosen server's. `PusherFake` gained `keys()` (the
  app key each connection asked for) and `port()` to test it.

## Open questions

1. **Switching to another server while signed in:** refused, sign out first
   (as built), or a switch like any other?
2. **The remembered server on a `401`:** forgotten, as on sign-out (as built,
   with the app keeping the address for the next sign-in if it wants), or
   kept so the app can sign in again to it at once?
3. **The offline copy across a switch:** dropped (as built), or kept per
   workspace with an identity template?
