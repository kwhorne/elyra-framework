//! A command called from Rust (RFC 0025): `#[command]` keeps the function
//! as `name::run`, with its own types, and `call` goes through it.

use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};

/// Adds two numbers.
#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[command]
fn shout(_ctx: Ctx, word: String) -> String {
    word.to_uppercase()
}

#[command]
async fn checked_div(_ctx: Ctx, a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        Err("division by zero".into())
    } else {
        Ok(a / b)
    }
}

/// Its logic reused by another command, through `run`.
#[command]
async fn add_twice(ctx: Ctx, a: i64) -> i64 {
    let once = add::run(ctx.clone(), a, a).await;
    add::run(ctx, once, once).await
}

/// Behind an ability: `run` is Rust calling Rust, and doesn't ask for it.
#[command(can = "numbers.secret")]
async fn secret(_ctx: Ctx) -> &'static str {
    "42"
}

/// The function's own attributes reach `run`: without this `allow`, clippy
/// (`-D warnings` in CI) fails on eight arguments.
#[command]
#[allow(clippy::too_many_arguments)]
async fn eight(_ctx: Ctx, a: i64, b: i64, c: i64, d: i64, e: i64, f: i64, g: i64, h: i64) -> i64 {
    a + b + c + d + e + f + g + h
}

fn app() -> TestApp {
    TestApp::new(App::new().commands(commands![add, shout, checked_div, add_twice, secret, eight]))
}

#[tokio::test]
async fn run_is_the_function_with_its_own_types() {
    let app = app();
    let ctx = app.ctx().clone();
    let sum: i64 = add::run(ctx.clone(), 2, 3).await;
    assert_eq!(sum, 5);
    assert_eq!(eight::run(ctx, 1, 2, 3, 4, 5, 6, 7, 8).await, 36);
}

#[test]
fn a_sync_commands_run_is_sync() {
    let app = app();
    assert_eq!(shout::run(app.ctx().clone(), "hey".into()), "HEY");
}

#[tokio::test]
async fn a_fallible_commands_run_returns_its_result() {
    let app = app();
    let ctx = app.ctx().clone();
    assert_eq!(checked_div::run(ctx.clone(), 9, 3).await, Ok(3));
    assert_eq!(
        checked_div::run(ctx, 1, 0).await,
        Err("division by zero".to_owned())
    );
}

#[tokio::test]
async fn call_goes_through_run() {
    let app = app();
    assert_eq!(app.invoke_ok::<i64>("add", (2, 3)).await, 5);
    assert_eq!(app.invoke_ok::<String>("shout", ("hey",)).await, "HEY");
    assert_eq!(app.invoke_ok::<i64>("checked_div", (9, 3)).await, 3);
    assert_eq!(
        app.invoke_err("checked_div", (1, 0)).await,
        "division by zero"
    );
    assert_eq!(app.invoke_ok::<i64>("add_twice", (3,)).await, 12);
    assert_eq!(
        app.invoke_ok::<i64>("eight", (1, 2, 3, 4, 5, 6, 7, 8))
            .await,
        36
    );
}

#[tokio::test]
async fn run_isnt_the_pipeline() {
    use elyra::testing::TestShell;
    use wry::http::Request;
    // The webview is refused: the ability isn't granted.
    let shell = TestShell::new(App::new().commands(commands![secret]).prepare());
    let req = Request::builder()
        .method("POST")
        .uri("elyra://localhost/__cmd/secret")
        .header("x-elyra-token", shell.token())
        .body(Vec::new())
        .unwrap();
    assert_eq!(shell.handle(req).await.status(), 403);
    // Rust isn't: `run` is the app calling itself.
    let app = app();
    assert_eq!(secret::run(app.ctx().clone()).await, "42");
}

#[test]
fn the_doc_comment_is_still_the_description() {
    use elyra::command::Command;
    assert_eq!(add.description(), "Adds two numbers.");
    assert_eq!(shout.description(), "");
}

/// Commands in a module of their own, as an app splits them.
mod numbers {
    use elyra::{command, Ctx};

    #[command]
    pub async fn double(_ctx: Ctx, a: i64) -> i64 {
        a * 2
    }
}

#[tokio::test]
async fn run_is_as_visible_as_the_function() {
    let app = TestApp::new(App::new().commands(commands![numbers::double]));
    assert_eq!(numbers::double::run(app.ctx().clone(), 21).await, 42);
    assert_eq!(app.invoke_ok::<i64>("double", (21,)).await, 42);
}
