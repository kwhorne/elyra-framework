//! Seeing what an agent does, and holding it back (RFC 0003 step 6): the
//! `AgentCalled` event, the `elyra:mcp` channel, and per-tool rate limits.

use std::sync::Mutex;
use std::time::Duration;

use elyra::mcp::server::MODERN;
use elyra::mcp::{AgentCalled, Mcp, DEFAULT_RATE_LIMIT};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde::Deserialize;
use serde_json::json;

#[command(can = "notes.create")]
async fn notes_store(_ctx: Ctx, body: String) -> Result<String, String> {
    if body.is_empty() {
        return Err("empty".into());
    }
    Ok(body)
}

/// For the rate-limit test, so its calls aren't in the other's events.
#[command(can = "notes.create")]
async fn notes_tag(_ctx: Ctx, tag: String) -> String {
    tag
}

#[command(can = "notes.view")]
async fn notes_count(_ctx: Ctx) -> i64 {
    3
}

#[command(can = "notes.delete")]
async fn notes_destroy(_ctx: Ctx) {}

static CALLED: Mutex<Vec<AgentCalled>> = Mutex::new(Vec::new());

fn app(mcp: Mcp) -> TestApp {
    TestApp::new(
        App::new()
            .commands(commands![
                notes_store,
                notes_tag,
                notes_count,
                notes_destroy
            ])
            .listen(|e: AgentCalled, _ctx: Ctx| async move {
                CALLED.lock().unwrap().push(e);
                Ok(())
            })
            .mcp(mcp),
    )
}

/// The `AgentCalled` events for `tool` so far, waiting for the background
/// dispatch.
async fn called(tool: &str, at_least: usize) -> Vec<AgentCalled> {
    for _ in 0..100 {
        let seen: Vec<AgentCalled> = CALLED
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.tool == tool)
            .cloned()
            .collect();
        if seen.len() >= at_least {
            return seen;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fewer than {at_least} AgentCalled for {tool}");
}

#[derive(Deserialize, Debug, PartialEq)]
struct Activity {
    tool: String,
    client: String,
    phase: String,
    ok: Option<bool>,
}

#[tokio::test]
async fn every_call_is_an_event_and_on_the_channel() {
    let app = app(Mcp::new().allow_ability("notes.create"));
    app.listen();
    let mcp = app.mcp();
    assert!(
        !mcp.call("notes_store", json!({ "body": "hi" }))
            .await
            .is_error
    );
    assert!(
        mcp.call("notes_store", json!({ "body": "" }))
            .await
            .is_error
    );

    let events = called("notes_store", 2).await;
    assert_eq!(events[0].client, "test-client");
    assert!(events[0].ok);
    assert!(!events[1].ok, "a failed call is one too");

    let activity: Vec<Activity> = app.events_on("elyra:mcp").await;
    let phases: Vec<(&str, Option<bool>)> =
        activity.iter().map(|a| (a.phase.as_str(), a.ok)).collect();
    assert_eq!(
        phases,
        [
            ("started", None),
            ("finished", Some(true)),
            ("started", None),
            ("finished", Some(false))
        ]
    );
    assert!(activity
        .iter()
        .all(|a| a.tool == "notes_store" && a.client == "test-client"));
}

#[tokio::test]
async fn a_tool_is_rate_limited_per_tool() {
    let app = app(Mcp::new()
        .allow_abilities(["notes.*"])
        .rate_limit("notes.*", 3, Duration::from_secs(60))
        .rate_limit("notes.create", 2, Duration::from_secs(60)));
    let mcp = app.mcp();
    for _ in 0..2 {
        assert!(!mcp.call("notes_tag", json!({ "tag": "x" })).await.is_error);
    }
    let third = mcp.call("notes_tag", json!({ "tag": "x" })).await;
    assert!(
        third.is_error && third.text.contains("rate limited to 2 calls per 60s"),
        "{}",
        third.text
    );
    // Counted per tool: another one gets its own 3 (the earlier, wider limit),
    // whatever the first one used.
    for _ in 0..3 {
        assert!(!mcp.call("notes_count", json!({})).await.is_error);
    }
    assert!(mcp.call("notes_count", json!({})).await.is_error);
    // And across connections to the server: another client is held back too.
    let other = mcp.server().connect();
    let reply = other
        .handle(
            json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "_meta": { "io.modelcontextprotocol/protocolVersion": MODERN },
            "name": "notes_tag", "arguments": { "tag": "x" } } }),
        )
        .await
        .unwrap();
    assert_eq!(reply["result"]["isError"], true, "{reply}");
}

#[tokio::test]
async fn a_throttled_tool_doesnt_ask_the_user_first() {
    let app = app(Mcp::new()
        .allow_ability("notes.delete")
        .confirm("notes.delete")
        .rate_limit("notes.delete", 1, Duration::from_secs(60)));
    let mcp = app.mcp().confirming("accept");
    let first = mcp.call("notes_destroy", json!({})).await;
    assert!(!first.is_error && first.confirmation.is_some());
    let second = mcp.call("notes_destroy", json!({})).await;
    assert!(second.is_error && second.text.contains("rate limited"));
    assert!(
        second.confirmation.is_none(),
        "no question for a call that can't run"
    );
}

#[test]
fn limits() {
    let mcp = Mcp::new()
        .rate_limit("*", 10, Duration::from_secs(1))
        .rate_limit("notes.*", 5, Duration::from_secs(2));
    assert_eq!(mcp.limit_for("notes.view"), (5, Duration::from_secs(2)));
    assert_eq!(mcp.limit_for("teams.view"), (10, Duration::from_secs(1)));
    assert_eq!(Mcp::new().limit_for("anything"), DEFAULT_RATE_LIMIT);
}
