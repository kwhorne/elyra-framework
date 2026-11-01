# RFC 0015 — Presence: who else has this record open, on the web or the desktop

**Status:** implemented (2026-11-01) · **Target:** 0.26.0 · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0006](0006-laravel-broadcasting.md) and [RFC 0012](0012-elyra-ui.md) · see [Decisions](#decisions)

## Summary

Two people open the same customer, one in the browser and one in the
desktop app. Today neither knows until one of them saves and the other gets
a `409 Conflict`. Laravel already has the answer: a presence channel, which
Echo joins in the browser and which tells every member who else is there.
This RFC has the desktop app join the same channels, so both halves see
each other:

```svelte
<!-- Show.svelte -->
<script>
  import { Presence } from "@elyra/ui";
</script>

<Card title={record.name}>
  {#snippet actions()}<Presence channel={`customers.${id}`} />{/snippet}
  …
</Card>
```

```php
// routes/channels.php: the same rule the web uses with Echo.join(..)
Broadcast::channel('customers.{id}', fn ($user, $id) =>
    $user->can('view', Customer::find($id)) ? ['id' => $user->id, 'name' => $user->name] : false);
```

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))
        .presence(["customers.*"]),          // the channels a window may join
)
```

Ada has customer 42 open in the browser. Grete opens it in the desktop app.
Each sees the other's avatar on the card within a moment, and it goes when
the other closes the record.

## Motivation

- **Conflicts are found too late.** `If-Unmodified-Since` (RFC 0007) catches
  a lost update at save time. Seeing the other person first avoids most of
  them.
- **It's the web and the desktop together.** A presence channel is shared by
  every client that joins it: a browser tab with Echo, a phone, the desktop
  app. Elyra joining the same channel is the whole feature; there is no
  second mechanism to build on the server.
- **The pieces are there.** The Pusher connection and its channel auth
  (RFC 0006), the signed-in user (RFC 0005), and `<Avatar>` / `<AvatarGroup>`
  (0.25).

## Current state (0.25)

- `Broadcaster` connects while the user is signed in and subscribes, per
  session, to a fixed list: the `follow(..)` channels and the user's own
  private channel. The list is known before the socket opens; nothing joins
  or leaves while it's up.
- Private channels are authorized through the backend's
  `/api/broadcasting/auth`, as the user, and the app secret stays on the
  server.
- `PusherFake` accepts subscriptions and broadcasts; it knows nothing of
  presence. CI runs the broadcasting tests against Reverb and Askr too.
- `@elyra/ui` has `Avatar` and `AvatarGroup`.

## Design

### The protocol

A presence channel is `presence-<name>`. Subscribing needs the auth
signature *and* `channel_data`, the member's `{ user_id, user_info }`,
both from Laravel's auth route; Laravel's `channels.php` callback returns
the `user_info`. The server then sends:

- `pusher_internal:subscription_succeeded` with `{ presence: { ids, hash, count } }`:
  everyone there, this member included.
- `pusher_internal:member_added` `{ user_id, user_info }` and
  `pusher_internal:member_removed` `{ user_id }` as they come and go.

A user with two tabs is one member: Pusher counts a `user_id` once.

### In Rust

- **`Backend::presence(patterns)`** lists the channel names a window may
  join (`customers.*`, `invoices.*`). A name outside them is refused before
  anything is sent. Laravel decides who may join; the list decides which
  channels the app asks about at all, as `follow(..)` does for private ones.
- **Joining while connected.** The session gets a command queue: `join`
  and `leave` frames go out on the open socket, and a reconnect joins every
  channel that's still wanted. A join while disconnected (or offline, or
  unplugged by the inspector) waits for the connection.
- **Counted per window.** A channel is joined once, however many windows
  want it, and left when the last one leaves or closes. The shell's
  `/__presence/join` and `/__presence/leave` `{ channel }` are per window,
  and a closed window leaves what it joined.
- **The members** of each joined channel are kept from the three internal
  events and sent on `elyra:presence` as `{ channel, members: [{ id,
  info }], me }`, where `me` is the signed-in user's id. They're cleared
  when the connection drops and refilled on rejoin.
- **Offline**, a channel has no members and says so (`joined: false`):
  nobody is shown rather than someone stale.

### In the runtime

```ts
import { presence } from "@elyra/runtime";

const here = presence("customers.42");   // a Svelte store
$here.joined;                            // false until the server answers
$here.members;                           // [{ id, info: { name, avatar? } }], others first
$here.me;                                // the signed-in user's id
```

The first subscriber joins, the last one leaves. A component that mounts
and unmounts quickly doesn't join twice.

### In `@elyra/ui`

`<Presence channel>` is an `AvatarGroup` of the other members, by
`info.name` and `info.avatar`, labelled "Also here". It shows nothing while
nobody else is there, and nothing in an app without broadcasting.

### Laravel

Nothing new on the server beyond the channel rule. The `elyracode/elyra-backend`
package's docs and skill get the presence example, and what `user_info`
should hold: a name and, if there is one, an avatar URL, nothing more.

### make:resource

With broadcasting configured, `--view` could put a `<Presence>` on the
detail and edit views for `<plural>.{id}`, and print the `channels.php`
line. See the open questions.

## Security

- **Laravel decides membership.** The channel rule runs as the signed-in
  user; a `false` is a refused join, and the window gets no members.
- **The app limits what it asks for.** `Backend::presence(..)` patterns are
  checked before a join is sent, so a window (or a compromised page in it)
  can't use the connection to probe arbitrary channel names.
- **`user_info` is what the server chose to share.** The desktop app never
  sends a member's info itself; it comes from Laravel's auth answer. The
  docs say to keep it to a name and an avatar.
- **The inspector** can mask the ability the channel rule checks on the
  Laravel side only by the abilities it already masks; presence adds no new
  ability. When the backend is unplugged, nothing is joined.

## Alternatives considered

- **Polling a "who's viewing" endpoint.** Simple, but a second mechanism
  the web half doesn't use, a request per window per interval, and the
  server has to expire viewers itself.
- **Client events (`client-*` whispers)** for "typing…" and cursors. Useful
  later, and on the same channels, but not needed to see who's there.
- **Every window joins its record's channel automatically.** Less code in
  the views, but joins channels the developer never asked for, and needs a
  convention for which record a window shows.

## Implementation plan

1. `PusherFake` learns presence: `channel_data` on subscribe, the internal
   events, members per channel, and two clients seeing each other.
2. The session's command queue, joins and leaves while connected, rejoin on
   reconnect, per-window counting, `Backend::presence(..)`, the shell
   routes, `elyra:presence`. Tests against the fake, then the Laravel CI
   job against Reverb and Askr with a real channel rule.
3. `presence(channel)` in the runtime, `<Presence>` in `@elyra/ui`, docs.
4. `make:resource --view` (if decided), the skills, the inspector showing
   joined channels and their members.

## As built

- **`elyra::presence::Presence`** (re-exported as `elyra::Presence`) holds
  the wanted channels, per window with when each joined, and the members of
  each channel the server answered for. It's bound in the container when
  `Backend::presence(..)` lists anything; without broadcasting that's a
  panic at start, as for notifications.
- **The session's command queue.** Each connection makes an unbounded
  channel and hands its sender to `Presence::attach`, which answers the
  channels to join now. The read loop `select!`s between the next frame
  and the queue, so a join or leave goes out on the open socket. A guard's
  `Drop` calls `detach` however the session ends (dropped, stopped,
  unplugged), clearing the members and telling the windows.
- **Joining** posts the presence channel's name and the socket id to the
  backend's auth route as the user, and subscribes with Laravel's `auth`
  and `channel_data`. No answer at all is a dropped connection, retried;
  any other refusal (a `403` from the channel rule, a missing
  `channel_data`, `pusher:subscription_error`) leaves that channel
  unjoined and the connection up.
- **The members** come from `subscription_succeeded`'s `presence.hash`,
  then `member_added` / `member_removed`, kept by id (Laravel's ids may be
  numbers or strings; they're strings here). They go to the windows that
  joined, with `emit_to`, on `elyra:presence`.
- **Closed windows.** The runtime leaves on `pagehide` (a `keepalive`
  request), and a sweep every 30 s lets go of windows the event bus no
  longer sees polling, after a minute's grace for one that hasn't polled
  yet. `Presence::sweep(grace)` runs it, for a test.
- **`Capability::Presence`** covers `/__presence/*`, granted by default.
  A channel the list doesn't cover is `denied`.
- **The runtime's store** joins on the first subscriber and leaves 300 ms
  after the last, so a quick unmount and mount doesn't leave and join
  again. It ignores the event channel's replay of an older event, and a
  join answer arriving after the members doesn't set it back to
  "not joined".
- **`PusherFake`** keeps members per socket, sends the three internal
  events, handles `pusher:unsubscribe` and disconnects, and has
  `join_as` / `leave_as` for a browser tab elsewhere and `members(channel)`.
- **`make:resource --presence`** (with `--backend`) adds `<Presence>` to
  the detail view's card and to the edit form's (only for an existing
  record), and prints the `.presence(..)` and `channels.php` lines.
- **The Laravel CI job** (after 0.26) runs presence against Reverb
  (`tests/laravel_presence.rs`): the desktop app and a browser tab, a plain
  Pusher client joined as Echo joins, see each other come and go on a
  customer, through the rule docs/backend.md gives; a customer the rule
  refuses (Laravel's 403) leaves the window unjoined on the same
  connection. Not against Askr's `--pusher`.
- **The inspector** (after 0.26) lists the wanted channels in a Presence
  section, through `/__inspector/presence`: each channel, whether it's
  joined, its members with the user marked, how many windows want it, and
  whether the asking window is one. The section is hidden in an app
  without presence, and the route isn't in a release.

## Open questions

1. **Allow-list:** must a channel match `Backend::presence(..)` to be joined
   (recommended: the app says which channels it uses, as with `follow`), or
   may a window join any name and leave it all to Laravel?
2. **Scope:** are joins counted per window and left when it closes
   (recommended), or per app?
3. **Showing yourself:** does `<Presence>` leave the user out by default
   (recommended; `self` shows them), or show everyone?
4. **make:resource:** should `--view` add `<Presence>` to the detail and
   edit views when broadcasting is configured (recommended: yes, behind
   `--presence`, so an app without the channel rule isn't left with a
   component that never fills), always, or never?
5. **Whispers** ("typing…"): out of scope for this RFC (recommended), or in?

## Decisions

Settled on 2026-11-01, all as recommended:

1. **A channel must match `Backend::presence(..)`** to be joined. The app
   says which channels it uses, as with `follow(..)`; Laravel still
   decides who may join them.
2. **Joins are counted per window,** and a window leaves what it joined
   when it closes.
3. **`<Presence>` leaves the user out** by default; `self` shows them too.
4. **`make:resource --view --presence`** adds `<Presence>` to the detail
   and edit views (and prints the `channels.php` line). Without the flag,
   nothing.
5. **Whispers ("typing…") are out of scope** for this RFC.
