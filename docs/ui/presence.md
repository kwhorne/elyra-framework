# Presence

Who else has this record open, on the web or in the desktop app
([RFC 0015](../proposals/0015-presence.md)). Presence joins a Laravel
presence channel while it's on screen, and shows the other members as an
[AvatarGroup](avatar.md). A browser tab joining the same channel with
Echo shows up here, and this window shows up there.

<!-- example: presence/basic -->
```svelte
<script>
  import { Card, Presence } from "@elyra/ui";

  const id = 42;
</script>

<!-- Ada and Linus have customer 42 open in the browser; the reader isn't shown. -->
<Card title="Ada Lovelace AS">
  {#snippet actions()}<Presence channel={`customers.${id}`} />{/snippet}
  <p class="note">Credit limit 50 000 kr · last order 31 October</p>
</Card>

<style>
  .note { margin: 0; color: var(--elyra-muted); }
</style>
```

- **`channel`** is the name without `presence-`: `customers.42`. The app
  must list it in `Backend::presence(..)` (`customers.*`), and Laravel's
  `routes/channels.php` decides who may join and what each member shows.
- **The user isn't shown**, since they know they're there. `self` shows
  them too.
- **Nothing shows** while nobody else is there, when the app is offline or
  signed out, or in an app without presence.
- **Joined once** however many windows show it, and left a moment after
  the last one goes, so moving between a record's tabs doesn't leave and
  join again.

## Setting it up

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))
        .presence(["customers.*"]),
)
```

```php
// routes/channels.php: the member's info is what the others see.
Broadcast::channel('customers.{id}', function ($user, $id) {
    return $user->can('view', Customer::find($id))
        ? ['id' => $user->id, 'name' => $user->name, 'avatar' => $user->avatar_url]
        : false;
});
```

Keep the info to a name and, if there is one, an avatar URL: everyone in
the channel gets it. The runtime's `presence(channel)` store has the
members for a view of your own: `$here.members`, `$here.me`,
`$here.joined`. See [the backend guide](../backend.md#who-else-is-here-presence).

## Accessibility

- It's an AvatarGroup: a `role="group"` named by `label` ("Also here"),
  each person an image named by their name, and the rest counted and
  named.

## API

### Presence

| Prop | Type | Default | Description |
|---|---|---|---|
| `channel` | `string` | — | The presence channel, without `presence-`: `customers.42`. |
| `self` | `boolean` | `false` | Show the signed-in user too. |
| `max` | `number` | `4` | Places, the count of the rest included. |
| `size` | `"sm" \| "md" \| "lg" \| number` | `"sm"` | Each avatar's size. |
| `label` | `string` | `"Also here"` | The group's name for a screen reader. |
| `class` | `string` | `""` | |

Other attributes go to the AvatarGroup's `<div>`.
