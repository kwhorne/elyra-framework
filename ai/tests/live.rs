//! Live provider tests. Skipped unless `ELYRA_AI_LIVE=1` and the relevant API
//! key is set — these make real (paid) API calls.

use elyra_ai::{Ai, JsonSchema, Provider};

fn live() -> bool {
    std::env::var("ELYRA_AI_LIVE").is_ok()
}

#[tokio::test]
async fn anthropic_chat_roundtrip() {
    if !live() || std::env::var("ANTHROPIC_API_KEY").is_err() {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and ANTHROPIC_API_KEY");
        return;
    }
    let ai = Ai::from_env();
    let resp = ai
        .chat()
        .provider(Provider::Anthropic)
        .instructions("You reply with a single word.")
        .prompt("Say hello")
        .await
        .expect("chat");
    assert!(!resp.text().is_empty());
}

#[derive(serde::Deserialize, JsonSchema)]
struct Sentiment {
    label: String,
    #[allow(dead_code)]
    score: i32,
}

#[tokio::test]
async fn openai_structured_output() {
    if !live() || std::env::var("OPENAI_API_KEY").is_err() {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and OPENAI_API_KEY");
        return;
    }
    let ai = Ai::from_env();
    let s: Sentiment = ai
        .chat()
        .provider(Provider::OpenAI)
        .instructions("Classify sentiment as positive/negative/neutral.")
        .prompt_as("I love this framework!")
        .await
        .expect("structured");
    assert!(!s.label.is_empty());
}
