---
name: elyra-laravel-backend
description: An Elyra Framework app as the desktop half of a Laravel app (the backend feature) — Backend and Auth with a Sanctum token in the keychain, a server chosen at sign-in, several servers and a token per workspace, Laravel's 422s in forms, error kinds, broadcasting with follow() and a window's own channels, working offline with the outbox and Idempotency-Key, conflicts, Laravel's notifications, the assistant and speech to text through Laravel, updates from the server, files (uploads, downloads, transfers), make:resource --backend, and HttpFake/PusherFake tests. Use when an Elyra app calls a Laravel API, or when changing what that API returns.
---

# Elyra with a Laravel backend

`elyra = { …, features = ["backend"] }` (with `database` for resources and
offline).

```rust
App::new().backend(
    Backend::new("https://crm.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))   // REVERB_APP_KEY
        .follow("customers", "/api/customers")
        .offline()
        .notifications(),
)

#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
}
```

## Which server, which user

- `Backend::new(url)` is one fixed server. `Backend::chosen_at_sign_in()`
  has none until the user signs in: the app's sign-in screen asks for the
  address, and `Auth::sign_in_with_token(server, token, user)` (or
  `sign_in`) chooses it. Before that, a request fails as unauthenticated
  and nothing is sent. `.broadcasting_from("/api/desktop/broadcasting")`
  reads Reverb's settings from the chosen server
  (`Route::elyraBroadcasting()`).
- `.user_route(..)` and `.user_id_path("user.id")` when the user value
  isn't Sanctum's default; `.follow_user("chat", "user.{user.id}", None)`
  follows a channel named from the user.
- `.multiple_servers()` keeps the user signed in to several servers, one
  active: `auth.switch_to(server)`, `sign_out_of(server)`,
  `sign_out_all()`, `auth.accounts()` (each with its `pending` and `failed`
  writes). `<AccountSwitcher>` is the UI.
- `.token_per_workspace("workspace.id")` when a token names one workspace:
  `switch_workspace(server, workspace)`, `sign_out_of_workspace(..)`; a
  `401` ends that workspace, not the server. `.offline_scope_path("workspace.id")`
  keeps the offline copy and the outbox per workspace.
- `Auth::update_user(user)` when the server says the user changed.

## The contract with Laravel

- Sign-in: `POST /api/sanctum/token` with `email`, `password`,
  `device_name` returns the plain-text token; `DELETE /api/sanctum/token`
  revokes it; `GET /api/user` is the user. The token lives in the OS keychain
  only. Other paths: `.token_route(..)`, `.revoke_route(..)`, `.user_route(..)`.
- Requests ask for JSON, so a failed validation is `422 { errors }`, which
  lands in the form per field. `401`/`419` sign the user out; `403`, `404`,
  `409`/`412`, `429`, `5xx` and no answer are the kinds `denied`,
  `not-found`, `conflict`, `too-many-requests`, `server`, `offline`.
- `.json()` decodes the body; `.resource()` also unwraps `{ "data": … }`;
  `Page<T>` reads `paginate()` and resource collections.
- Broadcasting: Pusher protocol (Reverb, Askr `--pusher`), private channels
  authorized at `/api/broadcasting/auth` with the Sanctum token; the app sends
  `X-Socket-ID` so `->toOthers()` works. A followed channel's event re-runs
  the live queries it could have changed: one whose `model` has an `id`
  (Laravel's `BroadcastsEvents`) the lists and that record's views; any
  other event everything that read the resource.
- Live reads over the backend are keyed per record (RFC 0017): `GET
  /api/customers/42` re-runs on a write or event for customer 42, a list on
  any change to customers. `.with_lists()` for a record's endpoint that
  shows others; `.record_key("/api/invoices", "uuid")` when the route key
  isn't `id`; `ctx.depends_on(&backend::list_key(..))` for every change.
- A window's own private channels (a run's output, one record's events):
  `.listen(["task.*"])` on the Backend lists what a window may ask for, and
  `listen(`task.${id}`, (data, e) => ..)` from `@elyra/runtime` in the view
  (it returns the unsubscribe, for `$effect`; `listening(channel)` says if
  it's joined). Laravel's `Broadcast::channel('task.{task}', ..)` rule
  decides who may.
- Presence (who else has a record open): `.presence(["customers.*"])` on the
  Backend, a `Broadcast::channel('customers.{id}', fn ($user, $id) => [...])`
  rule returning `id`, `name` (and `avatar`), and `<Presence
  channel={`customers.${id}`} />` in the view. Only listed channels are
  joined; Laravel's rule decides who. `make:resource --backend .. --presence`
  writes it. Test with `PusherFake::join_as(..)` / `leave_as(..)`.
- Who's editing: `<Presence channel=.. editing={changed} />` on an edit form.
  It's Echo's `whisper('editing', { editing })`, so the web half uses
  `Echo.join(..).whisper(..)` / `.listenForWhisper('editing', (e, { user_id }) => ..)`.
  Information, not a lock. Needs Reverb's default
  `REVERB_APP_ACCEPT_CLIENT_EVENTS_FROM=members` (it names the sender); not
  on Askr. Test with `PusherFake::whisper_as(channel, user, "client-editing", ..)`.
- Offline writes marked `.queue_offline()` wait in the outbox and are sent
  once, in order, each with its `Idempotency-Key`; the Laravel side needs the
  idempotency middleware after `auth:sanctum`. `.unless_changed_since(..)`
  sends `If-Unmodified-Since`; Laravel answers `412` on a conflict.
- `.optimistic()` on a `PUT`/`PATCH`/`DELETE` to a record (RFC 0039) shows
  the change in every live list at once, from the offline copy (needs
  `.offline()`), until the server confirms it or refuses it; only fields
  the record already has change (`.optimistic_with(&fields)` otherwise).
  Not for a `POST`. `$pending` and `<Table pending>` mark rows not yet sent.
- To try a screen offline, use the inspector's **Simulate offline** (⌘⌥E,
  debug builds): every request to Laravel answers as if there were no
  server, so the copy, the outbox and the StatusBar behave as they would
  for real. Don't stop Laravel or fake it in app code.
- Notifications: the `database` and `broadcast` channels, heard on
  `App.Models.User.{id}`, listed from `GET /api/notifications` (paginator +
  `unread`), `POST /api/notifications/{id}/read` and `/read-all`, and a
  `NotificationsRead` event broadcast `->toOthers()`.
- Files: an upload is `multipart/form-data` (`.attach("contract", &file)`
  with a `ChosenFile`, `.fields(&input)` beside it); a `PUT`/`PATCH` with a
  file goes as a `POST` with `_method`; a `413` (nginx, PHP's
  `post_max_size`) lands on the file's field like a `422`. Downloads stream
  to disk: `.download_to(&chosen_save_file)`, `.download_into(&folder)`,
  `.download()`; the name comes from `Content-Disposition`
  (`Storage::download($path, $name)`); `Backend::download_url(url)` fetches a
  signed URL without the token. `.queue_offline()` on an upload keeps a copy
  of the file in the outbox. `Transfers` / the `transfers` store show
  progress and cancel.

- Abilities: `.abilities()` reads the user's list from `GET /api/abilities`
  (`Route::elyraAbilities()`); a `#[command(can = "..")]` then needs the
  app's grant and the user's ability (kind `denied` otherwise, no request);
  `ctx.get::<Abilities>().allows(..)` in Rust, `$can(..)` in Svelte; read
  again on focus, reconnect and `AbilitiesChanged`.
- The assistant: `.assistant()` on the `Backend` sends the app's assistant
  through `Route::elyraAssistant()` with the user's token; the key stays in
  Laravel's `.env`. Never put a provider key in the desktop app. Its answer
  streams into the panel as it's written, through the proxy too
  (`ELYRA_ASSISTANT_STREAM=false` on a server that buffers).
  `Assistant::code_mode()` lets it call the app's tools from code; what's
  behind `.confirm(..)` still asks the user.
- Speech to text: `.transcription()` on the `Backend` sends a recording
  (`App::microphone(why)`, `<Microphone>`) to `Route::elyraTranscription()`
  with the user's token; Laravel adds `OPENAI_API_KEY` and caps the model,
  size and rate (`config('elyra.transcription')`). Signed out, nothing is
  sent. Needs elyra's `ai` feature.
- Updates: `UpdaterConfig::from_backend("/api/desktop/update", version)`
  asks the signed-in server (`Route::elyraUpdates()`), and
  `php artisan elyra:release <file> <version> <target>` publishes one.

**When the API changes** (a field, a route, a resource's shape), change the
Rust types that mirror it and run `rata codegen` so the frontend's types
follow; the generated `HttpFake` tests show what the app expects.

## Generating

- `rata make:resource Customer --backend /api/customers --generate …`: the
  commands call the API; no migration (the table is the server's); writes are
  `.queue_offline()`; tests run against `HttpFake` and `PusherFake`. rata
  prints Laravel's side.
- A `file` field (`'contract:file?'`, with `--backend` only): the commands
  take `Option<ChosenFile>`, `<plural>_<field>(id, to)` downloads it, and the
  form has `@elyra/ui`'s `FileField` in a `Field`.
- `rata make:notifications`: the page, the nav count and Laravel's side.
- The `elyracode/elyra-backend` Composer package has the Laravel half:
  `Route::elyraAuth()`, `elyraAbilities()`, `elyraNotifications()`,
  `elyraAssistant()`, `elyraTranscription()`, `elyraBroadcasting()` and
  `elyraUpdates()`, and the idempotency middleware.

`search_docs` ("backend", "outbox", "broadcasting", "notifications",
"files", "abilities", "multiple servers", "workspace", "transcription",
"updater") has the details.
