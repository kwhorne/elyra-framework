//! The assistant's answer as it's written (RFC 0018): what the model writes
//! goes to the window in pieces, step by step, before the answer; a stop
//! keeps what was written; a server that won't stream gets asked whole.
#![cfg(feature = "ai")]

use std::time::Duration;

use elyra::ai::{Ai, AiFake};
use elyra::assistant::{Assistant, AssistantEvent, CHANNEL};
use elyra::testing::{TestApp, TestShell};
use elyra::{command, commands, App, Ctx};
use serde::Serialize;
use serde_json::{json, Value};

/// Every customer.
#[command(can = "customers.view")]
async fn customers_index(_ctx: Ctx) -> Vec<String> {
    vec!["Ada".into(), "Grace".into()]
}

fn assistant() -> Assistant {
    Assistant::new().allow_abilities(["customers.*"])
}

fn app(fake: &AiFake) -> TestApp {
    TestApp::new(
        App::new()
            .allow_abilities(["customers.*"])
            .commands(commands![customers_index])
            .swap(Ai::fake(fake.clone()))
            .assistant(assistant()),
    )
}

/// What kind each event is, in order: `text 0`, `tool`, `answer`.
fn kinds(events: &[AssistantEvent]) -> Vec<String> {
    events
        .iter()
        .map(|e| match e {
            AssistantEvent::Text { step, .. } => format!("text {step}"),
            AssistantEvent::Tool { phase, .. } => format!("tool {phase}"),
            AssistantEvent::Answer { .. } => "answer".into(),
            AssistantEvent::Error { .. } => "error".into(),
            AssistantEvent::Confirm { .. } => "confirm".into(),
            AssistantEvent::Stopped => "stopped".into(),
            AssistantEvent::Script { phase, .. } => format!("script {phase}"),
            _ => "other".into(),
        })
        .collect()
}

#[tokio::test]
async fn what_it_writes_reaches_the_window_step_by_step_before_the_answer() {
    let fake = AiFake::new()
        .tool_call_saying(
            "Let me look at your customers.",
            "customers_index",
            json!({}),
        )
        .text("You have two: Ada and Grace.");
    let app = app(&fake);
    let turn = app.assistant().prompt("Who are my customers?").await;
    assert_eq!(turn.answer.as_deref(), Ok("You have two: Ada and Grace."));
    assert_eq!(
        turn.written(),
        [
            (0, "Let me look at your customers.".to_owned()),
            (1, "You have two: Ada and Grace.".to_owned()),
        ]
    );
    // Written quickly, each step's words went together; what was said
    // before the tool step came before it.
    assert_eq!(
        kinds(&turn.events),
        [
            "text 0",
            "tool started",
            "tool finished",
            "text 1",
            "answer"
        ]
    );
}

#[tokio::test]
async fn pieces_are_gathered_every_50ms() {
    let words: Vec<String> = (1..=12).map(|n| format!("w{n}")).collect();
    let fake = AiFake::new()
        .text(words.join(" "))
        .pace(Duration::from_millis(15));
    let app = app(&fake);
    let turn = app.assistant().prompt("Count").await;
    let texts: Vec<&AssistantEvent> = turn
        .events
        .iter()
        .filter(|e| matches!(e, AssistantEvent::Text { .. }))
        .collect();
    // 12 words over ~180 ms: a few events, not twelve, and not one.
    assert!(
        texts.len() >= 2 && texts.len() <= 6,
        "{} events",
        texts.len()
    );
    assert_eq!(turn.written(), [(0, words.join(" "))]);
}

#[tokio::test]
async fn a_server_that_wont_stream_is_asked_whole_and_not_asked_again() {
    let fake = AiFake::new()
        .error(422, "Streaming isn't proxied.")
        .text("Whole.")
        .text("Again, whole.");
    let app = app(&fake);
    let turn = app.assistant().prompt("Hi").await;
    assert_eq!(turn.answer.as_deref(), Ok("Whole."));
    assert!(turn.written().is_empty());
    // The next turn isn't streamed either: one request, not two.
    let turn = app.assistant().prompt("Hi again").await;
    assert_eq!(turn.answer.as_deref(), Ok("Again, whole."));
    assert!(turn.written().is_empty());
    assert_eq!(fake.requests().len(), 3);
    assert!(fake.is_spent());

    // Another 422 is the error it is.
    let fake = AiFake::new().error(422, "The model `x` isn't allowed here.");
    let app = self::app(&fake);
    let turn = app.assistant().prompt("Hi").await;
    assert!(turn.answer.unwrap_err().contains("isn't allowed here"));
    assert_eq!(fake.requests().len(), 1);
}

// -- Through the shell: a stop mid-answer ------------------------------------

async fn post(shell: &TestShell, op: &str, body: Vec<u8>) -> Value {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__assistant/{op}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "w1")
        .body(body)
        .unwrap();
    rmp_serde::from_slice(shell.handle(request).await.body()).unwrap()
}

fn msgpack(value: impl Serialize) -> Vec<u8> {
    rmp_serde::to_vec_named(&value).unwrap()
}

/// The window's assistant events, until one matches `until`.
async fn events_until(
    bus: &elyra::EventBus,
    until: impl Fn(&AssistantEvent) -> bool,
) -> Vec<AssistantEvent> {
    let mut seen = Vec::new();
    for _ in 0..50 {
        let batch = tokio::time::timeout(Duration::from_secs(5), bus.next_batch_for("w1"))
            .await
            .expect("an event in time");
        let batch: Vec<(String, rmpv::Value)> = rmp_serde::from_slice(&batch).unwrap();
        for (channel, value) in batch {
            if channel != CHANNEL {
                continue;
            }
            let mut buf = Vec::new();
            rmpv::encode::write_value(&mut buf, &value).unwrap();
            let event: AssistantEvent = rmp_serde::from_slice(&buf).unwrap();
            let done = until(&event);
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
    panic!("never saw it: {seen:?}");
}

#[tokio::test]
async fn a_stop_mid_answer_keeps_what_was_written_in_the_conversation() {
    let long: Vec<String> = (1..=40).map(|n| format!("word{n}")).collect();
    let fake = AiFake::new()
        .text(long.join(" "))
        .pace(Duration::from_millis(20))
        .text("Going on.");
    let prepared = App::new()
        .allow_abilities(["customers.*"])
        .commands(commands![customers_index])
        .swap(Ai::fake(fake.clone()))
        .assistant(assistant())
        .prepare();
    let bus = prepared.bus.clone();
    bus.register_client("w1");
    let shell = TestShell::new(prepared);

    assert_eq!(
        post(&shell, "prompt", msgpack("Tell me everything")).await,
        json!(true)
    );
    events_until(&bus, |e| matches!(e, AssistantEvent::Text { .. })).await;
    assert_eq!(post(&shell, "stop", Vec::new()).await, json!(true));
    let events = events_until(&bus, |e| matches!(e, AssistantEvent::Stopped)).await;
    assert!(!events
        .iter()
        .any(|e| matches!(e, AssistantEvent::Answer { .. })));

    // "Go on": the model got what it had written, and that it was stopped.
    assert_eq!(post(&shell, "prompt", msgpack("Go on")).await, json!(true));
    events_until(&bus, |e| matches!(e, AssistantEvent::Answer { .. })).await;
    let sent = &fake.requests()[1].messages;
    assert_eq!(sent[0]["content"], "Tell me everything");
    let said = sent[1]["content"].as_str().unwrap();
    assert_eq!(sent[1]["role"], "assistant");
    assert!(said.starts_with("word1 word2"), "{said}");
    assert!(said.ends_with("\n\n(stopped by the user)"), "{said}");
    assert!(!said.contains("word40"), "stopped before the end: {said}");
    assert_eq!(sent[2]["content"], "Go on");
}
