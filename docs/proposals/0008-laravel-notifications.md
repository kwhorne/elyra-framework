# RFC 0008 — Laravel's notifications on the desktop: in the OS, counted, read everywhere

**Status:** implemented (2026-10-13) · **Target:** 0.15.0 · the guide: [docs/backend.md](../backend.md#laravels-notifications) · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0006](0006-laravel-broadcasting.md) and [RFC 0007](0007-offline-and-sync.md) · see [Decisions](#decisions)

## Summary

Laravel already knows how to tell a user something: `$user->notify(new
InvoicePaid($invoice))`. With the `database` channel it's kept for the user to
read, and with `broadcast` it's pushed to the browser, where Echo's
`.notification(..)` hears it. In a browser tab that's a toast nobody sees,
because the tab is closed or buried. On the desktop it should be a
notification in the OS, a number on the dock icon, and a list in the app,
read once whichever half of the product the user reads it in.

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher(key))
        .notifications(),   // App.Models.User.{id}, and /api/notifications
)
```

```php
class InvoicePaid extends Notification
{
    public function via(object $notifiable): array
    {
        return ['database', 'broadcast'];
    }

    public function toArray(object $notifiable): array
    {
        return [
            'title' => 'Invoice paid',
            'body' => "Invoice {$this->invoice->number} was paid.",
            'url' => "/invoices/{$this->invoice->id}",
        ];
    }
}
```

- **It arrives** on Laravel's own channel for it, the private
  `App.Models.User.{id}`, which the app follows for whoever is signed in.
- **It's shown** as a native notification when the app isn't in front, and as
  a toast when it is. A click brings the window forward and opens `url`.
- **It's counted**: the unread number is on the dock icon (macOS), the
  launcher (Linux) or the taskbar (Windows), and in the tray's tooltip.
- **It's listed**: the `notifications` store holds the list from
  `/api/notifications`, the `database` channel's table. Offline, it's the
  last copy (RFC 0007).
- **It's read everywhere**: read on the desktop, it's read on the web; read
  on the web, the desktop's count goes down.

## Motivation

- **This is what a desktop app is for.** A web app can't put a number on the
  dock, can't reach the user while its tab is closed, and its browser
  notifications need a permission most users refuse. A desktop app runs in
  the tray all day; that's where "a customer replied" belongs.
- **Laravel has the model.** Notifications, their `database` and `broadcast`
  channels, `read_at`, and the user's private channel are all standard. The
  web half of the product most likely sends them already. Nothing new is
  needed on the server beyond a few routes.
- **The pieces are in place.** Private channels authorized by the backend
  (RFC 0006), the copy and the outbox (RFC 0007), `system::notify`, the tray,
  and the router that deep links already use. What's missing is the user's
  channel, the notification itself, and what the OS shows.

## Current state (0.14)

- Broadcasting follows channels fixed when the app is built (`.follow(..)`,
  `.follow_all(..)`). No channel depends on who is signed in.
- `system::notify` (the `system` feature) shows a title and a body, fire and
  forget. A click does nothing, and it's a free function, so a test can't
  swap it.
- There's no badge. tao has one per platform: `set_badge_label` on macOS,
  `set_badge_count` on Linux (the Unity launcher API) and `set_overlay_icon`
  on Windows.
- `Auth` knows the signed-in user (`/api/user`, cached), including the `id`.
- Laravel's default API has no routes for the `notifications` table.

## Design

### Turning it on

`Backend::notifications()` needs broadcasting to hear new ones (see question
5). It can be adjusted:

```rust
.notifications()
    .notifications_route("/api/notifications")          // the default
    .notifications_channel("App.Models.User.{id}")      // Laravel's default
```

The native notification needs the `system` feature. Without it, the
notifications are still heard, counted (the badge too) and listed, and the
toast is the only way they're shown.

### The user's channel

Once signed in, the app knows the user from `/api/user`. It fills `{id}`
with the user's `id` and follows `private-App.Models.User.1`, authorized by
`/api/broadcasting/auth` like every other private channel. Laravel's default
`routes/channels.php` already has the rule:

```php
Broadcast::channel('App.Models.User.{id}', function ($user, $id) {
    return (int) $user->id === (int) $id;
});
```

On sign-out the channel is left, and another user's sign-in follows theirs.
Broadcasting already connects on sign-in and closes on sign-out, so the
user's channel is worked out for each connection, from the signed-in user,
beside the channels fixed when the app is built.

### A notification

Laravel broadcasts `Illuminate\Notifications\Events\BroadcastNotificationCreated`
with the notification's `toArray` (or `toBroadcast`) data, plus its `id` and
`type`. The table has the same, plus `read_at` and `created_at`.

```rust
pub struct Notification {
    pub id: String,                 // Laravel's UUID
    pub kind: String,               // `type`: the class, or `broadcastType()`
    pub data: Value,                // what `toArray` returned
    pub read_at: Option<String>,
    pub created_at: Option<String>,
}
```

Laravel doesn't say what a notification's title is. By convention Elyra reads
`title`, `body` and `url` from `data` (see question 2). An app whose data
looks different says how to present it:

```rust
.present_notifications(|n| Presented {
    title: n.data["subject"].as_str().unwrap_or("New message").into(),
    body: n.data["preview"].as_str().map(Into::into),
    url: Some(format!("/tickets/{}", n.data["ticket_id"])),
})
```

Each one heard is dispatched as `NotificationReceived(Notification)`, a
domain event, and sent to the frontend on `elyra:notification`.

### In the OS

A notification is shown natively when no window of the app is focused, and
as an in-app toast when one is (question 1). A native notification:

- has the presented title and body, or just "New notification" with
  `.notification_previews(false)`, for what shows on a locked screen;
- on a click, brings the main window forward, marks it read, and opens `url`
  if it's an in-app path (`/invoices/7`), the way a deep link is opened. A
  full URL is never opened by itself (question 6); the app's listener can;
- is shown through a `Notifier` in the container, so a test swaps in
  `Notifier::fake()` and asserts on what was shown.

notify-rust waits for the click on every platform (`wait_for_response`), on
a thread of its own. At most eight wait at a time; past that, a notification
is shown without waiting for its click.
On macOS, delivery still needs a bundled app, as it does for
`system::notify` today.

### The count

The unread count comes with the list (`unread` beside the page, from the
controller rata prints). It goes up by one for each notification heard, and
is read again when one is marked read, when a window gets focus, and when the
connection comes back.

It's shown where the platform shows one: the dock's badge label on macOS, the
launcher count on Linux, and an overlay icon with the number on Windows. The
tray's tooltip says "3 unread". `.notification_badge(false)` turns it off. Signing out
clears it.

### Reading them

- `POST /api/notifications/{id}/read` and `POST /api/notifications/read-all`,
  marked `.queue_offline()`. Offline, they wait in the outbox and the count
  goes down at once, which is safe: marking read twice is harmless.
- **Read on the web** (question 3): the controller rata prints broadcasts
  `NotificationsRead { ids }` on the user's channel when the web marks them,
  and the desktop updates its list and count. Without it, the desktop finds
  out when a window gets focus.
- The list is a `GET`, so offline it's the last copy (RFC 0007), per user,
  cleared on sign-out.

### Missed while offline

Notifications broadcast while the app was offline, or not running, aren't
heard. When the connection comes back, or the app starts signed in, it reads
the list. If there are unread ones newer than the newest it has listed, it
shows a single native notification, "4 new notifications", that opens the
list (question 4). A single one is shown as itself. The newest listed is
kept per backend and user in the app's settings (`Store`), so it survives a
restart; a user's first sign-in on a machine shows nothing, rather than
everything unread.

### The frontend

```svelte
<script>
  import { notifications, onNotification } from "@elyra/runtime";
  onNotification((n) => console.log(n.data.title));
</script>

<button>🔔 {$notifications.unread}</button>
{#each $notifications.items as n}
  <a href={n.data.url} class:unread={!n.readAt} onclick={() => notifications.markRead(n.id)}>
    {n.data.title}
  </a>
{/each}
```

- `notifications` holds `{ unread, items, hasMore }`, with `markRead(id)`,
  `markAllRead()` and `more()` for the next page.
- `onNotification(handler)` is called for each one heard while subscribed.
- It all goes through `/__auth/notifications*` (`Capability::Auth`), like the
  outbox.

### `rata make:notifications`

- **The app:** the bell with its count and the list, as a page in
  `app/src/resources/notifications/`, picked up by the registry like a
  resource, with the same look as the generated resources. It's the app's
  code to restyle (question 7).
- **The hint:** `.notifications()` on the `Backend`.
- **Laravel's side, printed:** `php artisan make:notifications-table`, a
  `NotificationController` (`index` with `unread`, `read`, `readAll`), its
  routes behind `auth:sanctum`, the `NotificationsRead` event, and an example
  notification with `via` returning `['database', 'broadcast']`.

### Testing

```rust
let pusher = PusherFake::start().await;
// …an app with `.broadcasting(pusher.config()).notifications()`, signed in as user 1,
// and `.swap(Notifier::fake())`…
pusher.wait_for_subscriptions(1).await;
pusher.broadcast(
    "private-App.Models.User.1",
    "Illuminate\\Notifications\\Events\\BroadcastNotificationCreated",
    json!({ "id": "9b1…", "type": "invoice-paid", "title": "Invoice paid", "url": "/invoices/7" }),
);
app.get::<Notifier>().assert_shown(|n| n.title == "Invoice paid");
```

CI's Laravel job gets a notification sent when a customer is assigned. A new
test runs it through Reverb: the web assigns a customer, the desktop shows
the notification (through the fake `Notifier`) and counts it, marks it read,
and the web reads `read_at`.

## Security

- **The channel is authorized by Laravel.** The app follows only the signed-in
  user's channel, with the `id` from `/api/user`, and the server's rule
  decides. Another user's id would be refused, as any channel is today.
- **What the lock screen shows.** A notification's text is visible in the
  OS's notification center and maybe on a locked screen.
  `.notification_previews(false)` keeps it to "New notification".
- **Where a click goes.** Only an in-app path is opened by itself, through
  the router. A full URL, `javascript:` or anything else isn't, so a
  notification's data can't open a page the app didn't choose to.
- **Per user.** The list's copy is per user and cleared on sign-out, as in
  RFC 0007. Signing out also clears the badge and leaves the channel. Native
  notifications already shown are removed where the OS lets an app do that
  (notify-rust can on Linux, and on macOS with its newer backend); elsewhere
  they stay in the notification center.
- **The frontend** reaches it only with `Capability::Auth`, like the outbox.

## Alternatives considered

- **Polling `/api/notifications`.** No Reverb needed, but slower and a request
  a minute from every running app. It's a fallback to consider for apps
  without broadcasting (question 5), not the design.
- **OS push (APNs, Windows push).** It would reach a closed app, but needs
  Apple's push certificates, a signed app and a push service, which
  [are the app's own](../roadmap.md#out-of-scope) business. The app is
  usually running in the tray, and what was missed is caught up when it
  starts.
- **Only a per-user follow, and leave notifications to the app.** Less in
  the framework, but every app would write the same parsing, counting,
  badges and click handling. The per-user follow is built anyway, and can be
  exposed later.
- **A notification center built into the runtime,** like the toasts. It's a
  page in the app, which apps want to look their own; generated code is
  theirs to change.

## Implementation plan

1. **The user's channel and the notification.**
   - The user's channel, worked out for each connection: followed on
     sign-in and left on sign-out.
   - `Backend::notifications()` and its options, `Notification`,
     `.present_notifications(..)`, `NotificationReceived` and
     `elyra:notification`.
2. **In the OS.**
   - `Notifier` and `Notifier::fake()`; `system::notify` goes through it.
   - Native or toast by focus, the click (focus, read, the in-app path), and
     `.notification_previews(false)`.
   - The badge per platform and the tray's tooltip.
3. **The list and reading.**
   - The list with `unread`, mark read and read all through the outbox.
   - `NotificationsRead` from the web, re-reading on focus and on reconnect,
     and the catch-up for what was missed.
4. **The frontend.** The `notifications` store, `onNotification`,
   `/__auth/notifications*`.
5. **`rata make:notifications`,** with the Laravel side printed, the docs,
   and CI against the real Laravel app through Reverb.

As built, a few details differ from the text above:

- The user's channel is worked out for each connection, not added and
  removed while connected.
- The names are `.present_notifications(..)` and `.notification_badge(..)`.
- At most eight notifications wait for a click.
- One missed notification is shown as itself, and a first sign-in shows
  nothing.
- What was read here stays read until the server shows it read too.
- The generated list renders `data.title`/`body`/`url`; an app with
  `.present_notifications(..)` changes the page to match.
- The nav count needs `App.svelte` to render a nav item's `count` (`rata new`
  does since 0.15).

## Open questions

1. **Native or toast:** a native notification only when no window is focused,
   and a toast when one is (recommended: what users expect from Slack or
   Mail), or always native?
2. **What to show:** by convention `title`, `body` and `url` from the data,
   with `.present(..)` for other shapes (recommended), or require a fixed
   `toBroadcast` shape, or show the notification's type alone?
3. **Read on the web:** rata's controller broadcasts `NotificationsRead`, with
   re-reading on focus as the fallback (recommended), or only re-read on
   focus, with nothing new on the server?
4. **Missed while offline or closed:** one summary notification and the list
   (recommended: no flood after a weekend), one native notification each, or
   none, just the count?
5. **Without broadcasting:** `.notifications()` requires it and says so at
   startup (recommended), or polls the list every few minutes instead?
6. **A click on a full URL:** never opened by itself, the app's listener
   decides (recommended), or `https://` links opened in the browser?
7. **The notification list:** generated by `rata make:notifications`
   (recommended), or a built-in component in the runtime?

## Decisions

Settled on 2026-10-13, all as recommended:

1. **Native or toast by focus:** a native notification only when no window
   of the app is focused, a toast when one is.
2. **`title`, `body` and `url`** from the data by convention, with
   `.present(..)` for notifications shaped otherwise.
3. **Read on the web:** rata's controller broadcasts `NotificationsRead`, and
   the desktop also reads the list again when a window gets focus.
4. **What was missed:** one summary notification ("4 new notifications") that
   opens the list.
5. **Broadcasting is required:** `.notifications()` without it is refused at
   startup with a message saying so. No polling.
6. **A full URL is never opened by a click;** only an in-app path is. The
   app's own listener may open one.
7. **The list is generated** by `rata make:notifications`, the app's code to
   restyle; nothing new is built into the runtime.
