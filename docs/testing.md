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

## Related

- [Commands](commands.md) · [Events](events.md) · [Validation](validation.md) · [Security](security.md)
