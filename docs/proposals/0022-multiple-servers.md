# RFC 0022 — Signed in to more than one server

**Status:** implemented (2026-11-05) · **Target:** 0.35.0 · builds on [RFC 0021](0021-backend-chosen-at-sign-in.md), [RFC 0007](0007-offline-and-sync.md), [RFC 0006](0006-laravel-broadcasting.md), [RFC 0015](0015-presence.md) · see [Open questions](#open-questions)

## Summary

A backend chosen at sign-in that stays signed in to several servers at once,
one of them active, and switches between them without signing out:

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .multiple_servers()                 // new
        .user_route("/api/v1/me")
        .user_id_path("user.id")
        .revoke_route("/api/v1/me/token")
        .broadcasting_from("/api/desktop/broadcasting")
        .follow_user("chat", "user.{user.id}", None)
        .offline(),
)
```

```rust
let auth = ctx.get::<Auth>();
auth.sign_in_with_token("https://acme.felagi.no", &acme, acme_me).await?;   // signed in, Acme active
auth.sign_in_with_token("https://kunde.felagi.no", &kunde, kunde_me).await?; // added, Kunde active
auth.accounts();                                     // [Acme, Kunde (active)]
auth.switch_to("https://acme.felagi.no").await?;     // nothing revoked
auth.sign_out_of("https://kunde.felagi.no").await?;  // Kunde's token revoked, Acme carries on
auth.sign_out_all().await?;                          // each revoked on its own server
```

## Motivation

Félagi Desktop (RFC 0021's app) is used by people who work in more than one
company's Félagi: their own employer's, and a customer's or a partner's.
Each runs on its own server. Today:

- **One server at a time.** `sign_in_with_token` to another server while
  signed in is refused: sign out first. Signing out revokes the token, so
  going back means the browser sign-in again, with SSO and 2FA.
- **The offline copy and the outbox are one.** They're keyed by an owner
  hash (the server and the user's id), so they're separate in the table
  already, but every rule around them assumes one server: a sign-in drops
  every other owner's copy, a same-server switch drops every copy, and
  the outbox sends what the *current* owner has with the *current* token.
- **The app can't keep a second token itself and swap it in**, the way it
  does for workspaces on one server: that would be a switch to another
  server, which is refused, and would lose the first server's offline copy
  on the way.

People need to be signed in to both and move between them as in Slack's
workspace switcher: one click, nothing to sign in again, nothing lost.

## Current state (0.34)

- **`Auth`** holds one sign-in: the backend's token, the cached user, and
  in the keychain `backend-server` (the chosen server),
  `backend-token:<server>` and `backend-user:<server>` (the user's id).
- **`sign_in_with_token`** while signed in is a switch on the *same*
  server. To another server it's an error.
- **Watchers** hear `SignedIn`, `Switched` and `SignedOut`. On `Switched` the
  broadcaster restarts (base, token and settings read at connect time, user
  channels named again, presence channels joined again), abilities are
  forgotten and read again, notifications are reset, and live queries
  depending on `backend::ALL_KEY` re-run.
- **The offline copy** (`elyra_backend_cache`) keys each row by
  `hash(owner, url)` with `owner = hash(server, user id)`. A sign-in deletes
  every other owner's rows; a switch deletes every row.
- **The outbox** (`elyra_outbox`, `elyra_outbox_failed`) tags each row with
  its owner. `flush` sends the current owner's rows, oldest first, through
  `Backend::replay`, which builds the request from the backend's *current*
  base and token.
- **A request** reads the base and token when it's built, but the offline
  owner, the connection state and the 401 handling when it's *answered*.

## Design

### Turned on with `Backend::multiple_servers()`

An app opts in. Without it nothing changes: another server needs a sign-out
first, exactly as in 0.34, and no list is kept. It only means something with
`chosen_at_sign_in()`; on a fixed `Backend::new(url)` it does nothing.

An opt-in rather than the new default, because an app built for one server
may rely on the refusal (Félagi's own sign-in signs out first when the
server changes) and its UI has no switcher.

### One account per server

An account is a server the user is signed in to, with:

| | kept in | key |
|---|---|---|
| the token | keychain (`TokenStore`) | `backend-token:<server>` (as before) |
| the user's id | keychain | `backend-user:<server>` (as before) |
| the list of servers | keychain | `backend-servers`, a JSON array, in sign-in order |
| the active server | keychain | `backend-server` (as before) |
| the user value | memory | per server, this run only (RFC 0021's reasons) |

One account per server origin. Two tokens on one server (Félagi's
workspaces) stay what they were in RFC 0021: a switch *within* the server's
account, `sign_in_with_token` with the same server.

**At start**, `Auth::new` reads `backend-servers` (or, the first time, the
`backend-server` a 0.34 app kept: a list of one), drops any server whose
token is gone, and makes `backend-server` active, else the first left.

### The API

```rust
impl Backend {
    pub fn multiple_servers(self) -> Self;
    pub fn allows_multiple_servers(&self) -> bool;
}

impl Auth {
    pub fn accounts(&self) -> Vec<Account>;
    pub fn accounts_state(&self) -> AccountsState;
    pub fn active_server(&self) -> Option<String>;
    pub async fn switch_to(&self, server: &str) -> Result<(), BackendError>;
    pub async fn sign_out_of(&self, server: &str) -> Result<(), BackendError>;
    pub async fn sign_out_of_discarding(&self, server: &str) -> Result<u32, BackendError>;
    pub async fn sign_out_all(&self) -> Result<(), BackendError>;
    pub async fn sign_out_all_discarding(&self) -> Result<u32, BackendError>;
}

pub struct Account {
    pub server: String,           // https://acme.felagi.no
    pub user_id: Option<String>,  // at user_id_path, as kept beside the token
    pub user: Option<Value>,      // as given or read this run
    pub active: bool,
}
```

- **`sign_in_with_token(server, token, user)`** to a server other than the
  active one adds it (or replaces its token, if already signed in to it)
  and makes it active. The others are untouched: not revoked, their copies
  kept, their writes still waiting.
- **`switch_to(server)`** makes another signed-in server active. Nothing
  is revoked. Switching to the active one is a no-op; to a server not
  signed in to is an error.
- **`sign_out_of(server)`** revokes that server's token on that server,
  with that token, and forgets it and its offline copy. If it was the active
  one, the next server in the list becomes active first, so the UI never
  sees "signed out". The last one is a sign-out as before.
- **`sign_out()`** is `sign_out_of(active)`. That's what it always meant:
  the token requests carry is revoked. With one server it's unchanged.
- **`sign_out_all()`** signs out of the inactive ones, then the active one,
  so `elyra:auth` says `signedIn: false` once, at the end.
- Signing out of one server, or all, is **refused with `PendingChanges`**
  while writes for it wait, as `sign_out` is. The `_discarding` forms drop
  them, return how many, and dispatch `OutboxDropped { server, count }`.

### Events

- **`elyra:auth`** (`AuthState`) gains `server: Option<String>`, the active
  one. A switch, a sign-in to another server and a sign-out that leaves
  another active each send it once with `signedIn: true` and the now active
  user (`null` when it isn't known this run: `auth.user()` or the runtime's
  `auth.state()` reads it). The runtime's `$auth` store takes it as it
  takes RFC 0021's switch; `onSignedOut` isn't called.
- **`elyra:accounts`** (`AccountsState`), new: `{ accounts, active,
  signedOutOf, reason }`, after each change to the list or the active
  server. `signedOutOf` and `reason` (`"signed-out"`, `"expired"`) say which
  server was just left and why, for a toast, when the auth state alone
  can't (it still says signed in).
- **Watchers hear `Switched`** for every change of the active server: the
  same set as RFC 0021's switch, so broadcasting, abilities, notifications
  and live queries need no new code.
- **`OutboxDropped { server, count }`**, a domain event like `OutboxSent`,
  when writes are dropped by a `_discarding` sign-out (any, not only the new
  ones).

### Requests go to the active server, and stay whose they were

A request takes its server, its token and its offline owner when it's built,
as before for the first two. What changes is that everything after the
answer uses those, not the backend's state at the time of the answer:

- **The offline copy** is stored under and looked up for the request's own
  owner. A read of Acme answered after a switch to Kunde is kept as Acme's.
- **The connection state** (online/offline) is only changed by a request
  to the active server. A late "no answer" from Acme doesn't mark Kunde
  offline.
- **A `401`** signs out only if the request's token is the one requests
  carry now. One with a token a switch has since replaced, or another
  server's (a revoke, the outbox), signs out of nothing.
- **A queued write** is kept for the request's owner and server.

`Backend::request_as(base, token, method, path)` (crate-private) builds a
request to a server that may not be the active one, with its token, for the
revoke of an inactive server and the outbox.

### Realtime

A change of the active server is a `Switched`. The broadcaster closes its
connection and connects again: to the new server's host, with the settings
`broadcasting_from` reads from the new server, with its token, to the
channels `follow_user` names from its user. Presence: members are forgotten
with the old connection, and the channels windows want are joined on the new
one, as after any reconnect (RFC 0015). A window showing a record of the old
server should leave its channel once it shows the new server's records; the
app's switch does that as it re-renders.

### The offline copy, per server

- `elyra_backend_cache` gains a `server` column: a hash of the server's
  origin, as `owner` and `id` are hashes. Rows were already isolated by
  `owner` (the server is in it) and `id` (the full URL is in it), so a
  copy of server A can never answer a request on B. The column is what
  makes "forget this server's copy" one `DELETE`.
- **A switch between servers keeps every copy.** A switch *within* a server
  (another token there) drops that server's, as RFC 0021 dropped all.
- **A sign-in** drops the copies of everyone not signed in now (it dropped
  everyone else's), keeping each signed-in server's user's.
- **Signing out of a server** drops its copy, and no other's.
- The connection state is reset to online when the active server changes;
  the next request finds out.

### The outbox, per server

- `elyra_outbox` and `elyra_outbox_failed` gain `server`, in the clear as
  the path is: the origin the write is for. Each row also has its owner
  (server and user), as before.
- **Sent only there, with that server's token.** `flush` picks the active
  owner's rows, as before, and sends each to its row's `server` with the
  token `Auth` holds for that server, not the backend's current base and
  token. A switch between picking a row and sending it can't send it to the
  other server.
- **An inactive server's writes wait** in the outbox, untried, and go when
  it's active again (`switch_to` resumes the outbox). The counts on
  `elyra:connection` and `elyra:outbox`, and `Outbox::items()`, are the
  active server's.
- **Not in the background.** Sending an inactive server's writes would be
  simple on its own (the row has the server, `Auth` has the token), but what
  follows a send isn't: `OutboxSent` and `OutboxFailed` would reach an app
  showing another server, the live queries a write re-runs are named by path
  and would re-run the active server's, a refused write would sit in a list
  the user can't see, and the connection state is the active server's
  alone. Waiting is what a signed-out user's writes do today; it's safe and
  easy to explain. See [Open questions](#open-questions).
- **Signing out of a server** refuses while its writes wait, or drops them
  (`_discarding`, with a count and `OutboxDropped`).

### Migration and compatibility

- **Single-server apps** don't call `multiple_servers()` and behave as in
  0.34: the same keys, the same refusal, the same events plus
  `elyra:accounts` and `AuthState.server`, which a 0.34 frontend ignores.
- **Turning it on** in an app that was single-server: the `backend-server`
  it kept becomes a list of one at the next start.
- **The tables** get their `server` column on first use (`ALTER TABLE …
  ADD COLUMN server … NULL`, after checking it isn't there; no migration to
  run). Old rows keep `NULL`:
  - a cache row is still found by its id (the hash hasn't changed), and is
    dropped by the first per-server forget, as a 0.34 switch would have;
  - an outbox row without a server can only be picked when its owner is the
    active one, and the owner names the server, so it's sent to the active
    server.

### Frontend runtime

- `AuthState` gains `server`.
- `accounts` is a Svelte store of `AccountsState`, read from
  `/__auth/accounts` and followed on `elyra:accounts`.
- `auth.switchTo(server)`, `auth.signOutOf(server)`,
  `auth.signOutOfDiscarding(server)`, `auth.signOutAll()`,
  `auth.signOutAllDiscarding()`, through `/__auth/*` (`Capability::Auth`).

Signing *in* to a server stays Rust-only, as in RFC 0021: the page can move
between servers the user signed in to, and leave them, never point a token
at a new host.

### Testing

`Secrets` implements `TokenStore`, so a test can keep the tokens in
`Secrets::fake()` and look at them there. `framework/tests/multiple_servers.rs`,
with `HttpFake` answering both servers and a `PusherFake` per server:

- two servers signed in, each token under its server, the list and the
  active one kept, one `elyra:auth` per change;
- a switch that revokes nothing, and requests routed by the active server
  with its token;
- a restart with both, the last active one active;
- sign-out of an inactive server (only its token revoked, no `elyra:auth`),
  of the active one (the other becomes active), and of all;
- a `401` on one server, the other carrying on;
- a single-server backend unchanged, and a 0.34 sign-in becoming a list;
- broadcasting and presence moving to the active server's Reverb, with its
  key and token;
- each server answered offline from its own copy, across switches, and a
  sign-out dropping only its copy;
- a queued write tagged with its server, not sent while another is active,
  sent to its own server with its token when that's active again;
- `PendingChanges` and `sign_out_of_discarding` with its count and
  `OutboxDropped`;
- 0.34 tables gaining their column, and an old row still sent.

## Alternatives considered

- **One `Backend` and `Auth` per server**, picked per request. RFC 0021
  turned this down for workspaces, and the reasons hold: every
  `ctx.get::<Backend>()` would have to know which, and the frontend's stores
  are single. The user works in one server at a time.
- **Several servers by default** for `chosen_at_sign_in()`. It changes
  what a 0.34 app sees (no refusal, a list), and nothing says such an app
  has a switcher.
- **Keeping the user value per server in the keychain**, so `accounts()`
  has names at start. RFC 0021's reasons again: Windows' 2.5 KB, and
  staleness. An app that wants names before a server is active keeps a
  summary itself (Félagi does, in its store).
- **Reading inactive servers' users in `accounts()`** (async). A list that
  makes requests to every server each time it's shown, and fails partly
  offline. Left to the app.
- **Sending inactive servers' writes in the background.** See above.

## Implementation plan

1. `Backend::multiple_servers()`; requests capture server, token and owner;
   `request_as`; `replay` to a given server with a given token; the copy,
   the connection state and the `401` keyed on what the request captured.
2. The tables' `server` column and its migration; the copy's per-server
   forget and keep-only; the outbox's per-owner count and discard,
   `OutboxDropped`.
3. `Auth`: the list, restore, `accounts`, `switch_to`, `sign_out_of`,
   `sign_out_all`, the `_discarding` forms, `sign_out` as the active
   server's, a `401` moving to the next server, `elyra:accounts`,
   `AuthState.server`, `token_for` for the outbox.
4. `/__auth/*` ops and the runtime's `accounts` store and `auth` methods.
5. Tests, `docs/backend.md`, `docs/frontend-runtime.md`, the changelog.

## As built

As designed. Notes:

- **The `401` guard** (sign out only for the token requests carry now) also
  closes a 0.34 gap: a request in flight across a same-server switch that
  came back `401` signed the *new* token out.
- **`sign_out_discarding()`** now also dispatches `OutboxDropped` when it
  drops writes.

## Open questions

1. **Inactive servers' writes in the background**: worth it once the events
   carry their server and the outbox list can show other servers' refused
   writes?
2. **The order of the list**: sign-in order (as built), or most recently
   active first, so signing out of the active one goes back to the last one?
3. **A `401` on an inactive server** is only noticed when it's active
   again. A periodic check of each server's `user_route`?
