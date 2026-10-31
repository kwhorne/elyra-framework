//! The inspector's ability simulator (RFC 0014): a masked ability is gone
//! from the guard, the `can` state, the assistant and an MCP agent, and
//! comes back when unmasked. Debug builds only, as the inspector is.
#![cfg(debug_assertions)]

use std::borrow::Cow;

use elyra::inspector::Mask;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Ctx};
use serde::Deserialize;
use wry::http::{Request, Response, StatusCode};

#[command(can = "posts.delete")]
async fn posts_destroy(_ctx: Ctx, id: i64) -> i64 {
    id
}

#[command(can = "posts.view")]
async fn posts_count(_ctx: Ctx) -> i64 {
    3
}

fn shell(app: App) -> TestShell {
    TestShell::new(
        app.allow_abilities(["posts.*"])
            .commands(commands![posts_destroy, posts_count])
            .prepare(),
    )
}

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

fn text(res: &Response<Cow<'static, [u8]>>) -> String {
    String::from_utf8_lossy(res.body()).to_string()
}

#[derive(Deserialize, Debug, PartialEq)]
struct Row {
    ability: String,
    granted: bool,
    user: Option<bool>,
    masked: bool,
}

async fn mask(shell: &TestShell, ability: &str, off: bool) -> Response<Cow<'static, [u8]>> {
    #[derive(serde::Serialize)]
    struct One<'a> {
        ability: &'a str,
        off: bool,
    }
    post(
        shell,
        "/__inspector/mask",
        rmp_serde::to_vec_named(&One { ability, off }).unwrap(),
    )
    .await
}

#[tokio::test]
async fn a_masked_ability_is_refused_by_the_guard_until_unmasked() {
    let shell = shell(App::new());
    let call = || {
        post(
            &shell,
            "/__cmd/posts_destroy",
            rmp_serde::to_vec(&(7i64,)).unwrap(),
        )
    };
    assert_eq!(call().await.status(), StatusCode::OK);

    let res = mask(&shell, "posts.delete", true).await;
    assert!(!failed(&res), "{}", text(&res));
    let rows: Vec<Row> = rmp_serde::from_slice(res.body()).unwrap();
    assert!(rows.iter().any(|r| r.ability == "posts.delete" && r.masked));

    let refused = call().await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        refused.headers().get("x-elyra-error-kind").unwrap(),
        "denied"
    );
    assert!(
        text(&refused).contains("`posts.delete` ability (simulated by the inspector)"),
        "{}",
        text(&refused)
    );
    // Only that one: the rest of the namespace still works.
    assert_eq!(
        post(
            &shell,
            "/__cmd/posts_count",
            rmp_serde::to_vec(&()).unwrap()
        )
        .await
        .status(),
        StatusCode::OK
    );

    mask(&shell, "posts.delete", false).await;
    assert_eq!(call().await.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_namespace_masks_every_ability_in_it_and_a_live_query_too() {
    let shell = shell(App::new());
    mask(&shell, "posts.*", true).await;
    for path in ["/__cmd/posts_count", "/__live/posts_count"] {
        let res = post(&shell, path, rmp_serde::to_vec(&()).unwrap()).await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn the_list_has_every_ability_the_app_knows() {
    let shell = shell(App::new());
    let res = post(&shell, "/__inspector/abilities", Vec::new()).await;
    let rows: Vec<Row> = rmp_serde::from_slice(res.body()).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.ability.as_str()).collect();
    assert_eq!(names, ["posts.*", "posts.delete", "posts.view"]);
    assert!(rows
        .iter()
        .all(|r| r.granted && !r.masked && r.user.is_none()));
}

#[tokio::test]
async fn the_can_state_carries_the_mask() {
    let shell = shell(App::new());
    #[derive(Deserialize)]
    struct State {
        abilities: Vec<String>,
        #[serde(default)]
        masked: Vec<String>,
    }
    let state =
        |res: Response<Cow<'static, [u8]>>| rmp_serde::from_slice::<State>(res.body()).unwrap();
    let before = state(post(&shell, "/__auth/abilities", Vec::new()).await);
    assert_eq!(before.abilities, ["*"]);
    assert!(before.masked.is_empty());
    mask(&shell, "posts.delete", true).await;
    let after = state(post(&shell, "/__auth/abilities", Vec::new()).await);
    assert_eq!(after.masked, ["posts.delete"]);
}

#[tokio::test]
async fn a_name_that_isnt_an_ability_is_refused() {
    let shell = shell(App::new());
    for bad in ["", "posts delete", "<script>"] {
        assert!(failed(&mask(&shell, bad, true).await), "{bad:?}");
    }
}

#[tokio::test]
async fn the_inspector_can_be_turned_off() {
    let shell = shell(App::new().inspector(false));
    let res = mask(&shell, "posts.delete", true).await;
    assert!(failed(&res));
    assert!(
        text(&res).contains("the inspector is off"),
        "{}",
        text(&res)
    );
    assert_eq!(
        post(
            &shell,
            "/__cmd/posts_destroy",
            rmp_serde::to_vec(&(1i64,)).unwrap()
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_mcp_agent_is_refused_a_masked_ability() {
    use elyra::mcp::Mcp;
    let app = elyra::testing::TestApp::new(
        App::new()
            .commands(commands![posts_destroy, posts_count])
            .mcp(Mcp::new().allow_ability("posts.*")),
    );
    let mcp = app.mcp();
    assert!(
        !mcp.call("posts_count", serde_json::json!({}))
            .await
            .is_error
    );
    app.get::<Mask>().set("posts.view", true);
    let refused = mcp.call("posts_count", serde_json::json!({})).await;
    assert!(refused.is_error);
    assert!(
        format!("{refused:?}").contains("simulated by the inspector"),
        "{refused:?}"
    );
}

#[cfg(feature = "ai")]
#[tokio::test]
async fn the_assistant_isnt_offered_a_masked_tool() {
    use elyra::ai::{Ai, AiFake};
    use elyra::assistant::Assistant;
    let app = elyra::testing::TestApp::new(
        App::new()
            .allow_abilities(["posts.*"])
            .commands(commands![posts_destroy, posts_count])
            .swap(Ai::fake(AiFake::new()))
            .assistant(Assistant::new().allow_abilities(["posts.*"])),
    );
    let assistant = app.assistant();
    assert_eq!(assistant.tools(), ["posts_count", "posts_destroy"]);
    app.get::<Mask>().set("posts.delete", true);
    assert_eq!(assistant.tools(), ["posts_count"]);
    app.get::<Mask>().set("posts.delete", false);
    assert_eq!(assistant.tools(), ["posts_count", "posts_destroy"]);
}
