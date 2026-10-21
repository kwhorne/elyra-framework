//! Uploads in the outbox (RFC 0009 step 4): an upload with no answer waits
//! with a copy of its files, is sent from the copy once the server is back,
//! and its copies go when it's sent or discarded.
#![cfg(all(feature = "backend", feature = "database"))]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::files::ChosenFile;
use elyra::http::{Http, HttpFake, Method};
use elyra::offline::Outbox;
use elyra::testing::TestApp;
use elyra::transfers::{TransferState, Transfers};
use elyra::{command, commands, App, Ctx, Database, Dispatcher};
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";

fn url(path: &str) -> String {
    format!("{BASE}{path}")
}

#[command]
async fn attach(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}/contract"))
        .attach("contract", &file)
        .field("note", "Signed")
        .queue_offline()
        .json()
        .await?)
}

fn laravel() -> HttpFake {
    HttpFake::new()
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 1, "name": "Ada" }))
        .delete(url("/api/sanctum/token"), 204, Value::Null)
        .get(url("/up"), 200, json!("ok"))
}

fn go_offline(fake: &HttpFake) {
    for method in [Method::Get, Method::Post, Method::Put, Method::Delete] {
        fake.clone().unreachable(method, url("/*"));
    }
}

fn come_back(fake: &HttpFake) {
    fake.clone()
        .get(url("/up"), 200, json!("ok"))
        .post(url("/api/sanctum/token"), 200, json!("7|s3cr3t"))
        .get(url("/api/user"), 200, json!({ "id": 1, "name": "Ada" }))
        .post(url("/api/customers/*"), 200, json!({ "id": 1 }));
}

fn temp(name: &str) -> PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!(
        "elyra-outbox-uploads-{name}-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Every file under `dir`.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_owned()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                out.push(entry.path());
            }
        }
    }
    out
}

async fn app(dir: &Path, fake: &HttpFake, limit: Option<u64>) -> TestApp {
    let db = Database::connect(&elyra::db::sqlite_url(dir.join("app.db")))
        .await
        .unwrap();
    let mut backend = Backend::new(BASE)
        .offline()
        .offline_upload_folder(dir.join("queued"));
    if let Some(limit) = limit {
        backend = backend.offline_upload_limit(limit);
    }
    let app = TestApp::new(
        App::new()
            .bind(db)
            .backend(backend)
            .token_store(Arc::new(MemoryTokens::new()))
            .commands(commands![attach])
            .swap(Dispatcher::fake())
            .swap(Http::fake(fake.clone())),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .unwrap();
    app
}

async fn eventually(what: &str, mut check: impl AsyncFnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    while !check().await {
        assert!(tokio::time::Instant::now() < deadline, "never: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn an_upload_waits_with_a_copy_and_is_sent_from_it() {
    let dir = temp("sent");
    let fake = laravel();
    let app = app(&dir, &fake, None).await;
    let original = dir.join("contract.pdf");
    std::fs::write(&original, "%PDF signed").unwrap();
    go_offline(&fake);

    let kind = app
        .invoke_err_kind("attach", (7, ChosenFile::from_path(&original)))
        .await;
    assert_eq!(kind.as_deref(), Some("queued"));
    // The user may move or delete the original meanwhile.
    std::fs::remove_file(&original).unwrap();
    assert_eq!(files_in(&dir.join("queued")).len(), 1);

    let outbox = app.get::<Outbox>();
    let pending = outbox.pending().await;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].method, "POST");
    assert_eq!(
        pending[0].body,
        Some(json!({ "contract": { "file": "contract.pdf" }, "note": "Signed" }))
    );
    let transfers = app.get::<Transfers>();
    let queued: Vec<_> = transfers
        .list()
        .into_iter()
        .filter(|t| t.state == TransferState::Queued)
        .collect();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].name, "contract.pdf");

    come_back(&fake);
    eventually("the outbox sent it", async || {
        outbox.pending().await.is_empty()
    })
    .await;
    let sent = fake.sent_to("POST", &url("/api/customers/7/contract"));
    let last = sent.last().unwrap();
    assert_eq!(last.part("_method").unwrap().text(), "PUT");
    let contract = last.part("contract").unwrap();
    assert_eq!(contract.bytes, b"%PDF signed", "from the copy");
    assert_eq!(contract.file_name.as_deref(), Some("contract.pdf"));
    assert_eq!(contract.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(last.part("note").unwrap().text(), "Signed");
    assert_eq!(
        sent[0].header("idempotency-key"),
        last.header("idempotency-key"),
        "the same key as the first try"
    );
    // The row goes first, then the copy: a crash between leaves a stray
    // file, never a row pointing at a missing one.
    eventually("the copy is gone", async || {
        files_in(&dir.join("queued")).is_empty()
    })
    .await;
    eventually("listed as done", async || {
        let list = transfers.list();
        list.iter().any(|t| t.state == TransferState::Done)
            && !list.iter().any(|t| t.state == TransferState::Queued)
    })
    .await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn cancelling_a_queued_upload_discards_it_and_its_copy() {
    let dir = temp("cancel");
    let fake = laravel();
    let app = app(&dir, &fake, None).await;
    let file = dir.join("scan.pdf");
    std::fs::write(&file, "scan").unwrap();
    go_offline(&fake);
    app.invoke_err_kind("attach", (7, ChosenFile::from_path(&file)))
        .await;
    let transfers = app.get::<Transfers>();
    let queued = transfers
        .list()
        .into_iter()
        .find(|t| t.state == TransferState::Queued)
        .expect("queued");
    assert!(transfers.cancel(&queued.id));
    let outbox = app.get::<Outbox>();
    eventually("out of the outbox", async || {
        outbox.pending().await.is_empty()
    })
    .await;
    eventually("the copy is gone", async || {
        files_in(&dir.join("queued")).is_empty()
    })
    .await;
    assert_eq!(
        transfers
            .list()
            .into_iter()
            .find(|t| t.id == queued.id)
            .unwrap()
            .state,
        TransferState::Cancelled
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn discarding_and_signing_out_remove_the_copies() {
    let dir = temp("discard");
    let fake = laravel();
    let app = app(&dir, &fake, None).await;
    let file = dir.join("a.pdf");
    std::fs::write(&file, "a").unwrap();
    go_offline(&fake);
    for _ in 0..2 {
        app.invoke_err_kind("attach", (7, ChosenFile::from_path(&file)))
            .await;
    }
    assert_eq!(files_in(&dir.join("queued")).len(), 2);
    let outbox = app.get::<Outbox>();
    let first = outbox.pending().await[0].id;
    outbox.discard(first).await.unwrap();
    assert_eq!(files_in(&dir.join("queued")).len(), 1);
    app.get::<Auth>().sign_out_discarding().await.unwrap();
    assert!(files_in(&dir.join("queued")).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn over_the_limit_it_isnt_kept() {
    let dir = temp("limit");
    let fake = laravel();
    let app = app(&dir, &fake, Some(1000)).await;
    let big = dir.join("video.mov");
    std::fs::write(&big, vec![0u8; 2000]).unwrap();
    go_offline(&fake);
    let kind = app
        .invoke_err_kind("attach", (7, ChosenFile::from_path(&big)))
        .await;
    assert_eq!(kind.as_deref(), Some("offline"));
    let err = app
        .invoke_err("attach", (7, ChosenFile::from_path(&big)))
        .await;
    assert!(err.contains("would take more than"), "{err}");
    assert!(app.get::<Outbox>().pending().await.is_empty());
    assert!(files_in(&dir.join("queued")).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_refused_upload_keeps_its_copy_and_is_retried_as_it_was() {
    let dir = temp("refused");
    let fake = laravel();
    let app = app(&dir, &fake, None).await;
    let file = dir.join("scan.tiff");
    std::fs::write(&file, "tiff").unwrap();
    go_offline(&fake);
    app.invoke_err_kind("attach", (7, ChosenFile::from_path(&file)))
        .await;
    // Back, and Laravel refuses it.
    fake.clone().get(url("/up"), 200, json!("ok")).post(
        url("/api/customers/*"),
        422,
        json!({ "message": "PDF only.", "errors": { "contract": ["PDF only."] } }),
    );
    let outbox = app.get::<Outbox>();
    eventually("refused", async || outbox.failed().await.len() == 1).await;
    assert_eq!(files_in(&dir.join("queued")).len(), 1, "kept for a retry");
    let id = outbox.failed().await[0].id;
    let err = outbox
        .retry(id, Some(json!({ "note": "changed" })))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("sent again as it was"), "{err}");
    outbox.discard(id).await.unwrap();
    assert!(files_in(&dir.join("queued")).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}
