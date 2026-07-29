//! The IPC surface: token gating, capabilities, body limits, error kinds, asset
//! caching/ranges, and CORS — exercised through the real `route()` pipeline.

use std::borrow::Cow;

use elyra::security::Capability;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Asset, Ctx};
use wry::http::{Request, Response, StatusCode};

#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

#[command]
async fn explode(_ctx: Ctx) -> Result<i64, String> {
    Err("kaboom".into())
}

#[command]
async fn invalid(_ctx: Ctx) -> Result<i64, elyra::ValidationErrors> {
    let mut errors = elyra::ValidationErrors::new();
    errors.add("email", "The email must be a valid email address.");
    Err(errors)
}

#[command]
async fn panics(_ctx: Ctx) -> i64 {
    panic!("intentional test panic")
}

/// A tiny asset set: one HTML page and one fingerprinted JS bundle.
fn assets() -> elyra::AssetResolver {
    std::sync::Arc::new(|path: &str| match path {
        "index.html" => Some(
            Asset::new(
                b"<!doctype html><title>t</title>".to_vec(),
                "text/html; charset=utf-8",
            )
            .with_etag("aaaa1111"),
        ),
        "assets/app-B1x9Kd2f.js" => Some(
            Asset::new(
                b"console.log('hello world, from a bundle')".to_vec(),
                "text/javascript",
            )
            .with_etag("bbbb2222"),
        ),
        _ => None,
    })
}

fn shell(app: App) -> TestShell {
    TestShell::new(app.assets(assets()).prepare())
}

fn default_shell() -> TestShell {
    shell(App::new().commands(commands![add, explode, invalid, panics]))
}

/// A POST to an IPC route with the right token and a msgpack body.
fn ipc(shell: &TestShell, path: &str, body: Vec<u8>) -> Request<Vec<u8>> {
    Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", "test-window")
        .body(body)
        .unwrap()
}

fn get(uri: &str) -> Request<Vec<u8>> {
    Request::builder()
        .method("GET")
        .uri(format!("elyra://localhost{uri}"))
        .body(Vec::new())
        .unwrap()
}

fn kind(res: &Response<Cow<'static, [u8]>>) -> String {
    res.headers()
        .get("x-elyra-error-kind")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

fn text(res: &Response<Cow<'static, [u8]>>) -> String {
    String::from_utf8_lossy(res.body()).to_string()
}

// --- token gating -----------------------------------------------------------

#[tokio::test]
async fn ipc_requires_the_token() {
    let shell = default_shell();
    let no_token = Request::builder()
        .method("POST")
        .uri("elyra://localhost/__cmd/add")
        .body(rmp_serde::to_vec(&(1, 2)).unwrap())
        .unwrap();

    let res = shell.handle(no_token).await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(kind(&res), "forbidden");
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let shell = default_shell();
    let req = Request::builder()
        .method("POST")
        .uri("elyra://localhost/__cmd/add")
        .header("x-elyra-token", "not-the-token")
        .body(rmp_serde::to_vec(&(1, 2)).unwrap())
        .unwrap();
    assert_eq!(shell.handle(req).await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_right_token_dispatches_the_command() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(
            &shell,
            "/__cmd/add",
            rmp_serde::to_vec(&(2, 40)).unwrap(),
        ))
        .await;

    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()
            .get("x-elyra-status")
            .unwrap()
            .to_str()
            .unwrap(),
        "ok"
    );
    let sum: i64 = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(sum, 42);
}

#[tokio::test]
async fn assets_do_not_require_a_token() {
    // The page itself must load before any script can present a token.
    let res = default_shell().handle(get("/index.html")).await;
    assert_eq!(res.status(), StatusCode::OK);
}

// --- command errors ---------------------------------------------------------

#[tokio::test]
async fn a_failing_command_reports_its_own_message() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(
            &shell,
            "/__cmd/explode",
            rmp_serde::to_vec(&()).unwrap(),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(kind(&res), "command");
    // Verbatim: no "command failed: " prefix wrapping the message.
    assert_eq!(text(&res), "kaboom");
}

#[tokio::test]
async fn a_validation_bag_is_flagged_as_such() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(
            &shell,
            "/__cmd/invalid",
            rmp_serde::to_vec(&()).unwrap(),
        ))
        .await;
    assert_eq!(kind(&res), "validation");
    let bag: std::collections::BTreeMap<String, Vec<String>> =
        serde_json::from_str(&text(&res)).expect("the body must be a parseable bag");
    assert!(bag.contains_key("email"));
}

#[tokio::test]
async fn a_panicking_command_answers_with_an_error_not_a_hang() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(
            &shell,
            "/__cmd/panics",
            rmp_serde::to_vec(&()).unwrap(),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(kind(&res), "panic");
    assert!(text(&res).contains("intentional test panic"));
}

#[tokio::test]
async fn an_unknown_command_is_an_error() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(&shell, "/__cmd/nope", rmp_serde::to_vec(&()).unwrap()))
        .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(text(&res).contains("nope"));
}

// --- body limits ------------------------------------------------------------

#[tokio::test]
async fn an_oversized_body_is_refused_before_decoding() {
    let shell = shell(App::new().commands(commands![add]).max_request_body(1024));
    let res = shell
        .handle(ipc(&shell, "/__cmd/add", vec![0u8; 4096]))
        .await;
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(text(&res).contains("too large"));
}

#[tokio::test]
async fn a_deeply_nested_body_is_refused() {
    let shell = default_shell();
    // 5_000 nested arrays: small, valid framing, but enough to overflow the stack
    // in serde's recursive deserializer.
    let mut body = vec![0x91u8; 5_000];
    body.push(0xc0);
    let res = shell.handle(ipc(&shell, "/__cmd/add", body)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert!(text(&res).contains("nests deeper"));
}

// --- capabilities -----------------------------------------------------------

#[tokio::test]
async fn destructive_routes_are_denied_by_default() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(&shell, "/__store/clear", Vec::new()))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(kind(&res), "forbidden");
    assert!(text(&res).contains("StoreClear"));
}

#[tokio::test]
async fn granting_the_capability_opens_the_route() {
    let shell = shell(App::new().allow_frontend(Capability::StoreClear));
    let res = shell
        .handle(ipc(&shell, "/__store/clear", Vec::new()))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn revoking_a_capability_closes_a_default_route() {
    let shell = shell(App::new().deny_frontend(Capability::Store));
    let res = shell
        .handle(ipc(
            &shell,
            "/__store/get",
            rmp_serde::to_vec("theme").unwrap(),
        ))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn expensive_routes_are_rate_limited() {
    let shell = shell(App::new().allow_frontend(Capability::UpdaterInstall));
    let first = shell
        .handle(ipc(&shell, "/__update/install", Vec::new()))
        .await;
    assert_ne!(first.status(), StatusCode::TOO_MANY_REQUESTS);

    let second = shell
        .handle(ipc(&shell, "/__update/install", Vec::new()))
        .await;
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(kind(&second), "forbidden");
}

// --- CORS / CSP -------------------------------------------------------------

#[tokio::test]
async fn production_responses_have_no_cors_headers() {
    let shell = default_shell();
    let res = shell
        .handle(ipc(
            &shell,
            "/__cmd/add",
            rmp_serde::to_vec(&(1, 1)).unwrap(),
        ))
        .await;
    assert!(res.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn html_is_served_with_a_content_security_policy() {
    let res = default_shell().handle(get("/index.html")).await;
    let csp = res
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(csp.contains("default-src 'self' elyra:"), "csp = {csp}");
    assert!(csp.contains("object-src 'none'"));
}

#[tokio::test]
async fn csp_can_be_overridden_or_disabled() {
    let custom = shell(App::new().csp("default-src 'none'"));
    let res = custom.handle(get("/index.html")).await;
    assert_eq!(
        res.headers()
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap(),
        "default-src 'none'"
    );

    let off = shell(App::new().csp_disabled());
    let res = off.handle(get("/index.html")).await;
    assert!(res.headers().get("content-security-policy").is_none());
}

// --- assets: validators, caching, ranges ------------------------------------

#[tokio::test]
async fn assets_carry_an_etag_and_cache_headers() {
    let shell = default_shell();
    let res = shell.handle(get("/assets/app-B1x9Kd2f.js")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("etag").unwrap().to_str().unwrap(),
        "\"bbbb2222\""
    );
    // A fingerprinted filename can be cached forever.
    assert!(res
        .headers()
        .get("cache-control")
        .unwrap()
        .to_str()
        .unwrap()
        .contains("immutable"));
    assert_eq!(
        res.headers()
            .get("accept-ranges")
            .unwrap()
            .to_str()
            .unwrap(),
        "bytes"
    );
}

#[tokio::test]
async fn html_must_always_be_revalidated() {
    let res = default_shell().handle(get("/index.html")).await;
    assert_eq!(
        res.headers()
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap(),
        "no-cache"
    );
}

#[tokio::test]
async fn a_matching_validator_yields_304() {
    let shell = default_shell();
    let req = Request::builder()
        .method("GET")
        .uri("elyra://localhost/assets/app-B1x9Kd2f.js")
        .header("if-none-match", "\"bbbb2222\"")
        .body(Vec::new())
        .unwrap();
    let res = shell.handle(req).await;
    assert_eq!(res.status(), StatusCode::NOT_MODIFIED);
    assert!(res.body().is_empty());

    // A stale validator still gets the payload.
    let stale = Request::builder()
        .method("GET")
        .uri("elyra://localhost/assets/app-B1x9Kd2f.js")
        .header("if-none-match", "\"outdated\"")
        .body(Vec::new())
        .unwrap();
    assert_eq!(shell.handle(stale).await.status(), StatusCode::OK);
}

#[tokio::test]
async fn range_requests_return_partial_content() {
    let shell = default_shell();
    let req = Request::builder()
        .method("GET")
        .uri("elyra://localhost/assets/app-B1x9Kd2f.js")
        .header("range", "bytes=0-4")
        .body(Vec::new())
        .unwrap();
    let res = shell.handle(req).await;
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(res.body().as_ref(), b"conso");
    let cr = res
        .headers()
        .get("content-range")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cr.starts_with("bytes 0-4/"), "content-range = {cr}");
}

#[tokio::test]
async fn an_unsatisfiable_range_is_reported() {
    let shell = default_shell();
    let req = Request::builder()
        .method("GET")
        .uri("elyra://localhost/assets/app-B1x9Kd2f.js")
        .header("range", "bytes=9000-9100")
        .body(Vec::new())
        .unwrap();
    let res = shell.handle(req).await;
    assert_eq!(res.status(), StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn a_missing_asset_is_404() {
    let res = default_shell().handle(get("/nope.png")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_root_path_serves_index_html() {
    let res = default_shell().handle(get("/")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(text(&res).contains("<!doctype html>"));
}

// --- always-available routes ------------------------------------------------

#[tokio::test]
async fn about_and_cancel_need_no_capability() {
    let shell = default_shell();
    assert_eq!(
        shell
            .handle(ipc(&shell, "/__about", Vec::new()))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        shell
            .handle(ipc(
                &shell,
                "/__cancel",
                rmp_serde::to_vec("some-request-id").unwrap()
            ))
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_preflight_is_answered_without_a_token() {
    let shell = default_shell();
    let req = Request::builder()
        .method("OPTIONS")
        .uri("elyra://localhost/__cmd/add")
        .body(Vec::new())
        .unwrap();
    assert_eq!(shell.handle(req).await.status(), StatusCode::NO_CONTENT);
}
