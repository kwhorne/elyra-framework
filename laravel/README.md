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
- `Route::elyraAbilities()`: `GET /abilities`, what the signed-in user may
  do — spatie/laravel-permission's permissions when installed, else the
  Gate abilities in `config('elyra.abilities')` — and `AbilitiesChanged`,
  broadcast on the user's channel when spatie's roles or permissions change
  (with `permission.events_enabled`), so the desktop reads them again.
- `Route::elyraAssistant()`: `POST /elyra/assistant/v1/messages`, where the
  desktop's assistant reaches Claude with the user's token. The server adds
  its key (`ANTHROPIC_API_KEY`) and caps what may be asked: the models in
  `config('elyra.assistant.models')`, `max_tokens`, requests per user per
  minute, and no server tools unless the server allows them: code execution
  (`ELYRA_ASSISTANT_CODE_EXECUTION`) and web search
  (`ELYRA_ASSISTANT_WEB_SEARCH`). A streamed request is passed through as Claude
  writes it (`ELYRA_ASSISTANT_STREAM`, on by default). `AssistantUsed`
  records each request's tokens.
- `Route::elyraTranscription()`: `POST /elyra/transcription/v1/audio/transcriptions`,
  where the desktop's recordings become text with the user's token. The
  server adds its key (`OPENAI_API_KEY`) and caps what may be sent: the
  models in `config('elyra.transcription.models')`, the file's size
  (`max_kb`) and requests per user per minute. `TranscriptionUsed` records
  each request's model and size.
- `Route::elyraBroadcasting()`: `GET /desktop/broadcasting`, where the
  desktop connects to hear Laravel's broadcasts, as
  `Backend::broadcasting_from(..)` reads it: the default Reverb or Pusher
  connection's key, host, port and scheme, or the public address in
  `ELYRA_BROADCASTING_HOST`/`_PORT`/`_SCHEME`.
- `Route::elyraUpdates()` and `php artisan elyra:release`: the desktop's
  updates from this server, as `UpdaterConfig::from_backend(..)` checks
  them. `elyra:release <file> <version> <platform>` puts a release on
  `config('elyra.updates.disk')` with its size and SHA-256;
  `GET /desktop/update` offers the newest newer one, and
  `GET /desktop/download/{file}` hands it over to a signed-in user.
- What it expects of files: uploads as `multipart/form-data` beside the
  fields (a `PUT` as a `POST` with `_method`), refused with validation rules,
  downloads named with `Storage::download($path, $name)` — in the guidelines
  and the skill below.
- `php artisan elyra:contract --as=<email>`: the API as the desktop app
  sees it, for `rata check --backend` (RFC 0040): the routes under `/api`,
  each `FormRequest`'s rules, the abilities there are, and each `GET`'s
  answer sampled as that user (field names, types and null-ness, never a
  value).
- `Elyra\Backend\Concerns\ChecksUnmodifiedSince`: on a model,
  `$model->changedSince($request)`, whether it changed after the version
  the desktop's write names in `If-Unmodified-Since`, for a `412`: a file
  edited in its own app and saved back (RFC 0038), or a write made offline
  (RFC 0007).
- [Laravel Boost](https://laravel.com/docs/boost) guidelines and the
  `elyra-desktop-backend` skill, so coding agents working on the Laravel app
  keep the API the desktop app relies on.

## Installing

The package isn't on Packagist yet. It lives in `laravel/` of the framework's
repository, at the same tag as the desktop app's `elyra` dependency, so a
Laravel app adds that folder as a [path repository](https://getcomposer.org/doc/05-repositories.md#path):

```bash
git clone --depth 1 --branch v0.61.0 https://github.com/kwhorne/elyra-framework ../elyra-framework
composer config repositories.elyra '{"type": "path", "url": "../elyra-framework/laravel", "options": {"symlink": false}}'
composer require "elyracode/elyra-backend:@dev"
php artisan install:api        # Sanctum; add HasApiTokens to App\Models\User
```

`symlink: false` copies the package into `vendor/`, so a deploy doesn't need
the clone. With `[check] backend = "../web"` in the desktop app's
`elyra.toml`, `rata upgrade` moves the clone and runs `composer update` along
with the rest; `php artisan about` shows which release the package is
(`Elyra\Backend\ElyraBackend::VERSION`). This is what CI's Laravel app does (`scripts/laravel-backend.sh`).

```php
// routes/api.php
use Elyra\Backend\Http\Middleware\Idempotent;

Route::elyraAuth();

Route::middleware(['auth:sanctum', Idempotent::class])->group(function () {
    Route::elyraNotifications();           // with `php artisan make:notifications-table`
    Route::elyraBroadcasting();            // for Backend::broadcasting_from("/api/desktop/broadcasting")
    Route::elyraUpdates();                 // for UpdaterConfig::from_backend("/api/desktop/update", ..)
    Route::apiResource('customers', CustomerController::class);
});
```

Nothing is registered until `routes/api.php` calls a macro. `elyraAuth()`
reads the user model from `auth.providers.users.model`; pass another class
to it if yours differs.

The rest of broadcasting stays yours to set up, as in
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
