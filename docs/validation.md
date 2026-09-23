# Validation

Commands receive untrusted input from the frontend. `elyra::validation` adds a
Laravel-style validator: check a JSON value against a familiar rule string and
get a per-field error bag that surfaces straight to the UI.

## In a command

Return [`ValidationErrors`] and short-circuit with `?` — just like Laravel's
`$request->validate([...])`:

```rust
use elyra::{command, Ctx, Validator, ValidationErrors};

#[derive(serde::Deserialize, specta::Type)]
struct AccountInput { email: String, age: i64 }

#[command]
async fn create_account(input: AccountInput)
    -> std::result::Result<Account, ValidationErrors>
{
    let data = serde_json::to_value(&input).unwrap_or_default();
    Validator::new(&data)
        .rules(&[
            ("email", "required|email"),
            ("age",   "integer|min:18"),
        ])
        .validate()?;               // -> Err(ValidationErrors) on failure

    Ok(create(input))
}
```

`ValidationErrors` serializes to a Laravel-style bag and its `Display` is that
JSON, so returning it as the command error delivers the structure to the
frontend.

## On the frontend

```ts
import { ValidationError, validationErrors } from "@elyra/runtime";

try {
  await api.create_account({ email, age });
} catch (e) {
  if (e instanceof ValidationError) {
    // e.errors: { email: ["…"], age: ["…"] }
    for (const [field, messages] of Object.entries(e.errors)) {
      showFieldError(field, messages[0]);
    }
  }
}
```

The shell marks these responses with `x-elyra-error-kind: validation`, so the
runtime builds a typed `ValidationError` — no string sniffing. `validationErrors(e)`
still works and returns the bag (or `null`) for code that prefers a plain check.

## Rules

Pipe-separated, `rule:arg` for arguments (`min:18`, `in:a,b,c`). **An unknown
rule panics**, naming the rule and the field — a misspelt `"requried"` must not
quietly let invalid input through. (It used to be ignored.)

**Presence**

| Rule | Passes when |
| --- | --- |
| `required` | present and not null / blank string / empty array or object |
| `required_if:field,v1,v2` | present, whenever `field` is one of the values |
| `required_with:a,b` | present, whenever any of the fields is |
| `required_without:a,b` | present, whenever any of the fields isn't |
| `accepted` | `true`, `1`, `"1"`, `"yes"`, `"on"` or `"true"` (a terms checkbox) |
| `filled` | may be absent, but not present-and-empty |
| `nullable` / `sometimes` | (skip the other rules when the field is absent/null) |

**Type and format**

| Rule | Passes when |
| --- | --- |
| `string` / `integer` / `numeric` / `boolean` / `array` | of that JSON type |
| `email` | looks like an email address |
| `url` | starts with `http://` or `https://` |
| `uuid` | an `8-4-4-4-12` hex UUID |
| `ip` | an IPv4 or IPv6 address |
| `alpha` / `alpha_num` / `alpha_dash` | letters / letters and digits / plus `-` and `_` |
| `digits:N` / `digits_between:a,b` | only digits, exactly N / between a and b of them |
| `regex:pattern` | matches (`/…/` delimiters optional) |
| `date` | `YYYY-MM-DD` or an RFC 3339 timestamp |
| `before:x` / `after:x` / `before_or_equal:x` / `after_or_equal:x` | compared with a date, `today` / `tomorrow` / `yesterday`, or another field |

**Size and membership**

| Rule | Passes when |
| --- | --- |
| `min:N` / `max:N` / `size:N` / `between:a,b` | number, or string/array length, in range |
| `gt:x` / `gte:x` / `lt:x` / `lte:x` | compared with a number or another field |
| `in:a,b` / `not_in:a,b` | value is / isn't one of the list |
| `starts_with:a,b` / `ends_with:a,b` | string starts / ends with one of them |
| `same:field` | equals another field's value |
| `confirmed` | `<field>_confirmation` equals the value |
| `distinct` | no duplicates (within an array, or across a `*` wildcard) |

**Database** (feature `database`; see [below](#database-rules))

| Rule | Passes when |
| --- | --- |
| `unique:table[,column[,except[,id_column]]]` | no row has this value |
| `exists:table[,column]` | a row has this value |

Absent or null fields only meet the presence rules above — everything else is
skipped for them (Laravel's implicit "sometimes"), so optional fields validate
only when provided.

A `regex` alternation contains a `|`, which would split the rule string; pass
rules as a list instead:

```rust
Validator::new(&input).rule_list("code", &["required", "regex:^(ab|cd)[0-9]+$"])
```

## Nested fields and arrays

Fields may be paths. A `*` expands over every element, and errors are keyed by
the concrete path, like Laravel:

```rust
Validator::new(&input).rules(&[
    ("address.city", "required"),
    ("items.*.name", "required|string"),
    ("items.*.qty", "integer|min:1"),
    ("items.*.sku", "distinct"),
    ("items.*.weight", "required_if:items.*.kind,physical"),  // same index
])
```

→ `{"items.1.name": ["The items.1.name field is required."], …}`

A reference to another wildcard field (`items.*.kind` above) reads the element
at the same index. A wildcard over something that isn't there has nothing to
check.

## Database rules

`unique` and `exists` query the database, so they run through the async
`validate_with` / `errors_with`:

```rust
#[command]
async fn register(ctx: Ctx, input: serde_json::Value) -> Result<(), ValidationErrors> {
    let db = ctx.get::<Database>();
    Validator::new(&input)
        .rules(&[
            ("email", "required|email|unique:users"),         // column defaults to `email`
            ("role_id", "required|exists:roles,id"),
        ])
        .validate_with(&db)
        .await?;
    // …
}
```

- The column defaults to the field's own name (its last path segment).
- `unique:users,email,{id}` ignores the row with that `id` — the edit form case —
  and a fourth argument names a different key column (`unique:users,email,ada,handle`).
- Table and column names come from the rule literal and are spliced into SQL, so
  they must be plain identifiers; anything else panics before a query runs.
- The synchronous `validate()` **panics** on a database rule rather than
  skipping it, so a check can't silently not happen.

## Direct use

Outside a command you can inspect the bag:

```rust
let errors = Validator::new(&data).rule("email", "required|email").errors();
if errors.has("email") {
    eprintln!("{}", errors.first("email").unwrap());
}
```

## Related

- [Commands](commands.md) · [Frontend runtime](frontend-runtime.md)
