//! Pointing at it for a coding agent (RFC 0035), in a debug build: what the
//! page sends becomes a task, written where `rata dev` said the workspace
//! is, and kept for `rata mcp dev` until it's done.
#![cfg(debug_assertions)]

use std::borrow::Cow;

use elyra::testing::TestShell;
use elyra::App;
use serde_json::json;
use wry::http::{Request, Response};

async fn post(shell: &TestShell, path: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "w1")
        .body(body)
        .unwrap();
    shell.handle(request).await
}

fn failed(res: &Response<Cow<'static, [u8]>>) -> bool {
    res.headers().get("x-elyra-status").map(|v| v.as_bytes()) == Some(b"error")
}

#[derive(serde::Deserialize, Debug)]
struct Handed {
    id: u64,
    copied: bool,
    markdown: String,
    file: Option<String>,
}

#[tokio::test]
async fn what_the_page_points_at_becomes_a_task_in_the_workspace() {
    let workspace = std::env::temp_dir().join(format!("elyra-point-{}", std::process::id()));
    std::fs::create_dir_all(&workspace).unwrap();
    // Only this test sets it, in this test binary.
    std::env::set_var("ELYRA_WORKSPACE", &workspace);
    let shell = TestShell::new(App::new().prepare());

    let asked = json!({
        "note": "Make this a danger button",
        "route": "/todos",
        "rect": { "x": 10.0, "y": 20.0, "width": 200.0, "height": 100.0 },
        "viewport": { "x": 0.0, "y": 0.0, "width": 900.0, "height": 640.0 },
        "elements": [
            { "tree": "- button \"Delete\"", "source": "src/App.svelte:3", "stack": ["src/main.js:1 ‹App›"] }
        ],
        "sources": true,
    });
    let res = post(
        &shell,
        "/__inspector/task",
        rmp_serde::to_vec_named(&asked).unwrap(),
    )
    .await;
    assert!(!failed(&res), "{}", String::from_utf8_lossy(res.body()));
    let handed: Handed = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(handed.id, 1);
    assert!(
        handed.markdown.starts_with("## UI task 1 — "),
        "{}",
        handed.markdown
    );
    assert!(handed.markdown.contains("Make this a danger button"));
    assert!(handed
        .markdown
        .contains("- button \"Delete\" — src/App.svelte:3 (in src/main.js:1 ‹App›)"));
    // No event loop here, so no picture: the task says where it pointed all the same.
    assert!(handed
        .markdown
        .contains("Pointed at 200×100 CSS pixels at (10, 20):"));
    assert_eq!(handed.file.as_deref(), Some(".elyra/tasks/1.md"));
    let written = std::fs::read_to_string(workspace.join(".elyra/tasks/1.md")).unwrap();
    assert_eq!(written, handed.markdown);
    // Not on the clipboard here: no app is running, and the page copies it.
    assert!(!handed.copied);

    // A second one gets the next id.
    let res = post(
        &shell,
        "/__inspector/task",
        rmp_serde::to_vec_named(&asked).unwrap(),
    )
    .await;
    let handed: Handed = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(handed.id, 2);
    let _ = std::fs::remove_dir_all(workspace);
}

#[tokio::test]
async fn a_task_needs_what_was_pointed_at() {
    let shell = TestShell::new(App::new().prepare());
    let res = post(
        &shell,
        "/__inspector/task",
        rmp_serde::to_vec_named(&json!({ "note": "x" })).unwrap(),
    )
    .await;
    assert!(failed(&res));
}

#[tokio::test]
async fn without_a_microphone_there_is_no_dictation() {
    let shell = TestShell::new(App::new().prepare());
    let res = post(&shell, "/__inspector/point", Vec::new()).await;
    assert!(!failed(&res));
    let point: serde_json::Value = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(point["dictation"], false);
}
