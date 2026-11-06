# RFC 0023 — Private channels a window listens to

**Status:** implemented (2026-11-06) · **Target:** 0.36.0 · builds on [RFC 0006](0006-laravel-broadcasting.md), [RFC 0015](0015-presence.md) and [RFC 0022](0022-multiple-servers.md) · see [Decisions](#decisions)

## Summary

An agent's run is shown live in a window: its output as it's written, its
status as it changes. Laravel already broadcasts both, on a private channel
per run, `private-task.{ulid}`. The desktop app can't hear it: the channels
it follows are fixed when the app is built (`follow`, `follow_user`), and
the only channels a window can join while it shows something are presence
channels. So the window polls every two seconds.

This RFC lets a window listen to a private channel while it shows what the
channel is about, the way RFC 0015 lets it join a presence channel:

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .broadcasting_from("/api/desktop/broadcasting")
        .listen(["task.*"]),                 // the private channels a window may listen to
)
```

```svelte
<!-- RunLive.svelte -->
<script>
  import { listen, listening } from "@elyra/runtime";
  let { run } = $props();
  let output = $state("");
  let status = $state(run.status);

  // listen(..) returns its unsubscribe: the effect's cleanup.
  $effect(() => listen(`task.${run.id}`, (data, e) => {
    if (e.event === "RunOutput") output += data.chunk;
    if (e.event === "RunStatusChanged") status = data.status;
  }));
  const live = $derived(listening(`task.${run.id}`));
</script>

{#if !$live.joined}<small>Not live</small>{/if}
<pre>{output}</pre>
```

```php
// routes/channels.php: the rule the web already uses with Echo.private(..)
Broadcast::channel('task.{task}', fn ($user, Task $task) =>
    $user->workspaces->contains($task->workspace_id));
```

## Motivation

- **Polling is late and wasteful.** Two seconds behind at best, a request
  per open run every two seconds at worst, and the server already pushes
  the same thing to the browser.
- **A channel per record is the normal shape.** Laravel's model
  broadcasting puts each model on `private-App.Models.Task.{id}`, and
  hand-written events often do the same. An app can't list those channels
  when it's built; it knows which one it wants when a window opens a
  record.
- **Presence already solved the lifecycle.** Joining per window, once for
  all windows, rejoining after a reconnect or a switch, dropping a closed
  window's joins: all of it exists for presence channels. Private channels
  need the same, without members.

## Current state (0.35)

- `Backend::follow(name, resource)`, `follow_public(..)` and
  `follow_user(name, template, resource)` are subscribed at each
  connection, for every window, and fixed when the app is built. Their
  events are `BackendEvent`s on `elyra:backend`, for every window, and
  domain events.
- `Backend::presence([patterns])` lets a window join a presence channel
  matching a pattern, through `/__presence/*`, per window and refcounted,
  through a command queue on the open connection.
- A private follow is authorized at the broadcasting auth route as the
  user. A refusal there drops the connection and retries it, since a
  follow the app was built with is expected to be allowed.

## Design

### In Rust

- **`Backend::listen(patterns)`** lists the private channels a window may
  listen to: names or namespaces (`task.*`), without `private-` (stripped
  if given). The same pattern rules as presence: a valid Pusher name that a
  pattern covers. A name outside them is refused (`denied`) before
  anything is sent.
- **`Listening`** (`elyra::listening::Listening`, bound in the container)
  keeps the listened channels, per window with when each started, and which
  the server took. It's the presence code without members or whispers:
  `listen(window, channel)`, `unlisten(window, channel)`, `joined(channel)`,
  `channels()`, `state(channel)`.
- **On the connection.** The session gets a second command queue: a
  channel's first window subscribes it, its last one unsubscribes it, and
  a new connection subscribes every channel still listened to. Each
  subscription is authorized at the broadcasting auth route, as the user,
  exactly as a private follow is.
- **A refusal isn't a dropped connection.** Laravel's `403` (or any answer
  other than no answer at all), an answer without `auth`, or the server's
  `subscription_error` leaves that channel unjoined, logged, and the
  connection goes on. No answer at all is a dropped connection, retried, as
  for presence.
- **Events reach the windows listening, only.** An event on a listened
  channel is a `BackendEvent { channel, event, data }` on the channel's
  app name (`task.01JD…`), sent with `emit_to` on `elyra:backend` to each
  window that listens. It isn't a domain event and re-runs no live query:
  the window asked for it, the window gets it.
- **Whether it's joined** goes to the windows listening on
  `elyra:listening`, `{ channel, joined }`: `true` on
  `subscription_succeeded`, `false` when it's refused or the connection
  ends.
- **A channel the app follows too** isn't subscribed twice, nor
  unsubscribed when the last window stops: its events already reach every
  window.

### In the runtime

```ts
import { listen, listening } from "@elyra/runtime";

const stop = listen("task.01JD…", (data, e) => …);            // every event
const stop = listen("task.01JD…", "RunOutput", (data) => …);   // one event
const run = listening("task.01JD…");                           // a store: $run.joined, $run.last
```

- `listen` returns an unsubscribe; the handler gets `(data, event)`, as
  `onBackendEvent`'s does.
- `listen` and `listening` share one subscription per channel in a window,
  and the app one per channel for all windows. The first holder listens,
  the last one stops 300 ms later, so a quick unmount and mount doesn't
  unsubscribe and subscribe again.
- A refused name is logged to the console; the handler hears nothing.
- A throwing handler is logged, and the others still hear the event.

## Security

- **Laravel decides who listens.** The broadcasting auth runs the
  channel rule as the signed-in user, with their token, for this socket. A
  `false` is a refused subscription.
- **The app limits what it asks for.** `Backend::listen(..)` patterns are
  checked before anything is sent, so a window (or a compromised page in
  it) can't use the connection to probe channel names.
- **`Capability::Listen`** covers `/__listen/*`, granted by default. A
  window limited by RFC 0016 needs it to listen.
- **Events are data.** The `data` is the server's, untrusted like any
  input, and it only reaches the windows that asked.

## Alternatives considered

- **`follow_user` with a template per record.** Fixed per connection, and
  every window would hear every run, not the one it shows.
- **A presence channel per run.** Works with presence today, but turns a
  private channel into a presence one on the server, sends member lists
  nobody wants, and changes the web half.
- **Reusing `/__presence/*` with a `private` flag.** One route fewer, but
  presence's state (members, `me`, editing) means nothing here, and a
  separate capability lets a window be allowed one and not the other.
- **Events on their own channel** (`elyra:listen`) rather than
  `elyra:backend`. The shape is the same `BackendEvent`; reusing
  `elyra:backend` means `onBackendEvent` works for a listened channel too.

## Implementation plan

1. `Listening` with its unit tests; `Backend::listen(..)`; the container
   binding, with a panic at start without broadcasting.
2. The session's second command queue, `join_listened` through the auth
   route, `subscription_succeeded` and `subscription_error`, and events to
   the listening windows; the `Detach` guard detaches it.
3. `/__listen/join` and `/__listen/leave`, `Capability::Listen`, and
   `/__inspector/listening`.
4. `listen`, `listening` and the inspector's Listening section in the
   runtime; docs.

## As built

- **`elyra::listening::Listening`** (re-exported as `elyra::Listening`),
  bound when `Backend::listen(..)` lists anything; without broadcasting
  that's a panic at start, as for presence.
- **Closed windows.** The runtime stops listening on `pagehide` (a
  `keepalive` request), and a sweep every 30 s lets go of windows the event
  bus no longer sees polling, after a minute's grace. `Listening::sweep(grace)`
  runs it, for a test.
- **Lifecycle, as presence's.** Signed out (the broadcaster stopped),
  offline or unplugged in the inspector, there's no connection, so nothing
  is joined; listens made meanwhile wait. A reconnect, a token switch
  (`sign_in_with_token` while signed in) and a server switch (RFC 0022,
  `switch_to` or signing in to another server) each start a new
  connection, which subscribes every listened channel again, authorized by
  that server with that token.
- **The inspector** has a Listening section, through
  `/__inspector/listening`: each channel, whether it's joined, how many
  windows listen, and whether the asking window is one. Hidden in an app
  without `listen`.
- **Tests:** `framework/tests/listening.rs` against `PusherFake` and
  `HttpFake` (the auth and the socket, one subscription for two windows,
  the last one unsubscribing, refusal outside the patterns, a reconnect, a
  sign-out, a token switch, Laravel's `403`, a closed window swept, a
  followed channel, the inspector), `listened_channels_move_to_the_active_server`
  in `framework/tests/multiple_servers.rs`, and `runtime/src/listen.test.ts`.

## Open questions

1. **Name:** `Backend::listen` / `listen(..)` (recommended: Echo's verb,
   `Echo.private('task.1').listen('RunOutput', ..)`), or something closer to
   `follow`?
2. **Domain events:** should a listened channel's events also be
   `App::listen(|e: BackendEvent, ..|)` events (recommended: no, a window
   asked for them, and a run's output is many events a second)?
3. **Encrypted private channels** (`private-encrypted-…`): in this RFC, or
   later (recommended: later, when an app needs one)?
4. **Catching up:** events sent while unjoined are lost. Should the runtime
   re-read for the app when `joined` comes back (recommended: no, the app
   knows what to read; `listening(..).joined` tells it when)?

## Decisions

Built as recommended, to be confirmed before 0.36.0:

1. **`Backend::listen(..)` and `listen(..)`**, after Echo.
2. **No domain events** for a listened channel; `onBackendEvent` hears it
   in the windows that listen.
3. **Encrypted private channels later.**
4. **No catching up in the runtime.** `$listening.joined` going back to
   `true` is the app's cue to read again.
