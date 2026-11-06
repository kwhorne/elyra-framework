//! The assistant's code mode (RFC 0027): Claude calls the app's tools from
//! code in Anthropic's code execution sandbox (programmatic tool calling),
//! each call checked as any call is, confirmations still the user's. Against
//! a local server standing in for the Messages API.
#![cfg(feature = "ai")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use elyra::ai::{Ai, Provider};
use elyra::assistant::{Assistant, AssistantEvent};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

static SHOWN: AtomicUsize = AtomicUsize::new(0);
static DELETED: AtomicUsize = AtomicUsize::new(0);

/// One invoice.
#[command(can = "invoices.view")]
async fn invoices_show(_ctx: Ctx, id: i64) -> Value {
    SHOWN.fetch_add(1, Ordering::SeqCst);
    json!({ "id": id, "total": 100 })
}

/// Delete an invoice. It can't be undone.
#[command(can = "invoices.delete")]
async fn invoices_delete(_ctx: Ctx, id: i64) -> i64 {
    DELETED.fetch_add(1, Ordering::SeqCst);
    id
}

/// A server answering each request with the next of `answers`, recording
/// each request's body.
async fn model(answers: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>) {
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
                    let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                    let length: usize = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse().ok())
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
                    record.lock().push(body);
                    let answer = answers.lock().next().expect("an answer left").to_string();
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{answer}",
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

/// Code run `run` paused, waiting for `calls`; `code` when it starts here.
fn paused(run: &str, code: Option<&str>, calls: &[(&str, &str, Value)]) -> Value {
    let mut content = Vec::new();
    if let Some(code) = code {
        content.push(json!({ "type": "server_tool_use", "id": run, "name": "code_execution", "input": { "code": code } }));
    }
    for (id, name, input) in calls {
        content.push(json!({
            "type": "tool_use", "id": id, "name": name, "input": input,
            "caller": { "type": "code_execution_20260120", "tool_id": run },
        }));
    }
    json!({
        "type": "message", "role": "assistant", "content": content, "stop_reason": "tool_use",
        "container": { "id": "container_1", "expires_at": "2026-11-06T23:00:00Z" },
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })
}

/// The run's end and the answer.
fn done(run: &str, stdout: &str, text: &str) -> Value {
    json!({
        "type": "message", "role": "assistant", "stop_reason": "end_turn",
        "content": [
            { "type": "code_execution_tool_result", "tool_use_id": run,
              "content": { "type": "code_execution_result", "stdout": stdout, "stderr": "", "return_code": 0, "content": [] } },
            { "type": "text", "text": text },
        ],
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })
}

fn app(url: &str, assistant: Assistant) -> TestApp {
    let ai = Ai::builder()
        .provider_key(Provider::Anthropic, "sk-ant-test")
        .base_url(Provider::Anthropic, url)
        .retries(0)
        .build();
    TestApp::new(
        App::new()
            .commands(commands![invoices_show, invoices_delete])
            .swap(ai)
            .assistant(assistant),
    )
}

fn assistant() -> Assistant {
    Assistant::new()
        .allow_ability("invoices.*")
        .confirm("invoices.delete")
}

/// Each tool spec in a request, by name.
fn spec(body: &Value, name: &str) -> Value {
    body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .cloned()
        .unwrap_or(Value::Null)
}

#[tokio::test]
async fn code_calls_the_apps_tools_and_the_panel_sees_the_run_and_its_calls() {
    let code = "total = 0\nfor i in [1, 2]:\n    total += json.loads(await invoices_show({'id': i}))['total']\nprint(total)";
    let (url, seen) = model(vec![
        paused(
            "srv_1",
            Some(code),
            &[("t1", "invoices_show", json!({ "id": 1 }))],
        ),
        paused(
            "srv_1",
            None,
            &[("t2", "invoices_show", json!({ "id": 2 }))],
        ),
        done("srv_1", "200\n", "Invoices 1 and 2 total 200."),
    ])
    .await;
    let before = SHOWN.load(Ordering::SeqCst);
    let app = app(&url, assistant().code_mode());
    let turn = app.assistant().prompt("Total invoices 1 and 2").await;
    assert_eq!(turn.answer.as_deref(), Ok("Invoices 1 and 2 total 200."));
    // Each call from code ran the command, through the pipeline.
    assert_eq!(SHOWN.load(Ordering::SeqCst) - before, 2);

    // The run, its two calls between its start and its end, then the answer.
    let script = |phase: &str| {
        turn.events
            .iter()
            .position(|e| matches!(e, AssistantEvent::Script { phase: p, .. } if p == phase))
            .unwrap_or_else(|| panic!("no script {phase} in {:?}", turn.events))
    };
    let (started, finished) = (script("started"), script("finished"));
    assert!(matches!(
        &turn.events[started],
        AssistantEvent::Script { code: Some(c), .. } if c == code
    ));
    assert!(matches!(
        &turn.events[finished],
        AssistantEvent::Script { ok: Some(true), output: Some(o), .. } if o == "200"
    ));
    let calls: Vec<&AssistantEvent> = turn.events[started..finished]
        .iter()
        .filter(|e| matches!(e, AssistantEvent::Tool { phase, .. } if phase == "finished"))
        .collect();
    assert_eq!(calls.len(), 2, "{:?}", turn.events);
    assert!(matches!(
        turn.events.last(),
        Some(AssistantEvent::Answer { .. })
    ));

    let seen = seen.lock();
    // Code execution, and only the tool that needs no confirmation callable from code.
    assert_eq!(
        spec(&seen[0], "code_execution")["type"],
        "code_execution_20260120"
    );
    assert_eq!(
        spec(&seen[0], "invoices_show")["allowed_callers"],
        json!(["direct", "code_execution_20260120"])
    );
    assert!(spec(&seen[0], "invoices_delete")
        .get("allowed_callers")
        .is_none());
    assert_eq!(seen[1]["container"], "container_1");
}

#[tokio::test]
async fn code_cant_call_a_tool_the_user_confirms() {
    let (url, seen) = model(vec![
        paused(
            "srv_1",
            Some("await invoices_delete({'id': 7})"),
            &[("t1", "invoices_delete", json!({ "id": 7 }))],
        ),
        json!({
            "type": "message", "role": "assistant", "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "I'll ask you before deleting it." }],
            "usage": { "input_tokens": 10, "output_tokens": 5 },
        }),
    ])
    .await;
    let before = DELETED.load(Ordering::SeqCst);
    let app = app(&url, assistant().code_mode());
    let turn = app.assistant().accepting().prompt("Delete invoice 7").await;
    assert_eq!(
        turn.answer.as_deref(),
        Ok("I'll ask you before deleting it.")
    );
    // Not run, and the user wasn't asked about it from code either.
    assert_eq!(DELETED.load(Ordering::SeqCst), before);
    assert!(turn.asked().is_empty(), "{:?}", turn.events);
    let seen = seen.lock();
    let result = &seen[1]["messages"].as_array().unwrap().last().unwrap()["content"][0];
    assert!(result["content"]
        .as_str()
        .unwrap()
        .contains("call it directly, so the user can confirm it"));
}

#[tokio::test]
async fn without_code_mode_theres_no_code_execution() {
    let (url, seen) = model(vec![json!({
        "type": "message", "role": "assistant", "stop_reason": "end_turn",
        "content": [{ "type": "text", "text": "Hello." }],
        "usage": { "input_tokens": 10, "output_tokens": 5 },
    })])
    .await;
    let app = app(&url, assistant());
    let turn = app.assistant().prompt("Hi").await;
    assert_eq!(turn.answer.as_deref(), Ok("Hello."));
    let seen = seen.lock();
    assert!(spec(&seen[0], "code_execution").is_null());
    assert!(spec(&seen[0], "invoices_show")
        .get("allowed_callers")
        .is_none());
}
