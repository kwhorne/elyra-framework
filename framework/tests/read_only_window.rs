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

fn shell() -> TestShell {
    let prepared = App::new()
        .allow_ability("invoices.*")
        .commands(commands![
            invoices_show,
            invoices_destroy,
            invoices_touch,
            ping
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
    let request = wry::http::Request::builder()
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
