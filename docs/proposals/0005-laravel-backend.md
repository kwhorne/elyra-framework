# RFC 0005 — A Laravel backend: sign-in, a typed HTTP client, and its errors

**Status:** implemented (2026-10-10) · **Target:** 0.12.0 · see [Decisions](#decisions) · the guide: [docs/backend.md](../backend.md)

## Summary

Elyra is the desktop half of an app whose web half is Laravel, served by
[Askr](https://github.com/kwhorne/askr) or any other server. This RFC gives an
Elyra app what it needs to work against that Laravel backend:

- an `Http` client, Laravel's `Http` facade in Rust;
- a `Backend` on top of it, which signs the user in with a
  [Sanctum](https://laravel.com/docs/sanctum) token kept in the OS keychain;
- Laravel's error answers mapped to Elyra's. A `422` becomes the same
  `ValidationErrors` a local command returns, so a form shows the server's
  messages without knowing where they came from.

```rust
App::new().backend(Backend::new("https://crm.example.com"))
```

```rust
#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
}
```

```svelte
<script>
  import { auth } from "@elyra/runtime";
  // auth.signIn(email, password) — a 422 lands per field, like any form
</script>
{#if $auth.signedIn}Hi {$auth.user.name}{:else}<SignIn />{/if}
```

This is the first of three. RFC 0006 brings Laravel's broadcasting in, so a
change made on the web updates the open windows. RFC 0007 covers working
offline and syncing.

## Motivation

- **Desktop and web, one product.** The user's data lives in the Laravel app.
  The desktop app is another way in, so it needs to sign in as that user and
  call the same API.
- **It's mostly glue, and easy to get subtly wrong.** Laravel answers a failed
  validation with a redirect unless the request asks for JSON. A token
  belongs in the keychain, not in `localStorage`. A redirect to another host
  must not carry the token. A `401` means the token is gone, and the UI must
  say so. Every app would rediscover these one at a time.
- **Elyra already has the halves.** `Secrets` (the keychain), `ValidationErrors`
  in Laravel's exact shape (`items.0.name`), deep links, the event bus, live
  queries and `make:resource`'s forms. They only need to meet a backend.

## Current state (0.11)

- No HTTP client in the framework. `reqwest` is in the workspace for the AI
  SDK (rustls), and the updater uses `ureq`.
- `Secrets` stores strings in the OS keychain: Keychain on macOS, Credential
  Manager on Windows, and Secret Service on Linux.
- `ValidationErrors` serializes as Laravel's bag (`{ "field": ["message"] }`),
  and the generated forms render it per field.
- Abilities gate what the *webview* may call. Nothing knows who the user is.

## Design

### `Http`: the client

A thin layer over `reqwest`, shaped like Laravel's facade. It's behind a new
`http` feature.

```rust
let res = ctx.get::<Http>()
    .get("https://api.example.com/rates")
    .query(&[("base", "NOK")])
    .timeout(Duration::from_secs(10))
    .retry(3, Duration::from_millis(200))   // on connect errors and 5xx, not 4xx
    .send()
    .await?;
let rates: Rates = res.json()?;
```

- It sends `Accept: application/json`, and JSON bodies via `.json(&body)`.
- Every request has a timeout (30 s by default). Retries happen only on
  connection errors and `5xx`, and only for idempotent methods unless asked.
- HTTPS is required, except for loopback, as for the updater.
- A redirect to another host drops `Authorization` and cookies. That's tested
  against a server that answers `302` to another host.
- Logs (`elyra::http`) show the method, the host, the path, the status and
  the duration. They never show the query, the body or headers, since those
  can hold personal data or the token.

### `Backend`: the Laravel API

`Backend` is an `Http` with a base URL, the signed-in user's token, and
Laravel's errors.

```rust
App::new().backend(
    Backend::new(config.string("backend.url"))
        .token_route("/api/sanctum/token")  // the defaults
        .revoke_route("/api/sanctum/token") // DELETE
        .user_route("/api/user"),
)
```

Each answer that isn't a success becomes a `BackendError`, which turns into
`elyra::Error` as a command expects:

| Laravel answers | `BackendError` | What the app sees |
| --- | --- | --- |
| `422 { message, errors }` | `Validation(ValidationErrors)` | the same bag a local command returns: per field in a form, `-32602` with the messages over MCP |
| `401` | `Unauthenticated` | the token is dropped from the keychain, and `elyra:auth` says signed out |
| `403 { message }` | `Forbidden(message)` | a command error with Laravel's message |
| `404` | `NotFound` | a command error |
| `419` | `Expired` | as `401`: tokens don't use sessions, so it means a misrouted request |
| `429` + `Retry-After` | `TooManyRequests { retry_after }` | a command error that says when to try again |
| `5xx` | `Server(status)` | a command error, without the HTML of an error page |
| no connection, DNS, TLS, timeout | `Unreachable` | a command error with `kind: "offline"` the UI can tell apart |

A successful body is decoded as the command's type. Laravel's two paginator
shapes both decode into `Page<T>`, the type `make:resource` already uses.
Plain `paginate()` already has `Page`'s fields (`data`, `total`, `per_page`,
`current_page`, `last_page`). An API Resource's `{ data, links, meta }`
carries the same numbers in `meta`. `Page` lives in `elyra::db` today, so it
moves to the framework (re-exported from `elyra::db`, where it is now). An app
whose data is all on the backend then needn't enable `database`.

### Signing in

```rust
let auth = ctx.get::<Auth>();
auth.sign_in("ada@example.com", "secret").await?;   // a 422 for bad credentials
let user: User = auth.user().await?;                // GET /api/user, cached
auth.sign_out().await?;                             // DELETE the token, forget it
```

- **The token route** follows Laravel's own example for mobile apps. It gets
  `email`, `password` and `device_name`, and answers the plain-text token. The
  device name is the machine's and the app's ("Ada's MacBook · CRM"), so the
  user recognizes it on the web's "devices" page and can revoke it.
- **The token lives only in the keychain**, under the app and the backend's
  URL. It never reaches the webview, which calls commands and never sees a
  credential. A script injected into the page can't send the token anywhere;
  it can only do what commands allow.
- **Signing out** revokes the token on the server. That's best-effort: offline,
  it's forgotten locally all the same, and the user can revoke it from the
  web.
- **A `401` anywhere** signs the user out locally and emits
  `elyra:auth { signedIn: false, reason: "expired" }`.

For the frontend, the runtime gets an `auth` store and calls. They run as
built-in commands, behind the existing guard:

```ts
import { auth, onSignedOut } from "@elyra/runtime";
await auth.signIn(email, password);   // throws the validation bag on a 422
$auth                                  // { signedIn, user }
await auth.signOut();
onSignedOut((reason) => toast("Please sign in again"));
```

### Live queries over the backend

A live command that reads from the backend reads no table, so a write
wouldn't re-run it. `Backend` gives it keys instead:

- A `GET` inside a live command depends on its **resource path**: the path
  without trailing ids and without the query. `/api/customers?page=2` and
  `/api/customers/12` both depend on `backend:/api/customers`.
- A successful `POST`, `PUT`, `PATCH` or `DELETE` invalidates its resource
  path. So `customers_store` on the backend re-runs the open lists, in this
  app.
- `ctx.depends_on` and `ctx.invalidate` remain for anything that isn't shaped
  like `apiResource`.

Changes made elsewhere, on the web or by another user, come with RFC 0006.

### The Laravel side

Nothing beyond Sanctum, and three routes. The guide carries them, from
Laravel's own example:

```php
// php artisan install:api
Route::post('/sanctum/token', function (Request $request) {
    $request->validate(['email' => 'required|email', 'password' => 'required', 'device_name' => 'required']);
    $user = User::where('email', $request->email)->first();
    if (! $user || ! Hash::check($request->password, $user->password)) {
        throw ValidationException::withMessages(['email' => ['The provided credentials are incorrect.']]);
    }
    return $user->createToken($request->device_name)->plainTextToken;
});
Route::delete('/sanctum/token', fn (Request $r) => $r->user()->currentAccessToken()->delete())->middleware('auth:sanctum');
Route::get('/user', fn (Request $r) => $r->user())->middleware('auth:sanctum');
```

### Testing: a fake backend

Like Laravel's `Http::fake()`, a test stubs the backend and asserts on what was
sent. There's no network and no PHP:

```rust
let app = TestApp::new(app()).fake_backend(|fake| {
    fake.get("/api/customers", json!({ "data": [], "meta": { "total": 0 } }))
        .post("/api/customers", 422, json!({ "message": "…", "errors": { "email": ["Taken."] } }));
});
let errors = app.invoke_validation_errors("customers_store", (input,)).await;
assert_eq!(errors["email"], ["Taken."]);
app.backend().assert_sent("POST", "/api/customers");
```

A signed-in user is one call: `.signed_in_as(json!({ "id": 1, "name": "Ada" }))`.

## Security

- **The token stays in the keychain**, never in the webview, logs or files. If
  the platform has no keychain (Linux without Secret Service), sign-in fails
  with a message saying so. There's no plain-text fallback.
- **The backend's URL comes from the app**, not the webview. HTTPS is required
  except for loopback, and `Authorization` isn't carried across a redirect to
  another host.
- **The server decides what the user may do.** Laravel's policies and the
  token's abilities apply as on the web. The app's abilities still say what
  the webview, or an MCP agent, may call. An agent calling a command acts as
  the signed-in user, through the same API.

## Alternatives considered

- **Sanctum's SPA (cookie) mode.** It's made for a browser on the same domain,
  with CSRF cookies. A desktop app is the "mobile application" case, and
  Sanctum's docs say to use tokens for it.
- **Call Laravel from the webview with `fetch`.** It's simpler, but the token
  would sit where page scripts can read it. It also bypasses commands, so
  middleware, MCP and live queries wouldn't see the calls.
- **Generate a client from an OpenAPI spec.** That's useful when a spec
  exists, and could come later. Most Laravel apps don't publish one, and
  serde types written once do the job.
- **A Laravel package (`elyra/laravel`) first.** Three routes don't need a
  package. RFC 0006 and 0007 may add server pieces worth packaging; that's the
  time.

## Implementation plan

1. **`Http`** (feature `http`): the builder, JSON, timeouts, retries, the HTTPS
   rule, redirects and redacted logs. Plus `TestApp`'s fake, with recorded
   requests.
2. **`Backend` and Laravel's errors**: the mapping above, `Page<T>` from both
   paginator shapes, and `BackendError` into `elyra::Error` (`422` as
   `ValidationErrors`).
3. **`Auth`**: sign-in, sign-out and the user. The keychain, the device name,
   `401` handling, `elyra:auth`, and the runtime's `auth` store and calls.
4. **Live queries**: resource-path keys for `GET`s, and invalidation after
   writes.
5. **`make:resource --backend /api/customers`**: the same generated screens,
   with commands that call the backend instead of the local database.
6. **Docs, an example, and CI against a real Laravel app.** That's
   `composer create-project laravel/laravel`, `install:api`, the three routes
   and `php artisan serve` on the CI runner, then a test that signs in,
   creates, gets a `422`, lists, and signs out.

## Open questions

1. **How to sign in:** Sanctum email/password tokens only in v1 (recommended;
   it's what Laravel documents for apps), or also OAuth with PKCE (Passport,
   the system browser and a deep-link callback) for SSO now?
2. **The Laravel side:** the three routes in the guide (recommended for now), or
   an `elyra/laravel` composer package from the start?
3. **Live-query keys:** automatic, from resource paths (recommended: it fits
   `apiResource`, and explicit keys stay available), or explicit only?
4. **`make:resource --backend`:** in this RFC as step 5 (recommended: it's the
   payoff, generated screens over a Laravel API), or a later one?
5. **CI against a real Laravel app** (recommended: the only way to know the
   `422` and paginator shapes stay right), or fakes only?
6. **No keychain:** refuse to sign in (recommended), or fall back to a `0600`
   file?

## Decisions

Settled on 2026-10-10, all as recommended:

1. **Sanctum email/password tokens** in v1. OAuth with PKCE (SSO) can come
   later; `Auth` keeps the sign-in method separate so it can.
2. **The three routes in the guide.** A composer package waits until RFC 0006
   or 0007 needs server pieces.
3. **Automatic live-query keys** from resource paths, with `depends_on` /
   `invalidate` for the rest.
4. **`make:resource --backend`** is step 5 of this RFC.
5. **CI against a real Laravel app**, on the runner with `php artisan serve`.
6. **No keychain, no sign-in.** There's no plain-text fallback.
