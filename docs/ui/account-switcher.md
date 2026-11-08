# AccountSwitcher

The servers the user is signed in to, in a menu
([RFC 0022](../proposals/0022-multiple-servers.md)): who they are on the
active one, the others a click away, and signing out of one or all. An
app whose backend has `.multiple_servers()` puts it at the top of the
sidebar. In an app with one server it's the user's menu: who's signed in,
and Sign out.

<!-- example: account-switcher/basic -->
```svelte
<script>
  import { alert } from "@elyra/runtime";
  import { AccountSwitcher, NavItem, Sidebar } from "@elyra/ui";
</script>

<!-- Ada works at Acme and for a customer, Kunde: one click moves between them. -->
<div class="window">
  <Sidebar>
    {#snippet header()}
      <!-- Signing in stays the app's: its own form opens from here. -->
      <AccountSwitcher onadd={() => alert("The app's sign-in form opens here.")} />
    {/snippet}
    <NavItem path="/customers">Customers</NavItem>
    <NavItem path="/invoices">Invoices</NavItem>
  </Sidebar>
</div>

<style>
  .window { display: grid; height: 380px; width: min(100%, 280px); border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); }
</style>
```

The accounts here are made up. Switching moves the check, and signing out
takes a server off until you reload the page. In an app they're
`$accounts` from `@elyra/runtime`.

- **The button** shows the user on the active server: their avatar and
  name, and the server's host when there's more than one.
- **Each server** is in the menu, with the user's name there and its host.
  The active one is checked. A click on another calls
  `auth.switchTo(server)`. Nothing is signed out, and what's on screen
  re-reads from the new server.
- **Sign out of acme.example.com** leaves the active server. The next one
  becomes active, and only the last leaves the user signed out. **Sign out
  of all** shows when there's more than one.
- **Changes not sent yet** show under each server: "3 waiting" for
  writes saved offline, "1 refused" for those its server turned down. See
  [below](#changes-waiting-on-each-server).
- **Signing out with changes not sent:** the server refuses the sign-out.
  The switcher then asks, in the runtime's dialog, whether to sign out and
  discard them ("3 changes made on acme.example.com haven't been sent
  yet"), and only does so if the user says yes.
- **`onadd`** adds "Add an account…". Signing in to a server is the app's
  own code, on the Rust side (`auth.sign_in_with_token(..)`): a page can
  move between the servers the user signed in to, never point a token at
  a new host. So the switcher asks the app, which opens its sign-in form.
- **When something fails** (switching to a server whose token just
  expired, say), it's said in a toast.
- **Nothing shows** while the user isn't signed in, or with a runtime
  before 0.35.

## Changes waiting on each server

With `.offline()`, each server has its own outbox: a write saved while
Kunde was unreachable waits for Kunde, and is sent only once Kunde answers
and is the active server. So the menu says, under each server, how many
are waiting and how many were refused, whether it's active or not. In the
example, Kunde has three waiting and one refused.

- **A refusal on another server** puts a red dot on the button, and "Changes
  refused on another server" in its name. The user can't deal with it until
  they switch there: the [OutboxDrawer](outbox-drawer.md) and the
  [StatusBar](status-bar.md) show the active server's outbox.
- **The active server's own** writes don't mark the button: the StatusBar
  already shows them.
- **Kept current.** The counts are the shell's: `$accounts.accounts[i].pending`
  and `.failed`, sent again whenever an outbox changes. A backend without
  `.offline()` has none to show.

## The name and picture

The user value is what each server's `/api/user` returned. The switcher
shows its `name`, or its `email`, or the server's host when there's
neither, and a picture from `avatar`, `avatar_url` or Jetstream's
`profile_photo_url`. At a start, a server that isn't active yet may not
have its user read, so it shows by its host until it's been active once.

Your users say it differently? `person(account)` returns
`{ name, src }`:

```svelte
<AccountSwitcher person={(a) => ({ name: a.user?.full_name ?? a.server, src: a.user?.photo })} />
```

## Setting it up

```rust
App::new().backend(
    Backend::chosen_at_sign_in()
        .multiple_servers()
        .offline(),
)
```

Without `.multiple_servers()` the list has the one server the user is
signed in to, and the switcher is a user menu with Sign out. See
[signed in to more than one server](../backend.md#signed-in-to-more-than-one-server).

## Accessibility

- It's a [Dropdown](dropdown.md): a button with `aria-haspopup="menu"`,
  and a `role="menu"` with ↓/↑, Home/End, Enter and Esc.
- Each server is a `menuitemradio`, the active one `aria-checked="true"`,
  named by the user's name and the host. The avatars are hidden from a
  screen reader, since the name is already there.

## API

### AccountSwitcher

| Prop | Type | Default | Description |
|---|---|---|---|
| `person` | `(account) => { name, src? }` | — | The name and picture for an account, when its user isn't `name` and `avatar`. |
| `onadd` | `() => void` | — | Adds "Add an account…", which calls it: the app opens its sign-in. |
| `variant` | `"default" \| "ghost"` | `"ghost"` | The button's look, as a Dropdown's. |
| `align` | `"start" \| "end"` | `"start"` | Lines the menu up with the button's left or right edge. |
| `labels` | `{ signOut?, signOutAll?, add?, pending?, pendingAll?, discard?, waiting?, refused?, attention? }` | `{}` | Its words, over the English: `signOut(host)`, "Sign out of all", "Add an account…", `pending(host, n)` and `pendingAll` (the question before discarding), "Sign out and discard", `waiting(n)` ("3 waiting"), `refused(n)` ("1 refused"), and `attention` (the button's name while another server has refusals). |
| `class` | `string` | `""` | Added to the Dropdown. |
