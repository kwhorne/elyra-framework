# RFC 0042 — Follow a colleague: go where they go, without sharing a screen

**Status:** accepted (2026-11-20) · **Target:** 0.52.0 · builds on [RFC 0015](0015-presence.md) and [RFC 0016](0016-window-capabilities.md) · see [Decisions](#decisions)

## Summary

Two colleagues go through the overdue invoices together on the phone: one
clicks, the other says "which one?", and they share a screen to stop
saying it. A desktop app that already knows who else is here (RFC 0015)
can do better: **click a colleague's avatar and follow them**, and your
window goes where theirs goes, as yourself, with your own abilities, and
stops the moment you go somewhere on your own.

```rust
Backend::new(url)
    .broadcasting(Broadcasting::pusher(key))
    .presence(["office", "customers.*"])        // `office`: who's in the app at all
```

```svelte
<Toolbar title="CRM">
  {#snippet actions()}<Presence channel="office" follow />{/snippet}
</Toolbar>
```

- **Click Ada's avatar → Follow.** Ada's app asks her first: "Grete wants
  to follow you", **Allow** or **Don't**.
- **Allowed, Grete's window goes where Ada's goes**: Ada opens customer
  42, Grete's window opens customer 42; Ada switches to its invoices tab,
  so does Grete. A bar says "Following Ada", with **Stop**.
- **Grete sees what Grete may see.** Only the route travels; her window
  reads the record with her own token and abilities. A route she may not
  see shows her what it shows anyone without it.
- **Either can stop it**: Grete by Stop or by going somewhere herself, Ada
  by **Stop** on "Grete is following you".

## Motivation

- **Going through things together is a large part of office work**:
  training a new colleague, support helping a user, two people on a call
  over a list of cases. Screen sharing does it, badly: one screen, a
  remote mouse, and the follower can't scroll or look closer.
- **The app knows the route, and presence knows who's here.** RFC 0015
  already joins Laravel presence channels both ways between the web and
  the desktop, and whispers "editing" on them. A route is one more
  whisper.
- **It's private by nature, so it needs consent.** Where someone is in an
  app says what they're working on. Following must be asked for, visible
  while it lasts, and easy to end from both sides.

## Current state (0.49)

- **Presence** (RFC 0015): `Backend::presence([..])` lists the channels a
  window may join; `<Presence channel>` and `presence(channel)` join one
  while shown; Laravel's channel rule decides who's in and what's shown of
  them.
- **Whispers**: `editing` is sent as Echo's `client-editing` on the
  channel; Reverb names the sender (`user_id`) when client events are
  accepted from members (its default). Askr's `--pusher` doesn't pass
  whispers on.
- **The router**: each window has its route (`$route`), and `navigate`
  moves it.

## Design

### Asking

Following happens on a presence channel both are in. An app-wide one
(`office`, or `workspace.{id}`) lets anyone follow anyone in the app; a
record's channel only while both have the record open.

1. Grete chooses **Follow** on Ada's avatar. Her app whispers
   `client-follow-ask { to: "<Ada's id>" }`.
2. Ada's app shows, in its focused window, "Grete wants to follow you"
   with **Allow** and **Don't** (the runtime's dialog, as `confirm`).
   Nothing about Ada's route is sent until she allows.
3. **Allow** whispers `client-follow-yes { to: "<Grete's id>" }` and
   Ada's route at once; **Don't**, `client-follow-no`. No answer in a
   minute is a no.

A whisper reaches every member of the channel, so each says who it's for
(`to`), and an app acts only on what's for its own user and from a member
the server named.

### Following

- **Ada's app sends her route** as `client-route { route }` while anyone
  follows her: when it changes (at most every 250 ms), and every 30
  seconds, so a follower who reconnects catches up. It's the route of
  Ada's focused window, as its router has it. Nothing is sent while nobody
  follows her.
- **Grete's window navigates** to each route from Ada, as `navigate` does,
  with the same rule as RFC 0041's Quick Look: only one of the app's own
  routes (`/…`), never a URL.
- **A bar in Grete's window** says "Following Ada" with **Stop**; a small
  ring on Ada's avatar says the same in `<Presence>`.
- **Ada sees it**: "Grete is following you" in her app (a bar in her
  focused window, and on Grete's avatar), with **Stop**.

### Stopping

Following ends, for both, when:

- **Grete** presses Stop, or navigates herself (a link, the back button,
  ⌘K, a command that navigates). Only a route that came from Ada keeps it
  going.
- **Ada** presses Stop (`client-follow-stop`).
- **Either leaves the channel**: closes the window, signs out, goes
  offline. The presence channel's member list says so.

### Which routes travel

All of the app's routes by default. An app keeps some to itself:

```js
// app/src/main.js
import { following } from "@elyra/runtime";

following.share((route) => !route.startsWith("/settings"));
```

A route not shared isn't sent; Grete's bar says "Ada is somewhere you
can't follow" until Ada comes back to one that is.

### In the frontend

```ts
import { presence, following } from "@elyra/runtime";

const office = presence("office");
await office.follow(member.id);     // asks; resolves true when allowed
$following;                         // { leader, channel } | null: whom this window follows
$followers;                         // the members following this user
following.stop();                   // as Stop does
following.stopFollowers();          // as Ada's Stop does
```

`<Presence follow>` wires it: a click on an avatar shows **Follow**, the
ring, and the bars come with it.

### On the web

The web half takes part with Echo alone: the same whispers on the same
presence channel. A browser following a desktop user works where the two
apps share their routes (`/customers/42` on both); where they don't, the
web app maps them, or doesn't take part. `elyracode/elyra-backend`'s Boost
guideline says what to whisper. The desktop-to-desktop case needs nothing
from the web.

### Security and privacy

- **Nothing is sent before the leader allows it**, and only to the
  channel's members, who Laravel's channel rule let in.
- **Only the route travels**: never a record, a form's contents or a
  window's picture. What the follower's window shows, it reads as the
  follower; a route she may not see is refused as it would be if she typed
  it.
- **A whisper is acted on only from a member the server named**, as for
  `editing`; a route from anyone else is ignored.
- **The follower can't make the leader do anything**: following is
  one-way, and the leader's app never navigates on a whisper.
- **Askr's `--pusher`** doesn't pass whispers on, so following isn't
  offered there; `<Presence follow>` leaves out Follow.

### Testing

- **The whispers**: ask, yes, no, no answer, stop from either side, with
  `PusherFake` as the presence tests already use; a whisper for another
  user, or from someone the server didn't name, ignored.
- **The follower**: navigates to each route, ignores a URL, stops on its
  own navigation.
- **The leader**: sends nothing unasked, its route on each change and
  every 30 s while followed, and nothing for a route `share` says no to.
- **Against Laravel and Reverb** in CI's backend job, two apps on the
  fixture's `customers.{id}` channel.

## Implementation plan

1. The whispers in the shell's presence: ask, answer, route, stop; who
   follows whom, per channel; the timeouts.
2. `@elyra/runtime`: `follow`, `following`, `followers`, `share`, the
   navigation and its stopping.
3. `<Presence follow>`, the bars, the ring.
4. Docs (`docs/backend.md`, `docs/ui/presence.md`), the Boost guideline's
   whispers for the web half.

## Not in this RFC

- **Following inside a page**: scroll position, an open tab that isn't
  in the route, a form's fields. Only the route.
- **Leading a group** ("everyone follow me"): one follower asks at a
  time; several may follow the same person.
- **A cursor or pointer** on the other's screen.

## Alternatives considered

- **Screen sharing**, as now: no app work, but one screen, a remote
  mouse, and a follower who can't look closer.
- **A shared link** ("open this"): one place at a time, by hand.
- **Following without asking**, visible to the leader. Simpler, and
  wrong: where someone is in an app is theirs to share.

## Decisions

Settled on 2026-11-20, all as recommended:

1. **The leader is asked**, Allow or Don't; nothing of their route is sent
   before they allow it.
2. **Any presence channel the app lists**; the docs show an app-wide one in
   the toolbar.
3. **The follower's own navigation ends it**: only a route from the leader
   keeps it going.
4. **All of the app's routes are shared** by default; `following.share(fn)`
   keeps some back.
5. **Target 0.52.0.**
