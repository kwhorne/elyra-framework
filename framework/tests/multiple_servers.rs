//! Signed in to more than one server (RFC 0022): `Backend::multiple_servers()`
//! keeps a token, a user, an offline copy and an outbox per server, with one
//! server active. An `HttpFake` answers both servers, `Secrets::fake()` keeps
//! the tokens, and a `PusherFake` per server stands in for each one's Reverb.
#![cfg(feature = "backend")]

use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{
    AccountsState, Auth, MemoryTokens, TokenStore, ACCOUNTS_CHANNEL, SERVERS_KEY, SERVER_KEY,
};
use elyra::backend::{Backend, BackendError};
use elyra::broadcasting::{Broadcaster, PusherFake, State};
use elyra::http::{Http, HttpFake};
use elyra::secrets::Secrets;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

/// The company's own server, and a customer's.
const ACME: &str = "https://acme.felagi.test";
const BETA: &str = "https://beta.felagi.test";

fn at(server: &str, path: &str) -> String {
    format!("{server}{path}")
}

fn token_key(server: &str) -> String {
    format!("backend-token:{server}")
}

/// What Félagi's `/api/v1/me` answers.
fn me(user: i64, workspace: i64) -> Value {
    json!({
        "user": { "id": user, "name": format!("User {user}") },
        "workspace": { "id": workspace },
    })
}

/// Both servers: each its own channels, user and revoke route.
fn felagi() -> HttpFake {
    let mut fake = HttpFake::new();
    for (server, user, channel) in [(ACME, 7, "acme-general"), (BETA, 9, "beta-general")] {
        fake = fake
            .get(at(server, "/api/v1/me"), 200, me(user, 1))
            .get(at(server, "/api/v1/channels"), 200, json!([channel]))
            .delete(at(server, "/api/v1/token"), 204, Value::Null)
            .get(at(server, "/up"), 200, json!("ok"))
            .post(
                at(server, "/api/broadcasting/auth"),
                200,
                json!({
                    "auth": "fake-key:signed-by-laravel",
                    "channel_data": json!({ "user_id": user, "user_info": {} }).to_string(),
                }),
            );
    }
    fake
}

fn backend() -> Backend {
    Backend::chosen_at_sign_in()
        .multiple_servers()
        .user_route("/api/v1/me")
        .revoke_route("/api/v1/token")
        .user_id_path("user.id")
}

#[command]
async fn channels(ctx: Ctx) -> elyra::Result<Vec<String>> {
    Ok(ctx.get::<Backend>().get("/api/v1/channels").json().await?)
}

fn app_with(backend: Backend, fake: &HttpFake, store: Arc<dyn TokenStore>) -> TestApp {
    TestApp::new(
        App::new()
            .backend(backend)
            .token_store(store)
            .commands(commands![channels])
            .swap(Http::fake(fake.clone())),
    )
}

fn app(fake: &HttpFake, secrets: &Secrets) -> TestApp {
    app_with(backend(), fake, Arc::new(secrets.clone()))
}

fn stored(secrets: &Secrets, key: &str) -> Option<String> {
    secrets.get(key).unwrap().map(|s| s.expose().to_owned())
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AuthStateJson {
    signed_in: bool,
    user: Option<Value>,
    reason: Option<String>,
    server: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountJson {
    server: String,
    user_id: Option<String>,
    active: bool,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountsJson {
    accounts: Vec<AccountJson>,
    active: Option<String>,
    signed_out_of: Option<String>,
    reason: Option<String>,
}

/// What reached the windows since the last look: `elyra:auth` and
/// `elyra:accounts`, each in order.
async fn heard(app: &TestApp) -> (Vec<AuthStateJson>, Vec<AccountsJson>) {
    fn on<T: serde::de::DeserializeOwned>(all: &[(String, rmpv::Value)], channel: &str) -> Vec<T> {
        all.iter()
            .filter(|(name, _)| name == channel)
            .map(|(_, value)| {
                let mut buf = Vec::new();
                rmpv::encode::write_value(&mut buf, value).unwrap();
                rmp_serde::from_slice(&buf).unwrap()
            })
            .collect()
    }
    let mut all = Vec::new();
    loop {
        let batch = app.drain_events().await;
        if batch.is_empty() {
            break;
        }
        all.extend(batch);
    }
    (on(&all, "elyra:auth"), on(&all, ACCOUNTS_CHANNEL))
}

async fn signed_in_to_both(fake: &HttpFake, secrets: &Secrets) -> TestApp {
    let app = app(fake, secrets);
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
        .await
        .unwrap();
    auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
        .await
        .unwrap();
    app
}

#[tokio::test]
async fn two_servers_are_signed_in_each_with_its_own_token_and_user() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = app(&fake, &secrets);
    app.listen();
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
        .await
        .unwrap();
    heard(&app).await;

    // A second server: added, and active, without a sign-out between.
    auth.sign_in_with_token(&format!("{BETA}/"), "beta-token", me(9, 1))
        .await
        .unwrap();
    let (events, lists) = heard(&app).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].signed_in);
    assert_eq!(events[0].server.as_deref(), Some(BETA));
    assert_eq!(events[0].user.as_ref().unwrap()["user"]["id"], 9);

    let accounts = auth.accounts();
    assert_eq!(
        accounts
            .iter()
            .map(|a| (a.server.as_str(), a.user_id.as_deref(), a.active))
            .collect::<Vec<_>>(),
        [(ACME, Some("7"), false), (BETA, Some("9"), true)]
    );
    assert_eq!(accounts[0].user.as_ref().unwrap()["user"]["id"], 7);
    assert_eq!(auth.active_server().as_deref(), Some(BETA));

    // Each token in the keychain, under its server.
    assert_eq!(
        stored(&secrets, &token_key(ACME)).as_deref(),
        Some("acme-token")
    );
    assert_eq!(
        stored(&secrets, &token_key(BETA)).as_deref(),
        Some("beta-token")
    );
    assert_eq!(stored(&secrets, SERVER_KEY).as_deref(), Some(BETA));
    let list: Vec<String> = serde_json::from_str(&stored(&secrets, SERVERS_KEY).unwrap()).unwrap();
    assert_eq!(list, [ACME, BETA]);

    let last = lists.last().unwrap();
    assert_eq!(last.active.as_deref(), Some(BETA));
    assert_eq!(last.accounts.len(), 2);
    assert!(last.accounts[1].active && !last.accounts[0].active);
    assert_eq!(last.accounts[0].server, ACME);
    assert_eq!(last.accounts[0].user_id.as_deref(), Some("7"));
    fake.assert_not_sent("DELETE", &at(ACME, "/api/v1/token"));
}

#[tokio::test]
async fn a_switch_revokes_nothing_and_requests_follow_the_active_server() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = signed_in_to_both(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["beta-general"]
    );
    let sent = fake.assert_sent("GET", &at(BETA, "/api/v1/channels"));
    assert_eq!(sent.header("authorization"), Some("Bearer beta-token"));

    auth.switch_to(ACME).await.unwrap();
    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["acme-general"]
    );
    let sent = fake.assert_sent("GET", &at(ACME, "/api/v1/channels"));
    assert_eq!(sent.header("authorization"), Some("Bearer acme-token"));
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["user"]["id"], 7, "Acme's user, as given at sign-in");
    let (events, lists) = heard(&app).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].signed_in);
    assert_eq!(events[0].server.as_deref(), Some(ACME));
    assert_eq!(events[0].user.as_ref().unwrap()["user"]["id"], 7);
    assert_eq!(lists.last().unwrap().active.as_deref(), Some(ACME));

    // Back again; the active one again is a no-op.
    auth.switch_to(&format!("{BETA}/")).await.unwrap();
    auth.switch_to(BETA).await.unwrap();
    let (events, _) = heard(&app).await;
    assert_eq!(events.len(), 1, "{events:?}");
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    let last = fake
        .sent_to("GET", &at(BETA, "/api/v1/channels"))
        .pop()
        .unwrap();
    assert_eq!(last.header("authorization"), Some("Bearer beta-token"));

    for server in [ACME, BETA] {
        fake.assert_not_sent("DELETE", &at(server, "/api/v1/token"));
    }
    assert!(auth.accounts().len() == 2);
    assert!(stored(&secrets, &token_key(ACME)).is_some());

    // A server not signed in to can't be switched to.
    assert!(matches!(
        auth.switch_to("https://elsewhere.test").await,
        Err(BackendError::Other { .. })
    ));
}

#[tokio::test]
async fn both_servers_outlive_a_restart_with_the_last_one_active() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let first = signed_in_to_both(&fake, &secrets).await;
    first.get::<Auth>().switch_to(ACME).await.unwrap();
    drop(first);

    let again = app(&fake, &secrets);
    let auth = again.get::<Auth>();
    assert!(auth.is_signed_in());
    assert_eq!(auth.active_server().as_deref(), Some(ACME));
    let accounts = auth.accounts();
    assert_eq!(accounts.len(), 2);
    assert!(
        accounts.iter().all(|a| a.user.is_none()),
        "users aren't kept across a restart"
    );
    assert_eq!(accounts[1].user_id.as_deref(), Some("9"));
    assert_eq!(
        again.invoke_ok::<Vec<String>>("channels", ()).await,
        ["acme-general"]
    );

    // Beta's user is read from Beta, with Beta's token, once it's active.
    auth.switch_to(BETA).await.unwrap();
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["user"]["id"], 9);
    let read = fake.sent_to("GET", &at(BETA, "/api/v1/me")).pop().unwrap();
    assert_eq!(read.header("authorization"), Some("Bearer beta-token"));
}

#[tokio::test]
async fn signing_out_of_one_server_revokes_only_its_token() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = signed_in_to_both(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    // Acme isn't active: Beta carries on, untouched.
    auth.sign_out_of(ACME).await.unwrap();
    let revoked = fake.assert_sent("DELETE", &at(ACME, "/api/v1/token"));
    assert_eq!(revoked.header("authorization"), Some("Bearer acme-token"));
    fake.assert_not_sent("DELETE", &at(BETA, "/api/v1/token"));
    assert_eq!(stored(&secrets, &token_key(ACME)), None);
    assert_eq!(
        stored(&secrets, &token_key(BETA)).as_deref(),
        Some("beta-token")
    );
    assert_eq!(auth.active_server().as_deref(), Some(BETA));
    assert_eq!(auth.accounts().len(), 1);
    let (events, lists) = heard(&app).await;
    assert!(
        events.is_empty(),
        "the active server didn't change: {events:?}"
    );
    let last = lists.last().unwrap();
    assert_eq!(last.signed_out_of.as_deref(), Some(ACME));
    assert_eq!(last.reason.as_deref(), Some("signed-out"));
    assert_eq!(last.accounts.len(), 1);
    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["beta-general"]
    );
    assert!(matches!(
        auth.switch_to(ACME).await,
        Err(BackendError::Other { .. })
    ));
}

#[tokio::test]
async fn signing_out_of_the_active_server_makes_the_next_one_active() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = signed_in_to_both(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    // `sign_out` is the active server's.
    auth.sign_out().await.unwrap();
    let revoked = fake.assert_sent("DELETE", &at(BETA, "/api/v1/token"));
    assert_eq!(revoked.header("authorization"), Some("Bearer beta-token"));
    fake.assert_not_sent("DELETE", &at(ACME, "/api/v1/token"));
    assert!(auth.is_signed_in());
    assert_eq!(auth.active_server().as_deref(), Some(ACME));
    let (events, _) = heard(&app).await;
    assert!(
        events.iter().all(|e| e.signed_in),
        "never signed out between: {events:?}"
    );
    assert_eq!(events.last().unwrap().server.as_deref(), Some(ACME));
    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["acme-general"]
    );
    assert_eq!(stored(&secrets, SERVER_KEY).as_deref(), Some(ACME));

    // The last one: signed out, as always.
    auth.sign_out_of(ACME).await.unwrap();
    assert!(!auth.is_signed_in());
    assert_eq!(app.get::<Backend>().base_url(), None);
    let (events, _) = heard(&app).await;
    let last = events.last().unwrap();
    assert!(!last.signed_in);
    assert_eq!(last.reason.as_deref(), Some("signed-out"));
    assert_eq!(stored(&secrets, SERVER_KEY), None);
    assert_eq!(stored(&secrets, SERVERS_KEY), None);
    assert!(auth.accounts().is_empty());
}

#[tokio::test]
async fn signing_out_of_all_revokes_each_on_its_own_server() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = signed_in_to_both(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();
    auth.sign_out_all().await.unwrap();
    for (server, token) in [(ACME, "acme-token"), (BETA, "beta-token")] {
        let revoked = fake.assert_sent("DELETE", &at(server, "/api/v1/token"));
        assert_eq!(
            revoked.header("authorization"),
            Some(format!("Bearer {token}").as_str())
        );
        assert_eq!(stored(&secrets, &token_key(server)), None);
    }
    assert!(!auth.is_signed_in());
    let (events, _) = heard(&app).await;
    assert_eq!(
        events.iter().filter(|e| !e.signed_in).count(),
        1,
        "signed out once, at the end: {events:?}"
    );
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated")
    );
}

#[tokio::test]
async fn a_401_signs_out_of_that_server_and_the_other_carries_on() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = signed_in_to_both(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    fake.clone().get(
        at(BETA, "/api/v1/channels"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated")
    );
    let auth = app.get::<Auth>();
    assert!(auth.is_signed_in());
    assert_eq!(auth.active_server().as_deref(), Some(ACME));
    assert_eq!(stored(&secrets, &token_key(BETA)), None);
    let (events, lists) = heard(&app).await;
    let last = lists.last().unwrap();
    assert_eq!(last.signed_out_of.as_deref(), Some(BETA));
    assert_eq!(last.reason.as_deref(), Some("expired"));
    assert!(events.iter().all(|e| e.signed_in), "{events:?}");
    assert_eq!(
        app.invoke_ok::<Vec<String>>("channels", ()).await,
        ["acme-general"]
    );
}

#[tokio::test]
async fn one_server_only_behaves_as_before() {
    let fake = felagi();
    let store = Arc::new(MemoryTokens::new());
    let app = app_with(
        Backend::chosen_at_sign_in()
            .user_route("/api/v1/me")
            .revoke_route("/api/v1/token")
            .user_id_path("user.id"),
        &fake,
        store.clone(),
    );
    let auth = app.get::<Auth>();
    assert!(auth.accounts().is_empty());
    auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
        .await
        .unwrap();
    let accounts = auth.accounts();
    assert_eq!(accounts.len(), 1);
    assert!(accounts[0].active);
    assert!(
        auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
            .await
            .is_err(),
        "another server needs a sign-out first"
    );
    assert_eq!(store.stored(SERVERS_KEY), None, "no list kept");
    assert_eq!(store.stored(&token_key(BETA)), None);
    auth.sign_out().await.unwrap();
    assert!(!auth.is_signed_in());
    assert!(auth.accounts().is_empty());
}

#[tokio::test]
async fn a_sign_in_from_before_becomes_the_first_of_the_list() {
    // What a single-server app kept, before it turned on `multiple_servers`.
    let store = Arc::new(
        MemoryTokens::new()
            .with(SERVER_KEY, ACME)
            .with(token_key(ACME), "acme-token")
            .with(format!("backend-user:{ACME}"), "7"),
    );
    let fake = felagi();
    let app = app_with(backend(), &fake, store.clone());
    let auth = app.get::<Auth>();
    assert_eq!(auth.active_server().as_deref(), Some(ACME));
    assert_eq!(auth.accounts().len(), 1);
    assert_eq!(
        store.stored(SERVERS_KEY).as_deref(),
        Some(format!("[\"{ACME}\"]").as_str())
    );
    auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
        .await
        .unwrap();
    assert_eq!(auth.accounts().len(), 2);
}

// -- realtime, across a switch -------------------------------------------------

const SETTINGS: &str = "/api/desktop/broadcasting";

#[tokio::test]
async fn broadcasting_and_presence_move_to_the_active_server() {
    let (acme_pusher, beta_pusher) = (PusherFake::start().await, PusherFake::start().await);
    let fake = felagi();
    for (server, pusher, key) in [
        (ACME, &acme_pusher, "acme-key"),
        (BETA, &beta_pusher, "beta-key"),
    ] {
        fake.clone().get(
            at(server, SETTINGS),
            200,
            json!({ "key": key, "host": "127.0.0.1", "port": pusher.port(), "scheme": "ws" }),
        );
    }
    let app = app_with(
        backend()
            .broadcasting_from(SETTINGS)
            .follow_user("chat", "user.{user.id}", None)
            .presence(["conversation.*"]),
        &fake,
        Arc::new(Secrets::fake()),
    );
    let auth = app.get::<Auth>();
    let presence = app.get::<elyra::presence::Presence>();
    presence.join("main", "conversation.5").unwrap();

    auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
        .await
        .unwrap();
    acme_pusher.wait_for_subscriptions(2).await;
    let mut wanted: Vec<String> = acme_pusher
        .subscriptions()
        .iter()
        .map(|s| s.channel.clone())
        .collect();
    wanted.sort();
    assert_eq!(wanted, ["presence-conversation.5", "private-user.7"]);
    assert_eq!(acme_pusher.keys(), ["acme-key"]);

    // Beta: the connection moves there, as Beta's user, with Beta's key.
    auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
        .await
        .unwrap();
    beta_pusher.wait_for_subscriptions(2).await;
    acme_pusher
        .wait_until(|p| p.connected() == 0, "Acme's connection to close")
        .await;
    let mut wanted: Vec<String> = beta_pusher
        .subscriptions()
        .iter()
        .map(|s| s.channel.clone())
        .collect();
    wanted.sort();
    assert_eq!(wanted, ["presence-conversation.5", "private-user.9"]);
    assert_eq!(beta_pusher.keys(), ["beta-key"]);
    let authorized = fake.sent_to("POST", &at(BETA, "/api/broadcasting/auth"));
    assert!(authorized
        .iter()
        .all(|a| a.header("authorization") == Some("Bearer beta-token")));

    // And back to Acme, without signing in again.
    auth.switch_to(ACME).await.unwrap();
    acme_pusher.wait_for_subscriptions(4).await;
    beta_pusher
        .wait_until(|p| p.connected() == 0, "Beta's connection to close")
        .await;
    assert_eq!(acme_pusher.connections(), 2);
    let broadcaster = app.get::<Broadcaster>();
    acme_pusher
        .wait_until(|_| broadcaster.state() == State::Connected, "connected")
        .await;
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(acme_pusher.connected(), 1);
}

#[tokio::test]
async fn listened_channels_move_to_the_active_server() {
    let (acme_pusher, beta_pusher) = (PusherFake::start().await, PusherFake::start().await);
    let fake = felagi();
    for (server, pusher, key) in [
        (ACME, &acme_pusher, "acme-key"),
        (BETA, &beta_pusher, "beta-key"),
    ] {
        fake.clone().get(
            at(server, SETTINGS),
            200,
            json!({ "key": key, "host": "127.0.0.1", "port": pusher.port(), "scheme": "ws" }),
        );
    }
    let app = app_with(
        backend().broadcasting_from(SETTINGS).listen(["task.*"]),
        &fake,
        Arc::new(Secrets::fake()),
    );
    let auth = app.get::<Auth>();
    let listening = app.get::<elyra::listening::Listening>();
    // Before anyone is signed in: wanted, and nothing joined.
    listening.listen("main", "task.01JD").unwrap();
    assert!(!listening.joined("task.01JD"));

    let on = |pusher: &PusherFake| {
        pusher
            .subscriptions()
            .iter()
            .filter(|s| s.channel == "private-task.01JD")
            .count()
    };
    auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
        .await
        .unwrap();
    acme_pusher
        .wait_until(|p| on(p) == 1, "Acme's subscription")
        .await;
    acme_pusher
        .wait_until(|_| listening.joined("task.01JD"), "joined at Acme")
        .await;

    // Beta: subscribed there, authorized by Beta with Beta's token.
    auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
        .await
        .unwrap();
    beta_pusher
        .wait_until(|p| on(p) == 1, "Beta's subscription")
        .await;
    acme_pusher
        .wait_until(|p| p.connected() == 0, "Acme's connection to close")
        .await;
    let authorized = fake.sent_to("POST", &at(BETA, "/api/broadcasting/auth"));
    assert_eq!(authorized.len(), 1);
    assert_eq!(
        authorized[0].body.as_ref().unwrap()["channel_name"],
        "private-task.01JD"
    );
    assert_eq!(
        authorized[0].header("authorization"),
        Some("Bearer beta-token")
    );

    // And back to Acme.
    auth.switch_to(ACME).await.unwrap();
    acme_pusher
        .wait_until(|p| on(p) == 2, "Acme's subscription again")
        .await;
    beta_pusher
        .wait_until(|p| p.connected() == 0, "Beta's connection to close")
        .await;
    acme_pusher
        .wait_until(|_| listening.joined("task.01JD"), "joined at Acme again")
        .await;
}

// -- the offline copy and the outbox, per server -------------------------------

#[cfg(feature = "database")]
mod with_a_database {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;
    use elyra::db::sqlx::{self, Row};
    use elyra::http::Method;
    use elyra::offline::{Outbox, OutboxDropped, OutboxSent};
    use elyra::{Database, Dispatcher};

    #[command]
    async fn post_message(ctx: Ctx, body: String) -> elyra::Result<Value> {
        Ok(ctx
            .get::<Backend>()
            .post("/api/v1/messages")
            .body(&json!({ "body": body }))
            .queue_offline()
            .json()
            .await?)
    }

    fn db_path() -> std::path::PathBuf {
        static SEQ: AtomicU32 = AtomicU32::new(0);
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let path =
            std::env::temp_dir().join(format!("elyra-servers-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path
    }

    async fn connect(path: &std::path::Path) -> Database {
        Database::connect(&elyra::db::sqlite_url(path))
            .await
            .unwrap()
    }

    fn offline_app(db: Database, fake: &HttpFake, secrets: &Secrets) -> TestApp {
        TestApp::new(
            App::new()
                .bind(db)
                .backend(backend().offline())
                .token_store(Arc::new(secrets.clone()))
                .commands(commands![channels, post_message])
                .swap(Dispatcher::fake())
                .swap(Http::fake(fake.clone())),
        )
    }

    fn unplug(fake: &HttpFake, server: &str) {
        for method in [Method::Get, Method::Post, Method::Delete] {
            fake.clone().unreachable(method, at(server, "/*"));
        }
    }

    fn plug_in(fake: &HttpFake, server: &str, user: i64) {
        fake.clone()
            .get(at(server, "/up"), 200, json!("ok"))
            .get(at(server, "/api/v1/me"), 200, me(user, 1))
            .post(at(server, "/api/v1/messages"), 201, json!({ "id": 1 }))
            .delete(at(server, "/api/v1/token"), 204, Value::Null);
    }

    async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while !check().await {
            assert!(tokio::time::Instant::now() < deadline, "never: {what}");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    async fn signed_in(path: &std::path::Path, fake: &HttpFake, secrets: &Secrets) -> TestApp {
        let app = offline_app(connect(path).await, fake, secrets);
        let auth = app.get::<Auth>();
        auth.sign_in_with_token(ACME, "acme-token", me(7, 1))
            .await
            .unwrap();
        auth.sign_in_with_token(BETA, "beta-token", me(9, 1))
            .await
            .unwrap();
        app
    }

    #[tokio::test]
    async fn each_server_is_answered_from_its_own_copy() {
        let path = db_path();
        let fake = felagi();
        let secrets = Secrets::fake();
        let app = signed_in(&path, &fake, &secrets).await;
        let auth = app.get::<Auth>();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["beta-general"]
        );
        auth.switch_to(ACME).await.unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["acme-general"]
        );

        // Both servers go: each answers from its own copy, kept across the
        // switches, never from the other's.
        unplug(&fake, ACME);
        unplug(&fake, BETA);
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["acme-general"]
        );
        auth.switch_to(BETA).await.unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["beta-general"]
        );

        // Signing out of Beta forgets Beta's copy, and only Beta's.
        auth.sign_out_of(BETA).await.unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["acme-general"]
        );
        let db = connect(&path).await;
        let rows = sqlx::query("SELECT server FROM elyra_backend_cache")
            .fetch_all(db.pool())
            .await
            .unwrap();
        assert_eq!(rows.len(), 1, "Acme's answer alone");
        let server: Option<String> = rows[0].try_get("server").unwrap();
        assert!(server.is_some_and(|s| s.len() == 64 && !s.contains("acme")));
    }

    #[tokio::test]
    async fn a_queued_write_waits_for_its_own_server() {
        let path = db_path();
        let fake = felagi();
        let secrets = Secrets::fake();
        let app = signed_in(&path, &fake, &secrets).await;
        let auth = app.get::<Auth>();
        auth.switch_to(ACME).await.unwrap();

        // Acme is unreachable: the message waits, for Acme.
        unplug(&fake, ACME);
        assert_eq!(
            app.invoke_err_kind("post_message", ("Hei".to_string(),))
                .await
                .as_deref(),
            Some("queued")
        );
        let outbox = app.get::<Outbox>();
        assert_eq!(outbox.pending().await.len(), 1);
        let db = connect(&path).await;
        let row = sqlx::query("SELECT server FROM elyra_outbox")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(row.try_get::<String, _>("server").unwrap(), ACME);

        // Beta is active and answers: Acme's write isn't Beta's, and isn't
        // sent anywhere. Beta's outbox is empty.
        auth.switch_to(BETA).await.unwrap();
        plug_in(&fake, BETA, 9);
        outbox.flush().await;
        assert!(outbox.pending().await.is_empty(), "Beta's own: none");
        // The count follows the switch a moment later, recounted for Beta.
        eventually("Beta's count: none", async || {
            app.get::<Backend>().connection().unwrap().pending == 0
        })
        .await;
        fake.assert_not_sent("POST", &at(BETA, "/api/v1/messages"));
        assert_eq!(fake.sent_to("POST", &at(ACME, "/api/v1/messages")).len(), 1);

        // Acme is back, and active again: it's sent there, with Acme's token
        // and the key it was kept with.
        plug_in(&fake, ACME, 7);
        auth.switch_to(ACME).await.unwrap();
        eventually("Acme's write sent", async || {
            outbox.pending().await.is_empty()
        })
        .await;
        let sent = fake.sent_to("POST", &at(ACME, "/api/v1/messages"));
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].header("authorization"), Some("Bearer acme-token"));
        assert_eq!(
            sent[0].header("idempotency-key"),
            sent[1].header("idempotency-key")
        );
        fake.assert_not_sent("POST", &at(BETA, "/api/v1/messages"));
        let events = app.get::<Dispatcher>();
        eventually("OutboxSent", async || {
            !events.dispatched::<OutboxSent>().is_empty()
        })
        .await;
    }

    #[tokio::test]
    async fn signing_out_of_a_server_drops_its_waiting_writes_when_asked() {
        let path = db_path();
        let fake = felagi();
        let secrets = Secrets::fake();
        let app = signed_in(&path, &fake, &secrets).await;
        let auth = app.get::<Auth>();
        auth.switch_to(ACME).await.unwrap();
        unplug(&fake, ACME);
        for body in ["one", "two"] {
            app.invoke_err_kind("post_message", (body.to_string(),))
                .await;
        }
        auth.switch_to(BETA).await.unwrap();

        // Refused while they wait, as `sign_out` is.
        match auth.sign_out_of(ACME).await {
            Err(BackendError::PendingChanges(2)) => {}
            other => panic!("{other:?}"),
        }
        match auth.sign_out_all().await {
            Err(BackendError::PendingChanges(2)) => {}
            other => panic!("{other:?}"),
        }
        assert_eq!(auth.accounts().len(), 2);

        // Dropped when asked: counted, and said.
        assert_eq!(auth.sign_out_of_discarding(ACME).await.unwrap(), 2);
        let db = connect(&path).await;
        let left = sqlx::query("SELECT COUNT(*) AS n FROM elyra_outbox")
            .fetch_one(db.pool())
            .await
            .unwrap()
            .try_get::<i64, _>("n")
            .unwrap();
        assert_eq!(left, 0);
        let dropped = app.get::<Dispatcher>();
        eventually("OutboxDropped", async || {
            !dropped.dispatched::<OutboxDropped>().is_empty()
        })
        .await;
        let event = &dropped.dispatched::<OutboxDropped>()[0];
        assert_eq!((event.server.as_str(), event.count), (ACME, 2));
        assert_eq!(auth.active_server().as_deref(), Some(BETA));
        assert_eq!(auth.accounts().len(), 1);
    }

    #[tokio::test]
    async fn tables_from_before_gain_their_server_and_their_rows_still_go() {
        let path = db_path();
        // The tables as 0.34 made them, without `server`.
        {
            let db = connect(&path).await;
            for ddl in [
                "CREATE TABLE elyra_backend_cache (id TEXT PRIMARY KEY, owner TEXT NOT NULL, \
                 status INTEGER NOT NULL, body TEXT NOT NULL, size INTEGER NOT NULL, \
                 stored_at INTEGER NOT NULL)",
                "CREATE TABLE elyra_outbox (id INTEGER PRIMARY KEY AUTOINCREMENT, \
                 owner TEXT NOT NULL, method TEXT NOT NULL, target TEXT NOT NULL, body TEXT NULL, \
                 headers TEXT NOT NULL, idempotency_key TEXT NOT NULL, attempts INTEGER NOT NULL, \
                 available_at INTEGER NOT NULL, created_at INTEGER NOT NULL)",
                "CREATE TABLE elyra_outbox_failed (id INTEGER PRIMARY KEY, owner TEXT NOT NULL, \
                 method TEXT NOT NULL, target TEXT NOT NULL, body TEXT NULL, headers TEXT NOT NULL, \
                 idempotency_key TEXT NOT NULL, status INTEGER NOT NULL, error TEXT NOT NULL, \
                 errors TEXT NULL, failed_at INTEGER NOT NULL, created_at INTEGER NOT NULL)",
            ] {
                sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
                    .execute(db.pool())
                    .await
                    .unwrap();
            }
        }
        // A single-server app, as before.
        let fake = felagi();
        let app = TestApp::new(
            App::new()
                .bind(connect(&path).await)
                .backend(
                    Backend::new(ACME)
                        .offline()
                        .user_route("/api/v1/me")
                        .user_id_path("user.id"),
                )
                .token_store(Arc::new(MemoryTokens::new()))
                .commands(commands![channels, post_message])
                .swap(Http::fake(fake.clone())),
        );
        app.get::<Auth>()
            .sign_in_with_token(ACME, "acme-token", me(7, 1))
            .await
            .unwrap();
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["acme-general"]
        );
        unplug(&fake, ACME);
        assert_eq!(
            app.invoke_ok::<Vec<String>>("channels", ()).await,
            ["acme-general"],
            "the copy works in the old table"
        );
        assert_eq!(
            app.invoke_err_kind("post_message", ("Hei".to_string(),))
                .await
                .as_deref(),
            Some("queued")
        );
        // A row kept before RFC 0022 has no server: it's the current one's.
        let db = connect(&path).await;
        sqlx::query("UPDATE elyra_outbox SET server = NULL")
            .execute(db.pool())
            .await
            .unwrap();
        plug_in(&fake, ACME, 7);
        let outbox = app.get::<Outbox>();
        eventually("the old row sent", async || {
            outbox.flush().await;
            outbox.pending().await.is_empty()
        })
        .await;
        // Known offline by then, it went straight to the outbox: this is
        // its only send.
        let sent = fake.sent_to("POST", &at(ACME, "/api/v1/messages"));
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].header("authorization"), Some("Bearer acme-token"));
    }

    #[test]
    fn accounts_state_is_serialisable() {
        fn takes<T: serde::Serialize>() {}
        takes::<AccountsState>();
    }
}
