//! The providers that speak OpenAI's shape (RFC 0048): where each request
//! goes, how it authenticates, a tool round trip and a stream through one of
//! them, and failover with each provider's own model, against a local server.

use std::sync::Arc;

use elyra_ai::{Ai, Error, Provider, Tool};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the server saw it.
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

/// An answer: status, content type, body.
type Answer = (u16, &'static str, String);

/// A server that answers each request with the next of `answers` and records
/// what it was sent.
async fn server(answers: Vec<Answer>) -> (String, Arc<Mutex<Vec<Seen>>>) {
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
                        .lines()
                        .next()
                        .and_then(|l| l.split_whitespace().nth(1))
                        .unwrap_or_default()
                        .to_string();
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
                    record.lock().push(Seen {
                        path,
                        headers,
                        body,
                    });
                    let (status, kind, answer) = answers.lock().next().expect("an answer left");
                    let reply = format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: {kind}\r\ncontent-length: {}\r\n\r\n{answer}",
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

/// A Chat Completions answer with `message`.
fn completion(message: Value) -> Answer {
    (
        200,
        "application/json",
        json!({
            "choices": [{"index": 0, "message": message, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 7, "completion_tokens": 3},
        })
        .to_string(),
    )
}

fn text(content: &str) -> Answer {
    completion(json!({"role": "assistant", "content": content}))
}

/// A tool that reports what it was called with.
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
        Ok(format!("customer {} is Ada", args["id"]))
    }
}

#[tokio::test]
async fn each_provider_posts_to_its_chat_path_with_its_key() {
    for provider in [
        Provider::Gemini,
        Provider::Groq,
        Provider::Mistral,
        Provider::DeepSeek,
        Provider::XAi,
        Provider::OpenRouter,
    ] {
        let (url, seen) = server(vec![text("hello")]).await;
        let ai = Ai::builder()
            .provider_key(provider, "secret")
            .base_url(provider, format!("{url}/v9"))
            .retries(0)
            .build();
        let reply = ai
            .chat()
            .provider(provider)
            .model("some-model")
            .prompt("Hi")
            .await
            .unwrap();
        assert_eq!(reply.text(), "hello", "{provider:?}");
        let seen = seen.lock();
        assert_eq!(seen[0].path, "/v9/chat/completions", "{provider:?}");
        assert_eq!(seen[0].header("authorization"), Some("Bearer secret"));
        assert_eq!(seen[0].body["model"], "some-model");
        assert_eq!(seen[0].body["messages"][0]["content"], "Hi");
    }
}

#[tokio::test]
async fn a_provider_without_a_key_or_a_model_says_so_before_sending() {
    let ai = Ai::builder().retries(0).build();
    let err = ai
        .chat()
        .provider(Provider::Gemini)
        .model("m")
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::MissingKey("GEMINI_API_KEY")), "{err}");

    let ai = Ai::builder().provider_key(Provider::Groq, "k").build();
    let err = ai
        .chat()
        .provider(Provider::Groq)
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(matches!(err, Error::NotConfigured(_)), "{err}");
    let err = ai
        .chat()
        .provider(Provider::Compatible)
        .model("m")
        .prompt("Hi")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("base URL for compatible"), "{err}");
}

#[tokio::test]
async fn ollama_and_a_compatible_endpoint_send_a_key_only_when_there_is_one() {
    let (url, seen) = server(vec![text("local"), text("compatible"), text("keyed")]).await;
    let ai = Ai::builder()
        .base_url(Provider::Ollama, format!("{url}/v1"))
        .base_url(Provider::Compatible, format!("{url}/llm"))
        .text_model_for(Provider::Ollama, "llama3.2")
        .text_model_for(Provider::Compatible, "house-model")
        .retries(0)
        .build();
    assert_eq!(
        ai.chat()
            .provider(Provider::Ollama)
            .prompt("Hi")
            .await
            .unwrap()
            .text(),
        "local"
    );
    ai.chat()
        .provider(Provider::Compatible)
        .prompt("Hi")
        .await
        .unwrap();
    let keyed = Ai::builder()
        .base_url(Provider::Compatible, format!("{url}/llm"))
        .provider_key(Provider::Compatible, "house-key")
        .text_model_for(Provider::Compatible, "house-model")
        .retries(0)
        .build();
    keyed
        .chat()
        .provider(Provider::Compatible)
        .prompt("Hi")
        .await
        .unwrap();

    let seen = seen.lock();
    assert_eq!(seen[0].path, "/v1/chat/completions");
    assert_eq!(seen[0].header("authorization"), None);
    assert_eq!(seen[0].body["model"], "llama3.2");
    assert_eq!(seen[1].path, "/llm/chat/completions");
    assert_eq!(seen[1].header("authorization"), None);
    assert_eq!(seen[1].body["model"], "house-model");
    assert_eq!(seen[2].header("authorization"), Some("Bearer house-key"));
}

#[tokio::test]
async fn a_tool_round_trip_runs_on_the_shared_driver() {
    let call = completion(json!({
        "role": "assistant",
        "content": null,
        "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {"name": "customer", "arguments": "{\"id\": 42}"},
        }],
    }));
    let (url, seen) = server(vec![call, text("Customer 42 is Ada.")]).await;
    let ai = Ai::builder()
        .provider_key(Provider::Groq, "k")
        .base_url(Provider::Groq, &url)
        .text_model_for(Provider::Groq, "llama")
        .retries(0)
        .build();
    let reply = ai
        .chat()
        .provider(Provider::Groq)
        .tool(Customer)
        .prompt("Who is customer 42?")
        .await
        .unwrap();
    assert_eq!(reply.text(), "Customer 42 is Ada.");
    assert_eq!(reply.steps(), 1);
    assert_eq!(reply.usage().input_tokens, 14);

    let seen = seen.lock();
    assert_eq!(seen[0].body["tools"][0]["function"]["name"], "customer");
    let answered = &seen[1].body["messages"];
    let last = &answered[answered.as_array().unwrap().len() - 1];
    assert_eq!(last["role"], "tool");
    assert_eq!(last["tool_call_id"], "call_1");
    assert_eq!(last["content"], "customer 42 is Ada");
}

#[tokio::test]
async fn a_stream_runs_on_the_shared_driver() {
    let sse = [
        json!({"choices": [{"delta": {"content": "Hel"}}]}),
        json!({"choices": [{"delta": {"content": "lo"}}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 4, "completion_tokens": 2}}),
    ]
    .iter()
    .map(|v| format!("data: {v}\n\n"))
    .collect::<String>()
        + "data: [DONE]\n\n";
    let (url, seen) = server(vec![(200, "text/event-stream", sse)]).await;
    let ai = Ai::builder()
        .provider_key(Provider::Mistral, "k")
        .base_url(Provider::Mistral, &url)
        .text_model_for(Provider::Mistral, "mistral-small")
        .build();
    let mut stream = ai.chat().provider(Provider::Mistral).stream("Hi");
    let mut text = String::new();
    let mut usage = None;
    while let Some(chunk) = stream.next().await {
        match chunk.unwrap() {
            elyra_ai::StreamChunk::Delta(t) => text.push_str(&t),
            elyra_ai::StreamChunk::Done(u) => usage = Some(u),
        }
    }
    assert_eq!(text, "Hello");
    assert_eq!(usage.unwrap().output_tokens, 2);
    let seen = seen.lock();
    assert_eq!(seen[0].path, "/chat/completions");
    assert_eq!(seen[0].body["stream"], true);
    assert_eq!(seen[0].body["model"], "mistral-small");
}

#[tokio::test]
async fn failover_sends_each_provider_its_own_model() {
    let failed = (
        500,
        "application/json",
        r#"[{"error": {"code": 500, "message": "overloaded"}}]"#.to_string(),
    );
    let (url, seen) = server(vec![failed, text("from openai")]).await;
    let ai = Ai::builder()
        .provider_key(Provider::Gemini, "g")
        .provider_key(Provider::OpenAI, "o")
        .base_url(Provider::Gemini, format!("{url}/gemini"))
        .base_url(Provider::OpenAI, &url)
        .text_model("for-everyone")
        .text_model_for(Provider::Gemini, "gemini-model")
        .text_model_for(Provider::OpenAI, "openai-model")
        .retries(0)
        .build();
    let reply = ai
        .chat()
        .provider(Provider::Gemini)
        .failover([Provider::OpenAI])
        .prompt("Hi")
        .await
        .unwrap();
    assert_eq!(reply.text(), "from openai");
    let seen = seen.lock();
    assert_eq!(seen[0].path, "/gemini/chat/completions");
    assert_eq!(seen[0].body["model"], "gemini-model");
    assert_eq!(seen[1].path, "/v1/chat/completions");
    assert_eq!(seen[1].body["model"], "openai-model");
}

#[tokio::test]
async fn a_fallback_without_a_model_keeps_the_error_that_happened() {
    let failed = (503, "text/plain", "Service Unavailable".to_string());
    let (url, _seen) = server(vec![failed]).await;
    let ai = Ai::builder()
        .provider_key(Provider::OpenAI, "o")
        .provider_key(Provider::Groq, "g")
        .base_url(Provider::OpenAI, &url)
        .retries(0)
        .build();
    let err = ai
        .chat()
        .provider(Provider::OpenAI)
        .failover([Provider::Groq])
        .prompt("Hi")
        .await
        .unwrap_err();
    match err {
        Error::Api { status, message } => {
            assert_eq!(status, 503);
            assert_eq!(message, "Service Unavailable");
        }
        other => panic!("expected the 503, got {other}"),
    }
}
