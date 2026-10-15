---
name: elyra-commands
description: Write, gate and test Elyra Framework commands — #[command] handlers, Ctx and the container, elyra::Result and validation, abilities (can = "…"), per-command middleware, domain events and the EventBus. Use when adding or changing a command, a provider, middleware or an event in an Elyra app.
---

# Elyra commands

A command is the compiled counterpart of a Laravel controller action: an
`async fn` the frontend calls through the typed `api.*`.

## Writing one

```rust
use elyra::{command, commands, App, Ctx, Validator};

#[derive(serde::Serialize, serde::Deserialize, specta::Type)]
struct Point { x: i64, y: i64 }

#[command]
async fn shift(_ctx: Ctx, p: Point) -> Point { Point { x: p.x + 1, y: p.y + 1 } }

#[command(can = "users.update")]
async fn rename(ctx: Ctx, id: i64, name: String) -> elyra::Result<User> {
    let db = ctx.get::<Database>();
    Validator::new(&serde_json::json!({ "name": name }))
        .rule("name", "required|max:80")
        .validate()?;                                   // -> ValidationError in the frontend
    let mut user = User::find(&db, id).await?.ok_or_else(|| elyra::Error::command("no such user"))?;
    user.name = name;
    user.save(&db).await?;
    Ok(user)
}

App::new().commands(commands![shift, rename]).allow_ability("users.update");
```

- The first parameter is always `Ctx`; arguments are plain identifiers whose
  types implement `Deserialize + specta::Type`; the return type
  `Serialize + specta::Type`. No generics, no `self`.
- `Result<T, E: Display>` rejects the frontend's promise with a
  `CommandError`. `elyra::Error::with_kind("archived", "…")` gives it a
  `kind` the UI can switch on.
- After changing a signature or a type: `rata codegen`.

## Gating and middleware

- `can = "posts.delete"` denies the webview until the app grants it with
  `allow_ability("posts.delete")` or `allow_abilities(["posts.*"])`. MCP has
  its own grant (`Mcp::allow_abilities`). Rust-side callers aren't gated.
- `#[command(middleware = ["admin"])]` with
  `App::middleware_alias("auth", RequireSession)` and
  `middleware_group("admin", ["auth", "audit"])`. Global middleware
  (`App::middleware`) runs outermost. A misspelt name stops the app at start.
- A `Middleware` implements `handle(&self, ctx, req, next)`; `req.origin` is
  `Origin::Frontend` or `Origin::Agent { client }` (an MCP call).

## Services and events

- Bind with `App::bind`, `bind_as::<dyn Trait>(Arc::new(..))`, `bind_lazy`,
  or in a `Provider` (`register` binds, `boot` resolves).
- `ctx.dispatch(OrderShipped { .. }).await` runs listeners in order; the first
  error fails the command. `ctx.dispatch_background(..)` doesn't wait.
- `App::broadcast::<OrderShipped>("orders:shipped")` sends the event to the
  frontend as a typed channel; `ctx.get::<EventBus>().emit("chan", &v)` pushes
  any value (progress, ticks).

## Testing

```rust
use elyra::testing::TestApp;

#[tokio::test]
async fn renames() {
    let app = TestApp::new(App::new().database(url).commands(commands![rename]));
    let user: User = app.invoke_ok("rename", (1, "Ada")).await;
    let errors = app.invoke_validation_errors("rename", (1, "")).await.unwrap();
    assert!(errors.contains_key("name"));
}
```

`invoke`, `invoke_ok`, `invoke_err`, `invoke_err_kind`,
`invoke_validation_errors`; `App::swap(..)` for fakes (`Dispatcher::fake()`,
`Queue::fake()`, `Storage::fake()`, `Http::fake(..)`).

More: `search_docs` on the `elyra-framework` MCP server ("commands",
"middleware", "container", "events", "testing").
