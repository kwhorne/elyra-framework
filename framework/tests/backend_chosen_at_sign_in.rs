//! A server chosen at sign-in, a token from elsewhere, switching, and
//! channels named from the user (RFC 0021): `Backend::chosen_at_sign_in()`,
//! `Auth::sign_in_with_token` and `Backend::follow_user`, answered by an
//! `HttpFake` and a `PusherFake`. No network, no real keychain.
#![cfg(feature = "backend")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens, TokenStore, SERVER_KEY};
use elyra::backend::{Backend, BackendError};
use elyra::broadcasting::{BackendEvent, Broadcaster, PusherFake};
use elyra::http::{Http, HttpFake};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

/// The server the user types on the sign-in screen.
const TYPED: &str = "https://acme.felagi.test/";
const SERVER: &str = "https://acme.felagi.test";
const TOKEN_KEY: &str = "backend-token:https://acme.felagi.test";

fn url(path: &str) -> String {
    format!("{SERVER}{path}")
}

/// What Félagi's `/api/v1/me` answers: the user and the token's workspace.
fn me(user: i64, workspace: i64) -> Value {
    json!({
        "user": { "id": user, "name": "Ada" },
        "workspace": { "id": workspace, "name": format!("Workspace {workspace}") },
    })
}

/// Félagi: the user, channel auth, an API route, and revoking a token.
fn felagi() -> HttpFake {
    HttpFake::new()
        .get(url("/api/v1/me"), 200, me(7, 1))
        .get(url("/api/v1/channels"), 200, json!(["general"]))
        .post(
            url("/api/broadcasting/auth"),
            200,
            json!({ "auth": "fake-key:signed-by-laravel" }),
        )
        .delete(url("/api/v1/token"), 204, Value::Null)
}

fn backend() -> Backend {
    Backend::chosen_at_sign_in()
        .user_route("/api/v1/me")
        .revoke_route("/api/v1/token")
        .user_id_path("user.id")
}

#[command]
async fn channels(ctx: Ctx) -> elyra::Result<Vec<String>> {
    Ok(ctx.get::<Backend>().get("/api/v1/channels").json().await?)
}

fn app(backend: Backend, fake: &HttpFake, store: Arc<dyn TokenStore>) -> TestApp {
    TestApp::new(
        App::new()
            .backend(backend)
            .token_store(store)
            .commands(commands![channels])
            .swap(Http::fake(fake.clone())),
    )
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AuthStateJson {
    signed_in: bool,
    user: Option<Value>,
}

#[tokio::test]
async fn before_a_server_is_chosen_a_request_is_unauthenticated() {
    let fake = felagi();
    let app = app(backend(), &fake, Arc::new(MemoryTokens::new()));
    let backend = app.get::<Backend>();
    assert_eq!(backend.base_url(), None);
    assert!(!app.get::<Auth>().is_signed_in());

    match backend.get("/api/v1/channels").send().await {
        Err(BackendError::NoServer) => {}
        other => panic!("{other:?}"),
    }
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated"),
        "the UI shows the sign-in screen, as for a signed-out user"
    );
    assert!(fake.sent().is_empty(), "nothing went anywhere");
}

#[tokio::test]
async fn a_token_from_elsewhere_chooses_the_server_and_outlives_a_restart() {
    let fake = felagi();
    let store = Arc::new(MemoryTokens::new());
    let first = app(backend(), &fake, store.clone());
    first.listen();
    let auth = first.get::<Auth>();

    auth.sign_in_with_token(TYPED, "felagi-A", me(7, 1))
        .await
        .unwrap();
    assert!(auth.is_signed_in());
    assert_eq!(
        first.get::<Backend>().base_url().as_deref(),
        Some(SERVER),
        "the trailing slash trimmed"
    );
    assert_eq!(store.stored(TOKEN_KEY).as_deref(), Some("felagi-A"));
    assert_eq!(store.stored(SERVER_KEY).as_deref(), Some(SERVER));
    assert_eq!(
        store
            .stored("backend-user:https://acme.felagi.test")
            .as_deref(),
        Some("7"),
        "the id at `user.id`"
    );
    // The user as given, without asking the server.
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["workspace"]["id"], 1);
    fake.assert_not_sent("GET", &url("/api/v1/me"));
    let events: Vec<AuthStateJson> = first.events_on("elyra:auth").await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].signed_in);
    assert_eq!(events[0].user.as_ref().unwrap()["user"]["id"], 7);

    // Requests go to the chosen server, with the token.
    first.invoke_ok::<Vec<String>>("channels", ()).await;
    let sent = fake.assert_sent("GET", &url("/api/v1/channels"));
    assert_eq!(sent.header("authorization"), Some("Bearer felagi-A"));
    drop(first);

    // The next start: signed in, to the same server, the user read again.
    let again = app(backend(), &fake, store.clone());
    let auth = again.get::<Auth>();
    assert!(auth.is_signed_in());
    assert_eq!(again.get::<Backend>().base_url().as_deref(), Some(SERVER));
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["user"]["id"], 7);
    let read = fake.assert_sent("GET", &url("/api/v1/me"));
    assert_eq!(read.header("authorization"), Some("Bearer felagi-A"));
    again.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(fake.sent_to("GET", &url("/api/v1/channels")).len(), 2);
}

#[tokio::test]
async fn a_fixed_server_cant_be_pointed_elsewhere() {
    let fake = felagi();
    let store = Arc::new(MemoryTokens::new());
    let app = app(
        Backend::new(SERVER).user_id_path("user.id"),
        &fake,
        store.clone(),
    );
    let auth = app.get::<Auth>();
    match auth
        .sign_in_with_token("https://elsewhere.test", "felagi-A", me(7, 1))
        .await
    {
        Err(BackendError::Other { message, .. }) => {
            assert!(message.contains("https://elsewhere.test"), "{message}")
        }
        other => panic!("{other:?}"),
    }
    assert!(!auth.is_signed_in());
    assert_eq!(store.stored(TOKEN_KEY), None);

    // Its own server, however it's typed, is fine; nothing to remember.
    auth.sign_in_with_token(TYPED, "felagi-A", me(7, 1))
        .await
        .unwrap();
    assert!(auth.is_signed_in());
    assert_eq!(store.stored(TOKEN_KEY).as_deref(), Some("felagi-A"));
    assert_eq!(store.stored(SERVER_KEY), None);

    // Not a server address, or not a token.
    assert!(matches!(
        auth.sign_in_with_token("acme", "felagi-A", me(7, 1)).await,
        Err(BackendError::Http(_))
    ));
    assert!(auth
        .sign_in_with_token(SERVER, "two words", me(7, 1))
        .await
        .is_err());
}

#[tokio::test]
async fn a_switch_says_signed_in_once_with_the_new_user() {
    let fake = felagi();
    let store = Arc::new(MemoryTokens::new());
    let app = app(backend(), &fake, store.clone());
    app.listen();
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
        .await
        .unwrap();
    let _: Vec<AuthStateJson> = app.events_on("elyra:auth").await;

    // Another workspace's token, the same server.
    auth.sign_in_with_token(SERVER, "felagi-B", me(7, 2))
        .await
        .unwrap();
    let events: Vec<AuthStateJson> = app.events_on("elyra:auth").await;
    assert_eq!(
        events.len(),
        1,
        "one event, no sign-out between: {events:?}"
    );
    assert!(events[0].signed_in);
    assert_eq!(events[0].user.as_ref().unwrap()["workspace"]["id"], 2);
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["workspace"]["id"], 2);
    assert_eq!(store.stored(TOKEN_KEY).as_deref(), Some("felagi-B"));

    app.invoke_ok::<Vec<String>>("channels", ()).await;
    let sent = fake.assert_sent("GET", &url("/api/v1/channels"));
    assert_eq!(sent.header("authorization"), Some("Bearer felagi-B"));

    // Another server needs a sign-out first.
    assert!(auth
        .sign_in_with_token("https://other.felagi.test", "felagi-C", me(7, 3))
        .await
        .is_err());
    assert_eq!(app.get::<Backend>().base_url().as_deref(), Some(SERVER));
}

#[tokio::test]
async fn signing_out_forgets_the_server() {
    let fake = felagi();
    let store = Arc::new(MemoryTokens::new());
    let app = app(backend(), &fake, store.clone());
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
        .await
        .unwrap();

    auth.sign_out().await.unwrap();
    let revoke = fake.assert_sent("DELETE", &url("/api/v1/token"));
    assert_eq!(revoke.header("authorization"), Some("Bearer felagi-A"));
    assert!(!auth.is_signed_in());
    assert_eq!(app.get::<Backend>().base_url(), None);
    assert_eq!(store.stored(TOKEN_KEY), None);
    assert_eq!(store.stored(SERVER_KEY), None);
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated")
    );

    // The next start shows the sign-in screen.
    let again = app_again(&fake, store);
    assert!(!again.get::<Auth>().is_signed_in());
    assert_eq!(again.get::<Backend>().base_url(), None);
}

fn app_again(fake: &HttpFake, store: Arc<MemoryTokens>) -> TestApp {
    app(backend(), fake, store)
}

#[tokio::test]
async fn a_401_forgets_the_server_too() {
    let fake = felagi().get(
        url("/api/v1/channels"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    let store = Arc::new(
        MemoryTokens::new()
            .with(SERVER_KEY, SERVER)
            .with(TOKEN_KEY, "felagi-revoked"),
    );
    let app = app(backend(), &fake, store.clone());
    assert!(app.get::<Auth>().is_signed_in());
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated")
    );
    assert_eq!(store.stored(SERVER_KEY), None);
    assert_eq!(app.get::<Backend>().base_url(), None);
}

// -- channels named from the user --------------------------------------------

static HEARD: Mutex<Vec<BackendEvent>> = Mutex::new(Vec::new());

fn listening(fake: &HttpFake, pusher: &PusherFake) -> TestApp {
    TestApp::new(
        App::new()
            .backend(
                backend()
                    .broadcasting(pusher.config())
                    .follow_user("chat", "user.{user.id}", None)
                    .follow_user(
                        "workspace",
                        "workspace.{workspace.id}",
                        Some("/api/v1/issues"),
                    )
                    // Félagi's users have no team: never followed.
                    .follow_user("team", "team.{team.id}", None),
            )
            .token_store(Arc::new(MemoryTokens::new()))
            .listen(|e: BackendEvent, _ctx: Ctx| async move {
                HEARD.lock().unwrap().push(e);
                Ok(())
            })
            .swap(Http::fake(fake.clone())),
    )
}

fn channels_of(pusher: &PusherFake, skip: usize) -> Vec<String> {
    let mut names: Vec<String> = pusher
        .subscriptions()
        .iter()
        .skip(skip)
        .map(|s| s.channel.clone())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn channels_named_from_the_user_follow_a_switch() {
    let fake = felagi();
    let pusher = PusherFake::start().await;
    let app = listening(&fake, &pusher);
    app.listen();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(pusher.connections(), 0, "nobody to listen for yet");

    let auth = app.get::<Auth>();
    auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
        .await
        .unwrap();
    pusher.wait_for_subscriptions(2).await;
    assert_eq!(
        channels_of(&pusher, 0),
        ["private-user.7", "private-workspace.1"],
        "`team.{{team.id}}` doesn't resolve: skipped"
    );
    assert!(pusher
        .subscriptions()
        .iter()
        .all(|s| s.auth.as_deref() == Some("fake-key:signed-by-laravel")));
    let authorized = fake.sent_to("POST", &url("/api/broadcasting/auth"));
    assert_eq!(authorized.len(), 2);
    assert!(authorized
        .iter()
        .all(|a| a.header("authorization") == Some("Bearer felagi-A")));

    // Another workspace: a new connection, with its token, to its channels.
    auth.sign_in_with_token(SERVER, "felagi-B", me(7, 2))
        .await
        .unwrap();
    pusher.wait_for_subscriptions(4).await;
    pusher
        .wait_until(|p| p.connected() == 1, "the old connection to close")
        .await;
    assert_eq!(pusher.connections(), 2);
    assert_eq!(
        channels_of(&pusher, 2),
        ["private-user.7", "private-workspace.2"]
    );
    let authorized = fake.sent_to("POST", &url("/api/broadcasting/auth"));
    assert_eq!(authorized.len(), 4);
    for a in &authorized[2..] {
        assert_eq!(a.header("authorization"), Some("Bearer felagi-B"));
    }
    assert!(!pusher
        .subscriptions()
        .iter()
        .any(|s| s.channel.starts_with("private-team.")));

    // An event on it reaches the windows by the follow's name.
    pusher.broadcast(
        "private-workspace.2",
        "IssueUpdated",
        json!({ "issue": { "id": 40 } }),
    );
    pusher.broadcast(
        "private-user.7",
        "MessagePosted",
        json!({ "message": { "id": 1, "body": "Hei" } }),
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let heard = HEARD.lock().unwrap().len();
        if heard >= 2 || tokio::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let heard = HEARD.lock().unwrap().clone();
    assert!(heard
        .iter()
        .any(|e| e.channel == "workspace" && e.event == "IssueUpdated"));
    assert!(heard
        .iter()
        .any(|e| e.channel == "chat" && e.data["message"]["body"] == "Hei"));
    let events: Vec<Value> = app.events_on("elyra:backend").await;
    assert!(events
        .iter()
        .any(|e| e["channel"] == "workspace" && e["event"] == "IssueUpdated"));
    assert!(events
        .iter()
        .any(|e| e["channel"] == "chat" && e["event"] == "MessagePosted"));
    assert!(app.get::<Broadcaster>().follows().is_empty(), "none fixed");
}

// -- broadcasting settings from the server ------------------------------------

const SETTINGS: &str = "/api/desktop/broadcasting";

fn from_the_server(fake: &HttpFake, store: Arc<MemoryTokens>) -> TestApp {
    TestApp::new(
        App::new()
            .backend(backend().broadcasting_from(SETTINGS).follow_user(
                "chat",
                "user.{user.id}",
                None,
            ))
            .token_store(store)
            .commands(commands![channels])
            .swap(Http::fake(fake.clone())),
    )
}

#[tokio::test]
async fn the_broadcasting_settings_come_from_the_chosen_server() {
    let pusher = PusherFake::start().await;
    let fake = felagi().get(
        url(SETTINGS),
        200,
        json!({ "key": "felagi-key", "host": "127.0.0.1", "port": pusher.port(), "scheme": "ws" }),
    );
    let store = Arc::new(MemoryTokens::new());
    let app = from_the_server(&fake, store.clone());
    tokio::time::sleep(Duration::from_millis(50)).await;
    fake.assert_not_sent("GET", &url(SETTINGS));
    assert_eq!(pusher.connections(), 0);

    app.get::<Auth>()
        .sign_in_with_token(SERVER, "felagi-A", me(7, 1))
        .await
        .unwrap();
    pusher.wait_for_subscriptions(1).await;
    assert_eq!(pusher.keys(), ["felagi-key"], "the server's key");
    assert_eq!(pusher.subscriptions()[0].channel, "private-user.7");
    let read = fake.assert_sent("GET", &url(SETTINGS));
    assert_eq!(read.header("authorization"), Some("Bearer felagi-A"));
    // Authorized at the app's route, as any private channel.
    fake.assert_sent("POST", &url("/api/broadcasting/auth"));
    let kept: Vec<String> = [
        SERVER_KEY,
        TOKEN_KEY,
        "backend-user:https://acme.felagi.test",
    ]
    .iter()
    .filter_map(|k| store.stored(k))
    .collect();
    assert!(
        !kept.iter().any(|v| v.contains("felagi-key")),
        "nothing of it kept: {kept:?}"
    );
    // (The first run's connection stays up: a dropped `TestApp` doesn't
    // stop its broadcaster.)
    // The next start reads them again, and connects.
    let _first = app;
    let _again = from_the_server(&fake, store);
    pusher.wait_for_subscriptions(2).await;
    assert_eq!(pusher.keys(), ["felagi-key", "felagi-key"]);
    assert_eq!(fake.sent_to("GET", &url(SETTINGS)).len(), 2);
}

#[tokio::test]
async fn without_settings_from_the_server_broadcasting_stays_off() {
    let pusher = PusherFake::start().await;
    let fake = felagi().get(url(SETTINGS), 404, json!({ "message": "Not Found" }));
    let app = from_the_server(&fake, Arc::new(MemoryTokens::new()));
    app.listen();
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
        .await
        .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while fake.sent_to("GET", &url(SETTINGS)).is_empty() {
        assert!(tokio::time::Instant::now() < deadline, "never asked");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(pusher.connections(), 0);
    assert_eq!(
        fake.sent_to("GET", &url(SETTINGS)).len(),
        1,
        "a 404 isn't asked again"
    );
    assert_eq!(
        app.get::<Broadcaster>().state(),
        elyra::broadcasting::State::Offline
    );
    // Everything else works.
    assert!(auth.is_signed_in());
    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["general"]
    );
    let events: Vec<AuthStateJson> = app.events_on("elyra:auth").await;
    assert!(events.iter().all(|e| e.signed_in));
}

// -- live queries and the offline copy, across a switch -----------------------

#[cfg(feature = "database")]
mod with_a_database {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use elyra::http::Method;
    use elyra::Database;

    #[command(live)]
    async fn live_channels(ctx: Ctx) -> elyra::Result<Vec<String>> {
        Ok(ctx.get::<Backend>().get("/api/v1/channels").json().await?)
    }

    async fn database() -> Database {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("elyra-chosen-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        Database::connect(&elyra::db::sqlite_url(&path))
            .await
            .unwrap()
    }

    async fn offline_app(fake: &HttpFake) -> TestApp {
        TestApp::new(
            App::new()
                .bind(database().await)
                .backend(backend().offline())
                .token_store(Arc::new(MemoryTokens::new()))
                .live_batch_window(Duration::from_millis(5))
                .commands(commands![channels, live_channels])
                .swap(Http::fake(fake.clone())),
        )
    }

    #[tokio::test]
    async fn a_switch_re_runs_what_read_the_backend() {
        let fake = felagi().get(url("/up"), 200, json!("ok"));
        let app = offline_app(&fake).await;
        app.listen();
        let auth = app.get::<Auth>();
        auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
            .await
            .unwrap();
        let mut live = app.live::<Vec<String>>("live_channels", ()).await;
        assert_eq!(*live.value(), ["general"]);

        // Workspace 2 has other channels.
        fake.clone()
            .get(url("/api/v1/channels"), 200, json!(["random"]));
        auth.sign_in_with_token(SERVER, "felagi-B", me(7, 2))
            .await
            .unwrap();
        live.next().await;
        assert_eq!(*live.value(), ["random"], "re-run as workspace 2");
        let sent = fake.sent_to("GET", &url("/api/v1/channels")).pop().unwrap();
        assert_eq!(sent.header("authorization"), Some("Bearer felagi-B"));
    }

    #[tokio::test]
    async fn a_switch_never_answers_from_the_last_tokens_copy() {
        let fake = felagi().get(url("/up"), 200, json!("ok"));
        let app = offline_app(&fake).await;
        let auth = app.get::<Auth>();
        auth.sign_in_with_token(SERVER, "felagi-A", me(7, 1))
            .await
            .unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["general"]
        );

        // Another user; then the network goes.
        auth.sign_in_with_token(SERVER, "felagi-B", me(8, 1))
            .await
            .unwrap();
        fake.clone()
            .unreachable(Method::Get, url("/*"))
            .unreachable(Method::Post, url("/*"));
        assert_eq!(
            app.invoke_err_kind("channels", ()).await.as_deref(),
            Some("offline"),
            "user 7's copy isn't user 8's"
        );

        // The same user in another workspace: not that copy either.
        fake.clone().get(url("/*"), 200, json!("ok")).get(
            url("/api/v1/channels"),
            200,
            json!(["random"]),
        );
        auth.sign_in_with_token(SERVER, "felagi-C", me(8, 1))
            .await
            .unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["random"]
        );
        auth.sign_in_with_token(SERVER, "felagi-D", me(8, 2))
            .await
            .unwrap();
        fake.clone().unreachable(Method::Get, url("/*"));
        assert_eq!(
            app.invoke_err_kind("channels", ()).await.as_deref(),
            Some("offline"),
            "workspace 1's copy isn't workspace 2's"
        );
    }
}
