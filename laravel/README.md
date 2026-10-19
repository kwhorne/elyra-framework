# elyracode/elyra-backend

The Laravel half of an [Elyra Framework](https://elyracode.com/framework)
desktop app: what `Backend` in the desktop app expects of your API, as a
package. It's the code [docs/backend.md](../docs/backend.md) shows, so you
don't paste it into every app:

- `Route::elyraAuth()`: `POST /sanctum/token`, and with `auth:sanctum`
  `DELETE /sanctum/token` and `GET /user`, the routes the desktop signs in,
  out and reads the user with.
- `Elyra\Backend\Http\Middleware\Idempotent` (alias `elyra.idempotent`): a
  write sent again with the same `Idempotency-Key`, as the desktop's offline
  outbox does, is answered from the record instead of made twice.
- `Route::elyraNotifications()`: Laravel's database notifications for the
  desktop, the list with its `unread` count and marking them read, which
  broadcasts `NotificationsRead` to the user's other devices.
- What it expects of files: uploads as `multipart/form-data` beside the
  fields (a `PUT` as a `POST` with `_method`), refused with validation rules,
  downloads named with `Storage::download($path, $name)` — in the guidelines
  and the skill below.
- [Laravel Boost](https://laravel.com/docs/boost) guidelines and the
  `elyra-desktop-backend` skill, so coding agents working on the Laravel app
  keep the API the desktop app relies on.

## Installing

```bash
composer require elyracode/elyra-backend
php artisan install:api        # Sanctum; add HasApiTokens to App\Models\User
```

```php
// routes/api.php
use Elyra\Backend\Http\Middleware\Idempotent;

Route::elyraAuth();

Route::middleware(['auth:sanctum', Idempotent::class])->group(function () {
    Route::elyraNotifications();           // with `php artisan make:notifications-table`
    Route::apiResource('customers', CustomerController::class);
});
```

Nothing is registered until `routes/api.php` calls a macro. `elyraAuth()`
reads the user model from `auth.providers.users.model`; pass another class
to it if yours differs.

Broadcasting stays yours to set up, as in
[the guide](../docs/backend.md#broadcasting-changes-made-on-the-web): the
models' `BroadcastsEvents`, `routes/channels.php`, and channel auth under
`/api` with `auth:sanctum` in `bootstrap/app.php`.

## For coding agents (Laravel Boost)

With `laravel/boost` installed, `php artisan boost:install` offers this
package's guidelines and skill among the third-party packages. Run without
questions (`--no-interaction`), it installs the packages `boost.json` lists
under `"packages"`, so add `"elyracode/elyra-backend"` there. Agents then know that the desktop app reads
Laravel's `422`s per field, maps the status codes to what it shows, sends
offline writes with an `Idempotency-Key`, and needs `rata codegen` on the
desktop side when an API resource changes.

The desktop side has its own: `rata ai:install` in the Elyra app.

## Requirements

PHP 8.2+ and Laravel 11, 12 or 13. MIT licensed.
