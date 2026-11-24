//! A window that only reads (RFC 0041): `WindowConfig::read_only()` runs
//! only live commands, under the live write guard, says no in `can` to what
//! only writers need, and keeps only the routes that read.
#![cfg(feature = "database")]

use elyra::testing::TestShell;
use elyra::window::WindowConfig;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

/// One invoice, live.
#[command(live, can = "invoices.view")]
async fn invoices_show(_ctx: Ctx, id: i64) -> i64 {
    id
}

/// Delete an invoice.
#[command(can = "invoices.delete")]
async fn invoices_destroy(_ctx: Ctx, id: i64) -> i64 {
    id
}

/// Live, but it writes: what a read-only window's guard is for.
#[command(live, can = "invoices.view")]
async fn invoices_touch(_ctx: Ctx, id: i64) -> elyra::Result<i64> {
    elyra::db::live::check_write("invoices").map_err(|e| elyra::Error::Command(e.to_string()))?;
    Ok(id)
}

/// No ability, not live.
#[command]
async fn ping(_ctx: Ctx) -> String {
    "pong".into()
}

/// Live, and takes its time: something to cancel.
#[command(live)]
async fn slow(_ctx: Ctx) -> String {
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    "done".into()
}

fn shell() -> TestShell {
    let prepared = App::new()
        .allow_ability("invoices.*")
        .commands(commands![
            invoices_show,
            invoices_destroy,
            invoices_touch,
            ping,
            slow
        ])
        .prepare();
    TestShell::new(prepared)
        .window(WindowConfig::new("main"))
        .window(WindowConfig::new("look").read_only())
}

async fn call(
    shell: &TestShell,
    window: &str,
    route: &str,
    body: Value,
) -> Result<Vec<u8>, (String, String)> {
    let request = elyra::testing::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__{route}"))
        .header("x-elyra-token", shell.token())
        .body(rmp_serde::to_vec(&body).unwrap())
        .unwrap();
    let response = shell.handle_in(window, request).await;
    if response
        .headers()
        .get("x-elyra-status")
        .is_some_and(|v| v == "error")
    {
        let kind = response
            .headers()
            .get("x-elyra-error-kind")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        return Err((kind, String::from_utf8_lossy(response.body()).into_owned()));
    }
    Ok(response.body().to_vec())
}

#[tokio::test]
async fn only_live_commands_run_and_only_as_reads() {
    let shell = shell();
    let shown = call(&shell, "look", "cmd/invoices_show", json!([42]))
        .await
        .unwrap();
    assert_eq!(rmp_serde::from_slice::<i64>(&shown).unwrap(), 42);

    for command in ["invoices_destroy", "ping"] {
        let refused = call(&shell, "look", &format!("cmd/{command}"), json!([42]))
            .await
            .unwrap_err();
        assert_eq!(refused.0, "denied", "{command}");
        assert!(refused.1.contains("only reads"), "{command}: {}", refused.1);
    }

    // Live, but it writes: refused at the write, as a live query's re-run is.
    let refused = call(&shell, "look", "cmd/invoices_touch", json!([42]))
        .await
        .unwrap_err();
    assert!(refused.1.contains("must only read"), "{}", refused.1);

    // The main window has no such limit.
    for command in ["invoices_destroy", "invoices_touch", "ping"] {
        let body = if command == "ping" {
            json!([])
        } else {
            json!([42])
        };
        assert!(
            call(&shell, "main", &format!("cmd/{command}"), body)
                .await
                .is_ok(),
            "{command}"
        );
    }
}

#[tokio::test]
async fn only_the_routes_that_read_are_there() {
    let shell = shell();
    for route in [
        "store/set",
        "store/delete",
        "sys/clipboard-write",
        "sys/open",
        "window/quit",
        "queue/push",
        // What changes something outside the window.
        "assistant/key",
        "presence/editing",
        "presence/follow",
        "presence/follow-answer",
        "presence/stop-followers",
        "presence/route",
        "i18n/locale",
    ] {
        let refused = call(&shell, "look", route, json!(null)).await.unwrap_err();
        assert_eq!(refused.0, "denied", "{route}: {}", refused.1);
    }
    // Reading the store isn't refused for the window (the app may not have
    // a store; that's another answer).
    let read = call(&shell, "look", "store/get", json!("theme")).await;
    assert!(
        read.as_ref().err().is_none_or(|(kind, _)| kind != "denied"),
        "{read:?}"
    );
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Limits {
    only: Option<Vec<String>>,
    deny: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
struct Answer {
    window: Option<Limits>,
}

#[tokio::test]
async fn can_says_no_to_what_only_writers_need() {
    let shell = shell();
    let bytes = call(&shell, "look", "auth/abilities", json!(null))
        .await
        .unwrap();
    let limits = rmp_serde::from_slice::<Answer>(&bytes)
        .unwrap()
        .window
        .unwrap();
    // invoices.view is a live command's too, so it stays.
    assert_eq!(
        limits,
        Limits {
            only: None,
            deny: vec!["invoices.delete".into()]
        }
    );
    let bytes = call(&shell, "main", "auth/abilities", json!(null))
        .await
        .unwrap();
    assert!(rmp_serde::from_slice::<Answer>(&bytes)
        .unwrap()
        .window
        .is_none());
}

async fn call_as(
    shell: &TestShell,
    window: &str,
    id: &str,
) -> elyra::testing::http::Response<std::borrow::Cow<'static, [u8]>> {
    let request = elyra::testing::http::Request::builder()
        .method("POST")
        .uri("elyra://localhost/__cmd/slow")
        .header("x-elyra-token", shell.token())
        .header("x-elyra-request-id", id)
        .body(rmp_serde::to_vec(&json!([])).unwrap())
        .unwrap();
    shell.handle_in(window, request).await
}

fn cancelled(response: &elyra::testing::http::Response<std::borrow::Cow<'static, [u8]>>) -> bool {
    response
        .headers()
        .get("x-elyra-error-kind")
        .is_some_and(|v| v == "cancelled")
}

#[tokio::test]
async fn a_window_cancels_only_its_own_calls() {
    let shell = shell();
    let cancel = |window: &'static str| {
        let shell = &shell;
        async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            call(shell, window, "cancel", json!("1-1")).await.unwrap();
        }
    };
    // Another window, the read-only one here, can't stop main's call by
    // its id.
    let (answer, ()) = tokio::join!(call_as(&shell, "main", "1-1"), cancel("look"));
    assert!(!cancelled(&answer));
    assert_eq!(
        rmp_serde::from_slice::<String>(answer.body()).unwrap(),
        "done"
    );
    // The window that made it can.
    let (answer, ()) = tokio::join!(call_as(&shell, "main", "1-1"), cancel("main"));
    assert!(cancelled(&answer));
}
