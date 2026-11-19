//! The Theme Studio's draft (RFC 0037), in a debug build: changed through
//! the inspector, sent to every window, refused for what isn't a token or
//! a plain value, and reset.
#![cfg(debug_assertions)]

use std::borrow::Cow;

use elyra::testing::TestShell;
use elyra::App;
use serde_json::{json, Value};
use wry::http::{Request, Response};

async fn post(shell: &TestShell, body: Value) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri("elyra://localhost/__inspector/theme")
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "w1")
        .body(rmp_serde::to_vec_named(&body).unwrap())
        .unwrap();
    shell.handle(request).await
}

fn failed(res: &Response<Cow<'static, [u8]>>) -> bool {
    res.headers().get("x-elyra-status").map(|v| v.as_bytes()) == Some(b"error")
}

fn draft(res: &Response<Cow<'static, [u8]>>) -> Value {
    rmp_serde::from_slice(res.body()).unwrap()
}

#[tokio::test]
async fn a_draft_is_changed_sent_to_every_window_and_reset() {
    let prepared = App::new().prepare();
    let bus = prepared.bus.clone();
    let shell = TestShell::new(prepared);

    let (batch, res) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(5), bus.next_batch_for("w2")),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            post(
                &shell,
                json!({ "look": "dark", "token": "--elyra-accent", "value": "#f97316" }),
            )
            .await
        }
    );
    assert!(!failed(&res));
    assert_eq!(draft(&res)["looks"]["dark"]["--elyra-accent"], "#f97316");
    let batch: Vec<(String, rmpv::Value)> =
        rmp_serde::from_slice(&batch.expect("an event in time")).unwrap();
    assert_eq!(batch[0].0, "elyra:theme-draft");

    let res = post(
        &shell,
        json!({ "look": "all", "token": "--elyra-radius", "value": "10px" }),
    )
    .await;
    assert_eq!(draft(&res)["looks"]["all"]["--elyra-radius"], "10px");
    // Read as it is, without a change.
    let res = post(&shell, Value::Null).await;
    assert_eq!(draft(&res)["looks"]["dark"]["--elyra-accent"], "#f97316");
    // A token put back.
    let res = post(&shell, json!({ "look": "dark", "token": "--elyra-accent" })).await;
    assert!(draft(&res)["looks"].get("dark").is_none());
    // Everything.
    let res = post(&shell, json!({ "reset": true })).await;
    assert_eq!(draft(&res)["looks"], json!({}));
}

#[tokio::test]
async fn only_a_token_and_a_plain_value_are_taken() {
    let shell = TestShell::new(App::new().prepare());
    for body in [
        json!({ "look": "dark", "token": "color", "value": "red" }),
        json!({ "look": "dark", "token": "--elyra-accent", "value": "red; } body { display: none" }),
        json!({ "look": "dark", "token": "--elyra-accent", "value": "url(https://x.test/a.png)" }),
        json!({ "look": "nowhere", "token": "--elyra-accent", "value": "red" }),
    ] {
        let res = post(&shell, body.clone()).await;
        assert!(failed(&res), "{body}");
    }
    let res = post(&shell, Value::Null).await;
    assert_eq!(draft(&res)["looks"], json!({}), "nothing was kept");
}
