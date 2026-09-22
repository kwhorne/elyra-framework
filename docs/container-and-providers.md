# Container & providers

## The container

`Container` is a type-keyed registry of shared singletons (Laravel's container).
You rarely touch it directly — bind via `App::bind` or a provider, resolve via
`Ctx`.

```rust
App::new()
    .bind(Config { greeting: "hi".into() })   // bind a singleton
    .commands(commands![greet]);
```

Bindings must be `Send + Sync + 'static` (commands run on tokio). A later bind of
the same type replaces the previous one.

### Binding a trait (interface → implementation)

Laravel's `bind(Interface::class, Impl::class)`. Bind under the trait object and
depend on the trait, not the backend:

```rust
trait Mailer: Send + Sync { fn send(&self, to: &str) -> Result<()>; }

App::new().bind_as::<dyn Mailer>(Arc::new(SmtpMailer::new()));

#[command]
async fn invite(ctx: Ctx, email: String) -> Result<()> {
    ctx.get::<dyn Mailer>().send(&email)        // Arc<dyn Mailer>
}
```

Swap the implementation in one place — a `LogMailer` in development, a fake in a
test — without touching a command. This is also how to bind the
[`substrate`](substrate.md) contracts: `bind_as::<dyn substrate::Cache>(..)`.

### Lazy singletons

`bind_lazy` builds a singleton the first time it's resolved, with a full `Ctx`:

```rust
c.bind_lazy::<dyn Mailer>(|ctx| {
    Arc::new(SmtpMailer::new(&ctx.get::<MailConfig>().host))
});
```

This is how a provider's `register` can depend on another service: `register`
itself must not resolve anything (other providers may not have bound it yet), but
a lazy binding runs *after* every provider has registered, so order stops
mattering. It's still a singleton — built once, then shared.

A dependency cycle between lazy bindings (`A` needs `B` needs `A`) panics with the
chain — `circular dependency between lazy bindings: app::A -> app::B -> app::A` —
instead of deadlocking.

## `Ctx`

The context handed to every command. Cheap to clone (an `Arc` bump).

```rust
#[command]
async fn greet(ctx: Ctx, name: String) -> String {
    let cfg = ctx.get::<Config>();        // Arc<Config>; panics if unbound
    let db  = ctx.try_get::<Database>();  // Option<Arc<Database>>
    format!("{} {name}", cfg.greeting)
}
```

- `get::<T>() -> Arc<T>` — panics with a clear message if `T` isn't bound
  (a missing binding is a wiring bug, so fail loudly).
- `try_get::<T>() -> Option<Arc<T>>` — fallible resolution.
- `has::<T>() -> bool` — whether anything is bound, without building it.
- `T` may be a trait object in all three: `ctx.get::<dyn Mailer>()`.
- `dispatch(event).await` — send a [domain event](events.md#domain-events-dispatcher)
  to its listeners.

The `EventBus` and the [`Dispatcher`](events.md#domain-events-dispatcher) are
always bound, and — with the relevant features — so are
`Database` (via `App::database`) and `Windows`.

## Providers

A `Provider` is the idiomatic place to wire up a slice of the app (Laravel's
ServiceProvider). Two phases:

- **`register(&mut Container)`** — bind services. Runs for **all** providers
  first; don't resolve other services here, they may not be bound yet.
- **`boot(&Ctx)`** — runs after everything is registered, with a populated
  context. Resolve dependencies, seed state, spawn setup.

```rust
use elyra::{Container, Ctx, Provider};

struct ConfigProvider;

impl Provider for ConfigProvider {
    fn register(&self, c: &mut Container) {
        c.bind(Config { greeting: "hi".into() });
    }
    fn boot(&self, ctx: &Ctx) {
        // container is fully populated here
        let _ = ctx.get::<Config>();
    }
}

App::new().provider(ConfigProvider).run();
```

Both methods have default no-op bodies, so implement only the phase you need.

## Lifecycle

`App::run` (and the testable `App::prepare`) assemble the app in this order:

1. `App::bind(..)` values are placed in the container.
2. Each provider's `register` runs.
3. The `EventBus` (and `Database`/`Windows` if enabled) are bound.
4. The `Ctx` is built.
5. Each provider's `boot` runs.
6. The window(s) open and the event loop runs.

## Related

- [Commands](commands.md) — resolve services via `ctx.get`
- [Middleware](middleware.md)
- [Database](database.md) — `App::database` binds a `Database`
