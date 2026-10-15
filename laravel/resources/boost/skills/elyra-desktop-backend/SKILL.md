---
name: elyra-desktop-backend
description: Keep this Laravel app working as the backend of an Elyra Framework desktop app — the Sanctum token routes, 422 validation JSON, status codes the desktop maps to error kinds, idempotent writes from the offline outbox (Idempotency-Key), If-Unmodified-Since conflicts, broadcasting for live lists, and database + broadcast notifications. Use when adding or changing API routes, controllers, API resources, validation, broadcasting or notifications that the desktop app uses.
---

# The Laravel side of an Elyra desktop app

The desktop app (Rust + Svelte, Elyra Framework) is a client of this API.
Its `Backend` signs in with a Sanctum token kept in the OS keychain and asks
for JSON on every request.

## Routes

```php
// routes/api.php
use Elyra\Backend\Http\Middleware\Idempotent;

Route::elyraAuth();   // POST /sanctum/token, and with auth:sanctum: DELETE /sanctum/token, GET /user

Route::middleware(['auth:sanctum', Idempotent::class])->group(function () {
    Route::elyraNotifications();   // GET /notifications, POST /notifications/read-all, POST /notifications/{id}/read
    Route::apiResource('customers', CustomerController::class);
});
```

- `php artisan install:api` installs Sanctum; the user model needs
  `HasApiTokens`.
- The token route takes `email`, `password` and `device_name` (the desktop
  sends `<host> · <app>`) and answers the plain-text token. Bad credentials
  are a `422` on `email`.

## Answers the desktop understands

| Laravel answers | The desktop shows |
| --- | --- |
| `422 { message, errors }` | the messages under each field of the form |
| `401`, `419` | signed out ("please sign in again") |
| `403 { message }` | denied, with the message |
| `404` | not found |
| `409`, `412` | a conflict (keep mine / take theirs) |
| `429` + `Retry-After` | try again later |
| `5xx` | a server error, without the page |

So: validate with Form Requests or `$request->validate()`, authorize with
policies (`403`), and never answer an error with `200`.

## Shapes

- Lists: `->paginate()` or an API Resource collection (`{ data, links, meta }`).
  The desktop's list sends `search`, `sort`, `direction`, `page` and
  `per_page` as the query when it was made with `rata make:resource --backend`.
- One record: the model, or an API Resource (`{ data: … }`).
- Timestamps as Laravel serializes them (ISO 8601).

## Writes saved offline

The desktop queues writes while offline and sends each once, oldest first,
with the `Idempotency-Key` it was first tried with. `Idempotent` (after
`auth:sanctum`) answers a repeated key from its record for 24 hours, so a
write that reached the server before the connection dropped isn't made
twice. A refused write sent again carries a new key.

A write may carry `If-Unmodified-Since` (the record's `updated_at`):

```php
abort_if($request->hasHeader('If-Unmodified-Since')
    && $customer->updated_at->gt(Carbon::parse($request->header('If-Unmodified-Since'))), 412,
    'Someone changed this customer meanwhile.');
```

## Live lists: broadcasting

```php
class Customer extends Model
{
    use BroadcastsEvents;

    public function broadcastOn(string $event): array
    {
        return [new PrivateChannel('customers')];
    }
}

// routes/channels.php
Broadcast::channel('customers', fn ($user) => true);

// bootstrap/app.php: channel auth under /api, with the Sanctum token
->withBroadcasting(__DIR__.'/../routes/channels.php', [
    'prefix' => 'api', 'middleware' => ['api', 'auth:sanctum'],
])
```

Reverb or Askr (`askr serve --pusher`) carries it. The desktop sends
`X-Socket-ID`, so `->toOthers()` skips the app whose own write it was. Events
are queued: run a worker or `QUEUE_CONNECTION=sync`.

## Notifications

```php
public function via(object $notifiable): array
{
    return ['database', 'broadcast'];
}

public function toArray(object $notifiable): array
{
    return ['title' => 'Invoice paid', 'body' => 'Invoice 7 was paid.', 'url' => '/invoices/7'];
}
```

The `notifications` table (`php artisan make:notifications-table`) and
`Route::elyraNotifications()`. The desktop hears them on the user's private
channel `App.Models.User.{id}`; reading them anywhere broadcasts
`NotificationsRead` to the user's other devices. `url` should be a path in
the desktop app (`/invoices/7`).

## When the API changes

The desktop project mirrors responses in Rust types. After changing a field,
route or resource shape here, change those types there and run
`rata codegen`; its `HttpFake` tests show what it expects. Add fields rather
than renaming or removing them, and keep old routes until the desktop app is
updated.
