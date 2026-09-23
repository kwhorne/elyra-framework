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

## Translating messages

Pass the app's [`Translator`](i18n.md) and messages come out in the user's
language, from `validation.<key>` in your translation files; field names come
from `validation.attributes.<field>`:

```rust
Validator::new(&input)
    .translator(&ctx.get::<Translator>())
    .rules(&[("email", "required|email"), ("items.*.name", "required")])
    .validate()?;
```

```json
// lang/nb.json
{
  "validation": {
    "required": ":attribute må fylles ut.",
    "email": ":attribute må være en gyldig e-postadresse.",
    "min": { "string": ":attribute må ha minst :min tegn." },
    "attributes": { "email": "e-postadressen", "items.*.name": "varenavnet" }
  }
}
```

- A key you don't translate falls back to the built-in English, so you can
  translate the messages you use and leave the rest.
- Field names are looked up by concrete path (`items.0.name`), then by the
  pattern (`items.*.name`), then humanised (`first_name` → `first name`). The same
  goes for `:other` in `same`, `required_if` and friends.
- Without `.translator(..)`, messages are the built-in English — unchanged from
  before.
- `validation::message_keys()` lists every key with its English, if you want to
  generate a file or check one for completeness.

Every key, with the English to translate from:

<details>
<summary>The full <code>validation</code> skeleton (59 keys)</summary>

```json
{
  "validation": {
    "accepted": "The :attribute must be accepted.",
    "after": "The :attribute must be a date after :date.",
    "after_or_equal": "The :attribute must be a date after or equal to :date.",
    "alpha": "The :attribute must only contain letters.",
    "alpha_dash": "The :attribute must only contain letters, numbers, dashes and underscores.",
    "alpha_num": "The :attribute must only contain letters and numbers.",
    "array": "The :attribute must be an array.",
    "before": "The :attribute must be a date before :date.",
    "before_or_equal": "The :attribute must be a date before or equal to :date.",
    "between": {
      "array": "The :attribute must have between :min and :max items.",
      "numeric": "The :attribute must be between :min and :max.",
      "string": "The :attribute must be between :min and :max characters."
    },
    "boolean": "The :attribute must be true or false.",
    "confirmed": "The :attribute confirmation does not match.",
    "date": "The :attribute is not a valid date.",
    "digits": "The :attribute must be :digits digits.",
    "digits_between": "The :attribute must be between :min and :max digits.",
    "distinct": "The :attribute field has a duplicate value.",
    "email": "The :attribute must be a valid email address.",
    "ends_with": "The :attribute must end with one of the following: :values.",
    "exists": "The selected :attribute is invalid.",
    "filled": "The :attribute field must have a value.",
    "gt": {
      "array": "The :attribute must have greater than :value items.",
      "numeric": "The :attribute must be greater than :value.",
      "string": "The :attribute must be greater than :value characters."
    },
    "gte": {
      "array": "The :attribute must have greater than or equal to :value items.",
      "numeric": "The :attribute must be greater than or equal to :value.",
      "string": "The :attribute must be greater than or equal to :value characters."
    },
    "in": "The selected :attribute is invalid.",
    "integer": "The :attribute must be an integer.",
    "ip": "The :attribute must be a valid IP address.",
    "lt": {
      "array": "The :attribute must have less than :value items.",
      "numeric": "The :attribute must be less than :value.",
      "string": "The :attribute must be less than :value characters."
    },
    "lte": {
      "array": "The :attribute must have less than or equal to :value items.",
      "numeric": "The :attribute must be less than or equal to :value.",
      "string": "The :attribute must be less than or equal to :value characters."
    },
    "max": {
      "array": "The :attribute must not have more than :max items.",
      "numeric": "The :attribute must not be greater than :max.",
      "string": "The :attribute must not be greater than :max characters."
    },
    "min": {
      "array": "The :attribute must have at least :min items.",
      "numeric": "The :attribute must be at least :min.",
      "string": "The :attribute must be at least :min characters."
    },
    "not_in": "The selected :attribute is invalid.",
    "numeric": "The :attribute must be a number.",
    "regex": "The :attribute format is invalid.",
    "required": "The :attribute field is required.",
    "required_if": "The :attribute field is required when :other is :value.",
    "required_with": "The :attribute field is required when :values is present.",
    "required_without": "The :attribute field is required when :values is not present.",
    "same": "The :attribute and :other must match.",
    "size": {
      "array": "The :attribute must contain :size items.",
      "numeric": "The :attribute must be :size.",
      "string": "The :attribute must be :size characters."
    },
    "starts_with": "The :attribute must start with one of the following: :values.",
    "string": "The :attribute must be a string.",
    "unique": "The :attribute has already been taken.",
    "url": "The :attribute must be a valid URL.",
    "uuid": "The :attribute must be a valid UUID."
  }
}
```

</details>

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
