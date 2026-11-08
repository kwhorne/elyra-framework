# RFC 0031 — A token per workspace

**Status:** implemented (2026-11-08) · **Target:** 0.45.0 · builds on [RFC 0021](0021-backend-chosen-at-sign-in.md) and [RFC 0022](0022-multiple-servers.md) · see [Decisions](#decisions)

## Summary

On some servers a token names one workspace: Félagi issues a token per
workspace a user approves, and a user in three workspaces holds three.
`Auth` kept one token per server, so an app with workspaces kept the rest
itself — in its own settings and keychain entries, beside the backend's —
and had to keep the two in step: which workspace's token the backend held,
the commands that ran before it held any, and a `401` that ended one
workspace, not the server. Félagi Desktop's `client.rs` and `session.rs`
are that glue, some 400 lines. Now `Auth` keeps a token per workspace:

```rust
Backend::chosen_at_sign_in()
    .multiple_servers()
    .user_route("/api/v1/me")
    .token_per_workspace("workspace.id")
```

```rust
let auth = ctx.get::<Auth>();
// Each workspace approved at sign-in, its token and who the user is there:
for w in approved.workspaces {
    auth.sign_in_with_token(&server, &w.token, me(&user, &w)).await?;
}
auth.switch_workspace(&server, "12").await?;     // no revoke, no sign-out
auth.sign_out_of_workspace(&server, "7").await?; // that token, revoked
let unread = auth.request_in(&server, "7", Method::Get, "/api/v1/inbox")?.json().await?;
```

## Design

### What's kept

`token_per_workspace(path)` says where the workspace's id is in the user
value (`workspace.id`), as `offline_scope_path` does for the offline copy;
it also keeps the copy and the outbox per workspace. Each
`sign_in_with_token` whose user has a workspace keeps, beside what it kept
before:

- the token under `backend-token:<server>#<workspace>`;
- the user value under `backend-workspace:<server>#<workspace>`: who the
  user is there, the workspace's name with it, as the server described
  them. Unlike the server's user (RFC 0021), it's kept across restarts, so
  the app's workspace switcher and the channels named from the user are
  there at a start, before the server is asked;
- the workspace among the server's, `backend-workspaces:<server>`.

`backend-token:<server>` stays the token in use there: everything that
reads it — a start, `switch_to`, the outbox — carries on as it was. A
sign-in by another user on the same server forgets the workspaces of the
user before.

### What it does

- **`Account.workspaces`**: each server's, with its user value, whether
  it's the one in use, and its outbox's counts. `elyra:accounts` carries
  them.
- **`switch_workspace(server, workspace)`**: that workspace's token in use,
  on the active server or another (which becomes active), as a
  `sign_in_with_token` with the token kept for it: nothing revoked, the
  broadcaster reconnected, the abilities read again, live queries re-run,
  each workspace's offline copy its own.
- **`sign_out_of_workspace(server, workspace)`**: its token revoked (with
  itself) and forgotten. The one in use: the server's next workspace takes
  its place. The last on the server: `sign_out_of(server)`. Refused with
  `PendingChanges` while its writes wait.
- **`sign_out_of(server)`** revokes every workspace's token there.
- **A `401`** ends the workspace whose token was refused, not the server:
  the server's next workspace is in use, `elyra:auth` says so once
  (`signedIn: true`, its user), and `elyra:accounts` names the workspace
  left (`signedOutOfWorkspace`) and why (`expired`). The last workspace on
  the server: as before.
- **`request_in(server, workspace, method, path)`**: a request with that
  workspace's token, to ask a workspace that isn't in use (what's unread
  there). Like the outbox's requests to a server that isn't active, a
  `401` there signs nothing out, and nothing is kept offline.
- **`update_user(user)`**: the user as the server describes them now (a
  profile changed, a language chosen), kept as the workspace's. Reading
  `user_route` again keeps it too.

## Decisions

1. **The user value says the workspace.** A token that names one workspace
   comes with a user that does (`/api/v1/me`); `token_per_workspace` reads
   it where `offline_scope_path` reads the copy's. Nothing new for the app
   to pass.
2. **`sign_in_with_token` per workspace,** not one call with every token.
   A sign-in that approves three workspaces is three calls, the one to open
   last; each is what a single sign-in was, and a server that adds a
   workspace later adds one.
3. **The user value in the keychain.** `Auth` has the token store and
   nothing else; each workspace's is one entry, under the 2.5 KB Windows'
   Credential Manager takes. It's no secret, but it's who the user is, and
   it goes where the token goes.
4. **`backend-token:<server>` is still the one in use,** a copy of a
   workspace's. A start, a switch of server and the outbox didn't change.
5. **A `401` from `request_in` signs nothing out.** The app asked a
   workspace on the side; it decides (`sign_out_of_workspace`).

## Not in this RFC

- Discarding a workspace's waiting writes when leaving it (the server's
  `sign_out_of_discarding` covers all of them).
- A password sign-in (`sign_in`) with workspaces: it gets one token, as
  before; workspaces come from `sign_in_with_token`.

## Testing

`framework/tests/workspaces.rs`, against an `HttpFake` and the fake
keychain: three workspaces kept from three sign-ins and listed with their
users; a switch revokes nothing and requests carry the new token; a
restart brings back every workspace, the one in use and its user, before
the server is asked; leaving one revokes its token alone; leaving the one
in use moves to the next; leaving the last signs out of the server; a
`401` ends that workspace and the next carries on, and the last one's
ends the server; a switch to a workspace on another server makes it
active; `request_in` carries that workspace's token and a `401` there
signs nothing out; another user on the server forgets the first one's
workspaces; signing out of the server revokes every workspace's token.
Then Félagi Desktop, ported against it before the release: its own
session store, the `Adopted` map, `adopt`/`follow`, the retry before the
token was adopted and its own `401` rule went, and its sessions from
before are handed to `Auth` once (`sign_in_with_token` per workspace). It
found one thing missing — a profile changed on the server had nowhere to
go — which became `update_user`.
