# Commands

A command is the compiled equivalent of a Laravel controller action. Annotate an
`async fn` with `#[command]` and register it with `commands![...]`.

```rust
use elyra::{command, commands, App, Ctx};

#[command]
async fn greet(ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

App::new().commands(commands![greet, add]).run();
```

## How it works

`#[command]` turns the function into a zero-sized type of the same name that
implements the `Command` trait — so the function name doubles as the value you
pass to `commands![...]`. Arguments are decoded from a compact MessagePack array;
the return value is encoded as a named map (see [wire format](wire-format.md)).

## The `Ctx`

The **first parameter is always the context** and is passed through untouched.
Use it to resolve services from the [container](container-and-providers.md):

```rust
#[command]
async fn greet(ctx: Ctx, name: String) -> String {
    let cfg = ctx.get::<Config>();     // Arc<Config>, panics if unbound
    format!("{} {name}", cfg.greeting)
}
```

Name it `_ctx` if unused.

## Arguments and return types

- Arguments must be simple identifiers with types that implement
  `serde::Deserialize` **and** `specta::Type` (for [codegen](codegen.md)).
- The return type must implement `serde::Serialize` + `specta::Type`.
- Structs are serialized as named maps → plain JS objects, resilient to field
  reordering across versions.

```rust
#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
struct Point { x: i64, y: i64 }

#[command]
async fn shift(_ctx: Ctx, p: Point) -> Point { Point { x: p.x + 1, y: p.y + 1 } }
```

Zero-argument commands ignore the request body entirely.

## Fallible commands (`Result`)

Return `Result<T, E>` where `E: Display`. `Ok(v)` is serialized as `T`; `Err(e)`
becomes an error response — the frontend promise **rejects** with a
`CommandError`. Codegen surfaces `T`:

```rust
#[command]
async fn checked_div(_ctx: Ctx, a: i64, b: i64) -> Result<i64, String> {
    if b == 0 { Err("cannot divide by zero".into()) } else { Ok(a / b) }
}
```

```ts
try { await api.checked_div(1, 0); } catch (e) { /* CommandError */ }
```

## Calling from the frontend

```ts
import { invoke } from "@elyra/runtime";
const greeting = await invoke<string>("greet", "world");

// or the typed facade after `rata codegen`:
import { api } from "./bindings";
const greeting = await api.greet("world");
```

## Limitations (deliberate)

- The macro assumes the first parameter is the `Ctx`.
- No generics, no `Option<Ctx>`, no `self` receivers.
- Numeric codegen: 64-bit integers render as `number` — see
  [codegen](codegen.md#number-policy).

## Related

- [Container & providers](container-and-providers.md)
- [Middleware](middleware.md) — wrap dispatch
- [Codegen](codegen.md) — the typed `api.*`
