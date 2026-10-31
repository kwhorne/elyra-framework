//! The assistant sees the window (RFC 0013 steps 1 and 2): each turn carries
//! a note of the route it was asked from and the live queries the window
//! shows, filtered by what the user's assistant is offered.
#![cfg(all(feature = "ai", feature = "database"))]

use std::time::Duration;

use elyra::ai::{Ai, AiFake};
use elyra::assistant::{Assistant, AssistantEvent, CHANNEL};
use elyra::testing::{TestApp, TestShell};
use elyra::{command, commands, App, Ctx};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Serialize, Deserialize, specta::Type, Debug, Default)]
struct Customer {
    id: i64,
    name: String,
}

/// One customer.
#[command(live, can = "customers.view")]
async fn customers_show(_ctx: Ctx, id: i64) -> Customer {
    Customer {
        id,
        name: "Ada".into(),
    }
}

/// Customers, a page at a time.
#[command(live, can = "customers.view")]
async fn customers_index(_ctx: Ctx, search: Option<String>, page: Option<i64>) -> Vec<Customer> {
    let _ = (search, page);
    Vec::new()
}

/// Change a customer.
#[command(can = "customers.update")]
async fn customers_update(_ctx: Ctx, id: i64, credit_limit: i64) -> Customer {
    let _ = credit_limit;
    Customer {
        id,
        name: "Ada".into(),
    }
}

/// The audit log: shown, but not the assistant's.
#[command(live, can = "audit.view")]
async fn audit_index(_ctx: Ctx) -> Vec<String> {
    Vec::new()
}

fn app_with(fake: &AiFake, assistant: Assistant) -> TestApp {
    TestApp::new(
        App::new()
            .allow_abilities(["customers.*", "audit.*"])
            .commands(commands![
                customers_show,
                customers_index,
                customers_update,
                audit_index
            ])
            .swap(Ai::fake(fake.clone()))
            .assistant(assistant),
    )
}

fn app(fake: &AiFake) -> TestApp {
    app_with(fake, Assistant::new().allow_abilities(["customers.*"]))
}

/// The JSON in a user message's `<window>` block, and the words after it.
fn note(message: &Value) -> (Option<Value>, String) {
    let content = message["content"].as_str().expect("text content");
    match content.strip_prefix("<window>\n") {
        Some(rest) => {
            let (json, words) = rest.split_once("\n</window>\n\n").expect("a closed block");
            (Some(serde_json::from_str(json).unwrap()), words.to_owned())
        }
        None => (None, content.to_owned()),
    }
}

#[tokio::test]
async fn a_turn_says_where_the_window_is_and_what_it_shows() {
    let fake = AiFake::new().text("Done.");
    let app = app(&fake);
    let window = app.assistant().at("/customers/42?tab=invoices");
    window
        .live("customers_index", (Some("acme"), None::<i64>))
        .await;
    window.live("customers_show", (42,)).await;
    window.live("audit_index", ()).await;
    app.assistant()
        .window("other")
        .live("customers_show", (7,))
        .await;

    window.prompt("Set her credit limit to 50 000").await;
    let request = &fake.requests()[0];
    let (seen, words) = note(&request.messages[0]);
    assert_eq!(words, "Set her credit limit to 50 000");
    assert_eq!(
        seen,
        Some(json!({
            "route": "/customers/42?tab=invoices",
            // Newest first; unset arguments left out; the audit log isn't the
            // assistant's, and the other window's customer isn't this one's.
            "showing": [
                { "command": "customers_show", "arguments": { "id": 42 } },
                { "command": "customers_index", "arguments": { "search": "acme" } },
            ],
        }))
    );
    assert!(
        request
            .system
            .contains("A <window> block before the user's message"),
        "{}",
        request.system
    );
}

#[tokio::test]
async fn the_conversation_keeps_each_turns_note() {
    let fake = AiFake::new().text("Ada.").text("Grace.");
    let app = app(&fake);
    app.assistant()
        .at("/customers/42")
        .prompt("Who's this?")
        .await;
    app.assistant()
        .at("/customers/43")
        .prompt("And this?")
        .await;
    let messages = &fake.requests()[1].messages;
    assert_eq!(
        note(&messages[0]).0,
        Some(json!({ "route": "/customers/42" }))
    );
    assert_eq!(
        messages[1],
        json!({ "role": "assistant", "content": "Ada." })
    );
    assert_eq!(
        note(&messages[2]).0,
        Some(json!({ "route": "/customers/43" }))
    );
}

#[tokio::test]
async fn a_window_with_nothing_to_say_sends_no_note() {
    let fake = AiFake::new().text("Hello.");
    let app = app(&fake);
    app.assistant().at("  ").prompt("Hi").await;
    assert_eq!(
        fake.requests()[0].messages[0],
        json!({ "role": "user", "content": "Hi" })
    );
}

#[tokio::test]
async fn window_context_false_leaves_it_out() {
    let fake = AiFake::new().text("Hello.");
    let app = app_with(
        &fake,
        Assistant::new()
            .allow_abilities(["customers.*"])
            .window_context(false),
    );
    let window = app.assistant().at("/customers/42");
    window.live("customers_show", (42,)).await;
    window.prompt("Hi").await;
    let request = &fake.requests()[0];
    assert_eq!(
        request.messages[0],
        json!({ "role": "user", "content": "Hi" })
    );
    assert!(!request.system.contains("<window>"), "{}", request.system);
}

#[tokio::test]
async fn the_note_is_bounded() {
    let fake = AiFake::new().text("Hello.");
    let app = app(&fake);
    let window = app.assistant();
    let long = "x".repeat(300);
    for page in 0..20 {
        window
            .live("customers_index", (Some(long.as_str()), Some(page)))
            .await;
    }
    window.prompt("Hi").await;
    let (seen, _) = note(&fake.requests()[0].messages[0]);
    let seen = seen.unwrap();
    let showing = seen["showing"].as_array().unwrap();
    let json = seen.to_string();
    assert!(json.len() <= 2048, "{} bytes", json.len());
    assert!(
        showing.len() < 16,
        "the oldest went to fit: {}",
        showing.len()
    );
    // The newest stays; a long string is cut, with an ellipsis.
    assert_eq!(showing[0]["arguments"]["page"], 19);
    let search = showing[0]["arguments"]["search"].as_str().unwrap();
    assert_eq!(search.chars().count(), 201);
    assert!(search.ends_with('…'));
}

// ---------------------------------------------------------------------------
// The route: a window sends its route with the prompt.
// ---------------------------------------------------------------------------

async fn post(shell: &TestShell, client: &str, body: Vec<u8>) -> Result<Value, String> {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri("elyra://localhost/__assistant/prompt")
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", client)
        .body(body)
        .unwrap();
    let response = shell.handle(request).await;
    if response
        .headers()
        .get("x-elyra-status")
        .is_some_and(|v| v == "error")
    {
        return Err(String::from_utf8_lossy(response.body()).into_owned());
    }
    Ok(rmp_serde::from_slice(response.body()).unwrap())
}

async fn answered(bus: &elyra::EventBus, client: &str) {
    for _ in 0..20 {
        let batch = tokio::time::timeout(Duration::from_secs(5), bus.next_batch_for(client))
            .await
            .expect("an event in time");
        let batch: Vec<(String, rmpv::Value)> = rmp_serde::from_slice(&batch).unwrap();
        for (channel, value) in batch {
            let mut buf = Vec::new();
            rmpv::encode::write_value(&mut buf, &value).unwrap();
            if channel == CHANNEL
                && matches!(
                    rmp_serde::from_slice(&buf).unwrap(),
                    AssistantEvent::Answer { .. }
                )
            {
                return;
            }
        }
    }
    panic!("no answer");
}

#[tokio::test]
async fn a_window_sends_its_route_with_the_prompt() {
    let fake = AiFake::new().text("Ada.").text("Hello.");
    let prepared = App::new()
        .allow_abilities(["customers.*"])
        .commands(commands![customers_show])
        .swap(Ai::fake(fake.clone()))
        .assistant(Assistant::new().allow_abilities(["customers.*"]))
        .prepare();
    let bus = prepared.bus.clone();
    bus.register_client("w1");
    let shell = TestShell::new(prepared);

    let turn = rmp_serde::to_vec_named(&json!({ "text": "Who's this?", "route": "/customers/42" }))
        .unwrap();
    assert_eq!(post(&shell, "w1", turn).await, Ok(json!(true)));
    answered(&bus, "w1").await;
    assert_eq!(
        note(&fake.requests()[0].messages[0]),
        (
            Some(json!({ "route": "/customers/42" })),
            "Who's this?".into()
        )
    );

    // A bare string, as before: no route, no note.
    bus.register_client("w2");
    let bare = rmp_serde::to_vec_named(&"Hi").unwrap();
    assert_eq!(post(&shell, "w2", bare).await, Ok(json!(true)));
    answered(&bus, "w2").await;
    assert_eq!(
        fake.requests()[1].messages[0],
        json!({ "role": "user", "content": "Hi" })
    );
}

// ---------------------------------------------------------------------------
// The user's abilities: a query the user couldn't call isn't in the note.
// ---------------------------------------------------------------------------

/// A customer's private notes.
#[cfg(feature = "backend")]
#[command(live, can = "customers.notes")]
async fn customers_notes(_ctx: Ctx, id: i64) -> Vec<String> {
    let _ = id;
    Vec::new()
}

#[cfg(feature = "backend")]
#[tokio::test]
async fn a_query_the_user_may_not_run_isnt_in_the_note() {
    use elyra::auth::{Auth, MemoryTokens};
    use elyra::backend::Backend;
    use elyra::http::{Http, HttpFake};

    const BASE: &str = "https://crm.test";
    let laravel = HttpFake::new()
        .post(format!("{BASE}/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(format!("{BASE}/api/user"), 200, json!({ "id": 1, "name": "Ada" }))
        .get(format!("{BASE}/api/abilities"), 200, json!(["customers.view"]));
    let fake = AiFake::new().text("Ada.");
    let app = TestApp::new(
        App::new()
            .backend(Backend::new(BASE).abilities())
            .token_store(std::sync::Arc::new(MemoryTokens::new()))
            .allow_abilities(["customers.*"])
            .commands(commands![customers_show, customers_notes])
            .swap(Http::fake(laravel))
            .swap(Ai::fake(fake.clone()))
            .assistant(Assistant::new().allow_abilities(["customers.*"])),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let window = app.assistant();
    window.live("customers_notes", (42,)).await;
    window.live("customers_show", (42,)).await;
    window.prompt("Who's this?").await;
    assert_eq!(
        note(&fake.requests()[0].messages[0]).0,
        Some(json!({
            "showing": [{ "command": "customers_show", "arguments": { "id": 42 } }],
        }))
    );
}
