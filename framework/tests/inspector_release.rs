//! A release build has no inspector (RFC 0014): its routes aren't there,
//! and asking one to mask an ability changes nothing. Run with
//! `cargo test --release --features backend --test inspector_release` (CI
//! does).
#![cfg(not(debug_assertions))]

use std::borrow::Cow;

use elyra::inspector::Mask;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Ctx};
use wry::http::{Request, Response, StatusCode};

#[command(can = "posts.delete")]
async fn posts_destroy(_ctx: Ctx, id: i64) -> i64 {
    id
}

async fn post(shell: &TestShell, path: &str, body: Vec<u8>) -> Response<Cow<'static, [u8]>> {
    let request = Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .body(body)
        .unwrap();
    shell.handle(request).await
}

#[tokio::test]
async fn the_inspectors_routes_arent_in_a_release() {
    let shell = TestShell::new(
        App::new()
            .allow_ability("posts.delete")
            .commands(commands![posts_destroy])
            .prepare(),
    );
    #[derive(serde::Serialize)]
    struct One {
        ability: &'static str,
        off: bool,
    }
    let body = rmp_serde::to_vec_named(&One {
        ability: "posts.delete",
        off: true,
    })
    .unwrap();
    for path in [
        "/__inspector/mask",
        "/__inspector/abilities",
        "/__inspector/presence",
    ] {
        let res = post(&shell, path, body.clone()).await;
        let ok = res.status() == StatusCode::OK
            && res.headers().get("x-elyra-status").map(|v| v.as_bytes()) != Some(b"error");
        assert!(!ok, "{path} answered in a release build");
    }
    // Nothing was masked.
    let res = post(
        &shell,
        "/__cmd/posts_destroy",
        rmp_serde::to_vec(&(7i64,)).unwrap(),
    )
    .await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[test]
fn a_mask_masks_nothing_in_a_release() {
    let mask = Mask::default();
    assert!(!mask.masks("posts.delete"));
    assert!(mask.list().is_empty());
}

/// The offline switch isn't there either: a request still reaches the server.
#[cfg(feature = "backend")]
#[tokio::test]
async fn the_backend_cant_be_unplugged_in_a_release() {
    use elyra::backend::Backend;
    use elyra::http::{Http, HttpFake};

    #[command]
    async fn customers_count(ctx: Ctx) -> elyra::Result<i64> {
        Ok(ctx
            .get::<Backend>()
            .get("/api/customers/count")
            .json()
            .await?)
    }

    let fake = HttpFake::new().get(
        "https://crm.test/api/customers/count",
        200,
        serde_json::json!(3),
    );
    let shell = TestShell::new(
        App::new()
            .backend(Backend::new("https://crm.test"))
            .commands(commands![customers_count])
            .swap(Http::fake(fake.clone()))
            .prepare(),
    );
    #[derive(serde::Serialize)]
    struct Unplug {
        on: bool,
    }
    let res = post(
        &shell,
        "/__inspector/offline",
        rmp_serde::to_vec_named(&Unplug { on: true }).unwrap(),
    )
    .await;
    let answered = res.status() == StatusCode::OK
        && res.headers().get("x-elyra-status").map(|v| v.as_bytes()) != Some(b"error");
    assert!(
        !answered,
        "/__inspector/offline answered in a release build"
    );
    let res = post(
        &shell,
        "/__cmd/customers_count",
        rmp_serde::to_vec(&()).unwrap(),
    )
    .await;
    assert_eq!(rmp_serde::from_slice::<i64>(res.body()).unwrap(), 3);
    fake.assert_sent("GET", "https://crm.test/api/customers/count");
}
