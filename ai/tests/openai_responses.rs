//! Web search on OpenAI through the Responses API (RFC 0049), against a local
//! server: what's sent, the app's tools around a search, structured output,
//! errors, and failover from Anthropic keeping the search.

mod common;

use common::{server, Answer};
use elyra_ai::{Ai, Error, JsonSchema, Provider, Tool, UserLocation, WebFetch, WebSearch};
use serde_json::{json, Value};

/// A Responses API answer with these output items.
fn response(output: Value) -> Answer {
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

fn said(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{"type": "output_text", "text": text, "annotations": [
            {"type": "url_citation", "url": "https://blog.rust-lang.org", "title": "Rust Blog"}
        ]}],
    })
}

fn searched() -> Value {
    json!({"type": "web_search_call", "id": "ws_1", "status": "completed"})
}

fn openai(url: &str) -> Ai {
    Ai::builder()
        .provider_key(Provider::OpenAI, "sk-o")
        .base_url(Provider::OpenAI, url)
        .retries(0)
        .build()
}

struct Customer;

#[elyra_ai::async_trait]
impl Tool for Customer {
    fn name(&self) -> String {
        "customer".into()
    }
    fn description(&self) -> String {
        "Look a customer up by id.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"id": {"type": "integer"}}, "required": ["id"]})
    }
    async fn call(&self, args: Value) -> elyra_ai::Result<String> {
        Ok(format!("customer {} is in Oslo", args["id"]))
    }
}

#[tokio::test]
async fn a_search_goes_to_the_responses_api_with_what_openai_can_honour() {
    let (url, seen) = server(vec![response(json!([
        searched(),
        said("Rust 2.0 shipped.")
    ]))])
    .await;
    let reply = openai(&url)
        .chat()
        .provider(Provider::OpenAI)
        .instructions("Cite sources.")
        .web_search(
            WebSearch::new()
                .max(2)
                .allow(["rust-lang.org"])
                .location(UserLocation {
                    country: Some("NO".into()),
                    ..Default::default()
                }),
        )
        .prompt("What's new in Rust?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "Rust 2.0 shipped.");
    assert_eq!(reply.usage().input_tokens, 20);
    assert_eq!(reply.steps(), 0);

    let seen = seen.lock();
    assert_eq!(seen[0].path, "/v1/responses");
    assert_eq!(seen[0].header("authorization"), Some("Bearer sk-o"));
    let body = &seen[0].body;
    assert_eq!(body["instructions"], "Cite sources.");
    assert_eq!(
        body["input"],
        json!([{"role": "user", "content": "What's new in Rust?"}])
    );
    assert_eq!(body["store"], false);
    assert_eq!(
        body["tools"],
        json!([{
            "type": "web_search",
            "filters": {"allowed_domains": ["rust-lang.org"]},
            "user_location": {"type": "approximate", "country": "NO"},
        }])
    );
    assert_eq!(body["model"], "gpt-4o");
}

#[tokio::test]
async fn the_apps_tools_run_around_the_search() {
    let call = json!({
        "type": "function_call",
        "id": "fc_1",
        "call_id": "call_1",
        "name": "customer",
        "arguments": "{\"id\": 42}",
    });
    let (url, seen) = server(vec![
        response(json!([searched(), call])),
        response(json!([said("Customer 42 is in Oslo, where it's sunny.")])),
    ])
    .await;
    let reply = openai(&url)
        .chat()
        .provider(Provider::OpenAI)
        .web_search(WebSearch::new())
        .tool(Customer)
        .prompt("Weather where customer 42 is?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "Customer 42 is in Oslo, where it's sunny.");
    assert_eq!(reply.steps(), 1);
    assert_eq!(reply.usage().output_tokens, 12);

    let seen = seen.lock();
    assert_eq!(seen[0].body["tools"][1]["type"], "function");
    assert_eq!(seen[0].body["tools"][1]["name"], "customer");
    // The second request carries the first answer back, then the call's output.
    let input = seen[1].body["input"].as_array().unwrap();
    assert_eq!(input.len(), 4);
    assert_eq!(input[1]["type"], "web_search_call");
    assert_eq!(input[2]["type"], "function_call");
    assert_eq!(
        input[3],
        json!({"type": "function_call_output", "call_id": "call_1", "output": "customer 42 is in Oslo"})
    );
}

#[derive(serde::Deserialize, JsonSchema)]
struct Release {
    version: String,
}

#[tokio::test]
async fn structured_output_is_the_responses_text_format() {
    let (url, seen) = server(vec![response(json!([
        searched(),
        said(r#"{"version": "2.0"}"#)
    ]))])
    .await;
    let release: Release = openai(&url)
        .chat()
        .provider(Provider::OpenAI)
        .web_search(WebSearch::new())
        .prompt_as("Latest Rust version?")
        .await
        .unwrap();
    assert_eq!(release.version, "2.0");
    let seen = seen.lock();
    assert_eq!(seen[0].body["text"]["format"]["type"], "json_schema");
    assert_eq!(seen[0].body["text"]["format"]["name"], "respond");
    assert!(seen[0].body["text"]["format"]["schema"]["properties"]["version"].is_object());
}

#[tokio::test]
async fn what_openai_cant_do_is_said_before_sending() {
    let (url, seen) = server(vec![]).await;
    let ai = openai(&url);
    let err = ai
        .chat()
        .provider(Provider::OpenAI)
        .web_search(WebSearch::new().block(["spam.example"]))
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("WebSearch::block on OpenAI"),
        "{err}"
    );
    let err = ai
        .chat()
        .provider(Provider::OpenAI)
        .web_fetch(WebFetch::new())
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("web fetch requires the Anthropic provider"),
        "{err}"
    );
    // Other OpenAI-shaped providers have no web search here.
    let err = Ai::builder()
        .provider_key(Provider::Groq, "g")
        .text_model_for(Provider::Groq, "m")
        .build()
        .chat()
        .provider(Provider::Groq)
        .web_search(WebSearch::new())
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Unsupported(_)), "{err}");
    assert!(seen.lock().is_empty());
}

#[tokio::test]
async fn a_failed_response_and_a_refusal_are_errors() {
    let failed = (
        200,
        "application/json",
        json!({"status": "failed", "error": {"code": "server_error", "message": "search backend down"}, "output": []})
            .to_string(),
    );
    let refused = response(json!([{
        "type": "message",
        "role": "assistant",
        "content": [{"type": "refusal", "refusal": "I can't help with that."}],
    }]));
    let bad_request = (
        400,
        "application/json",
        json!({"error": {"message": "web_search is not supported with this model"}}).to_string(),
    );
    let (url, _seen) = server(vec![failed, refused, bad_request]).await;
    let ai = openai(&url);
    let ask = || {
        ai.chat()
            .provider(Provider::OpenAI)
            .web_search(WebSearch::new())
            .prompt("Hi")
    };
    let err = ask().await.unwrap_err();
    assert!(err.to_string().contains("search backend down"), "{err}");
    let err = ask().await.unwrap_err();
    assert!(
        matches!(err, Error::Refused(ref r) if r == "I can't help with that."),
        "{err}"
    );
    match ask().await.unwrap_err() {
        Error::Api { status, message } => {
            assert_eq!(status, 400);
            assert_eq!(message, "web_search is not supported with this model");
        }
        other => panic!("expected the 400, got {other}"),
    }
}

#[tokio::test]
async fn failover_from_anthropic_keeps_the_search() {
    let overloaded = (
        529,
        "application/json",
        json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})
            .to_string(),
    );
    let (url, seen) = server(vec![
        overloaded,
        response(json!([searched(), said("From OpenAI.")])),
    ])
    .await;
    let ai = Ai::builder()
        .provider_key(Provider::Anthropic, "sk-a")
        .provider_key(Provider::OpenAI, "sk-o")
        .base_url(Provider::Anthropic, &url)
        .base_url(Provider::OpenAI, &url)
        .retries(0)
        .build();
    let reply = ai
        .chat()
        .provider(Provider::Anthropic)
        .failover([Provider::OpenAI])
        .web_search(WebSearch::new())
        .prompt("What's new?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "From OpenAI.");
    let seen = seen.lock();
    assert_eq!(seen[0].path, "/v1/messages");
    assert_eq!(seen[1].path, "/v1/responses");
    assert_eq!(seen[1].body["tools"][0]["type"], "web_search");
}
