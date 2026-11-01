//! Capabilities per window (RFC 0016): a window narrowed to fewer
//! capabilities and abilities than the app grants, known by its own
//! protocol handler (`TestShell::handle_in`), never by what the page sends.

use std::sync::Arc;

use elyra::command::BoxFuture;
use elyra::security::Capability;
use elyra::testing::TestShell;
use elyra::window::WindowConfig;
use elyra::{command, commands, App, CommandRequest, Ctx, Middleware, Next};
use parking_lot::Mutex;
use serde_json::{json, Value};

/// One invoice.
#[command(can = "invoices.view")]
async fn invoices_show(_ctx: Ctx, id: i64) -> i64 {
    id
}

/// Delete an invoice.
#[command(can = "invoices.delete")]
async fn invoices_destroy(_ctx: Ctx, id: i64) -> i64 {
    id
}

/// No ability: the app's blanket `Commands` grant covers it.
#[command]
async fn ping(_ctx: Ctx) -> String {
    "pong".into()
}

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

/// Records each call's command and window.
struct Audit(Seen);

impl Middleware for Audit {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, elyra::Result<Vec<u8>>> {
        self.0.lock().push((req.name.clone(), req.window.clone()));
        next.run(ctx, req)
    }
}

/// The app grants every invoice ability; `main` has no limits, `preview`
/// may only view, `quiet` may not run commands at all.
fn shell_with(seen: &Seen) -> TestShell {
    let prepared = App::new()
        .allow_ability("invoices.*")
        .commands(commands![invoices_show, invoices_destroy, ping])
        .middleware(Audit(seen.clone()))
        .prepare();
    TestShell::new(prepared)
        .window(WindowConfig::new("main"))
        .window(WindowConfig::new("preview").only_abilities(["invoices.view"]))
        .window(WindowConfig::new("quiet").deny(Capability::Commands))
}

fn shell() -> TestShell {
    shell_with(&Seen::default())
}

/// `/__<route>` from window `window` (or the app's grant, with `None`): the
/// answer, or the error's kind and message.
async fn call(
    shell: &TestShell,
    window: Option<&str>,
    client: Option<&str>,
    route: &str,
    body: Vec<u8>,
) -> Result<Vec<u8>, (String, String)> {
    let mut request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__{route}"))
        .header("x-elyra-token", shell.token());
    if let Some(client) = client {
        request = request.header("x-elyra-client-id", client);
    }
    let request = request.body(body).unwrap();
    let response = match window {
        Some(label) => shell.handle_in(label, request).await,
        None => shell.handle(request).await,
    };
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

fn args(v: Value) -> Vec<u8> {
    rmp_serde::to_vec(&v).unwrap()
}

fn kind<T>(result: Result<T, (String, String)>) -> Option<String> {
    result.err().map(|(kind, _)| kind)
}

#[tokio::test]
async fn a_window_refuses_an_ability_the_app_grants() {
    let shell = shell();
    let id = || args(json!([42]));
    // The preview may view…
    let shown = call(&shell, Some("preview"), None, "cmd/invoices_show", id())
        .await
        .unwrap();
    assert_eq!(rmp_serde::from_slice::<i64>(&shown).unwrap(), 42);
    // …but not delete: `denied`, as a user without it would be.
    let refused = call(&shell, Some("preview"), None, "cmd/invoices_destroy", id())
        .await
        .unwrap_err();
    assert_eq!(refused.0, "denied");
    assert!(refused.1.contains("not in this window"), "{}", refused.1);
    // The main window, and the app's grant without a window, may.
    assert!(
        call(&shell, Some("main"), None, "cmd/invoices_destroy", id())
            .await
            .is_ok()
    );
    assert!(call(&shell, None, None, "cmd/invoices_destroy", id())
        .await
        .is_ok());
}

#[tokio::test]
async fn a_command_without_an_ability_is_refused_only_where_commands_are() {
    let shell = shell();
    assert!(
        call(&shell, Some("preview"), None, "cmd/ping", args(json!([])))
            .await
            .is_ok()
    );
    let refused = call(&shell, Some("quiet"), None, "cmd/ping", args(json!([])))
        .await
        .unwrap_err();
    assert_eq!(refused.0, "forbidden");
    assert!(
        refused.1.contains("not granted to this window"),
        "{}",
        refused.1
    );
}

#[tokio::test]
async fn a_client_id_belongs_to_the_window_that_used_it_first() {
    let shell = shell();
    let none = || args(json!([]));
    assert!(
        call(&shell, Some("main"), Some("doc-1"), "cmd/ping", none())
            .await
            .is_ok()
    );
    // The same id again from its window: fine.
    assert!(
        call(&shell, Some("main"), Some("doc-1"), "cmd/ping", none())
            .await
            .is_ok()
    );
    // From another window: refused, whatever the route.
    let refused = call(&shell, Some("preview"), Some("doc-1"), "cmd/ping", none())
        .await
        .unwrap_err();
    assert_eq!(refused.0, "forbidden");
    assert!(refused.1.contains("another window"), "{}", refused.1);
    // A header can't lend the preview the main window's limits either: its
    // own id, and its own limits.
    assert_eq!(
        kind(
            call(
                &shell,
                Some("preview"),
                Some("doc-2"),
                "cmd/invoices_destroy",
                args(json!([1]))
            )
            .await
        ),
        Some("denied".into())
    );
    // The main window closed: its ids are free.
    shell.close("main");
    assert!(
        call(&shell, Some("preview"), Some("doc-1"), "cmd/ping", none())
            .await
            .is_ok()
    );
    // A refused request claims nothing: a wrong token can't take an id.
    let request = wry::http::Request::builder()
        .method("POST")
        .uri("elyra://localhost/__cmd/ping")
        .header("x-elyra-token", "wrong")
        .header("x-elyra-client-id", "doc-3")
        .body(none())
        .unwrap();
    assert_eq!(shell.handle_in("quiet", request).await.status(), 403);
    assert!(
        call(&shell, Some("main"), Some("doc-3"), "cmd/ping", none())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn middleware_sees_the_window_the_call_came_from() {
    let seen = Seen::default();
    let shell = shell_with(&seen);
    call(&shell, Some("preview"), None, "cmd/ping", args(json!([])))
        .await
        .unwrap();
    call(&shell, None, None, "cmd/ping", args(json!([])))
        .await
        .unwrap();
    assert_eq!(
        *seen.lock(),
        [
            ("ping".to_owned(), Some("preview".to_owned())),
            ("ping".to_owned(), None)
        ]
    );
}

#[derive(serde::Deserialize, Debug, PartialEq)]
struct Limits {
    only: Option<Vec<String>>,
    deny: Vec<String>,
}

#[derive(serde::Deserialize, Debug)]
struct AbilitiesAnswer {
    window: Option<Limits>,
}

#[tokio::test]
async fn can_learns_the_windows_limits() {
    let shell = shell();
    let answer = |bytes: Vec<u8>| rmp_serde::from_slice::<AbilitiesAnswer>(&bytes).unwrap();
    let preview = call(&shell, Some("preview"), None, "auth/abilities", Vec::new())
        .await
        .unwrap();
    assert_eq!(
        answer(preview).window,
        Some(Limits {
            only: Some(vec!["invoices.view".into()]),
            deny: vec![],
        })
    );
    // No ability limits: nothing to say.
    for window in [Some("main"), Some("quiet"), None] {
        let bytes = call(&shell, window, None, "auth/abilities", Vec::new())
            .await
            .unwrap();
        assert_eq!(answer(bytes).window, None, "{window:?}");
    }
}

#[tokio::test]
async fn window_control_without_a_native_window_is_unavailable_not_misdirected() {
    // The harness has no event loop: no `Windows`, so the route says so
    // rather than acting on another window.
    let shell = shell();
    let refused = call(&shell, Some("preview"), None, "window/close", Vec::new())
        .await
        .unwrap_err();
    assert!(refused.1.contains("unavailable"), "{}", refused.1);
}

#[cfg(debug_assertions)]
#[tokio::test]
async fn the_inspector_shows_the_windows_limits() {
    #[derive(serde::Deserialize)]
    struct Scope {
        label: Option<String>,
        denied: Vec<String>,
        abilities: Option<Limits>,
    }
    #[derive(serde::Deserialize)]
    struct Row {
        ability: String,
        window: Option<bool>,
    }
    let shell = shell();
    let scope = |bytes: Vec<u8>| rmp_serde::from_slice::<Scope>(&bytes).unwrap();
    let quiet = scope(
        call(&shell, Some("quiet"), None, "inspector/scope", Vec::new())
            .await
            .unwrap(),
    );
    assert_eq!(quiet.label.as_deref(), Some("quiet"));
    assert_eq!(quiet.denied, ["Commands"]);
    assert!(quiet.abilities.is_none());
    let preview = scope(
        call(&shell, Some("preview"), None, "inspector/scope", Vec::new())
            .await
            .unwrap(),
    );
    assert!(preview.denied.is_empty());
    assert_eq!(preview.abilities.unwrap().only.unwrap(), ["invoices.view"]);

    let rows: Vec<Row> = rmp_serde::from_slice(
        &call(
            &shell,
            Some("preview"),
            None,
            "inspector/abilities",
            Vec::new(),
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let row = |name: &str| rows.iter().find(|r| r.ability == name).unwrap().window;
    assert_eq!(row("invoices.view"), Some(true));
    assert_eq!(row("invoices.delete"), Some(false));
}

#[cfg(feature = "database")]
mod live {
    use super::*;

    /// Invoices, live.
    #[command(live, can = "invoices.view")]
    async fn invoices_index(ctx: Ctx) -> Vec<i64> {
        ctx.depends_on("invoices");
        vec![1, 2]
    }

    /// Deleted invoices, live: needs more than viewing.
    #[command(live, can = "invoices.delete")]
    async fn invoices_trashed(_ctx: Ctx) -> Vec<i64> {
        Vec::new()
    }

    #[tokio::test]
    async fn a_live_query_is_held_to_the_window_and_its_runs_carry_it() {
        let seen = Seen::default();
        let prepared = App::new()
            .allow_ability("invoices.*")
            .commands(commands![invoices_index, invoices_trashed])
            .middleware(Audit(seen.clone()))
            .live_batch_window(std::time::Duration::from_millis(5))
            .prepare();
        let ctx = prepared.ctx.clone();
        let shell = TestShell::new(prepared)
            .window(WindowConfig::new("preview").only_abilities(["invoices.view"]));
        assert!(call(
            &shell,
            Some("preview"),
            Some("p1"),
            "live/invoices_index",
            args(json!([]))
        )
        .await
        .is_ok());
        assert_eq!(
            kind(
                call(
                    &shell,
                    Some("preview"),
                    Some("p1"),
                    "live/invoices_trashed",
                    args(json!([]))
                )
                .await
            ),
            Some("denied".into())
        );
        assert_eq!(
            *seen.lock(),
            [("invoices_index".to_owned(), Some("preview".to_owned()))]
        );
        // A re-run carries the window it was opened from.
        ctx.invalidate("invoices");
        let wait = async {
            while seen.lock().len() < 2 {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), wait)
            .await
            .expect("a re-run");
        assert_eq!(
            seen.lock()[1],
            ("invoices_index".to_owned(), Some("preview".to_owned()))
        );
    }
}

#[cfg(feature = "ai")]
mod assistant {
    use super::*;
    use elyra::ai::{Ai, AiFake};
    use elyra::assistant::Assistant;

    fn shell(fake: &AiFake) -> TestShell {
        let prepared = App::new()
            .allow_ability("invoices.*")
            .commands(commands![invoices_show, invoices_destroy])
            .swap(Ai::fake(fake.clone()))
            .assistant(Assistant::new().allow_abilities(["invoices.*"]))
            .prepare();
        // The windows' event queues, as the runtime's first poll opens them.
        for client in ["m", "p", "q"] {
            prepared.bus.register_client(client);
        }
        TestShell::new(prepared)
            .window(WindowConfig::new("main"))
            .window(WindowConfig::new("preview").only_abilities(["invoices.view"]))
            .window(WindowConfig::new("quiet").deny(Capability::Assistant))
    }

    fn names(bytes: Vec<u8>) -> Vec<String> {
        let mut names: Vec<String> = rmp_serde::from_slice(&bytes).unwrap();
        names.sort();
        names
    }

    #[tokio::test]
    async fn the_assistant_offers_only_what_the_window_allows() {
        let shell = shell(&AiFake::new());
        let main = call(
            &shell,
            Some("main"),
            Some("m"),
            "assistant/tools",
            Vec::new(),
        )
        .await
        .unwrap();
        assert_eq!(names(main), ["invoices_destroy", "invoices_show"]);
        let preview = call(
            &shell,
            Some("preview"),
            Some("p"),
            "assistant/tools",
            Vec::new(),
        )
        .await
        .unwrap();
        assert_eq!(names(preview), ["invoices_show"]);
        // No assistant in this window: every route refused, so no panel.
        assert_eq!(
            kind(
                call(
                    &shell,
                    Some("quiet"),
                    Some("q"),
                    "assistant/source",
                    Vec::new()
                )
                .await
            ),
            Some("forbidden".into())
        );
    }

    #[tokio::test]
    async fn a_turn_in_a_narrowed_window_sends_only_its_tools() {
        let fake = AiFake::new().text("Invoice 42.");
        let shell = shell(&fake);
        let turn = rmp_serde::to_vec_named(&"Show invoice 42").unwrap();
        call(&shell, Some("preview"), Some("p"), "assistant/prompt", turn)
            .await
            .unwrap();
        let wait = async {
            while fake.requests().is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), wait)
            .await
            .expect("the turn reached the model");
        assert_eq!(fake.requests()[0].tools, ["invoices_show"]);
    }
}
