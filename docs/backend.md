# A Laravel backend

An Elyra app can be the desktop half of a product whose web half is Laravel,
served by [Askr](https://github.com/kwhorne/askr) or any other server. The app
signs the user in with a [Sanctum](https://laravel.com/docs/sanctum) token,
calls the same API as the web, and shows Laravel's validation messages in its
forms as if they were its own. The design is
[RFC 0005](proposals/0005-laravel-backend.md).

## In five lines

```toml
elyra = { …, features = ["backend"] }   # with "database" for make:resource
```

```rust
App::new().backend(Backend::new("https://crm.example.com"))

#[command(live, can = "customers.view")]
async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
    Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
}
```

```svelte
<script>
  import { auth } from "@elyra/runtime";
</script>
{#if $auth.signedIn}Hi {$auth.user.name}{:else}<SignIn />{/if}
```

Or generate the whole resource, with its screens:
`rata make:resource Customer --backend /api/customers --generate name:string email:email`.

## The Laravel side

Sanctum and three routes, in `routes/api.php` (Laravel prefixes them with
`/api`, as `Backend` expects):

```bash
php artisan install:api      # Sanctum; then add `HasApiTokens` to App\Models\User
```

```php
Route::post('/sanctum/token', function (Request $request) {
    $request->validate(['email' => 'required|email', 'password' => 'required', 'device_name' => 'required']);
    $user = User::where('email', $request->email)->first();
    if (! $user || ! Hash::check($request->password, $user->password)) {
        throw ValidationException::withMessages(['email' => ['The provided credentials are incorrect.']]);
    }
    return $user->createToken($request->device_name)->plainTextToken;
});

Route::middleware('auth:sanctum')->group(function () {
    Route::delete('/sanctum/token', fn (Request $r) => $r->user()->currentAccessToken()->delete());
    Route::get('/user', fn (Request $r) => $r->user());
    Route::apiResource('customers', CustomerController::class);
});
```

Different paths? `Backend::new(url).token_route(..).revoke_route(..).user_route(..)`.
[`scripts/laravel-backend.sh`](../scripts/laravel-backend.sh) builds a complete
app like this. CI runs Elyra against it.

## Calling the API

```rust
let backend = ctx.get::<Backend>();
let page: Page<Customer> = backend.get("/api/customers").query(&query).json().await?;
let customer: Customer   = backend.get(format!("/api/customers/{id}")).resource().await?;
let created: Customer    = backend.post("/api/customers").body(&input).resource().await?;
backend.delete(format!("/api/customers/{id}")).send().await?;
```

- **The token:** it's sent as `Bearer` when the user is signed in. Requests
  ask for JSON, which is what makes Laravel answer a failed validation with
  `422` JSON instead of a redirect.
- **`.json()` and `.resource()`:** `.json()` decodes the body as the type. A
  `204` decodes as `()`. `.resource()` also unwraps an API Resource's
  `{ "data": … }`, so a controller can return a model or a resource.
- **Pages:** `Page<T>` reads both of Laravel's paginators, `paginate()`'s flat
  one and an API Resource collection's `{ data, links, meta }`.
- **The rest of the request:** `.query`, `.header`, `.timeout` and `.retry`
  work as on [`Http`](http.md), which `Backend` is built on.
- **Updates send the whole record.** An `Option` that's `None` goes as
  `null`, which is what a form means. For a partial `PATCH`, mark the
  fields `#[serde(skip_serializing_if = "Option::is_none")]`.

## Laravel's answers

| Laravel | `BackendError` | The frontend sees |
| --- | --- | --- |
| `422 { errors }` | `Validation` | a `ValidationError`, per field, as from a local command |
| `401`, `419` | `Unauthenticated` | kind `unauthenticated`; the token is forgotten and `elyra:auth` says `expired` |
| `403 { message }` | `Forbidden` | kind `denied`, with Laravel's message |
| `404` | `NotFound` | kind `not-found` |
| `429` + `Retry-After` | `TooManyRequests` | kind `too-many-requests`, saying when |
| `5xx` | `Server` | kind `server`, without the error page |
| no answer | `Unreachable` | kind `offline` |

`?` in a command turns each into the right error. The kind reaches the
frontend as `CommandError.kind`, from a command and from a live query's
re-run, so the UI can say "you're offline" rather than show an error.

## Signing in

```rust
let auth = ctx.get::<Auth>();
auth.sign_in("ada@example.com", "secret").await?;   // a 422 for bad credentials
let user: User = auth.user().await?;                // GET /api/user, cached
auth.sign_out().await?;                             // revoke, forget
```

- **The token lives in the OS keychain only**, never in the webview or a
  file. It's restored when the app starts, so a signed-in user stays signed
  in. With no keychain (Linux without Secret Service), sign-in fails, and
  the token the server just issued is revoked.
- **The device is named `<host> · <app>`**, so the user recognizes it among
  their tokens on the web.
- **A `401` from any request signs the user out**, and `elyra:auth` carries
  `{ signedIn: false, reason: "expired" }`.

From the frontend, through `/__auth/*` (`Capability::Auth`, granted by
default):

```ts
import { auth, onSignedOut, ValidationError } from "@elyra/runtime";

try {
  await auth.signIn(email, password);
} catch (e) {
  if (e instanceof ValidationError) errors = e.errors;   // { email: ["…"] }
}
$auth;                                                    // { signedIn, user, reason }
onSignedOut((reason) => reason === "expired" && toast("Please sign in again"));
```

## Live queries

A live command that reads from the backend updates itself when this app
writes. Inside a live command, a `GET` depends on its **resource**: the path
without the query and without trailing ids (a number, UUID or ULID). So
`/api/customers?page=2` and `/api/customers/12` are both
`backend:/api/customers`. A successful `POST`, `PUT`, `PATCH` or `DELETE` to
that resource re-runs them. `ctx.depends_on` and `ctx.invalidate` cover paths
that aren't shaped like an `apiResource`.

As with the database, a live command's re-run may not write.

Changes made on the web come with [RFC 0006](roadmap.md#next--open):
Laravel's broadcasting in.

## `make:resource --backend`

```bash
rata make:resource Customer --backend /api/customers --generate name:string email:email 'phone:string?'
```

It makes the same resource as `--generate`, with the same screens, types,
abilities and events. The difference is that its commands call the API:

- `index` sends `search`, `sort`, `direction`, `page` and `per_page` as the
  query.
- `show`, `store`, `update` and `destroy` are one request each.

The model is a plain serde struct, with Laravel's ISO timestamps. There's no
migration or seeder, because the table is the server's. Validation is the
server's too: a `422` lands in the form, per field. The tests run against an
`HttpFake`.

rata prints what the Laravel side needs (`Route::apiResource`). The
`CustomerController` in `scripts/laravel-backend.sh` shows an `index` that
reads that query. For now the project needs the `database` feature too,
because the resource registry also lists migrations.

## Testing

```rust
use elyra::auth::MemoryTokens;
use elyra::http::{Http, HttpFake, Method};

let fake = HttpFake::new()
    .get("https://crm.test/api/customers", 200, json!({ "data": [], "total": 0, "per_page": 25, "current_page": 1, "last_page": 1 }))
    .post("https://crm.test/api/customers", 422, json!({ "errors": { "email": ["Taken."] } }));
let app = TestApp::new(
    App::new()
        .backend(Backend::new("https://crm.test"))
        .token_store(Arc::new(MemoryTokens::new().with("backend-token:https://crm.test", "1|token")))  // signed in
        .commands(commands())
        .swap(Http::fake(fake.clone())),
);
let errors = app.invoke_validation_errors("customers_store", (input,)).await.unwrap();
fake.assert_sent("POST", "https://crm.test/api/customers");
```

`TestApp` keeps tokens in memory unless told otherwise, so a test never
touches the keychain. `MemoryTokens::failing()` behaves like a system without
one.

Against a real Laravel app:

```bash
scripts/laravel-backend.sh /tmp/api && (cd /tmp/api && php artisan serve --port 8765 &)
ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend --test laravel_backend
```

## Related

- [HTTP client](http.md) · [Live queries](live-queries.md) ·
  [Resources](resources.md) · [Secrets](secrets.md) · [Security](security.md)
