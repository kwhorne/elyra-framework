//! An app without a window (RFC 0054): prepared, its commands called by
//! name from Rust as a command-line twin would, with the page's middleware,
//! validation and abilities. Runs with and without the `gui` feature.

use elyra::{command, commands, App, Ctx};

#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[command(can = "posts.delete")]
async fn posts_destroy(_ctx: Ctx, id: i64) -> String {
    format!("deleted {id}")
}

#[tokio::test]
async fn a_prepared_app_calls_its_commands_by_name() {
    let app = App::new().commands(commands![add, posts_destroy]).prepare();
    let sum: i64 = app.call("add", (2, 3)).await.unwrap();
    assert_eq!(sum, 5);
    // As a CLI would: the name and its arguments from its own command line.
    let sum: serde_json::Value = app.call("add", serde_json::json!([40, 2])).await.unwrap();
    assert_eq!(sum, serde_json::json!(42));

    let err = app.call::<String>("posts_destroy", (7,)).await.unwrap_err();
    assert!(err.to_string().contains("`posts.delete` ability"), "{err}");
    let err = app.call::<i64>("nope", ()).await.unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
    let err = app.call::<i64>("add", ("two", 3)).await.unwrap_err();
    assert!(err.to_string().contains("decode"), "{err}");
}

#[tokio::test]
async fn a_granted_ability_lets_the_call_through() {
    let app = App::new()
        .allow_ability("posts.delete")
        .commands(commands![posts_destroy])
        .prepare();
    let said: String = app.call("posts_destroy", (7,)).await.unwrap();
    assert_eq!(said, "deleted 7");
}

#[cfg(not(feature = "gui"))]
#[test]
fn without_the_window_run_says_so() {
    let err = App::new().commands(commands![add]).run().unwrap_err();
    assert!(err.to_string().contains("`gui` feature is off"), "{err}");
}
