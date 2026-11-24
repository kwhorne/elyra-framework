//! The diagnostics the user can send (RFC 0033): opt-in, behind their
//! capability; the page's errors kept and logged in every app; and the file
//! holding what went wrong but never the token, the password or a signature.

use std::borrow::Cow;

use elyra::security::Capability;
use elyra::testing::http::{Request, Response, StatusCode};
use elyra::testing::TestShell;
use elyra::App;

fn ipc(shell: &TestShell, path: &str, body: Vec<u8>) -> Request<Vec<u8>> {
    Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "test-window")
        .body(body)
        .unwrap()
}

fn kind(res: &Response<Cow<'static, [u8]>>) -> String {
    res.headers()
        .get("x-elyra-error-kind")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn shell(app: App) -> TestShell {
    TestShell::new(
        app.about(elyra::AboutInfo::new("Acme Notes", "2.3.1"))
            .prepare(),
    )
}

#[derive(serde::Serialize)]
struct PageError<'a> {
    message: &'a str,
    at: Option<&'a str>,
    kind: Option<&'a str>,
}

fn page_error(message: &str, at: Option<&str>, kind: Option<&str>) -> Vec<u8> {
    rmp_serde::to_vec_named(&PageError { message, at, kind }).unwrap()
}

#[tokio::test]
async fn without_app_diagnostics_the_routes_are_refused() {
    assert!(!Capability::defaults().contains(&Capability::Diagnostics));
    let shell = shell(App::new());
    for op in ["text", "save"] {
        let res = shell
            .handle(ipc(&shell, &format!("/__diagnostics/{op}"), Vec::new()))
            .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{op}");
        assert_eq!(kind(&res), "forbidden", "{op}");
    }
}

#[tokio::test]
async fn the_pages_errors_are_logged_in_every_app() {
    // Kept, and logged, without App::diagnostics(): no capability needed.
    let plain = shell(App::new());
    let res = plain
        .handle(ipc(
            &plain,
            "/__page-error",
            page_error("TypeError: x is undefined", Some("Card.svelte:41"), None),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(elyra::log::recent(elyra::log::KEPT).iter().any(|r| {
        r.target == "page"
            && r.message
                == "an uncaught error in `main`: TypeError: x is undefined (at Card.svelte:41)"
    }));

    let res = plain
        .handle(ipc(&plain, "/__page-error", b"not msgpack".to_vec()))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[cfg(not(feature = "system"))]
#[test]
fn without_the_system_feature_it_is_a_problem() {
    let problems = App::new().diagnostics().problems();
    assert!(
        problems.iter().any(|p| p
            .message()
            .starts_with("App::diagnostics() needs elyra's `system` feature")),
        "{problems:?}"
    );
}

/// What needs `App::diagnostics()` to start: the `system` feature.
#[cfg(feature = "system")]
mod with_app_diagnostics {
    use super::*;

    async fn text_of(shell: &TestShell) -> String {
        let res = shell
            .handle(ipc(shell, "/__diagnostics/text", Vec::new()))
            .await;
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(res.body())
        );
        rmp_serde::from_slice(res.body()).unwrap()
    }

    #[tokio::test]
    async fn a_window_can_be_denied_them() {
        let shell = TestShell::new(App::new().diagnostics().prepare())
            .window(elyra::WindowConfig::new("popover").deny(Capability::Diagnostics));
        let res = shell
            .handle_in("popover", ipc(&shell, "/__diagnostics/text", Vec::new()))
            .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn the_text_has_every_section_and_the_apps_own() {
        let shell = shell(App::new().diagnostics_with(|d| {
            d.section("Sync", "last full sync: 14:02");
        }));
        let text = text_of(&shell).await;
        assert!(
            text.starts_with("Elyra diagnostics — Acme Notes 2.3.1\n"),
            "{text}"
        );
        for heading in [
            "## App",
            "## System",
            "## Connection",
            "## Outbox",
            "## Updates",
            "## Windows",
            "## The page",
            "## Log",
            "## From the app",
            "## Sync",
        ] {
            assert!(text.contains(heading), "{heading}: {text}");
        }
        assert!(text.contains("identifier: acme-notes"));
        // The app's own memory, on the three systems Elyra runs on.
        assert!(
            text.contains(" MB (the app's own; the webview's processes aren't counted)"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn the_pages_errors_are_in_the_file() {
        let shell = shell(App::new().diagnostics());
        shell
            .handle(ipc(
                &shell,
                "/__page-error",
                page_error(
                    "an answer from /api/customers/7 was not JSON",
                    None,
                    Some("rejection"),
                ),
            ))
            .await;
        let text = text_of(&shell).await;
        assert!(
            text.contains("Rejection [main] an answer from /api/customers/7 was not JSON"),
            "{text}"
        );
    }

    #[cfg(feature = "tray")]
    #[test]
    fn the_tray_item_needs_app_diagnostics() {
        let problems = App::new()
            .tray(elyra::tray::TrayConfig::new().diagnostics())
            .problems();
        assert!(
            problems.iter().any(|p| p
                .message()
                .starts_with("TrayConfig::diagnostics() needs App::diagnostics()")),
            "{problems:?}"
        );
        assert!(App::new()
            .diagnostics()
            .tray(elyra::tray::TrayConfig::new().diagnostics())
            .problems()
            .is_empty());
    }
}
/// Signed in, a write refused with a 422, a signed URL fetched: the file
/// says so, with the server's message, and holds no token, password or
/// signature.
#[cfg(all(feature = "backend", feature = "database", feature = "system"))]
mod against_the_backend {
    use std::sync::Arc;

    use elyra::auth::{Auth, MemoryTokens};
    use elyra::backend::Backend;
    use elyra::http::{Http, HttpFake, Method};
    use elyra::offline::Outbox;
    use elyra::testing::TestApp;
    use elyra::{command, commands, App, Ctx, Database};
    use serde_json::{json, Value};

    const BASE: &str = "https://crm.test";
    const TOKEN: &str = "7|s3cr3tSanctumToken";
    const PASSWORD: &str = "hunter2-correct-horse";

    fn url(path: &str) -> String {
        format!("{BASE}{path}")
    }

    #[command]
    async fn invoices_update(ctx: Ctx, id: i64) -> elyra::Result<Value> {
        Ok(ctx
            .get::<Backend>()
            .patch(format!("/api/invoices/{id}"))
            .body(&json!({ "due": "2020-01-01", "customer_email": "kari@example.no" }))
            .queue_offline()
            .json()
            .await?)
    }

    #[tokio::test]
    async fn the_file_has_the_422_and_no_secret() {
        let path =
            std::env::temp_dir().join(format!("elyra-diagnostics-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Database::connect(&elyra::db::sqlite_url(&path))
            .await
            .unwrap();
        let fake = HttpFake::new()
            .post(url("/api/sanctum/token"), 200, json!(TOKEN))
            .get(
                url("/api/user"),
                200,
                json!({ "id": 1, "name": "Ada", "email": "ada@example.com" }),
            )
            .get(url("/up"), 200, json!("ok"));
        let app = TestApp::new(
            App::new()
                .about(elyra::AboutInfo::new("Acme Notes", "2.3.1"))
                .diagnostics()
                .bind(db)
                .backend(Backend::new(BASE).offline())
                .token_store(Arc::new(MemoryTokens::new()))
                .commands(commands![invoices_update])
                .swap(Http::fake(fake.clone())),
        );
        app.get::<Auth>()
            .sign_in("ada@example.com", PASSWORD)
            .await
            .unwrap();

        // No answer: the write waits; then the server refuses it.
        fake.clone().unreachable(Method::Patch, url("/*"));
        app.invoke_err_kind("invoices_update", (881,)).await;
        fake.clone().patch(
            url("/api/invoices/881"),
            422,
            json!({ "message": "The due date must be after the issue date.",
                    "errors": { "due": ["The due date must be after the issue date."] } }),
        );
        let outbox = app.get::<Outbox>();
        outbox.flush().await;
        assert_eq!(outbox.failed().await.len(), 1);

        // What an app, or a library under it, might log.
        elyra::warn!(
            target: "acme::files",
            "GET https://files.crm.test/a.pdf?expires=1&signature=deadbeefcafe: 403, sent with Authorization: Bearer {TOKEN}"
        );
        elyra::info!(target: "acme::login", "retrying sign-in with password={PASSWORD}");

        let text = elyra::diagnostics::text(app.ctx())
            .await
            .expect("diagnostics are on");
        assert!(
            text.contains(&format!("server: {BASE} · signed in: yes")),
            "{text}"
        );
        assert!(text.contains("online: yes"), "{text}");
        // When it last heard from the server, and last sent from the outbox:
        // the refused write left it too.
        assert!(
            text.contains(" ago\n") && text.contains("\nlast answer: "),
            "{text}"
        );
        assert!(!text.contains("last answer: none this run"), "{text}");
        assert!(text.contains("\nlast sent: "), "{text}");
        assert!(
            text.contains(
                "refused: PATCH /api/invoices/881 — validation: \"due: The due date must be after the issue date.\""
            ),
            "{text}"
        );
        assert!(
            text.contains("signed in to https://crm.test"),
            "the framework's log: {text}"
        );
        assert!(
            text.contains("GET https://files.crm.test/a.pdf?••• 403"),
            "{text}"
        );

        for secret in [
            "s3cr3tSanctumToken",
            PASSWORD,
            "deadbeefcafe",
            "kari@example.no",
            "ada@example.com",
            "Ada",
        ] {
            assert!(!text.contains(secret), "{secret} is in the file:\n{text}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn off_without_app_diagnostics() {
        let app = TestApp::new(App::new());
        assert!(elyra::diagnostics::text(app.ctx()).await.is_none());
    }
}
