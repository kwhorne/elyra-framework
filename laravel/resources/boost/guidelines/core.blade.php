## Elyra desktop backend (elyracode/elyra-backend)

This Laravel app is also the backend of an Elyra Framework desktop app (Rust + Svelte). The desktop app calls the same API as the web, so changes to the API change the desktop app too.

### What the desktop app expects of the API

- Sign-in: `POST /api/sanctum/token` with `email`, `password` and `device_name` returns Sanctum's plain-text token; `DELETE /api/sanctum/token` revokes it; `GET /api/user` returns the user. `Route::elyraAuth()` in `routes/api.php` registers these three.
- Validation errors must stay Laravel's `422` JSON (`{ "message", "errors": { field: [..] } }`): the desktop shows them under each field. Keep using Form Requests or `$request->validate()`; don't return custom error shapes.
- `401`/`419` sign the desktop user out, `403` (with `message`), `404`, `409`/`412` (a conflict), `429` with `Retry-After` and `5xx` are shown as such. Use the right status.
- Lists are `paginate()` or API Resource collections; single records may be a model or `{ "data": … }`.
- Writes the desktop saved offline arrive later with an `Idempotency-Key` header. The `Elyra\Backend\Http\Middleware\Idempotent` middleware (alias `elyra.idempotent`) must come after `auth:sanctum` on routes that write.
- A write may carry `If-Unmodified-Since`; answer `412` when the record changed since.
- Broadcasting: models use `BroadcastsEvents` on private channels; channel auth is under `/api` with `auth:sanctum` (`withBroadcasting(..., ['prefix' => 'api', 'middleware' => ['api', 'auth:sanctum']])`). Use `->toOthers()` so the desktop doesn't hear its own writes.
- Files from the desktop are `multipart/form-data` uploads beside the other fields (`$request->file('contract')`); a `PUT`/`PATCH` with a file arrives as a `POST` with `_method`. Validate them with `file`/`mimes`/`max` so a refusal is a `422` on the field. Downloads should set the file's name: `Storage::download($path, $name)`.
- What a user may do comes from `GET /api/abilities` (`Route::elyraAbilities()`: spatie's permissions, or the Gate abilities in `config('elyra.abilities')`). The desktop hides and refuses what the list doesn't allow, so name abilities `resource.verb` (`customers.delete`) and keep policies and permissions in step. Per record, an API Resource may add `can: { update, delete }`. When roles change outside spatie's events, `broadcast(new Elyra\Backend\Events\AbilitiesChanged($user))`.
- The desktop's assistant reaches Claude through `POST /api/elyra/assistant/v1/messages` (`Route::elyraAssistant()`, inside `auth:sanctum`), which adds `ANTHROPIC_API_KEY` and caps the model, `max_tokens` and requests per user (`config('elyra.assistant')`). Keep the key on the server; never return it. The assistant's tools are the desktop's commands, so the API's own authorization still decides every write.
- Notifications for the desktop go through the `database` and `broadcast` channels, with `title`, `body` and `url` in `toArray()`. `Route::elyraNotifications()` (inside `auth:sanctum`) registers the list and read routes.

@verbatim
<code-snippet name="routes/api.php for the desktop app" lang="php">
use Elyra\Backend\Http\Middleware\Idempotent;

Route::elyraAuth();

Route::middleware(['auth:sanctum', Idempotent::class])->group(function () {
    Route::elyraNotifications();
    Route::apiResource('customers', CustomerController::class);
});
</code-snippet>
@endverbatim

### When you change the API

Changing a field, a route or a resource's shape breaks the desktop app unless it changes too: update the Rust types that mirror the response in the desktop project and run `rata codegen` there. Prefer adding fields over renaming or removing them. Use the `elyra-desktop-backend` skill for the details.
