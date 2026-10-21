# RFC 0010 — Laravel's abilities on the desktop: what the signed-in user may do

**Status:** proposed (2026-10-21) · **Target:** 0.18.0 · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0006](0006-laravel-broadcasting.md) and [RFC 0007](0007-offline-and-sync.md) · see [Open questions](#open-questions)

## Summary

Who may delete a customer is decided by Laravel: a policy, a Gate, or
spatie/laravel-permission's roles. The desktop app doesn't know. Its
abilities (`#[command(can = "customers.delete")]`) are granted once, in code,
to every user alike, so a clerk sees a Delete button that the server answers
with `403`, and, offline, a delete is queued that the server will refuse
tomorrow. The desktop should know what *this* user may do, ask nobody but
Laravel, and keep knowing while offline.

```rust
App::new()
    .allow_abilities(resources::abilities())       // what the app's commands may need
    .backend(
        Backend::new("https://crm.example.com")
            .abilities()                            // …and what this user has, from Laravel
            .offline(),
    )
```

```php
// routes/api.php (elyracode/elyra-backend)
Route::middleware('auth:sanctum')->group(function () {
    Route::elyraAbilities();   // GET /api/abilities: ["customers.view", "customers.create", …]
});
```

```svelte
<script>
  import { can } from "@elyra/runtime";
</script>

{#if $can("customers.delete")}
  <button onclick={remove}>Delete</button>
{/if}
```

- **From Laravel.** After sign-in the app reads the user's abilities from
  `GET /api/abilities`: spatie's permissions when the package is there, else
  the Gate abilities the app lists. Signing out forgets them.
- **Both must agree.** A `can = ".."` command runs when the app grants the
  ability (as today) *and* the user has it. A command the user can't run is
  refused at once, locally, without a request: online, offline, and before
  it would be queued.
- **The frontend knows.** The `can` store answers `$can("customers.delete")`,
  and the screens `make:resource` generates hide what the user can't do.
- **It stays current.** Read again on focus and on reconnect, and at once
  when Laravel broadcasts that the user's roles changed. Offline, the last
  list is used.

## Motivation

- **A button that always fails is a bug the user sees.** The web half hides
  actions with `@can`; the desktop half should too, from the same rules.
- **Offline makes it worse.** A write the user isn't allowed waits in the
  outbox and fails later, when they've moved on (RFC 0007). Knowing locally
  refuses it while they're still looking at the form.
- **The rules are Laravel's, and should stay there.** Copying roles into the
  desktop app would drift from the server. One list, from the server, per
  user.
- **The pieces exist.** Commands declare their ability; generated resources
  list theirs in `ABILITIES`; `Auth` knows who's signed in; broadcasting
  reaches the user's own channel (RFC 0008); the outbox keeps a per-user copy
  of what the server said (RFC 0007).

## Current state (0.17)

- `App::allow_ability(..)` / `allow_abilities(..)` grant abilities to the
  webview once, at startup, for every user. `Policy::grants_ability` checks a
  fixed list; `*` wildcards are supported.
- A command refused for a missing ability answers kind `forbidden` (the IPC
  guard's kind). A `403` from Laravel is kind `denied`.
- The frontend can't ask what's granted: it learns by trying.
- `Auth` knows the signed-in user (`/api/user`) and tells watchers when it
  changes. The user's private channel `App.Models.User.{id}` is followed when
  notifications are on.
- `make:resource` generates `ABILITIES` (`customers.view`, `.create`,
  `.update`, `.delete`) and a hint to grant them; the views show every
  button.

## Design

### Turning it on

```rust
Backend::new(url)
    .abilities()                          // GET /api/abilities
    .abilities_route("/api/me/abilities") // elsewhere
```

Without `.abilities()`, nothing changes: the app's grants are all there is.

### Where they come from

`GET /api/abilities`, as the signed-in user, answers a list of strings:

```json
["customers.view", "customers.create", "invoices.*"]
```

- `*` at the end covers a namespace, as in `allow_ability`.
- `Route::elyraAbilities()` in `elyracode/elyra-backend` answers it. With
  spatie/laravel-permission installed it's the user's permissions
  (`$user->getAllPermissions()->pluck('name')`, roles included). Without it,
  it's every ability in `config('elyra.abilities')` that `Gate::allows(..)`
  for the user, so plain `Gate::define` and policy abilities work too.
- Any other source works: it's one route that returns a list.

### What may run

A command with `can = "customers.delete"` runs from the webview when:

1. the app grants it (`allow_ability`, as today): what the app lets its
   frontend reach at all; **and**
2. the signed-in user has it, by Laravel's list.

Signed out, or before the list has been read, the user has none: a gated
command is refused. Commands without `can` are unchanged. Rust callers
(tests, jobs, providers) aren't gated, as today.

A command refused because the user lacks the ability answers kind `denied`,
the kind a `403` from Laravel has, so the UI handles both the same way. The
app's own missing grant stays `forbidden`.

### Per record

Laravel's policies decide per record too: Ada may update her customers, not
Bob's. That can't be a flat list. The convention Laravel apps already use is
a `can` object in the API Resource:

```php
public function toArray($request): array
{
    return [
        ...parent::toArray($request),
        'can' => [
            'update' => $request->user()->can('update', $this->resource),
            'delete' => $request->user()->can('delete', $this->resource),
        ],
    ];
}
```

The generated views read `record.can?.update` when it's there and fall back
to the user's ability otherwise. The server still decides: a `403` is still
shown as it is today.

### Keeping it current

- Read at sign-in, when a window gets focus, and when the connection comes
  back (RFC 0007).
- At once when Laravel broadcasts `AbilitiesChanged` on the user's private
  channel, which `elyracode/elyra-backend` dispatches when spatie's roles or
  permissions change for that user (and an app can dispatch itself).
- A list that changed re-runs the live queries and tells the frontend's
  `can` store.

### Offline

The last list is kept per user, with the offline copy (RFC 0007), and used
while the server can't be reached. A write the user isn't allowed is refused
before it's queued.

### In Rust

```rust
let abilities = ctx.get::<Abilities>();
if abilities.allows("customers.export") { … }
abilities.list();   // what Laravel said, for this user
```

`Abilities` is in the container with `.abilities()`, and publishes its list
on `elyra:abilities`.

### The frontend

```ts
import { can, abilities } from "@elyra/runtime";

$can("customers.delete")     // true / false, kept current
$abilities                   // the user's list
```

`can` is a store of a function, as `$t` is. Before the list is read it
answers `false`.

### `make:resource --backend`

- The Index, Show and Form views hide New, Edit and Delete when
  `$can("<plural>.<verb>")` is false, and prefer `record.can.update` /
  `record.can.delete` when the API sends them.
- rata prints the Laravel side: the permissions to create (spatie) or the
  Gate abilities to list in `config/elyra.php`, and `Route::elyraAbilities()`.

### Testing

```rust
let app = TestApp::new(app().swap(Http::fake(
    HttpFake::new().get(url("/api/abilities"), 200, json!(["customers.view"])),
)));
app.sign_in(..).await;
assert_eq!(app.invoke_err_kind("customers_destroy", (1,)).await.as_deref(), Some("denied"));
```

`TestApp` has `as_user_with_abilities(&[..])` for tests that don't go
through sign-in.

## Security

- **The server still decides.** This is the desktop agreeing with Laravel,
  not replacing it: every request is authorized by Laravel as before, and a
  `403` is handled as before.
- **The list can only narrow.** It never grants what the app didn't
  (`allow_ability`), so a server, or a stale list, can't open a command the
  app keeps from its webview.
- **The page can't change it.** The list comes from Laravel through Rust;
  the frontend can read it, not set it.
- **Per user.** The kept list is the signed-in user's, and it's forgotten on
  sign-out and when someone else signs in.

## Alternatives considered

- **Abilities inside `/api/user`.** One request fewer, but it mixes the
  user's profile with what they may do, and `/api/user` is often an API
  Resource the app shapes for the web. A dedicated route is easy to cache
  and to change.
- **Ask Laravel per action** (`GET /api/can?ability=..`). Exact, but a
  request per button, and nothing offline.
- **Roles, not abilities.** The desktop would need to know what each role
  allows: the rules would live in two places.
- **Hide only; don't gate commands.** Less to build, but a hidden button
  isn't a boundary, and offline writes would still be queued to fail.

## Implementation plan

1. **`Abilities`.** `.abilities()` / `.abilities_route(..)`, the read at
   sign-in, forgetting at sign-out, `allows` / `list`, the kept copy per user
   offline, and `elyra:abilities`.
2. **The gate.** A `can` command runs only when the app grants it and the
   user has it; kind `denied` otherwise; refused before queueing offline.
3. **Keeping it current.** Focus, reconnect, `AbilitiesChanged` on the
   user's channel, and the live queries re-run.
4. **The frontend.** `/__auth/abilities`, the `can` and `abilities` stores.
5. **Laravel and rata.** `Route::elyraAbilities()` and `AbilitiesChanged` in
   `elyracode/elyra-backend`; `make:resource --backend` views that hide what
   the user can't do and read `record.can`; the Laravel side printed; CI
   against the real Laravel app with spatie's permissions; the docs.

## Open questions

1. **Combining:** the app's grant *and* the user's ability (recommended: the
   list only narrows), the user's alone, or either?
2. **The source:** a dedicated `GET /api/abilities` route (recommended),
   or an `abilities` field in `/api/user`?
3. **Per record:** generated views read `record.can.*` when the API sends it
   and fall back to the user's ability (recommended), or user abilities only?
4. **Freshness:** sign-in, focus, reconnect and an `AbilitiesChanged`
   broadcast (recommended), or sign-in only?
5. **Offline:** keep the last list per user and use it (recommended), or
   deny every gated command while offline?
6. **The refusal's kind:** `denied`, as a `403` from Laravel (recommended),
   or `forbidden`, as the app's own missing grant?
7. **The Laravel side:** `Route::elyraAbilities()` in the package, reading
   spatie's permissions when installed and configured Gate abilities
   otherwise (recommended), or only documented?
8. **In the generated views:** hide what the user can't do (recommended), or
   show it disabled?
