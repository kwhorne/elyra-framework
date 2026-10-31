---
name: elyra-desktop-backend
description: Keep this Laravel app working as the backend of an Elyra Framework desktop app — the Sanctum token routes, 422 validation JSON, status codes the desktop maps to error kinds, idempotent writes from the offline outbox (Idempotency-Key), If-Unmodified-Since conflicts, broadcasting for live lists, database + broadcast notifications, file uploads and downloads, and the user's abilities. Use when adding or changing API routes, controllers, API resources, validation, broadcasting or notifications that the desktop app uses.
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

## What a user may do

```php
// routes/api.php, inside auth:sanctum
Route::elyraAbilities();   // GET /abilities: ["customers.view", "customers.delete", …]
```

- With spatie/laravel-permission it answers the user's permissions (roles
  included); without it, each ability in `config('elyra.abilities')` the Gate
  allows (`vendor:publish --tag=elyra-config`).
- The desktop hides and refuses what the list doesn't allow, and the server
  still decides: keep `->authorize(..)` / `abort_unless(..can(..), 403)`.
  Name abilities `resource.verb`, as `rata make:resource` does.
- Per record, add `can` to the API Resource (`'update' => $request->user()->can('update', $this->resource)`).

## The desktop's assistant

```php
// routes/api.php, inside auth:sanctum
Route::elyraAssistant();   // POST /elyra/assistant/v1/messages
```

- Set `ANTHROPIC_API_KEY` in `.env`; it never leaves the server.
- `config('elyra.assistant')` caps what may be asked: `models`,
  `max_tokens`, `per_minute` per user, `server_tools` (off).
- Listen to `Elyra\Backend\Events\AssistantUsed` to record each user's
  tokens.
- The assistant calls the desktop's commands, which call this API as the
  user: policies and `abort_unless(..can(..))` still decide.
- With `permission.events_enabled`, the package broadcasts
  `AbilitiesChanged` on the user's channel when spatie's roles or
  permissions change; dispatch it yourself for other changes.

## Files

The desktop uploads a file as `multipart/form-data`, the form's fields
beside it; a `PUT`/`PATCH` with a file arrives as a `POST` with
`_method=PUT`, which Laravel routes as `update`. An upload saved offline
arrives later with its `Idempotency-Key`, like any write.

```php
$request->validate(['contract' => ['nullable', 'file', 'mimes:pdf', 'max:20480']]);
if ($request->hasFile('contract')) {
    $customer->contract_path = $request->file('contract')->store('contracts');
    $customer->contract = $request->file('contract')->getClientOriginalName();
}

// routes/api.php
Route::get('/customers/{customer}/contract', fn (Customer $customer) =>
    Storage::download($customer->contract_path, $customer->contract));
```

- Refuse a file with validation rules: the `422` shows on its field. A body
  over `post_max_size` (or nginx's `client_max_body_size`) is a `413`, which
  the desktop shows on the field too; raise those limits for large files.
- Downloads: give the name (`Storage::download`'s second argument, or a
  `Content-Disposition`), which the desktop uses, cleaned. A signed URL
  (`Storage::temporaryUrl`) is fetched without the token.
- Keep the stored path out of the API (`$hidden`); answer the file's name.

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
