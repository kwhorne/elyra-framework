//! Live provider tests. Skipped unless `ELYRA_AI_LIVE=1` and the relevant API
//! key is set — these make real (paid) API calls.

use elyra_ai::{Ai, JsonSchema, Provider};

fn live() -> bool {
    std::env::var("ELYRA_AI_LIVE").is_ok()
}

/// Whether `var` is set to something: CI passes an unset secret as "".
fn set(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| !v.is_empty())
}

#[tokio::test]
async fn anthropic_chat_roundtrip() {
    if !live() || !set("ANTHROPIC_API_KEY") {
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
    if !live() || !set("OPENAI_API_KEY") {
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

#[derive(serde::Deserialize, JsonSchema)]
struct Mood {
    label: String,
    score: u8,
}

#[tokio::test]
async fn anthropic_structured_output() {
    if !live() || !set("ANTHROPIC_API_KEY") {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and ANTHROPIC_API_KEY");
        return;
    }
    let ai = Ai::from_env();
    let mood: Mood = ai
        .chat()
        .provider(Provider::Anthropic)
        .instructions("Classify the sentiment: positive, negative or neutral, scored 1-10.")
        .prompt_as("I love building with Elyra!")
        .await
        .expect("structured output");
    assert!(!mood.label.is_empty());
    assert!(mood.score <= 10);
}

/// One chat per OpenAI-shaped provider (RFC 0048) that has its key (Ollama:
/// none) and a model set: `GEMINI_MODEL`, `GROQ_MODEL`, … — they have no
/// default model.
#[tokio::test]
async fn every_openai_shaped_provider_with_a_key_and_a_model() {
    if !live() {
        eprintln!("skipping: set ELYRA_AI_LIVE=1, a provider's key and its *_MODEL");
        return;
    }
    let ai = Ai::from_env();
    for provider in &Provider::ALL[2..] {
        let model_var = format!("{}_MODEL", provider.as_str().to_uppercase());
        let Some(model) = std::env::var(&model_var).ok().filter(|m| !m.is_empty()) else {
            eprintln!("skipping {}: set {model_var}", provider.as_str());
            continue;
        };
        if provider.needs_key() && !set(provider.env_key()) {
            eprintln!("skipping {}: set {}", provider.as_str(), provider.env_key());
            continue;
        }
        let resp = ai
            .chat()
            .provider(*provider)
            .model(model)
            .instructions("You reply with a single word.")
            .prompt("Say hello")
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", provider.as_str()));
        assert!(!resp.text().is_empty(), "{}", provider.as_str());
    }
}

/// Web search on OpenAI, through the Responses API (RFC 0049).
#[tokio::test]
async fn openai_web_search() {
    if !live() || !set("OPENAI_API_KEY") {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and OPENAI_API_KEY");
        return;
    }
    let ai = Ai::from_env();
    let resp = ai
        .chat()
        .provider(Provider::OpenAI)
        .web_search(elyra_ai::WebSearch::new().allow(["rust-lang.org"]))
        // Norwegian, so letters past ASCII come before the citations.
        .prompt("Hva er den nyeste stabile Rust-versjonen? Svar på norsk, med kilde.")
        .await
        .expect("web search");
    assert!(!resp.text().is_empty());
    citations_fit(&resp);
}

/// Web search on Anthropic, with its citations (RFC 0051).
#[tokio::test]
async fn anthropic_web_search() {
    if !live() || !set("ANTHROPIC_API_KEY") {
        eprintln!("skipping: set ELYRA_AI_LIVE=1 and ANTHROPIC_API_KEY");
        return;
    }
    let resp = Ai::from_env()
        .chat()
        .provider(Provider::Anthropic)
        .web_search(elyra_ai::WebSearch::new().max(2).allow(["rust-lang.org"]))
        .prompt("Hva er den nyeste stabile Rust-versjonen? Svar på norsk, med kilde.")
        .await
        .expect("web search");
    citations_fit(&resp);
}

/// Every citation's span slices the text: what settles how OpenAI counts
/// its indices (RFC 0051, open question 2). Prints them to look at.
fn citations_fit(resp: &elyra_ai::Response) {
    eprintln!("{}", resp.text());
    for c in resp.citations() {
        let words = resp
            .text()
            .get(c.span.clone())
            .unwrap_or_else(|| panic!("{:?} doesn't slice the text", c.span));
        eprintln!("  [{words}] {} {:?}", c.url, c.title);
    }
}
