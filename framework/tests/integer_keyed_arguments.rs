//! A map keyed by integers, sent from the page (RFC 0050): its keys arrive
//! as strings, and the command reads them as the integers it takes.

use std::collections::{BTreeMap, HashMap};

use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::json;

/// The quantity per order line, summed with the line numbers.
#[command]
async fn set_quantities(_ctx: Ctx, by_line: HashMap<i64, u32>) -> Vec<(i64, u32)> {
    let sorted: BTreeMap<i64, u32> = by_line.into_iter().collect();
    sorted.into_iter().collect()
}

#[command]
async fn count(_ctx: Ctx, n: u32) -> u32 {
    n
}

fn app() -> TestApp {
    TestApp::new(App::new().commands(commands![set_quantities, count]))
}

#[tokio::test]
async fn the_page_s_string_keys_are_read_as_the_integers_the_command_takes() {
    let app = app();
    // What `@elyra/runtime` sends for `api.set_quantities({3: 2, 7: 1})`: an
    // object's keys are strings.
    let sent = json!([{ "3": 2, "7": 1, "9007199254740993": 4 }]);
    let lines: Vec<(i64, u32)> = app.invoke_ok("set_quantities", sent).await;
    assert_eq!(lines, [(3, 2), (7, 1), (9_007_199_254_740_993, 4)]);

    // Integer keys (as Rust sends them) decode as always.
    let lines: Vec<(i64, u32)> = app
        .invoke_ok("set_quantities", (HashMap::from([(5i64, 1u32)]),))
        .await;
    assert_eq!(lines, [(5, 1)]);
}

#[tokio::test]
async fn what_isn_t_an_integer_key_is_the_error_it_always_was() {
    let app = app();
    let err = app
        .invoke_err("set_quantities", json!([{ "three": 2 }]))
        .await;
    // rmp_serde's own message, as before: the key's marker.
    assert!(err.contains("wrong msgpack marker FixStr(5)"), "{err}");
    // Only map keys: a string for an integer argument is still refused.
    let err = app.invoke_err("count", json!(["3"])).await;
    assert!(err.contains("failed to decode command arguments"), "{err}");
}
