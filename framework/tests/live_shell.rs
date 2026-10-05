//! Live queries over the real protocol handler (RFC 0002 step 2): the
//! `/__live/<command>` and `/__live-stop` routes, the ability check they share
//! with `/__cmd/`, and the update arriving on the subscribing window's
//! event poll — and only there.
#![cfg(feature = "database")]

use std::borrow::Cow;

use elyra::db::sqlx;
use elyra::testing::TestShell;
use elyra::{command, commands, App, Ctx, Database, Model};
use serde::{Deserialize, Serialize};
use wry::http::{Request, Response, StatusCode};

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[model(table = "posts")]
struct Post {
    id: i64,
    title: String,
}

#[command(live, can = "posts.view")]
async fn posts_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(Post::query().count(&ctx.get::<Database>()).await?)
}

#[command]
async fn posts_store(ctx: Ctx, title: String) -> elyra::Result<i64> {
    let mut post = Post { id: 0, title };
    post.insert(&ctx.get::<Database>()).await?;
    Ok(post.id)
}

#[command]
async fn plain(_ctx: Ctx) -> i64 {
    1
}

async fn shell(app: App) -> (TestShell, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-lives-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql("CREATE TABLE posts (id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL)")
        .execute(db.pool())
        .await
        .unwrap();
    let app = app
        .bind(db)
        .live_batch_window(std::time::Duration::ZERO)
        .commands(commands![posts_count, posts_store, plain]);
    (TestShell::new(app.prepare()), path)
}

fn ipc(shell: &TestShell, path: &str, client: &str, body: Vec<u8>) -> Request<Vec<u8>> {
    Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", client)
        .body(body)
        .unwrap()
}

fn text(res: &Response<Cow<'static, [u8]>>) -> String {
    String::from_utf8_lossy(res.body()).to_string()
}

fn field<'a>(value: &'a rmpv::Value, name: &str) -> Option<&'a rmpv::Value> {
    value
        .as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(name))
        .map(|(_, v)| v)
}

const NO_ARGS: [u8; 1] = [0x90]; // an empty msgpack array

#[tokio::test]
async fn subscribing_needs_the_commands_ability() {
    let (shell, path) = shell(App::new()).await;
    let res = shell
        .handle(ipc(&shell, "/__live/posts_count", "w", NO_ARGS.to_vec()))
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(
        text(&res).contains("requires the `posts.view` ability"),
        "{}",
        text(&res)
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn subscribe_update_stop() {
    let (shell, path) = shell(App::new().allow_ability("posts.*")).await;

    // Subscribe: `{ id, value }`, the first result inline.
    let res = shell
        .handle(ipc(&shell, "/__live/posts_count", "w1", NO_ARGS.to_vec()))
        .await;
    assert_eq!(res.status(), StatusCode::OK, "{}", text(&res));
    let body: rmpv::Value = rmp_serde::from_slice(res.body()).unwrap();
    let id = field(&body, "id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_owned();
    assert_eq!(field(&body, "value").and_then(|v| v.as_i64()), Some(0));

    // Another window writes.
    let write = shell
        .handle(ipc(
            &shell,
            "/__cmd/posts_store",
            "w2",
            rmp_serde::to_vec(&("hello",)).unwrap(),
        ))
        .await;
    assert_eq!(write.status(), StatusCode::OK, "{}", text(&write));

    // The update arrives on w1's poll as `{ value: 1 }` on elyra:live:<id>.
    let poll = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        shell.handle(ipc(&shell, "/__events", "w1", Vec::new())),
    )
    .await
    .expect("an update for w1");
    let batch: Vec<(String, rmpv::Value)> = rmp_serde::from_slice(poll.body()).unwrap();
    let update = batch
        .iter()
        .find(|(channel, _)| *channel == format!("elyra:live:{id}"))
        .unwrap_or_else(|| panic!("no live update in {batch:?}"));
    assert_eq!(field(&update.1, "value").and_then(|v| v.as_i64()), Some(1));

    // Only the window that opened it can stop it.
    let stop = |client: &'static str| {
        shell.handle(ipc(
            &shell,
            "/__live-stop",
            client,
            rmp_serde::to_vec(&id).unwrap(),
        ))
    };
    let other: bool = rmp_serde::from_slice(stop("w2").await.body()).unwrap();
    assert!(!other);
    let mine: bool = rmp_serde::from_slice(stop("w1").await.body()).unwrap();
    assert!(mine);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn an_ordinary_command_cant_be_subscribed() {
    let (shell, path) = shell(App::new()).await;
    let res = shell
        .handle(ipc(&shell, "/__live/plain", "w", NO_ARGS.to_vec()))
        .await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        text(&res).contains("isn't a live command"),
        "{}",
        text(&res)
    );
    let _ = std::fs::remove_file(path);
}
