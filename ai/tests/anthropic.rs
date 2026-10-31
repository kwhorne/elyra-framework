//! What the Anthropic driver sends and how it reads the answers, against a
//! local server standing in for the Messages API.

use std::sync::Arc;

use elyra_ai::{Ai, Error, JsonSchema, Provider, Tool};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the server saw it.
struct Seen {
    headers: Vec<(String, String)>,
    body: Value,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A server that answers each request with the next of `answers`
/// (`content-type`, body) and records what it was sent.
async fn server(answers: Vec<(&'static str, String)>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let answers = Arc::new(Mutex::new(answers.into_iter()));
    let record = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (record, answers) = (record.clone(), answers.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                loop {
                    // Headers, then a body of Content-Length bytes.
                    let end = loop {
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            break i + 4;
                        }
                        let mut chunk = [0u8; 4096];
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    };
                    let head = String::from_utf8_lossy(&buf[..end]).to_string();
                    let headers: Vec<(String, String)> = head
                        .lines()
                        .skip(1)
                        .filter_map(|l| l.split_once(':'))
                        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
                        .collect();
                    let length: usize = headers
                        .iter()
                        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                        .and_then(|(_, v)| v.parse().ok())
                        .unwrap_or(0);
                    while buf.len() < end + length {
                        let mut chunk = [0u8; 4096];
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let body =
                        serde_json::from_slice(&buf[end..end + length]).unwrap_or(Value::Null);
                    buf.drain(..end + length);
                    record.lock().push(Seen { headers, body });
                    let (kind, answer) = answers.lock().next().expect("an answer left");
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\n\r\n{answer}",
                        answer.len()
                    );
                    if socket.write_all(reply.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (url, seen)
}

fn message(content: Value, stop_reason: &str) -> (&'static str, String) {
    (
        "application/json",
        json!({
            "type": "message",
            "role": "assistant",
            "content": content,
            "stop_reason": stop_reason,
            "usage": {"input_tokens": 10, "output_tokens": 5},
        })
        .to_string(),
    )
}

fn ai(url: &str) -> Ai {
    Ai::builder()
        .provider_key(Provider::Anthropic, "sk-test")
        .base_url(Provider::Anthropic, url)
        .retries(0)
        .build()
}

#[tokio::test]
async fn a_prompt_speaks_the_current_api_and_sends_no_temperature() {
    let (url, seen) = server(vec![message(
        json!([{"type": "text", "text": "Hi"}]),
        "end_turn",
    )])
    .await;
    let reply = ai(&url)
        .chat()
        .instructions("Be brief.")
        .temperature(0.3)
        .prompt("Hello")
        .await
        .unwrap();
    assert_eq!(reply.text(), "Hi");

    let seen = seen.lock();
    let sent = &seen[0];
    assert_eq!(sent.header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(sent.header("x-api-key"), Some("sk-test"));
    assert_eq!(sent.header("accept"), Some("application/json"));
    assert_eq!(sent.body["model"], "claude-sonnet-5-5");
    assert_eq!(sent.body["system"], "Be brief.");
    assert_eq!(sent.body["max_tokens"], 4096);
    for refused in ["temperature", "top_p", "top_k"] {
        assert!(
            sent.body.get(refused).is_none(),
            "{refused} sent: {}",
            sent.body
        );
    }
}

#[derive(serde::Deserialize, JsonSchema)]
struct Sentiment {
    label: String,
    score: u8,
}

#[tokio::test]
async fn structured_output_asks_for_json_instead_of_forcing_a_tool() {
    let answer = json!({"label": "positive", "score": 9}).to_string();
    let (url, seen) = server(vec![message(
        json!([{"type": "text", "text": answer}]),
        "end_turn",
    )])
    .await;
    let sentiment: Sentiment = ai(&url)
        .chat()
        .prompt_as("I love building with Elyra!")
        .await
        .unwrap();
    assert_eq!(sentiment.label, "positive");
    assert_eq!(sentiment.score, 9);

    let seen = seen.lock();
    let body = &seen[0].body;
    // The newest models refuse a forced tool_choice.
    assert!(body.get("tool_choice").is_none(), "{body}");
    assert!(body.get("tools").is_none(), "{body}");
    let format = &body["output_config"]["format"];
    assert_eq!(format["type"], "json_schema");
    assert_eq!(format["schema"]["additionalProperties"], false);
    assert_eq!(format["schema"]["properties"]["label"]["type"], "string");
    assert!(format["schema"]["properties"]["score"]
        .get("maximum")
        .is_none());
}

struct Weather;

#[elyra_ai::async_trait]
impl Tool for Weather {
    fn name(&self) -> String {
        "weather".into()
    }
    fn description(&self) -> String {
        "Today's weather in a city.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {"city": {"type": "string"}}})
    }
    async fn call(&self, _args: Value) -> elyra_ai::Result<String> {
        Ok("Sunny, 21°C".into())
    }
}

#[tokio::test]
async fn structured_output_still_lets_the_tools_run_first() {
    let (url, seen) = server(vec![
        message(
            json!([{"type": "tool_use", "id": "t1", "name": "weather", "input": {"city": "Oslo"}}]),
            "tool_use",
        ),
        message(
            json!([{"type": "text", "text": "{\"label\":\"sunny\",\"score\":8}"}]),
            "end_turn",
        ),
    ])
    .await;
    let sentiment: Sentiment = ai(&url)
        .chat()
        .tool(Weather)
        .prompt_as("How is the weather in Oslo?")
        .await
        .unwrap();
    assert_eq!(sentiment.label, "sunny");

    let seen = seen.lock();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].body["tools"][0]["name"], "weather");
    let messages = seen[1].body["messages"].as_array().unwrap();
    let result = &messages.last().unwrap()["content"][0];
    assert_eq!(result["type"], "tool_result");
    assert_eq!(result["content"], "Sunny, 21°C");
    assert_eq!(
        seen[1].body["output_config"]["format"]["type"],
        "json_schema"
    );
}

#[tokio::test]
async fn a_refusal_is_its_own_error() {
    let (url, _) = server(vec![(
        "application/json",
        json!({
            "type": "message",
            "content": [],
            "stop_reason": "refusal",
            "stop_details": {"type": "refusal", "category": "cyber", "explanation": "Can't help with that."},
            "usage": {"input_tokens": 10, "output_tokens": 0},
        })
        .to_string(),
    )])
    .await;
    let err = ai(&url).chat().prompt("…").await.unwrap_err();
    assert!(
        matches!(&err, Error::Refused(r) if r == "Can't help with that."),
        "{err}"
    );
}

fn sse(events: &[Value]) -> (&'static str, String) {
    let body = events
        .iter()
        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
        .collect();
    ("text/event-stream", body)
}

#[tokio::test]
async fn a_stream_sends_no_temperature_and_stops_on_a_refusal() {
    let (url, seen) = server(vec![
        sse(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 3}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Hel"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "lo"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 2}}),
            json!({"type": "message_stop"}),
        ]),
        sse(&[
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 3}}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "Sure"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "refusal"}, "usage": {"output_tokens": 1}}),
            json!({"type": "message_stop"}),
        ]),
    ])
    .await;
    let ai = ai(&url);
    let text = ai
        .chat()
        .temperature(0.2)
        .stream("Hi")
        .collect_text()
        .await
        .unwrap();
    assert_eq!(text, "Hello");
    {
        let seen = seen.lock();
        assert_eq!(seen[0].header("anthropic-version"), Some("2026-10-21"));
        assert_eq!(seen[0].body["stream"], true);
        assert!(seen[0].body.get("temperature").is_none());
    }

    let err = ai.chat().stream("…").collect_text().await.unwrap_err();
    assert!(matches!(err, Error::Refused(_)), "{err}");
}

#[tokio::test]
async fn web_tools_follow_the_model() {
    let text = || message(json!([{"type": "text", "text": "Found it."}]), "end_turn");
    let (url, seen) = server(vec![text(), text()]).await;
    let ai = ai(&url);
    ai.chat()
        .web_fetch(elyra_ai::WebFetch::new())
        .prompt("Read https://example.com")
        .await
        .unwrap();
    ai.chat()
        .model("claude-haiku-4-5")
        .web_fetch(elyra_ai::WebFetch::new())
        .prompt("Read https://example.com")
        .await
        .unwrap();

    let seen = seen.lock();
    assert_eq!(seen[0].body["tools"][0]["type"], "web_fetch_20260209");
    assert_eq!(seen[0].header("anthropic-beta"), None);
    assert_eq!(seen[1].body["tools"][0]["type"], "web_fetch_20250910");
    assert_eq!(
        seen[1].header("anthropic-beta"),
        Some("web-fetch-2025-09-10")
    );
}

#[tokio::test]
async fn a_paused_turn_is_sent_back_and_resumed() {
    let search = json!({"type": "server_tool_use", "id": "s1", "name": "web_search", "input": {"query": "elyra"}});
    let (url, seen) = server(vec![
        message(
            json!([{"type": "text", "text": "Searching…"}, search]),
            "pause_turn",
        ),
        message(
            json!([{"type": "text", "text": "Elyra is a desktop framework."}]),
            "end_turn",
        ),
    ])
    .await;
    let reply = ai(&url)
        .chat()
        .web_search(elyra_ai::WebSearch::new())
        .prompt("What is Elyra?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "Elyra is a desktop framework.");
    assert_eq!(reply.steps(), 1);

    let seen = seen.lock();
    assert_eq!(seen.len(), 2);
    // The paused turn goes back unchanged, last, with no "continue" after it.
    let messages = seen[1].body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[1]["content"][1], search);
}

#[tokio::test]
async fn pauses_count_against_the_step_budget() {
    let paused = || {
        message(
            json!([{"type": "server_tool_use", "id": "s1", "name": "web_search", "input": {}}]),
            "pause_turn",
        )
    };
    let (url, seen) = server(vec![paused(), paused(), paused()]).await;
    let err = ai(&url)
        .chat()
        .max_steps(2)
        .web_search(elyra_ai::WebSearch::new())
        .prompt("What is Elyra?")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MaxSteps(2)), "{err}");
    assert_eq!(seen.lock().len(), 3);
}

#[tokio::test]
async fn an_api_key_goes_where_each_provider_wants_it() {
    let (url, seen) = server(vec![
        message(json!([{"type": "text", "text": "Hi"}]), "end_turn"),
        (
            "application/json",
            json!({"choices": [{"message": {"content": "Hi"}}]}).to_string(),
        ),
    ])
    .await;
    let ai = Ai::builder()
        .provider_key(Provider::Anthropic, "sk-ant")
        .provider_key(Provider::OpenAI, "sk-oai")
        .base_url(Provider::Anthropic, &url)
        .base_url(Provider::OpenAI, &url)
        .retries(0)
        .build();
    ai.chat().prompt("Hello").await.unwrap();
    ai.chat()
        .provider(Provider::OpenAI)
        .prompt("Hello")
        .await
        .unwrap();

    let seen = seen.lock();
    assert_eq!(seen[0].header("x-api-key"), Some("sk-ant"));
    assert_eq!(seen[0].header("authorization"), None);
    assert_eq!(seen[1].header("authorization"), Some("Bearer sk-oai"));
    assert_eq!(seen[1].header("x-api-key"), None);
}

#[tokio::test]
async fn a_bearer_token_is_asked_for_at_each_step() {
    let (url, seen) = server(vec![
        message(
            json!([{"type": "tool_use", "id": "t1", "name": "weather", "input": {"city": "Oslo"}}]),
            "tool_use",
        ),
        message(json!([{"type": "text", "text": "Sunny."}]), "end_turn"),
    ])
    .await;
    // Renewed between the steps.
    let n = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let issued = n.clone();
    let ai = Ai::builder()
        .base_url(Provider::Anthropic, &url)
        .bearer(Provider::Anthropic, move || {
            let n = issued.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            Some(format!("{n}|session"))
        })
        .retries(0)
        .build();
    let reply = ai.chat().tool(Weather).prompt("Oslo?").await.unwrap();
    assert_eq!(reply.text(), "Sunny.");

    let seen = seen.lock();
    assert_eq!(seen[0].header("authorization"), Some("Bearer 1|session"));
    assert_eq!(seen[1].header("authorization"), Some("Bearer 2|session"));
    for sent in seen.iter() {
        assert_eq!(sent.header("x-api-key"), None);
        assert_eq!(sent.header("anthropic-version"), Some("2026-10-21"));
    }
}

#[tokio::test]
async fn without_a_token_nothing_is_sent() {
    let (url, seen) = server(vec![]).await;
    let ai = Ai::builder()
        .base_url(Provider::Anthropic, &url)
        .bearer(Provider::Anthropic, || None)
        .retries(0)
        .build();
    let err = ai.chat().prompt("Hello").await.unwrap_err();
    assert!(matches!(err, Error::NoToken("anthropic")), "{err}");
    assert_eq!(err.to_string(), "no token for anthropic: not signed in");
    let err = ai.chat().stream("Hello").collect_text().await.unwrap_err();
    assert!(matches!(err, Error::NoToken("anthropic")), "{err}");
    assert!(seen.lock().is_empty());

    // An unset provider is still a missing key.
    let err = Ai::builder()
        .build()
        .chat()
        .prompt("Hello")
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::MissingKey("ANTHROPIC_API_KEY")),
        "{err}"
    );
}

#[tokio::test]
async fn a_streams_bearer_token_is_sent_too() {
    let (url, seen) = server(vec![sse(&[
        json!({"type": "message_start", "message": {"usage": {"input_tokens": 3}}}),
        json!({"type": "content_block_delta", "delta": {"type": "text_delta", "text": "Hi"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}, "usage": {"output_tokens": 1}}),
        json!({"type": "message_stop"}),
    ])])
    .await;
    let ai = Ai::builder()
        .base_url(Provider::Anthropic, &url)
        .bearer(Provider::Anthropic, || Some("7|s3cr3t".into()))
        .retries(0)
        .build();
    assert_eq!(
        ai.chat().stream("Hello").collect_text().await.unwrap(),
        "Hi"
    );
    assert_eq!(
        seen.lock()[0].header("authorization"),
        Some("Bearer 7|s3cr3t")
    );
}
