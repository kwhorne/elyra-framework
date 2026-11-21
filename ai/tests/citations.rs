//! The sources a web-search answer cites (RFC 0051), read from Anthropic's
//! text-block citations and OpenAI's `url_citation` annotations, against a
//! local server; and scripted with the fake.

mod common;

use common::{server, Answer};
use elyra_ai::{Ai, AiFake, Citation, Provider, WebSearch};
use serde_json::{json, Value};

fn anthropic_message(content: Value) -> Answer {
    (
        200,
        "application/json",
        json!({
            "type": "message",
            "role": "assistant",
            "content": content,
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5},
        })
        .to_string(),
    )
}

fn found(url: &str, title: &str, quote: &str) -> Value {
    json!({
        "type": "web_search_result_location",
        "url": url,
        "title": title,
        "cited_text": quote,
        "encrypted_index": "Eo8BCioIAhgBIiQ=",
    })
}

/// What Anthropic answers after a search: the search, then text blocks, some
/// with the results they rest on.
fn anthropic_answer() -> Value {
    json!([
        {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {"query": "rust release"}},
        {"type": "web_search_tool_result", "tool_use_id": "srvtoolu_1", "content": []},
        {"type": "text", "text": "Here's what changed. "},
        {"type": "text", "text": "Rust 2.0 shipped in May", "citations": [
            found("https://blog.rust-lang.org/2.0", "Announcing Rust 2.0", "We're happy to announce Rust 2.0"),
            found("https://news.example/rust", "Rust 2.0 is out", "Rust 2.0 is out today"),
        ]},
        {"type": "text", "text": ", and édition 2027 is next", "citations": [
            found("https://doc.rust-lang.org/edition-guide", "The Edition Guide", "Edition 2027"),
            {"type": "char_location", "document_index": 0, "cited_text": "x", "start_char_index": 0, "end_char_index": 1},
        ]},
        {"type": "text", "text": "."},
    ])
}

fn anthropic(url: &str) -> Ai {
    Ai::builder()
        .provider_key(Provider::Anthropic, "sk-a")
        .base_url(Provider::Anthropic, url)
        .retries(0)
        .build()
}

/// The words `c` backs in `text`.
fn backs<'t>(text: &'t str, c: &Citation) -> &'t str {
    &text[c.span.clone()]
}

#[tokio::test]
async fn anthropic_s_cited_blocks_become_spans_of_the_text() {
    let (url, _seen) = server(vec![anthropic_message(anthropic_answer())]).await;
    let reply = anthropic(&url)
        .chat()
        .web_search(WebSearch::new())
        .prompt("What changed in Rust?")
        .await
        .unwrap();
    let text = reply.text();
    assert_eq!(
        text,
        "Here's what changed. Rust 2.0 shipped in May, and édition 2027 is next."
    );
    let cited = reply.citations();
    // The document location isn't a web search result: skipped.
    assert_eq!(cited.len(), 3);
    assert_eq!(backs(text, &cited[0]), "Rust 2.0 shipped in May");
    assert_eq!(cited[0].url, "https://blog.rust-lang.org/2.0");
    assert_eq!(cited[0].title.as_deref(), Some("Announcing Rust 2.0"));
    assert_eq!(
        cited[0].cited_text.as_deref(),
        Some("We're happy to announce Rust 2.0")
    );
    // Two sources on one block share its span, in the lab's order.
    assert_eq!(cited[1].span, cited[0].span);
    assert_eq!(cited[1].url, "https://news.example/rust");
    assert_eq!(backs(text, &cited[2]), ", and édition 2027 is next");
}

fn sse(events: &[Value]) -> Answer {
    let body = events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect();
    (200, "text/event-stream", body)
}

#[tokio::test]
async fn a_streamed_answer_has_the_citations_a_whole_one_would() {
    let citation = found(
        "https://blog.rust-lang.org/2.0",
        "Announcing Rust 2.0",
        "Rust 2.0",
    );
    let (url, _seen) = server(vec![sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 9}}}),
        json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "In short: "}}),
        json!({"type": "content_block_stop", "index": 0}),
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": "", "citations": []}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "citations_delta", "citation": citation}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Rust 2.0 "}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "shipped"}}),
        json!({"type": "content_block_stop", "index": 1}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 4}}),
        json!({"type": "message_stop"}),
    ])])
    .await;
    let reply = anthropic(&url)
        .chat()
        .web_search(WebSearch::new())
        .on_text(|_, _| {})
        .prompt("Rust?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "In short: Rust 2.0 shipped");
    let cited = reply.citations();
    assert_eq!(cited.len(), 1);
    assert_eq!(backs(reply.text(), &cited[0]), "Rust 2.0 shipped");
    assert_eq!(cited[0].cited_text.as_deref(), Some("Rust 2.0"));
}

fn openai_response(output: Value) -> Answer {
    (
        200,
        "application/json",
        json!({
            "object": "response",
            "status": "completed",
            "output": output,
            "usage": {"input_tokens": 20, "output_tokens": 6},
        })
        .to_string(),
    )
}

fn url_citation(url: &str, start: usize, end: usize) -> Value {
    json!({"type": "url_citation", "url": url, "title": format!("About {url}"), "start_index": start, "end_index": end})
}

#[tokio::test]
async fn openai_s_annotations_count_characters_from_their_own_part() {
    // "Blåbær: " is 8 characters and 10 bytes; the second part starts after
    // the first, so its indices are moved by the first's length in bytes.
    let first = "Blåbær: Norway's berry. ";
    let second = "Søt og blå.";
    let (url, _seen) = server(vec![openai_response(json!([
        {"type": "web_search_call", "id": "ws_1", "status": "completed"},
        {"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": first, "annotations": [
                url_citation("https://berries.example/blabaer", 8, 23),
                // Empty once read: dropped.
                url_citation("https://empty.example", 3, 3),
            ]},
            {"type": "output_text", "text": second, "annotations": [
                // Past the end: clamped to the part.
                url_citation("https://ordbok.example/sot", 0, 400),
                {"type": "file_citation", "file_id": "f_1", "index": 0},
            ]},
        ]},
    ]))])
    .await;
    let reply = Ai::builder()
        .provider_key(Provider::OpenAI, "sk-o")
        .base_url(Provider::OpenAI, &url)
        .retries(0)
        .build()
        .chat()
        .provider(Provider::OpenAI)
        .web_search(WebSearch::new())
        .prompt("Blåbær?")
        .await
        .unwrap();
    let text = reply.text();
    let cited = reply.citations();
    assert_eq!(cited.len(), 2);
    assert_eq!(backs(text, &cited[0]), "Norway's berry.");
    assert_eq!(cited[0].url, "https://berries.example/blabaer");
    assert_eq!(
        cited[0].title.as_deref(),
        Some("About https://berries.example/blabaer")
    );
    assert_eq!(cited[0].cited_text, None);
    assert_eq!(backs(text, &cited[1]), "Søt og blå.");
}

#[tokio::test]
async fn failover_cites_what_the_provider_that_answered_found() {
    let overloaded = (
        529,
        "application/json",
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})
            .to_string(),
    );
    let (url, seen) = server(vec![
        overloaded,
        openai_response(json!([
            {"type": "message", "role": "assistant", "content": [
                {"type": "output_text", "text": "From OpenAI.", "annotations": [
                    url_citation("https://openai.example", 5, 11),
                ]},
            ]},
        ])),
    ])
    .await;
    let reply = Ai::builder()
        .provider_key(Provider::Anthropic, "sk-a")
        .provider_key(Provider::OpenAI, "sk-o")
        .base_url(Provider::Anthropic, &url)
        .base_url(Provider::OpenAI, &url)
        .retries(0)
        .build()
        .chat()
        .provider(Provider::Anthropic)
        .failover([Provider::OpenAI])
        .web_search(WebSearch::new())
        .prompt("Who?")
        .await
        .unwrap();
    assert_eq!(reply.citations().len(), 1);
    assert_eq!(backs(reply.text(), &reply.citations()[0]), "OpenAI");
    let seen = seen.lock();
    assert_eq!(seen[1].path, "/v1/responses");
    assert_eq!(seen[1].header("authorization"), Some("Bearer sk-o"));
    assert_eq!(seen[1].body["tools"][0]["type"], "web_search");
}

#[tokio::test]
async fn an_answer_without_a_search_or_from_the_cache_cites_nothing() {
    let (url, seen) = server(vec![anthropic_message(
        json!([{"type": "text", "text": "Hi."}]),
    )])
    .await;
    let ai = Ai::builder()
        .provider_key(Provider::Anthropic, "sk-a")
        .base_url(Provider::Anthropic, &url)
        .retries(0)
        .cache(true)
        .build();
    let first = ai.chat().prompt("Hello").await.unwrap();
    assert!(first.citations().is_empty());
    let cached = ai.chat().prompt("Hello").await.unwrap();
    assert_eq!(cached.text(), "Hi.");
    assert!(cached.citations().is_empty());
    assert_eq!(seen.lock().len(), 1);
}

#[tokio::test]
async fn the_fake_scripts_a_cited_answer() {
    let fake = AiFake::new().text_citing(
        "Rust 2.0 shipped.",
        [Citation::new("https://blog.rust-lang.org", 0..8)
            .title("Rust Blog")
            .cited_text("Rust 2.0")],
    );
    let reply = Ai::fake(fake.clone())
        .chat()
        .web_search(WebSearch::new())
        .prompt("Rust?")
        .await
        .unwrap();
    assert_eq!(
        reply.citations(),
        [Citation::new("https://blog.rust-lang.org", 0..8)
            .title("Rust Blog")
            .cited_text("Rust 2.0")]
    );
    assert_eq!(backs(reply.text(), &reply.citations()[0]), "Rust 2.0");
    assert!(fake.is_spent());
}

#[test]
#[should_panic(expected = "isn't a range of the text")]
fn the_fake_refuses_a_span_off_the_text() {
    // "é" is two bytes: 0..1 splits it.
    let _ = AiFake::new().text_citing("é", [Citation::new("https://x.example", 0..1)]);
}

#[test]
fn a_citation_serializes_for_the_page() {
    let c = Citation::new("https://x.example", 2..5).title("X");
    assert_eq!(
        serde_json::to_value(&c).unwrap(),
        json!({"url": "https://x.example", "title": "X", "cited_text": null, "span": {"start": 2, "end": 5}})
    );
}
