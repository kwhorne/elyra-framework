---
name: elyra-laravel-backend
description: An Elyra Framework app as the desktop half of a Laravel app (the backend feature) — Backend and Auth with a Sanctum token in the keychain, Laravel's 422s in forms, error kinds, broadcasting with follow(), working offline with the outbox and Idempotency-Key, conflicts, Laravel's notifications, files (uploads, downloads, transfers), make:resource --backend, and HttpFake/PusherFake tests. Use when an Elyra app calls a Laravel API, or when changing what that API returns.
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
  the live queries that read its resource.
- Offline writes marked `.queue_offline()` wait in the outbox and are sent
  once, in order, each with its `Idempotency-Key`; the Laravel side needs the
  idempotency middleware after `auth:sanctum`. `.unless_changed_since(..)`
  sends `If-Unmodified-Since`; Laravel answers `412` on a conflict.
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
  form gets `FileField.svelte`.
- `rata make:notifications`: the page, the nav count and Laravel's side.
- The `elyracode/elyra-backend` Composer package has the routes, the
  idempotency middleware and the notifications controller for the Laravel
  half.

`search_docs` ("backend", "outbox", "broadcasting", "notifications",
"files", "abilities") has the details.
