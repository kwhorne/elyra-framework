//! `elyra::testing::TestApp` — the supported way to test commands without a window.

use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, EventBus};
use serde::{Deserialize, Serialize};

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[command]
async fn boom(_ctx: Ctx) -> Result<i64, String> {
    Err("nope".into())
}

#[derive(Serialize, Deserialize, specta::Type, PartialEq, Debug)]
struct Progress {
    percent: u8,
}

#[command]
async fn work(ctx: Ctx) {
    let bus = ctx.get::<EventBus>();
    let _ = bus.emit("progress", &Progress { percent: 42 });
    let _ = bus.emit("progress", &Progress { percent: 100 });
}

#[derive(Serialize, Deserialize, specta::Type)]
struct NewAccount {
    email: String,
    age: i64,
}

#[command]
async fn create_account(_ctx: Ctx, input: NewAccount) -> Result<bool, elyra::ValidationErrors> {
    let value = serde_json::json!({ "email": input.email, "age": input.age });
    let errors = elyra::Validator::new(&value)
        .rules(&[("email", "required|email"), ("age", "integer|min:18")])
        .errors();
    if errors.is_empty() {
        Ok(true)
    } else {
        Err(errors)
    }
}

/// A service resolved from the container, to prove DI works in tests.
struct Greeting(String);

fn test_app() -> TestApp {
    TestApp::new(App::new().bind(Greeting("hi".into())).commands(commands![
        greet,
        add,
        boom,
        work,
        create_account
    ]))
}

#[tokio::test]
async fn invokes_commands_with_typed_arguments_and_results() {
    let app = test_app();
    let greeting: String = app.invoke("greet", ("World",)).await.unwrap();
    assert_eq!(greeting, "Hello, World!");
    assert_eq!(app.invoke_ok::<i64>("add", (2, 3)).await, 5);
}

#[tokio::test]
async fn surfaces_command_errors_and_unknown_commands() {
    let app = test_app();
    assert_eq!(app.invoke_err("boom", ()).await, "nope");
    assert!(app
        .invoke_err("nope_missing", ())
        .await
        .contains("nope_missing"));
}

#[tokio::test]
async fn collects_and_decodes_emitted_events() {
    let app = test_app();
    app.listen();
    app.invoke::<()>("work", ()).await.unwrap();

    let payloads: Vec<Progress> = app.events_on("progress").await;
    assert_eq!(
        payloads,
        vec![Progress { percent: 42 }, Progress { percent: 100 }]
    );
}

#[tokio::test]
async fn asserts_on_event_channels() {
    let app = test_app();
    app.listen();
    app.invoke::<()>("work", ()).await.unwrap();
    app.assert_emitted("progress").await;

    let quiet = test_app();
    quiet.listen();
    quiet.invoke::<i64>("add", (1, 1)).await.unwrap();
    quiet.assert_not_emitted("progress").await;
}

#[tokio::test]
async fn extracts_validation_error_bags() {
    let app = test_app();
    let errors = app
        .invoke_validation_errors(
            "create_account",
            (serde_json::json!({"email": "not-an-email", "age": 15}),),
        )
        .await
        .expect("a validation bag");
    assert!(errors.contains_key("email"));
    assert!(errors.contains_key("age"));

    let ok: bool = app
        .invoke_ok(
            "create_account",
            (serde_json::json!({"email": "a@b.co", "age": 30}),),
        )
        .await;
    assert!(ok);
}

#[tokio::test]
async fn exposes_the_container_registry_and_policy() {
    let app = test_app();
    assert_eq!(
        app.commands(),
        vec!["add", "boom", "create_account", "greet", "work"]
    );
    assert_eq!(app.get::<Greeting>().0, "hi");
    let _bus = app.get::<EventBus>();
    assert!(app.policy().grants(elyra::security::Capability::Commands));
    assert!(!app.policy().grants(elyra::security::Capability::StoreClear));
}

#[tokio::test]
async fn middleware_runs_around_test_invocations() {
    use elyra::{CommandRequest, Middleware, Next, Result as ElyraResult};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    static SEEN: AtomicUsize = AtomicUsize::new(0);

    struct Counting;
    impl Middleware for Counting {
        fn handle(
            &self,
            ctx: Ctx,
            req: CommandRequest,
            next: Next,
        ) -> elyra::command::BoxFuture<'static, ElyraResult<Vec<u8>>> {
            Box::pin(async move {
                SEEN.fetch_add(1, Ordering::Relaxed);
                next.run(ctx, req).await
            })
        }
    }

    let app = TestApp::new(App::new().middleware(Counting).commands(commands![add]));
    let _ = Arc::new(());
    assert_eq!(app.invoke_ok::<i64>("add", (1, 2)).await, 3);
    assert_eq!(SEEN.load(Ordering::Relaxed), 1);
}
