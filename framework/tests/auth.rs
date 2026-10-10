//! Signing in (RFC 0005 step 3): a Sanctum token in the keychain — here, in
//! memory — the user, `401`s, signing out, and the `/__auth` route. Answered
//! by an `HttpFake`; no network, no real keychain.
#![cfg(feature = "backend")]

use std::sync::Arc;

use elyra::auth::{Auth, AuthState, MemoryTokens, TokenStore};
use elyra::backend::{Backend, BackendError};
use elyra::http::{Http, HttpFake, Method};
use elyra::testing::{TestApp, TestShell};
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};
use wry::http::Request;

const BASE: &str = "https://crm.test";
const KEY: &str = "backend-token:https://crm.test";

/// Laravel's documented routes, answering as Laravel does.
fn laravel() -> HttpFake {
    HttpFake::new()
        // The token route's plain-text answer.
        .on(
            Method::Post,
            format!("{BASE}/api/sanctum/token"),
            200,
            json!("7|s3cr3t"),
        )
        .get(
            format!("{BASE}/api/user"),
            200,
            json!({ "id": 1, "name": "Ada" }),
        )
        .delete(format!("{BASE}/api/sanctum/token"), 204, Value::Null)
}

fn laravel_app(fake: &HttpFake, store: Arc<dyn TokenStore>) -> App {
    App::new()
        .backend(Backend::new(BASE))
        .token_store(store)
        .commands(commands![customers_count])
        .swap(Http::fake(fake.clone()))
}

#[command]
async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(ctx
        .get::<Backend>()
        .get("/api/customers/count")
        .json()
        .await?)
}

#[tokio::test]
async fn signing_in_keeps_the_token_and_reads_the_user() {
    let fake = laravel();
    let store = Arc::new(MemoryTokens::new());
    let app = TestApp::new(laravel_app(&fake, store.clone()));
    app.listen();
    let auth = app.get::<Auth>();
    assert!(!auth.is_signed_in());

    let user = auth.sign_in("ada@example.com", "secret").await.unwrap();
    assert_eq!(user["name"], "Ada");
    assert!(auth.is_signed_in());
    assert_eq!(
        store.stored(KEY).as_deref(),
        Some("7|s3cr3t"),
        "in the keychain"
    );

    let sent = fake.assert_sent("POST", &format!("{BASE}/api/sanctum/token"));
    let body = sent.body.unwrap();
    assert_eq!(body["email"], "ada@example.com");
    assert_eq!(body["password"], "secret");
    assert_eq!(body["device_name"], auth.device_name());
    let (host, app_name) = auth
        .device_name()
        .split_once(" · ")
        .expect("<host> · <app>");
    assert!(
        !host.is_empty() && !app_name.is_empty(),
        "{}",
        auth.device_name()
    );
    let user_call = fake.assert_sent("GET", &format!("{BASE}/api/user"));
    assert_eq!(user_call.header("authorization"), Some("Bearer 7|s3cr3t"));

    // Cached: a second read doesn't ask again.
    let again: Value = auth.user().await.unwrap();
    assert_eq!(again["id"], 1);
    assert_eq!(fake.sent_to("GET", &format!("{BASE}/api/user")).len(), 1);

    let events: Vec<Value> = app.events_on("elyra:auth").await;
    assert_eq!(events.last().unwrap()["signedIn"], true);
    assert_eq!(events.last().unwrap()["user"]["name"], "Ada");
}

#[tokio::test]
async fn wrong_credentials_are_the_validation_bag_and_store_nothing() {
    let fake = laravel().on(
        Method::Post,
        format!("{BASE}/api/sanctum/token"),
        422,
        json!({ "message": "The provided credentials are incorrect.",
                "errors": { "email": ["The provided credentials are incorrect."] } }),
    );
    let store = Arc::new(MemoryTokens::new());
    let app = TestApp::new(laravel_app(&fake, store.clone()));
    let auth = app.get::<Auth>();
    match auth.sign_in("ada@example.com", "wrong").await {
        Err(BackendError::Validation(bag)) => {
            assert_eq!(
                bag.first("email"),
                Some("The provided credentials are incorrect.")
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(!auth.is_signed_in());
    assert_eq!(store.stored(KEY), None);
}

#[tokio::test]
async fn no_keychain_means_no_sign_in_and_the_token_is_revoked() {
    let fake = laravel();
    let app = TestApp::new(laravel_app(&fake, Arc::new(MemoryTokens::failing())));
    let auth = app.get::<Auth>();
    match auth.sign_in("ada@example.com", "secret").await {
        Err(BackendError::Keychain(why)) => assert!(why.contains("no keychain"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert!(!auth.is_signed_in(), "no plain-text fallback");
    let revoke = fake.assert_sent("DELETE", &format!("{BASE}/api/sanctum/token"));
    assert_eq!(
        revoke.header("authorization"),
        Some("Bearer 7|s3cr3t"),
        "the token the server just issued isn't left behind"
    );
}

#[tokio::test]
async fn a_stored_token_signs_the_user_back_in() {
    let fake = laravel().get(format!("{BASE}/api/customers/count"), 200, json!(42));
    let store = Arc::new(MemoryTokens::new().with(KEY, "7|earlier"));
    let app = TestApp::new(laravel_app(&fake, store));
    assert!(app.get::<Auth>().is_signed_in());
    assert_eq!(app.invoke_ok::<i64>("customers_count", ()).await, 42);
    let sent = fake.assert_sent("GET", &format!("{BASE}/api/customers/count"));
    assert_eq!(sent.header("authorization"), Some("Bearer 7|earlier"));
}

#[tokio::test]
async fn a_401_anywhere_signs_out_and_says_so() {
    let fake = laravel().get(
        format!("{BASE}/api/customers/count"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    let store = Arc::new(MemoryTokens::new().with(KEY, "7|revoked-on-the-web"));
    let app = TestApp::new(laravel_app(&fake, store.clone()));
    app.listen();
    let err = app.invoke_err("customers_count", ()).await;
    assert!(err.contains("sign in again"), "{err}");
    assert!(!app.get::<Auth>().is_signed_in());
    assert_eq!(store.stored(KEY), None, "forgotten in the keychain too");
    let events: Vec<AuthStateJson> = app.events_on("elyra:auth").await;
    let last = events.last().expect("an elyra:auth event");
    assert!(!last.signed_in);
    assert_eq!(last.reason.as_deref(), Some("expired"));
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthStateJson {
    signed_in: bool,
    reason: Option<String>,
}

#[tokio::test]
async fn signing_out_revokes_and_forgets_even_offline() {
    let fake = laravel();
    let store = Arc::new(MemoryTokens::new().with(KEY, "7|s3cr3t"));
    let app = TestApp::new(laravel_app(&fake, store.clone()));
    let auth = app.get::<Auth>();
    auth.sign_out().await.unwrap();
    let revoke = fake.assert_sent("DELETE", &format!("{BASE}/api/sanctum/token"));
    assert_eq!(revoke.header("authorization"), Some("Bearer 7|s3cr3t"));
    assert!(!auth.is_signed_in());
    assert_eq!(store.stored(KEY), None);

    // Offline: the server can't be told, but the token is forgotten anyway.
    let offline = HttpFake::new().unreachable(Method::Delete, format!("{BASE}/api/sanctum/token"));
    let store = Arc::new(MemoryTokens::new().with(KEY, "7|s3cr3t"));
    let offline_app = TestApp::new(laravel_app(&offline, store.clone()));
    offline_app.get::<Auth>().sign_out().await.unwrap();
    assert_eq!(store.stored(KEY), None);
}

/// The frontend's route: through the real protocol handler and its guard.
async fn auth_route(shell: &TestShell, op: &str, body: Vec<u8>) -> (Option<String>, Vec<u8>) {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__auth/{op}"))
        .header("x-elyra-token", shell.token())
        .body(body)
        .unwrap();
    let response = shell.handle(request).await;
    let kind = response
        .headers()
        .get("x-elyra-error-kind")
        .map(|v| v.to_str().unwrap().to_owned());
    (kind, response.body().to_vec())
}

#[tokio::test]
async fn the_frontend_signs_in_through_its_route_and_never_sees_the_token() {
    let fake = laravel();
    let shell = TestShell::new(laravel_app(&fake, Arc::new(MemoryTokens::new())).prepare());

    let body =
        rmp_serde::to_vec_named(&json!({ "email": "ada@example.com", "password": "secret" }))
            .unwrap();
    let (kind, bytes) = auth_route(&shell, "sign-in", body).await;
    assert_eq!(kind, None);
    let state: Value = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(state["signedIn"], true);
    assert_eq!(state["user"]["name"], "Ada");
    assert!(
        !String::from_utf8_lossy(&bytes).contains("s3cr3t"),
        "no token in the answer"
    );

    let (_, bytes) = auth_route(&shell, "state", Vec::new()).await;
    assert_eq!(
        rmp_serde::from_slice::<Value>(&bytes).unwrap()["signedIn"],
        true
    );

    let (_, bytes) = auth_route(&shell, "sign-out", Vec::new()).await;
    let state: Value = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(state["signedIn"], false);
    assert_eq!(state["reason"], "signed-out");
}

#[tokio::test]
async fn a_422_on_the_route_is_a_validation_error() {
    let fake = laravel().on(
        Method::Post,
        format!("{BASE}/api/sanctum/token"),
        422,
        json!({ "errors": { "password": ["The password field is required."] } }),
    );
    let shell = TestShell::new(laravel_app(&fake, Arc::new(MemoryTokens::new())).prepare());
    let body =
        rmp_serde::to_vec_named(&json!({ "email": "ada@example.com", "password": "" })).unwrap();
    let (kind, bytes) = auth_route(&shell, "sign-in", body).await;
    assert_eq!(kind.as_deref(), Some("validation"));
    let bag: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(bag["password"][0], "The password field is required.");
}

#[tokio::test]
async fn the_route_can_be_revoked_from_the_frontend() {
    let fake = laravel();
    let shell = TestShell::new(
        laravel_app(&fake, Arc::new(MemoryTokens::new()))
            .deny_frontend(elyra::security::Capability::Auth)
            .prepare(),
    );
    let (kind, _) = auth_route(&shell, "state", Vec::new()).await;
    assert_eq!(kind.as_deref(), Some("forbidden"));
    let _: Option<AuthState> = None;
}

#[tokio::test]
async fn a_401_before_signing_in_isnt_a_sign_out() {
    let fake = laravel().get(format!("{BASE}/api/customers/count"), 401, json!({}));
    let app = TestApp::new(laravel_app(&fake, Arc::new(MemoryTokens::new())));
    app.listen();
    let _ = app.invoke_err("customers_count", ()).await;
    let events: Vec<Value> = app.events_on("elyra:auth").await;
    assert!(events.is_empty(), "nothing expired: {events:?}");
}
