# RFC 0040 — The two halves agree: Laravel's API checked against the desktop app

**Status:** implemented (2026-11-20) · **Target:** 0.50.0 · builds on [RFC 0005](0005-laravel-backend.md), [RFC 0010](0010-laravel-abilities.md) and [RFC 0036](0036-rata-check.md) · see [Decisions](#decisions)

## Summary

An Elyra app with a Laravel backend is two codebases that have to agree:
the routes the desktop calls, the fields of what comes back, the fields a
form sends and the rules the server holds them to, the abilities a command
needs. Today nothing checks that they do. A field renamed in a
`JsonResource`, made nullable in a migration, or made `required` in a
`FormRequest` breaks the desktop app at run time, for a user, and the
error ("missing field `phone`") says nothing about the server.

This RFC makes **Laravel describe its API** and **`rata check` compare
the desktop app with it**:

```bash
# In the Laravel app, in its CI, with its test database seeded:
php artisan elyra:contract --as=ada@example.com > elyra-contract.json

# In the desktop app's CI:
rata check --backend elyra-contract.json
```

```text
error    customers_show returns Customer, whose `phone: String` GET /api/customers/{id} doesn't send.
         The server's resource has `phone_number`. Rename the field, or #[serde(rename = "phone_number")].
error    customers_store sends CustomerInput, which has no `country`; POST /api/customers requires it.
         Add `country` to CustomerInput, or make the rule `sometimes`.
warning  Customer's `email: String` is null in GET /api/customers for 2 of 25 records.
         Make it Option<String>, or the list fails to decode for those.
warning  invoices_destroy needs `invoices.delete`, which no user on the server has.
error    notes_archive calls POST /api/notes/{id}/archive, which the server doesn't have.
```

- **Laravel says what it has**: routes, the rules each one validates,
  what its reads actually answer (sampled as a real user), and the
  abilities there are.
- **The desktop says what it uses**: which command calls which route,
  what it sends and what type it decodes the answer into, and the
  abilities its commands need.
- **The same `rata check`**: the same levels, `--json`, `--strict`,
  `[check] allow`, and `rata mcp dev`'s `app_check` for the coding agent.

## Motivation

- **It's the most common break in a two-part codebase.** The web half
  moves faster, its tests pass, and the desktop's tests run against an
  `HttpFake` that says what the desktop expects, so they pass too.
- **The error comes late and far away.** A decode error in the user's
  window, from a list that was fine yesterday. Sentry on the server shows
  nothing, because the server answered `200`.
- **Both halves already know their own shape.** Laravel knows its routes,
  `FormRequest` rules and permissions; the desktop knows its commands,
  their types (specta, for codegen) and their abilities (RFC 0036). What's
  missing is putting them side by side.

## Current state (0.49)

- **`rata check`** (RFC 0036) runs the app in a check mode and compares
  the frontend, the grants and the commands. It knows nothing of Laravel.
- **`make:resource --backend`** writes a resource's Rust types from the
  fields it was given, and prints the Laravel side. After that, the two
  drift apart without anyone noticing.
- **`elyracode/elyra-backend`** has artisan commands (`elyra:release`)
  and route macros. It knows nothing of the desktop either.

## Design

### Laravel: `php artisan elyra:contract`

A JSON document, written to stdout:

```json
{
  "laravel": "12.x", "package": "0.51.0", "sampled_as": "ada@example.com",
  "routes": [
    { "method": "GET", "uri": "/api/customers", "middleware": ["auth:sanctum"],
      "answer": { "kind": "page", "fields": { "id": "int", "name": "string", "email": "string|null", "phone_number": "string" }, "sampled": 25 } },
    { "method": "POST", "uri": "/api/customers",
      "rules": { "name": "required|string|max:255", "email": "required|email", "country": "required|string" } }
  ],
  "abilities": ["customers.view", "customers.create", "customers.update", "invoices.view"]
}
```

- **Routes** come from `Route::getRoutes()`: method, URI, middleware.
  Only `api` routes, and only those under `auth:sanctum` or the package's
  own, which are what a desktop app calls.
- **Rules** come from a `FormRequest` the action type-hints: the command
  makes the request and calls `rules()`. A controller that calls
  `$request->validate([..])` inline can't be read without running it; its
  route says `"rules": null`, and the desktop's side isn't checked for it.
- **Answers are sampled**, not inferred. For each `GET` route, the command
  sends a request through the kernel as the user `--as` names, as the
  desktop would, and records the shape of what came back: each field's
  type, and whether it was ever `null`, across the records it saw. A route
  with a `{parameter}` takes one from a list route's answer when it can
  (`/api/customers/{customer}` from `/api/customers`'s first `id`), and is
  skipped otherwise. No writes are sent, and the sample is only as good
  as the database: CI's seeded test database, typically.
- **Abilities** are what `Route::elyraAbilities()` would answer for
  anyone: every permission (spatie), or `config('elyra.abilities')`.

### The desktop: what it uses

`rata check` already runs the app in a check mode. With `--backend`, the
check mode adds what the commands use of the backend:

- **Each command's types**: its argument types and its return type, as
  JSON Schema (what MCP and the assistant already build from specta).
- **Which routes it calls**: read from the source with `syn`, as
  `rata mcp dev` reads it. A `Backend` call with a literal path, a
  `format!` of one (`format!("/api/customers/{id}")` →
  `/api/customers/{}`), or a resource's `PATH` constant (what
  `make:resource --backend` writes) is known. A path built otherwise is
  listed as **not checked**, with the command, rather than guessed.

### The checks

| Check | Level | What it finds |
| --- | --- | --- |
| `backend-route-missing` | error | A command calls a method and path the server doesn't have. |
| `backend-field-missing` | error | A field the command's return type requires (not `Option`, no `#[serde(default)]`) isn't in the sampled answer. Says which field the answer has that's closest by name. |
| `backend-field-nullable` | warning | A required field was `null` in some sampled records. |
| `backend-field-type` | error | A field's type can't decode what was sent (`i64` for a string). |
| `backend-input-required` | error | The route's rules require a field the command's input type doesn't have. |
| `backend-input-unknown` | warning | The input type has a field the route's rules don't name: Laravel's `validated()` drops it. |
| `backend-ability-unknown` | warning | A command's ability that no user on the server has, nor a wildcard covers. |
| `backend-not-checked` | info | A command whose path, or a route whose rules, couldn't be read. |

Nesting follows the shapes: a `Page<Customer>` is checked as Laravel's
paginator (`data[]`), a resource as its `data` or the model itself, as the
desktop already decodes them.

### Where it runs

- **In CI**, with two repositories: the Laravel app's CI writes the
  contract as an artifact or commits it (`elyra-contract.json`), and the
  desktop app's CI checks against it. With one repository, one job does
  both.
- **Locally**, with the Laravel app beside the desktop app
  (`[check] backend = "../web"` in `elyra.toml`), `rata check --backend`
  runs `php artisan elyra:contract` there itself, as the user
  `[check] backend_as` names; given a file instead, it reads the file.
- **For the coding agent**, `rata mcp dev`'s `app_check` includes the
  backend findings when `elyra.toml` says where the contract is.

### Security

- The contract has the API's shape: routes, rule strings, field names and
  types, ability names. Never a value: the sampler records types and
  null-ness, not data, so a contract from a production database leaks no
  record. It's still the API's map, so it belongs with the code, not in a
  public place.
- `elyra:contract` sends only `GET`s, through the kernel, as the user
  `--as` names, so it sees what that user may see and nothing else.

### Testing

- **Laravel**: the command against the fixture app CI already builds
  (`scripts/laravel-backend.sh`): its customers routes, the `FormRequest`
  of a new route, the paginator, a nullable column, the abilities.
- **rata**: each check over a contract file and a test app, the levels,
  `allow`, `--json`; the source reading over the path forms above.
- **End to end** in CI's Laravel job: the fixture's contract against the
  example app's backend resource, clean; then a renamed field, found.

## Implementation plan

1. `php artisan elyra:contract`: routes, `FormRequest` rules, abilities;
   then the sampled answers.
2. The check mode's types per command; `syn` for the routes each calls.
3. The checks in `rata check --backend <file>`, with `allow` and `--json`.
4. `[check] backend` in `elyra.toml`, running the artisan command locally;
   `app_check`.
5. Docs: `docs/cli.md` (`rata check`), `docs/backend.md` (a section), the
   package's README, the Boost guideline.

## Not in this RFC

- **Generating Rust types from Laravel** (or the reverse). The check says
  where they differ; which side is right is the developer's call.
- **Reading inline `$request->validate([..])`**, or a controller's code,
  without running it. A route without a `FormRequest` isn't checked.
- **Writes' answers.** A `POST` isn't sent to sample its answer; what
  `store` and `update` return is usually the resource the `GET` sampled.
- **Broadcast events' payloads** (RFC 0006): only routes.

## Alternatives considered

- **OpenAPI** (Scribe, Scramble) as the contract. Many Laravel apps have
  one, and `rata check --backend openapi.json` could read it too. But a
  generated OpenAPI document is inferred, and as often wrong about
  responses as the desktop's own types are; and an app without one would
  have to adopt it first. A later addition, if apps ask.
- **Inferring answers from `JsonResource::toArray`** with static analysis.
  Too often a resource is code (conditional fields, `whenLoaded`), and
  wrong inferences make a check nobody trusts. Sampling is what the
  server actually sends.
- **Checking at run time** in the desktop (a decode error reported with
  the route): useful, and the diagnostics already have the log, but late:
  it's the user who finds it.

## As built

- **`php artisan elyra:contract`** in `elyracode/elyra-backend`, with
  `--as`, `--prefix` (`api`) and `--samples` (25). It sends each `GET`
  through the HTTP kernel with a Sanctum token it makes for the user and
  deletes after, so the user model needs `HasApiTokens`. A record's route
  takes the first `id` of its list's answer. Rules are read from a
  `FormRequest` the action type-hints; anything else is `"rules": null`.
  Abilities are spatie's permissions when the package is installed, else
  `config('elyra.abilities')`.
- **The check mode** adds `types`: each command's arguments and return
  type as JSON Schema, the way MCP builds them (specta through serde's
  wire shapes), with their `$defs`.
- **rata reads the calls with `syn`**: a `get`, `post`, `put`, `patch` or
  `delete` on a receiver that names the backend (`ctx.get::<Backend>()`,
  a `backend` variable), with a literal, a `format!` (whose `{PATH}` is a
  constant in the file) or a constant; `.body(&arg)` names the input. A
  path built otherwise is `backend-not-checked`.
- **`--backend <file or folder>`**, and `[check] backend` / `backend_as`
  in `elyra.toml`, so `rata mcp dev`'s `app_check` includes it. A folder
  runs `php artisan elyra:contract` there.
- **A return type** is matched as a record, an `Option` or `Vec` of one, a
  page (`{ data: [T] }`) or a resource (`{ data: T }`). An `Option` field
  may be missing or null; a required one may not.
- **Tested**: rata's unit tests over the source forms and every check;
  the check mode's types in `tests/check_shape.rs`; CI's Laravel job runs
  `elyra:contract` on the fixture app. By hand: a `make:resource Customer
  --backend` app checked against the fixture's contract is clean, and
  renaming `email` to `mail` in its model gives "customers_index decodes
  `mail`, which GET /api/customers doesn't send; it sends `email`."

## Decisions

Settled on 2026-11-20, all as recommended:

1. **The answers are sampled**, as the user `--as` names, through the
   kernel, `GET`s only; field names, types and null-ness, never values.
2. **The contract may be committed or kept as a CI artifact**; the docs
   show committing it, so a pull request on the web half shows its diff.
3. **`backend-input-unknown` is a warning.**
4. **A file only**: no route that describes the API over HTTP in this
   RFC.
5. **Target 0.51.0**, after 0.50.0.
