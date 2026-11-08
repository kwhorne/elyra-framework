# NotificationBell

Laravel's notifications, behind a bell
([RFC 0008](../proposals/0008-laravel-notifications.md)). The unread count
is on the bell. A click opens a panel with the newest ones, Mark all read,
and a link to the page with all of them. It follows the runtime's
`notifications` store, so one read on the web is read here too, and one
that arrives shows at once.

<!-- example: notification-bell/basic -->
```svelte
<script>
  import { NotificationBell, Toolbar } from "@elyra/ui";
</script>

<!-- Two unread: an invoice paid, and a mention. A click on one opens its page in the app. -->
<div class="window">
  <Toolbar title="CRM">
    {#snippet actions()}
      <NotificationBell href="/notifications" />
    {/snippet}
  </Toolbar>
  <div class="page"></div>
</div>

<style>
  .window { width: 100%; border: 1px solid var(--elyra-border); border-radius: var(--elyra-radius-lg); }
  .page { height: 360px; }
</style>
```

The notifications here are made up. Reading one marks it until you reload
the page. In an app they're the signed-in user's, from
`$user->notify(..)` on the server.

- **Each one shows** its data's `title`, `body` and `url`: the same
  convention the shell uses for the OS notification. A notification
  without a `title` is "Notification". When your notifications say it
  differently, `present(n)` returns `{ title, body, url }` from the
  notification, as `.present_notifications(..)` does on the Rust side.
- **A click** marks it read and opens its `url` when it's a path in the
  app (`/invoices/2041`). A full URL isn't opened. To do something else
  (open it in a new window, or open a full URL in the browser), pass
  `onopen(n, presented)`. The bell then hands every click to it.
- **Unread** ones have a dot and a bold title. When it was sent shows as
  "4 minutes ago", "yesterday", or a date after a week. The exact time is
  in its tooltip.
- **Mark all read** and **More** (when there are more than the first
  page) go through the store. If one fails, the panel says why.
- **`href`** adds "See all" at the bottom, a link to the page with all of
  them: `/notifications`, the page
  [`rata make:notifications`](../backend.md#rata-makenotifications)
  writes.
- **Offline** it shows the list as it was, and says that reading one is
  sent once the app is back online.
- **Nothing shows** with a runtime that has no notifications store.

## Setting it up

The backend needs `.notifications()`, and Laravel the `notifications`
table and routes. See
[Laravel's notifications](../backend.md#laravels-notifications).

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))
        .notifications(),
)
```

The bell doesn't show a toast for one that arrives while the app is in
front, since it may not be on screen. `rata make:notifications` sets that
up for the whole app, and so does this, once:

```js
import { onNotification, toast } from "@elyra/runtime";
onNotification((e) => e.shown === "toast" && toast(e.presented.title));
```

## Accessibility

- The bell is a button named by `title`, with the count in its name:
  "Notifications, 2 unread". The badge on it is hidden from a screen
  reader, since the name says it.
- The panel is a non-modal `role="dialog"` named by its heading. Focus
  moves in when it opens. Esc closes it and focus goes back to the bell.
  A click outside also closes it.
- Each notification is a button, with its title, body and time as its
  name.

## API

### NotificationBell

| Prop | Type | Default | Description |
|---|---|---|---|
| `open` (bindable) | `boolean` | `false` | Whether the panel is open. |
| `title` | `string` | `"Notifications"` | The panel's heading and the bell's name. |
| `href` | `string` | — | The page with all of them, linked as "See all". |
| `present` | `(n) => { title, body, url }` | — | How one is shown, when its data isn't `title`, `body` and `url`. |
| `onopen` | `(n, presented) => void` | — | Takes a click on one, instead of opening its path in the app. |
| `align` | `"start" \| "end"` | `"end"` | Lines the panel up with the bell's left or right edge. |
| `labels` | `{ unread?, markAll?, more?, all?, empty?, offline?, untitled?, justNow? }` | `{}` | Its words, over the English: `unread(n)` ("2 unread"), "Mark all read", "More", "See all", "Nothing yet", the offline note, "Notification", "just now". |
| `class` | `string` | `""` | Added to the outer element. |

Other attributes go to the outer `<div>`.
