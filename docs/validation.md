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
import { validationErrors } from "@elyra/runtime";

try {
  await api.create_account({ email, age });
} catch (e) {
  const errors = validationErrors(e);   // { email: ["…"], age: ["…"] } | null
  if (errors?.email) showFieldError("email", errors.email[0]);
}
```

## Rules

Pipe-separated, `rule:arg` for arguments (`min:18`, `in:a,b,c`):

| Rule | Passes when |
| --- | --- |
| `required` | present and not null / empty string / empty array |
| `nullable` / `sometimes` | (skips the other rules when the field is absent/null) |
| `string` | a string |
| `integer` | an integer |
| `numeric` | any number |
| `boolean` | `true` / `false` |
| `email` | looks like an email address |
| `url` | starts with `http://` or `https://` |
| `min:N` | number ≥ N, or string/array length ≥ N |
| `max:N` | number ≤ N, or string/array length ≤ N |
| `size:N` | number == N, or string/array length == N |
| `in:a,b,c` | value is one of the list |
| `same:field` | equals another field's value |
| `confirmed` | `<field>_confirmation` equals the value |

Absent or null fields only fail `required` — every other rule is skipped for
them (Laravel's implicit "sometimes" behaviour), so optional fields validate
only when provided. Unknown rules are ignored.

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
