//! The matrix (RFC 0034), in a debug build: the frames an app declares are
//! kept for the inspector to open, the window they came from tells them
//! its route, and without the event loop nothing pretends to open.
#![cfg(debug_assertions)]

use std::borrow::Cow;

use elyra::testing::http::{Request, Response};
use elyra::testing::TestShell;
use elyra::App;

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

#[derive(serde::Serialize)]
struct At {
    route: &'static str,
}

#[test]
fn the_frames_an_app_declares_are_kept_in_order() {
    let prepared = App::new()
        .matrix(|m| {
            m.frame("Admin");
            m.frame("Can't delete").light().deny_abilities(["*.delete"]);
        })
        .prepare();
    let matrix = prepared.ctx.get::<elyra::matrix::Matrix>();
    let names: Vec<_> = matrix.frames().iter().map(|f| f.name()).collect();
    assert_eq!(names, ["Admin", "Can't delete"]);
}

#[tokio::test]
async fn the_frames_are_told_where_the_window_goes() {
    let prepared = App::new().prepare();
    let bus = prepared.bus.clone();
    let shell = TestShell::new(prepared);
    let (batch, res) = tokio::join!(
        tokio::time::timeout(std::time::Duration::from_secs(5), bus.next_batch_for("w2")),
        async {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            post(
                &shell,
                "/__inspector/matrix-route",
                rmp_serde::to_vec_named(&At {
                    route: "/customers/7",
                })
                .unwrap(),
            )
            .await
        }
    );
    assert!(!failed(&res));
    let batch: Vec<(String, rmpv::Value)> =
        rmp_serde::from_slice(&batch.expect("an event in time")).unwrap();
    let (channel, value) = &batch[0];
    assert_eq!(channel, "elyra:matrix-route");
    assert_eq!(value["route"].as_str(), Some("/customers/7"));
}

#[tokio::test]
async fn without_the_event_loop_no_frame_opens() {
    let shell = TestShell::new(App::new().prepare());
    let res = post(
        &shell,
        "/__inspector/matrix",
        rmp_serde::to_vec_named(&At { route: "/" }).unwrap(),
    )
    .await;
    assert!(failed(&res));
    assert!(String::from_utf8_lossy(res.body()).contains("no windows to open frames in"));
}

#[tokio::test]
async fn the_inspector_off_opens_no_frames() {
    let shell = TestShell::new(App::new().inspector(false).prepare());
    let res = post(
        &shell,
        "/__inspector/matrix",
        rmp_serde::to_vec_named(&At { route: "/" }).unwrap(),
    )
    .await;
    assert!(failed(&res));
    assert!(String::from_utf8_lossy(res.body()).contains("the inspector is off"));
}
