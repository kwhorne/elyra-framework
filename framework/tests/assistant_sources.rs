//! Where the assistant reaches its model (RFC 0011 step 4): through the
//! Laravel backend with the user's token, else the user's own key in the
//! keychain, else the container's client.
#![cfg(all(feature = "ai", feature = "backend"))]

use std::sync::Arc;

use elyra::ai::{Ai, AiFake};
use elyra::assistant::{Assistant, ModelSource, KEY_SECRET};
use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::http::{Http, HttpFake};
use elyra::secrets::Secrets;
use elyra::testing::{TestApp, TestShell};
use elyra::{command, commands, App, Ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the stand-in server saw it.
struct Seen {
    path: String,
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

/// A server standing in for `Route::elyraAssistant()`: it answers every
/// request with `answer` and records what it was sent.
async fn proxy(answer: Value) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let answer = answer.to_string();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (record, answer) = (record.clone(), answer.clone());
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
                let path = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap_or_default()
                    .to_owned();
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
                record.lock().push(Seen {
                    path,
                    headers,
                    body,
                });
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    (url, seen)
}

fn answer(text: &str) -> Value {
    json!({
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "text", "text": text }],
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })
}

/// Sign-in, as Laravel answers it, at `base`.
fn laravel(base: &str) -> HttpFake {
    HttpFake::new()
        .post(format!("{base}/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(
            format!("{base}/api/user"),
            200,
            json!({ "id": 1, "name": "Ada" }),
        )
        .delete(format!("{base}/api/sanctum/token"), 204, Value::Null)
}

/// Every customer.
#[command(can = "customers.view")]
async fn customers_index(_ctx: Ctx) -> Vec<String> {
    vec!["Ada".into()]
}

#[tokio::test]
async fn through_the_backend_with_the_users_token() {
    let (base, seen) = proxy(answer("You have one customer.")).await;
    // A client in the container too: the backend comes first.
    let fake = AiFake::new().text("never");
    let app = TestApp::new(
        App::new()
            .backend(Backend::new(&base).assistant())
            .token_store(Arc::new(MemoryTokens::new()))
            .swap(Http::fake(laravel(&base)))
            .swap(Ai::fake(fake.clone()))
            .swap(Secrets::fake())
            .commands(commands![customers_index])
            .assistant(Assistant::new().allow_ability("customers.view")),
    );
    let assistant = app.assistant();
    assert_eq!(
        app.get::<elyra::assistant::Assistants>().source(),
        ModelSource::Backend
    );

    // Signed out: nothing is sent.
    let turn = assistant.prompt("How many?").await;
    assert_eq!(turn.answer.unwrap_err(), "Sign in to use the assistant.");
    assert!(seen.lock().is_empty());

    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    let turn = assistant.prompt("How many?").await;
    assert_eq!(turn.answer.as_deref(), Ok("You have one customer."));
    assert!(
        fake.requests().is_empty(),
        "the container's client wasn't asked"
    );

    let seen = seen.lock();
    assert_eq!(seen.len(), 1);
    let sent = &seen[0];
    assert_eq!(sent.path, "/api/elyra/assistant/v1/messages");
    assert_eq!(sent.header("authorization"), Some("Bearer 7|s3cr3t"));
    assert_eq!(sent.header("x-api-key"), None, "no key in the app");
    assert_eq!(sent.header("anthropic-version"), Some("2026-10-21"));
    assert_eq!(sent.body["model"], "claude-sonnet-5-5");
    assert_eq!(sent.body["tools"][0]["name"], "customers_index");
}

#[tokio::test]
async fn another_route_on_the_backend() {
    let (base, seen) = proxy(answer("Hi.")).await;
    let app = TestApp::new(
        App::new()
            .backend(Backend::new(&base).assistant_route("/api/ai"))
            .token_store(Arc::new(MemoryTokens::new()))
            .swap(Http::fake(laravel(&base)))
            .commands(commands![customers_index])
            .assistant(Assistant::new().model("claude-opus-5-5")),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    assert!(app.assistant().prompt("Hi").await.answer.is_ok());
    let seen = seen.lock();
    assert_eq!(seen[0].path, "/api/ai/v1/messages");
    assert_eq!(seen[0].body["model"], "claude-opus-5-5");
}

/// `POST /__assistant/<op>`: the decoded answer, or the error's text.
async fn post(shell: &TestShell, op: &str, body: Vec<u8>) -> Result<Value, String> {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__assistant/{op}"))
        .header("x-elyra-token", shell.token())
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

fn msgpack(value: Value) -> Vec<u8> {
    rmp_serde::to_vec_named(&value).unwrap()
}

#[tokio::test]
async fn the_users_own_key_in_the_keychain() {
    let secrets = Secrets::fake();
    let shell = TestShell::new(
        App::new()
            .swap(Ai::fake(AiFake::new()))
            .swap(secrets.clone())
            .commands(commands![customers_index])
            .assistant(Assistant::new())
            .prepare(),
    );
    assert_eq!(
        post(&shell, "source", Vec::new()).await,
        Ok(json!("environment"))
    );

    // Kept, and used from now on.
    assert_eq!(
        post(&shell, "key", msgpack(json!("sk-ant-mine"))).await,
        Ok(json!(true))
    );
    assert_eq!(
        secrets.get(KEY_SECRET).unwrap().as_deref(),
        Some("sk-ant-mine")
    );
    assert_eq!(
        post(&shell, "source", Vec::new()).await,
        Ok(json!("keychain"))
    );
    // Blank is no key.
    assert_eq!(
        post(&shell, "key", msgpack(json!("  "))).await,
        Ok(json!(false))
    );
    post(&shell, "key", msgpack(json!("sk-ant-mine")))
        .await
        .unwrap();
    // Forgotten.
    assert_eq!(
        post(&shell, "key", msgpack(Value::Null)).await,
        Ok(json!(false))
    );
    assert!(secrets.get(KEY_SECRET).unwrap().is_none());
    assert_eq!(
        post(&shell, "source", Vec::new()).await,
        Ok(json!("environment"))
    );
}

#[tokio::test]
async fn the_backend_comes_before_the_keychain_and_none_is_none() {
    let secrets = Secrets::fake();
    secrets.set(KEY_SECRET, "sk-ant-mine").unwrap();
    let app = TestApp::new(
        App::new()
            .backend(Backend::new("https://crm.test").assistant())
            .token_store(Arc::new(MemoryTokens::new()))
            .swap(secrets)
            .assistant(Assistant::new()),
    );
    assert_eq!(
        app.get::<elyra::assistant::Assistants>().source(),
        ModelSource::Backend
    );

    let app = TestApp::new(
        App::new()
            .backend(Backend::new("https://crm.test"))
            .token_store(Arc::new(MemoryTokens::new()))
            .assistant(Assistant::new()),
    );
    let assistants = app.get::<elyra::assistant::Assistants>();
    assert_eq!(assistants.source(), ModelSource::None);
    assert_eq!(
        assistants.set_key(Some("sk-ant-mine")),
        Err("Keeping a key needs the keychain: add SecretsProvider.".into())
    );
}
