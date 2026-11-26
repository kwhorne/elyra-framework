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
    // As a CLI would: the name and its arguments from its own command line,
    // the answer as JSON.
    let sum = app
        .call_json("add", serde_json::json!([40, 2]))
        .await
        .unwrap();
    assert_eq!(sum, serde_json::json!(42));

    let err = app.call::<String>("posts_destroy", (7,)).await.unwrap_err();
    assert!(err.to_string().contains("`posts.delete` ability"), "{err}");
    let err = app.call::<i64>("nope", ()).await.unwrap_err();
    assert!(err.to_string().contains("nope"), "{err}");
    let err = app.call::<i64>("add", ("two", 3)).await.unwrap_err();
    assert!(err.to_string().contains("decode"), "{err}");
}

#[command]
async fn qty_by_line(_ctx: Ctx) -> std::collections::HashMap<i64, u32> {
    std::collections::HashMap::from([(3, 2), (-7, 1)])
}

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

#[tokio::test]
async fn an_answer_keyed_by_integers_reads_as_json_with_their_digits() {
    let app = App::new().commands(commands![qty_by_line]).prepare();
    let json = app.call_json("qty_by_line", ()).await.unwrap();
    assert_eq!(json, serde_json::json!({"3": 2, "-7": 1}));
    // Typed, it's the map it was.
    let map: std::collections::HashMap<i64, u32> = app.call("qty_by_line", ()).await.unwrap();
    assert_eq!(map[&3], 2);
}

#[tokio::test]
async fn a_call_is_held_to_what_the_page_may_do() {
    // The page's commands turned off: none is called from here either.
    let app = App::new()
        .deny_frontend(elyra::security::Capability::Commands)
        .commands(commands![add])
        .prepare();
    let err = app.call::<i64>("add", (1, 2)).await.unwrap_err();
    assert!(err.to_string().contains("Capability::Commands"), "{err}");

    // Arguments past the app's limit, as the page's would be.
    let app = App::new()
        .max_request_body(64)
        .commands(commands![greet])
        .prepare();
    let err = app
        .call::<String>("greet", ("x".repeat(200),))
        .await
        .unwrap_err();
    assert!(err.to_string().to_lowercase().contains("large"), "{err}");
    let ok: String = app.call("greet", ("Ada",)).await.unwrap();
    assert_eq!(ok, "Hello, Ada!");
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
