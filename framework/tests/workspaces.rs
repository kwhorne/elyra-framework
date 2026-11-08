//! A token per workspace (RFC 0031): `Backend::token_per_workspace(..)`
//! keeps each workspace's token and user, from its own
//! `sign_in_with_token`, beside the others' on its server. An `HttpFake`
//! answers both servers and `Secrets::fake()` is the keychain.
#![cfg(feature = "backend")]

use std::sync::Arc;

use elyra::auth::{Auth, ACCOUNTS_CHANNEL};
use elyra::backend::{Backend, BackendError};
use elyra::http::{Http, HttpFake, Method};
use elyra::secrets::Secrets;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

const ACME: &str = "https://acme.felagi.test";
const BETA: &str = "https://beta.felagi.test";

fn at(server: &str, path: &str) -> String {
    format!("{server}{path}")
}

/// What Félagi's `/api/v1/me` answers for a user in a workspace.
fn me(user: i64, workspace: i64, name: &str) -> Value {
    json!({
        "user": { "id": user, "name": format!("User {user}") },
        "workspace": { "id": workspace, "name": name },
    })
}

fn felagi() -> HttpFake {
    let mut fake = HttpFake::new();
    for server in [ACME, BETA] {
        fake = fake
            .get(at(server, "/api/v1/me"), 200, me(7, 1, "Acme"))
            .get(at(server, "/api/v1/channels"), 200, json!(["general"]))
            .get(
                at(server, "/api/v1/inbox"),
                200,
                json!({ "unread_count": 3 }),
            )
            .delete(at(server, "/api/v1/token"), 204, Value::Null);
    }
    fake
}

fn backend() -> Backend {
    Backend::chosen_at_sign_in()
        .multiple_servers()
        .user_route("/api/v1/me")
        .revoke_route("/api/v1/token")
        .user_id_path("user.id")
        .token_per_workspace("workspace.id")
}

#[command]
async fn channels(ctx: Ctx) -> elyra::Result<Vec<String>> {
    Ok(ctx.get::<Backend>().get("/api/v1/channels").json().await?)
}

fn app(fake: &HttpFake, secrets: &Secrets) -> TestApp {
    TestApp::new(
        App::new()
            .backend(backend())
            .token_store(Arc::new(secrets.clone()))
            .commands(commands![channels])
            .swap(Http::fake(fake.clone())),
    )
}

fn stored(secrets: &Secrets, key: &str) -> Option<String> {
    secrets.get(key).unwrap().map(|s| s.expose().to_owned())
}

/// Acme's three workspaces, the last signed in to in use.
async fn in_three(fake: &HttpFake, secrets: &Secrets) -> TestApp {
    let app = app(fake, secrets);
    let auth = app.get::<Auth>();
    for (id, name) in [(1, "Acme"), (2, "Support"), (3, "Sales")] {
        auth.sign_in_with_token(ACME, &format!("token-{id}"), me(7, id, name))
            .await
            .unwrap();
    }
    app
}

/// The bearer token the last request to `path` on `server` carried.
fn last_token(fake: &HttpFake, method: &str, server: &str, path: &str) -> Option<String> {
    fake.sent_to(method, &at(server, path))
        .pop()
        .and_then(|r| r.header("authorization").map(str::to_owned))
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AccountsJson {
    signed_out_of: Option<String>,
    reason: Option<String>,
    signed_out_of_workspace: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct AuthJson {
    signed_in: bool,
    user: Option<Value>,
}

async fn heard(app: &TestApp) -> (Vec<AuthJson>, Vec<AccountsJson>) {
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

#[tokio::test]
async fn each_workspace_is_kept_with_its_token_and_user() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    let auth = app.get::<Auth>();

    let accounts = auth.accounts();
    assert_eq!(accounts.len(), 1, "one server");
    let workspaces = &accounts[0].workspaces;
    assert_eq!(
        workspaces
            .iter()
            .map(|w| (w.id.as_str(), w.active))
            .collect::<Vec<_>>(),
        [("1", false), ("2", false), ("3", true)]
    );
    assert_eq!(
        workspaces[1].user.as_ref().unwrap()["workspace"]["name"],
        "Support"
    );
    for id in 1..=3 {
        assert_eq!(
            stored(&secrets, &format!("backend-token:{ACME}#{id}")),
            Some(format!("token-{id}"))
        );
    }
    // The one in use, where requests take it from.
    assert_eq!(
        stored(&secrets, &format!("backend-token:{ACME}")).as_deref(),
        Some("token-3")
    );
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-3")
    );
    fake.assert_not_sent("DELETE", &at(ACME, "/api/v1/token"));
}

#[tokio::test]
async fn a_switch_revokes_nothing_and_requests_carry_its_token() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    auth.switch_workspace(ACME, "1").await.unwrap();
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-1")
    );
    let user: Value = auth.user().await.unwrap();
    assert_eq!(user["workspace"]["name"], "Acme");
    let (events, _) = heard(&app).await;
    assert_eq!(events.len(), 1, "{events:?}");
    assert!(events[0].signed_in);
    assert_eq!(events[0].user.as_ref().unwrap()["workspace"]["id"], 1);
    assert!(auth.accounts()[0].workspaces[0].active);

    // The one in use already: nothing happens.
    auth.switch_workspace(ACME, "1").await.unwrap();
    assert!(heard(&app).await.0.is_empty());
    fake.assert_not_sent("DELETE", &at(ACME, "/api/v1/token"));

    // A workspace not signed in to, or a server.
    assert!(matches!(
        auth.switch_workspace(ACME, "9").await,
        Err(BackendError::Other { .. })
    ));
    assert!(matches!(
        auth.switch_workspace(BETA, "1").await,
        Err(BackendError::Other { .. })
    ));
}

#[tokio::test]
async fn a_restart_brings_back_every_workspace_and_who_the_user_is() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let first = in_three(&fake, &secrets).await;
    first
        .get::<Auth>()
        .switch_workspace(ACME, "2")
        .await
        .unwrap();
    drop(first);

    let again = app(&fake, &secrets);
    let auth = again.get::<Auth>();
    assert!(auth.is_signed_in());
    // Known before the server is asked: the channels named from it resolve.
    assert_eq!(auth.state().user.unwrap()["workspace"]["name"], "Support");
    let workspaces = &auth.accounts()[0].workspaces;
    assert_eq!(workspaces.len(), 3);
    assert!(workspaces[1].active);
    assert!(workspaces.iter().all(|w| w.user.is_some()));
    again.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-2")
    );
    fake.assert_not_sent("GET", &at(ACME, "/api/v1/me"));
}

#[tokio::test]
async fn leaving_a_workspace_revokes_its_token_alone() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    auth.sign_out_of_workspace(ACME, "1").await.unwrap();
    let revoked = fake.sent_to("DELETE", &at(ACME, "/api/v1/token"));
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].header("authorization"), Some("Bearer token-1"));
    assert_eq!(stored(&secrets, &format!("backend-token:{ACME}#1")), None);
    assert_eq!(
        stored(&secrets, &format!("backend-workspace:{ACME}#1")),
        None
    );
    let workspaces = &auth.accounts()[0].workspaces;
    assert_eq!(
        workspaces.iter().map(|w| w.id.as_str()).collect::<Vec<_>>(),
        ["2", "3"]
    );
    assert!(workspaces[1].active, "the one in use didn't change");
    let (events, lists) = heard(&app).await;
    assert!(events.is_empty(), "{events:?}");
    let last = lists.last().unwrap();
    assert_eq!(last.signed_out_of.as_deref(), Some(ACME));
    assert_eq!(last.signed_out_of_workspace.as_deref(), Some("1"));
    assert_eq!(last.reason.as_deref(), Some("signed-out"));
}

#[tokio::test]
async fn leaving_the_one_in_use_moves_to_the_next() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    let auth = app.get::<Auth>();

    auth.sign_out_of_workspace(ACME, "3").await.unwrap();
    let revoked = fake.sent_to("DELETE", &at(ACME, "/api/v1/token"));
    assert_eq!(revoked.len(), 1);
    assert_eq!(revoked[0].header("authorization"), Some("Bearer token-3"));
    assert!(auth.is_signed_in());
    assert!(
        auth.accounts()[0].workspaces[0].active,
        "the first left in use"
    );
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-1")
    );
    let (events, _) = heard(&app).await;
    assert!(events.iter().all(|e| e.signed_in), "{events:?}");
}

#[tokio::test]
async fn leaving_the_last_workspace_signs_out_of_the_server() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = app(&fake, &secrets);
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(ACME, "token-1", me(7, 1, "Acme"))
        .await
        .unwrap();
    auth.sign_out_of_workspace(ACME, "1").await.unwrap();
    assert!(!auth.is_signed_in());
    assert!(auth.accounts().is_empty());
    assert_eq!(stored(&secrets, &format!("backend-token:{ACME}#1")), None);
    assert_eq!(
        stored(&secrets, &format!("backend-workspaces:{ACME}")),
        None
    );
}

#[tokio::test]
async fn a_401_ends_that_workspace_and_the_next_carries_on() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    app.listen();
    heard(&app).await;
    fake.clone().get(
        at(ACME, "/api/v1/channels"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    assert_eq!(
        app.invoke_err_kind("channels", ()).await.as_deref(),
        Some("unauthenticated")
    );
    let auth = app.get::<Auth>();
    assert!(
        auth.is_signed_in(),
        "still in the server's other workspaces"
    );
    assert_eq!(stored(&secrets, &format!("backend-token:{ACME}#3")), None);
    let workspaces = &auth.accounts()[0].workspaces;
    assert_eq!(
        workspaces
            .iter()
            .map(|w| (w.id.as_str(), w.active))
            .collect::<Vec<_>>(),
        [("1", true), ("2", false)]
    );
    let (events, lists) = heard(&app).await;
    assert_eq!(
        events.len(),
        1,
        "once, never signed out between: {events:?}"
    );
    assert!(events[0].signed_in);
    assert_eq!(events[0].user.as_ref().unwrap()["workspace"]["id"], 1);
    let last = lists.last().unwrap();
    assert_eq!(last.signed_out_of.as_deref(), Some(ACME));
    assert_eq!(last.signed_out_of_workspace.as_deref(), Some("3"));
    assert_eq!(last.reason.as_deref(), Some("expired"));

    fake.clone()
        .get(at(ACME, "/api/v1/channels"), 200, json!(["general"]));
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-1")
    );
}

#[tokio::test]
async fn the_last_workspace_refused_ends_the_server() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = app(&fake, &secrets);
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(ACME, "token-1", me(7, 1, "Acme"))
        .await
        .unwrap();
    fake.clone().get(
        at(ACME, "/api/v1/channels"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    app.invoke_err_kind("channels", ()).await;
    assert!(!auth.is_signed_in());
    assert!(auth.accounts().is_empty());
    assert_eq!(stored(&secrets, &format!("backend-token:{ACME}#1")), None);
}

#[tokio::test]
async fn a_switch_to_a_workspace_on_another_server_makes_it_active() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(BETA, "beta-1", me(7, 1, "Beta"))
        .await
        .unwrap();
    assert_eq!(auth.active_server().as_deref(), Some(BETA));

    auth.switch_workspace(ACME, "2").await.unwrap();
    assert_eq!(auth.active_server().as_deref(), Some(ACME));
    app.invoke_ok::<Vec<String>>("channels", ()).await;
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/channels").as_deref(),
        Some("Bearer token-2")
    );
    // Beta's workspace is listed, with who the user is there.
    let beta = &auth.accounts()[1];
    assert_eq!(beta.workspaces.len(), 1);
    assert_eq!(
        beta.workspaces[0].user.as_ref().unwrap()["workspace"]["name"],
        "Beta"
    );
    fake.assert_not_sent("DELETE", &at(BETA, "/api/v1/token"));
}

#[tokio::test]
async fn a_request_in_another_workspace_carries_its_token_and_signs_nothing_out() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    let auth = app.get::<Auth>();

    let inbox: Value = auth
        .request_in(ACME, "1", Method::Get, "/api/v1/inbox")
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(inbox["unread_count"], 3);
    assert_eq!(
        last_token(&fake, "GET", ACME, "/api/v1/inbox").as_deref(),
        Some("Bearer token-1")
    );

    fake.clone().get(
        at(ACME, "/api/v1/inbox"),
        401,
        json!({ "message": "Unauthenticated." }),
    );
    let refused = auth
        .request_in(ACME, "1", Method::Get, "/api/v1/inbox")
        .unwrap()
        .send()
        .await;
    assert!(refused.is_err());
    assert!(auth.is_signed_in());
    assert_eq!(auth.accounts()[0].workspaces.len(), 3, "the app decides");

    assert!(auth
        .request_in(ACME, "9", Method::Get, "/api/v1/inbox")
        .is_err());
}

#[tokio::test]
async fn another_user_on_the_server_forgets_the_first_ones_workspaces() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(ACME, "other-1", me(8, 1, "Acme"))
        .await
        .unwrap();
    let workspaces = &auth.accounts()[0].workspaces;
    assert_eq!(workspaces.len(), 1);
    assert_eq!(workspaces[0].user.as_ref().unwrap()["user"]["id"], 8);
    assert_eq!(stored(&secrets, &format!("backend-token:{ACME}#2")), None);
}

#[tokio::test]
async fn signing_out_of_the_server_revokes_every_workspaces_token() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let app = in_three(&fake, &secrets).await;
    let auth = app.get::<Auth>();
    auth.sign_in_with_token(BETA, "beta-1", me(7, 1, "Beta"))
        .await
        .unwrap();

    // Acme isn't active.
    auth.sign_out_of(ACME).await.unwrap();
    let mut revoked: Vec<String> = fake
        .sent_to("DELETE", &at(ACME, "/api/v1/token"))
        .iter()
        .filter_map(|r| r.header("authorization").map(str::to_owned))
        .collect();
    revoked.sort();
    assert_eq!(
        revoked,
        ["Bearer token-1", "Bearer token-2", "Bearer token-3"]
    );
    for id in 1..=3 {
        assert_eq!(
            stored(&secrets, &format!("backend-token:{ACME}#{id}")),
            None
        );
    }
    assert_eq!(
        stored(&secrets, &format!("backend-workspaces:{ACME}")),
        None
    );

    // The last server, active: every workspace there too.
    auth.sign_in_with_token(BETA, "beta-2", me(7, 2, "Beta 2"))
        .await
        .unwrap();
    auth.sign_out_all().await.unwrap();
    let mut revoked: Vec<String> = fake
        .sent_to("DELETE", &at(BETA, "/api/v1/token"))
        .iter()
        .filter_map(|r| r.header("authorization").map(str::to_owned))
        .collect();
    revoked.sort();
    assert_eq!(revoked, ["Bearer beta-1", "Bearer beta-2"]);
    assert!(!auth.is_signed_in());
}

#[tokio::test]
async fn a_profile_read_again_is_kept_as_the_workspaces() {
    let fake = felagi();
    let secrets = Secrets::fake();
    let first = in_three(&fake, &secrets).await;
    let auth = first.get::<Auth>();
    let mut renamed = me(7, 3, "Sales");
    renamed["user"]["locale"] = json!("nb");
    auth.update_user(renamed).unwrap();
    assert_eq!(auth.state().user.unwrap()["user"]["locale"], "nb");
    // Another user, or another workspace, isn't this one's.
    assert!(auth.update_user(me(8, 3, "Sales")).is_err());
    assert!(auth.update_user(me(7, 1, "Acme")).is_err());
    drop(first);

    let again = app(&fake, &secrets);
    let auth = again.get::<Auth>();
    assert_eq!(auth.state().user.unwrap()["user"]["locale"], "nb");
    let workspaces = &auth.accounts()[0].workspaces;
    assert_eq!(workspaces[2].user.as_ref().unwrap()["user"]["locale"], "nb");
    assert!(workspaces[0].user.as_ref().unwrap()["user"]["locale"].is_null());
}
