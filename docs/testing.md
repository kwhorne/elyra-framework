# Testing

`elyra::testing` runs your app's commands through the real pipeline — container,
providers, middleware, validation — without opening a window.

## Commands

```rust
use elyra::testing::TestApp;
use elyra::{commands, App};

#[tokio::test]
async fn greets() {
    let app = TestApp::new(App::new().commands(commands![greet]));

    let greeting: String = app.invoke("greet", ("World",)).await.unwrap();
    assert_eq!(greeting, "Hello, World!");

    // Convenience wrappers
    assert_eq!(app.invoke_ok::<i64>("add", (2, 3)).await, 5);
    assert_eq!(app.invoke_err("boom", ()).await, "nope");
}
```

`args` is a tuple matching the parameters after `Ctx`, exactly like the frontend's
`invoke("name", a, b)`.

## Events

```rust
let app = TestApp::new(app);
app.listen(); // register before the code under test emits

app.invoke::<()>("start_import", ()).await.unwrap();
app.assert_emitted("progress").await;

let payloads: Vec<Progress> = app.events_on("progress").await;
```

## Validation

```rust
let errors = app
    .invoke_validation_errors("create_account", (input,))
    .await
    .expect("a validation bag");
assert!(errors.contains_key("email"));
```

## Services and capabilities

```rust
let db = app.get::<elyra::Database>();
assert!(app.policy().grants(elyra::security::Capability::Commands));
assert_eq!(app.commands(), vec!["add", "greet"]);
```

## The IPC surface

`TestShell` drives the actual protocol handler, for testing headers, capabilities,
asset caching and error kinds:

```rust
use elyra::testing::TestShell;
use wry::http::Request;

let shell = TestShell::new(App::new().commands(commands![add]).prepare());
let req = Request::builder()
    .method("POST")
    .uri("elyra://localhost/__cmd/add")
    .header("x-elyra-token", shell.token())
    .body(rmp_serde::to_vec(&(2, 40)).unwrap())
    .unwrap();

let res = shell.handle(req).await;
assert_eq!(res.status(), 200);
```

## Fakes

Test a command against fakes instead of the real queue, disk or event
listeners — Laravel's `Queue::fake()`, `Storage::fake()` and `Event::fake()`.
`App::swap` puts them in place **after** every provider has registered, so the
production wiring stays exactly as it is and the fake still wins:

```rust
let app = TestApp::new(
    App::new()
        .provider(StorageProvider::at(data_dir))   // production wiring, untouched
        .provider(QueueProvider::new())
        .swap(Storage::fake())                     // …replaced for this test
        .swap(Queue::fake())
        .swap(Dispatcher::fake())
        .commands(commands![export]),
);
app.invoke_ok::<()>("export", ("q3",)).await;

app.get::<Storage>().assert_contents("exports/q3.csv", "id,total\n1,42\n");
app.get::<Queue>().assert_pushed_with("upload", |p| p["path"] == "exports/q3.csv");
app.get::<Dispatcher>().assert_dispatched_with(|e: &ReportExported| e.path.ends_with("q3.csv"));
```

| Fake | Does | Assertions |
|---|---|---|
| `Queue::fake()` | records pushes (incl. `push_later`, `push_confirmed`, `dispatch`); runs nothing | `assert_pushed`, `assert_pushed_times`, `assert_pushed_with`, `assert_not_pushed`, `assert_nothing_pushed`; `pushed(job)`, `pushed_as::<T>(job)` |
| `Storage::fake()` | a private temp directory per call, removed with its last clone | `assert_exists`, `assert_missing`, `assert_contents` |
| `Dispatcher::fake()` | records dispatched events; listeners are kept but never run | `assert_dispatched::<E>`, `assert_dispatched_times`, `assert_dispatched_with`, `assert_not_dispatched`, `assert_nothing_dispatched`; `dispatched::<E>()` |

A failed assertion names what *did* happen (`expected `export` to be pushed;
pushed: ["resize"]`) and points at your test line, not into the framework.
Calling an assertion on a real (non-fake) instance panics with a hint to swap one
in.

Your own services fake the same way. Bind the trait in production and swap an
implementation for the test:

```rust
App::new()
    .provider(MailProvider)                        // binds dyn Mailer -> Smtp
    .swap_as::<dyn Mailer>(Arc::new(FakeMailer::default()))
```

## Database fixtures

[Model factories](models.md#factories) build valid rows in one line, so a test
states only what it's about:

```rust
let admin = User::factory().state(|u| u.admin = true).create_one(&db).await?;
Post::factory().count(3).state(move |p| p.user_id = admin.id).create(&db).await?;
```

## Related

- [Commands](commands.md) · [Events](events.md) · [Validation](validation.md) · [Security](security.md)
