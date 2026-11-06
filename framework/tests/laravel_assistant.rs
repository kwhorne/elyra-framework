//! RFC 0011 against a real Laravel app (scripts/laravel-backend.sh) with this
//! repository's elyracode/elyra-backend: the desktop's assistant reaches
//! Claude through `Route::elyraAssistant()`, which adds the server's key and
//! caps what may be asked. Anthropic is a local stand-in at
//! `ELYRA_ASSISTANT_UPSTREAM`, where the app's .env points
//! `ELYRA_ASSISTANT_UPSTREAM`. Skipped unless `ELYRA_LARAVEL_URL` and
//! `ELYRA_ASSISTANT_UPSTREAM` say where they are; CI starts them.
#![cfg(all(feature = "ai", feature = "backend"))]

use std::collections::VecDeque;
use std::sync::Arc;

use elyra::ai::{Ai, Error as AiError, Provider, WebSearch};
use elyra::assistant::Assistant;
use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::http::Http;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the stand-in for Anthropic saw it.
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

type Answers = Arc<Mutex<VecDeque<Value>>>;

/// Anthropic's Messages API, at `address`: each request gets the next of
/// the queued answers, or a plain "OK." when there are none.
async fn anthropic(address: &str) -> (Answers, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind(address).await.expect("the upstream port");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let answers: Answers = Arc::new(Mutex::new(VecDeque::new()));
    let (record, queue) = (seen.clone(), answers.clone());
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (record, queue) = (record.clone(), queue.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
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
                let body = serde_json::from_slice(&buf[end..end + length]).unwrap_or(Value::Null);
                record.lock().push(Seen { headers, body });
                let answer = queue.lock().pop_front().unwrap_or_else(|| text("OK."));
                // `{ "sse": [..], "pause": ms }`: half, a pause, the rest, with
                // no length, as a stream comes.
                if let (Some(events), Some(pause)) = (
                    answer.get("sse").and_then(Value::as_array),
                    answer.get("pause").and_then(Value::as_u64),
                ) {
                    let text: Vec<String> = events
                        .iter()
                        .map(|e| format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap()))
                        .collect();
                    let half = text.len() / 2;
                    let head = "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n";
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(text[..half].concat().as_bytes()).await;
                    let _ = socket.flush().await;
                    tokio::time::sleep(std::time::Duration::from_millis(pause)).await;
                    let _ = socket.write_all(text[half..].concat().as_bytes()).await;
                    return;
                }
                // `{ "sse": [events] }`: answered as the API streams.
                let (kind, answer) = match answer.get("sse").and_then(Value::as_array) {
                    Some(events) => (
                        "text/event-stream",
                        events
                            .iter()
                            .map(|e| {
                                format!("event: {}\ndata: {e}\n\n", e["type"].as_str().unwrap())
                            })
                            .collect::<String>(),
                    ),
                    None => ("application/json", answer.to_string()),
                };
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (answers, seen)
}

/// A step as the API streams it: `said`, then a call to `tool` if any.
fn streamed(said: &str, tool: Option<&str>) -> Value {
    let mut events = vec![
        json!({ "type": "message_start", "message": { "usage": { "input_tokens": 12 } } }),
        json!({ "type": "content_block_start", "index": 0, "content_block": { "type": "text", "text": "" } }),
    ];
    for word in said.split_inclusive(' ') {
        events.push(json!({ "type": "content_block_delta", "index": 0, "delta": { "type": "text_delta", "text": word } }));
    }
    events.push(json!({ "type": "content_block_stop", "index": 0 }));
    if let Some(tool) = tool {
        events.push(json!({ "type": "content_block_start", "index": 1, "content_block": { "type": "tool_use", "id": "t9", "name": tool, "input": {} } }));
        events.push(json!({ "type": "content_block_delta", "index": 1, "delta": { "type": "input_json_delta", "partial_json": "{}" } }));
        events.push(json!({ "type": "content_block_stop", "index": 1 }));
    }
    let stop = if tool.is_some() {
        "tool_use"
    } else {
        "end_turn"
    };
    events.push(json!({ "type": "message_delta", "delta": { "stop_reason": stop }, "usage": { "output_tokens": 7 } }));
    events.push(json!({ "type": "message_stop" }));
    json!({ "sse": events })
}

fn text(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })
}

#[command(can = "customers.view")]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    let page: Value = ctx.get::<Backend>().get("/api/customers").json().await?;
    Ok(page["total"].as_i64().unwrap_or(0))
}

#[command(can = "customers.delete")]
async fn customers_destroy(ctx: Ctx, id: i64) -> elyra::Result<()> {
    ctx.get::<Backend>()
        .delete(format!("/api/customers/{id}"))
        .send()
        .await?;
    Ok(())
}

/// A Sanctum token for `email`, as the desktop gets one.
async fn token(url: &str, email: &str) -> String {
    let response = Http::new()
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": email, "password": "secret", "device_name": "tests" }))
        .send()
        .await
        .unwrap();
    response.text().trim_matches('"').to_owned()
}

/// The proxy, as `Backend::assistant()` reaches it, with `token`.
fn through_laravel(url: &str, token: String) -> Ai {
    Ai::builder()
        .base_url(Provider::Anthropic, format!("{url}/api/elyra/assistant"))
        .bearer(Provider::Anthropic, move || Some(token.clone()))
        .retries(0)
        .build()
}

#[tokio::test]
async fn the_assistant_reaches_claude_through_laravel() {
    let (Ok(url), Ok(upstream)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_ASSISTANT_UPSTREAM"),
    ) else {
        eprintln!("skipped: ELYRA_LARAVEL_URL / ELYRA_ASSISTANT_UPSTREAM not set");
        return;
    };
    let (answers, seen) = anthropic(&upstream).await;

    // Bob may only see customers: the assistant offers him no more.
    let app = TestApp::new(
        App::new()
            .backend(Backend::new(&url).abilities().assistant())
            .token_store(Arc::new(MemoryTokens::new()))
            .commands(commands![customers_count, customers_destroy])
            .assistant(Assistant::new().allow_abilities(["customers.*"])),
    );
    let signed_out = app.assistant().prompt("Hi").await;
    assert_eq!(
        signed_out.answer.unwrap_err(),
        "Sign in to use the assistant."
    );
    assert!(seen.lock().is_empty());

    app.get::<Auth>()
        .sign_in("bob@example.com", "secret")
        .await
        .unwrap();
    let abilities = app.get::<elyra::abilities::Abilities>();
    for _ in 0..100 {
        if abilities.loaded() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    answers.lock().extend([
        json!({
            "type": "message",
            "role": "assistant",
            "content": [{ "type": "tool_use", "id": "t1", "name": "customers_count", "input": {} }],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 },
        }),
        text("There are some customers."),
    ]);
    let turn = app.assistant().prompt("How many customers?").await;
    assert_eq!(turn.answer.as_deref(), Ok("There are some customers."));
    assert_eq!(turn.ran(), [("customers_count", true)]);
    {
        let seen = seen.lock();
        assert_eq!(seen.len(), 2);
        let first = &seen[0];
        // Laravel's key, not the user's token, reaches Anthropic.
        assert_eq!(first.header("x-api-key"), Some("sk-ant-server-test"));
        assert_eq!(first.header("authorization"), None);
        assert_eq!(first.header("anthropic-version"), Some("2026-10-21"));
        let tools: Vec<&str> = first.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(tools, ["customers_count"]);
        let result = &seen[1].body["messages"][2]["content"][0];
        assert_eq!(result["type"], "tool_result");
        assert!(result["content"].as_str().unwrap().parse::<i64>().is_ok());
    }

    // The answer as it's written (RFC 0018): asked for a stream, Laravel
    // passes Claude's through, tool step and all.
    app.assistant().reset();
    answers.lock().extend([
        streamed("Let me count them.", Some("customers_count")),
        streamed("You have several customers.", None),
    ]);
    let turn = app.assistant().prompt("How many customers now?").await;
    assert_eq!(turn.answer.as_deref(), Ok("You have several customers."));
    assert_eq!(
        turn.written(),
        [
            (0, "Let me count them.".to_owned()),
            (1, "You have several customers.".to_owned()),
        ]
    );
    assert_eq!(turn.ran(), [("customers_count", true)]);
    {
        let seen = seen.lock();
        let last = &seen[seen.len() - 1];
        assert_eq!(last.body["stream"], true);
        assert_eq!(last.header("x-api-key"), Some("sk-ant-server-test"));
        assert_eq!(
            last.body["messages"][1]["content"][0],
            json!({ "type": "text", "text": "Let me count them." })
        );
    }

    // …as it comes: the first words reach the desktop before the rest is
    // written, not after the server has it all.
    let mut slow = streamed("one two three four five six seven eight", None);
    slow["pause"] = json!(600);
    answers.lock().push_back(slow);
    let started = std::time::Instant::now();
    let told = Arc::new(Mutex::new(Vec::new()));
    let seen_at = told.clone();
    let reply = through_laravel(&url, token(&url, "bob@example.com").await)
        .chat()
        .on_text(move |_, piece| seen_at.lock().push((started.elapsed(), piece.to_owned())))
        .prompt("Count")
        .await
        .unwrap();
    assert_eq!(reply.text(), "one two three four five six seven eight");
    let told = told.lock().clone();
    let (first, last) = (told.first().unwrap().0, told.last().unwrap().0);
    assert!(
        last - first >= std::time::Duration::from_millis(400),
        "the first words came {first:?}, the last {last:?}: held back"
    );

    // What may be asked is the server's to say.
    let ai = through_laravel(&url, token(&url, "bob@example.com").await);
    let before = seen.lock().len();
    let err = ai
        .chat()
        .model("claude-opus-5-5")
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AiError::Api { status: 422, message } if message.contains("isn't allowed here")),
        "{err}"
    );
    let err = ai
        .chat()
        .web_search(WebSearch::default())
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(
        matches!(&err, AiError::Api { status: 422, message } if message == "Server tools aren't allowed here."),
        "{err}"
    );
    assert_eq!(seen.lock().len(), before, "neither reached Anthropic");
    // RFC 0027: code execution passes where the server allows it
    // (ELYRA_ASSISTANT_CODE_EXECUTION=true in this app); web search above
    // still didn't.
    ai.chat().code_execution().prompt("Hi").await.unwrap();
    let last = seen.lock().last().unwrap().body.clone();
    assert!(
        last["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["type"] == "code_execution_20260120"),
        "{last}"
    );
    ai.chat().max_tokens(99_999).prompt("Hi").await.unwrap();
    assert_eq!(seen.lock().last().unwrap().body["max_tokens"], 4096);

    // A user's requests per minute are capped (ELYRA_ASSISTANT_PER_MINUTE).
    let ai = through_laravel(&url, token(&url, "ada@example.com").await);
    let mut limited = None;
    for _ in 0..20 {
        if let Err(e) = ai.chat().prompt("Hi").await {
            limited = Some(e);
            break;
        }
    }
    let err = limited.expect("a 429 within 20 requests");
    assert!(
        matches!(&err, AiError::Api { status: 429, message } if message.contains("try again in a minute")),
        "{err}"
    );

    // Without a token, Laravel says no.
    let err = through_laravel(&url, "nope".into())
        .chat()
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(matches!(err, AiError::Api { status: 401, .. }), "{err}");
}
