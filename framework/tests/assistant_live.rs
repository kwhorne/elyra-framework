//! The app's assistant against Claude itself (RFC 0011 step 6): one in-app
//! round trip, with a command as its tool and a confirmation. Skipped unless
//! `ELYRA_AI_LIVE=1` and `ANTHROPIC_API_KEY` are set — it makes real (paid)
//! API calls.
#![cfg(feature = "ai")]

use std::sync::Mutex;

use elyra::ai::AiProvider;
use elyra::assistant::Assistant;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};

/// The ids `todos_destroy` ran for.
static DELETED: Mutex<Vec<i64>> = Mutex::new(Vec::new());

/// How many todos are open, and their ids and titles.
#[command(can = "todos.view")]
async fn todos_index(_ctx: Ctx) -> Vec<(i64, String)> {
    vec![
        (1, "Buy milk".into()),
        (2, "Call Ada".into()),
        (3, "File taxes".into()),
    ]
}

/// Delete the todo with this `id`. It can't be undone.
#[command(can = "todos.delete")]
async fn todos_destroy(_ctx: Ctx, id: i64) {
    DELETED.lock().unwrap().push(id);
}

fn live() -> bool {
    std::env::var("ELYRA_AI_LIVE").is_ok() && std::env::var("ANTHROPIC_API_KEY").is_ok()
}

#[tokio::test]
async fn claude_uses_the_apps_commands_and_asks_before_deleting() {
    if !live() {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and ANTHROPIC_API_KEY");
        return;
    }
    let app = TestApp::new(
        App::new()
            .provider(AiProvider)
            .commands(commands![todos_index, todos_destroy])
            .assistant(
                Assistant::new()
                    .allow_abilities(["todos.*"])
                    .confirm("todos.delete")
                    .instructions("You help the user keep their todo list. Be brief."),
            ),
    );

    let assistant = app.assistant();
    let turn = assistant.prompt("How many todos do I have?").await;
    let answer = turn.answer.clone().expect("an answer");
    assert!(
        turn.ran().contains(&("todos_index", true)),
        "{:?}",
        turn.events
    );
    assert!(
        answer.contains('3') || answer.to_lowercase().contains("three"),
        "{answer}"
    );

    // Declined: Claude is told it didn't run, and says so.
    let turn = assistant.prompt("Delete the one about taxes.").await;
    assert!(turn.answer.is_ok(), "{:?}", turn.answer);
    assert_eq!(turn.asked(), ["todos_destroy"], "{:?}", turn.events);
    assert!(DELETED.lock().unwrap().is_empty());

    // Accepted, in the same conversation.
    let turn = assistant
        .accepting()
        .prompt("Yes, I'm sure: delete it.")
        .await;
    assert!(turn.answer.is_ok(), "{:?}", turn.answer);
    assert_eq!(*DELETED.lock().unwrap(), [3]);
}
