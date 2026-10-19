//! Files to and from a Laravel backend (RFC 0009 step 3): uploads as the
//! signed-in user, Laravel's and the web server's refusals on the field,
//! downloads where the user chose, `transfers`, and cancelling one.
#![cfg(feature = "backend")]

use std::path::{Path, PathBuf};
use std::time::Duration;

use elyra::backend::Backend;
use elyra::files::ChosenFile;
use elyra::http::{Http, HttpFake};
use elyra::testing::TestApp;
use elyra::transfers::{Direction, TransferState, Transfers};
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const BASE: &str = "https://crm.test";

#[command]
async fn customers_attach(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .post(format!("/api/customers/{id}/attachments"))
        .attach("file", &file)
        .field("note", "Signed copy")
        .max_size(1024)
        .json()
        .await?)
}

#[command]
async fn customers_replace_contract(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Value> {
    Ok(ctx
        .get::<Backend>()
        .put(format!("/api/customers/{id}/contract"))
        .attach("contract", &file)
        .label("Contract for Acme")
        .idempotent()
        .json()
        .await?)
}

#[command]
async fn invoices_pdf(ctx: Ctx, id: i64, to: ChosenFile) -> elyra::Result<String> {
    let saved = ctx
        .get::<Backend>()
        .get(format!("/api/invoices/{id}/pdf"))
        .download_to(&to)
        .await?;
    Ok(saved.display().to_string())
}

#[command]
async fn exports_into(ctx: Ctx, folder: ChosenFile) -> elyra::Result<String> {
    let saved = ctx
        .get::<Backend>()
        .get("/api/exports/42")
        .download_into(&folder)
        .await?;
    Ok(saved.display().to_string())
}

#[command]
async fn signed_download(ctx: Ctx, folder: ChosenFile) -> elyra::Result<String> {
    let backend = ctx.get::<Backend>();
    let link: Value = backend.get("/api/exports/42/link").json().await?;
    let saved = backend
        .download_url(link["url"].as_str().unwrap_or_default())
        .download_into(&folder)
        .await?;
    Ok(saved.display().to_string())
}

fn app_with(base: &str, http: Option<Http>) -> TestApp {
    let mut app = App::new().backend(Backend::new(base)).commands(commands![
        customers_attach,
        customers_replace_contract,
        invoices_pdf,
        exports_into,
        signed_download
    ]);
    if let Some(http) = http {
        app = app.swap(http);
    }
    let app = TestApp::new(app);
    app.get::<Backend>().use_token(Some("t0ken".into()));
    app
}

fn app(fake: &HttpFake) -> TestApp {
    app_with(BASE, Some(Http::fake(fake.clone())))
}

fn folder(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("elyra-backend-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn an_upload_goes_as_the_user_and_is_listed() {
    let dir = folder("upload");
    let file = dir.join("contract.pdf");
    std::fs::write(&file, "%PDF-1.7 signed").unwrap();
    let fake = HttpFake::new().post(
        format!("{BASE}/api/customers/7/attachments"),
        201,
        json!({ "id": 1, "name": "contract.pdf" }),
    );
    let app = app(&fake);
    let answer: Value = app
        .invoke_ok("customers_attach", (7, ChosenFile::from_path(&file)))
        .await;
    assert_eq!(answer["id"], 1);

    let sent = fake.assert_sent("POST", &format!("{BASE}/api/customers/7/attachments"));
    assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));
    let part = sent.part("file").unwrap();
    assert_eq!(part.file_name.as_deref(), Some("contract.pdf"));
    assert_eq!(part.bytes, b"%PDF-1.7 signed");
    assert_eq!(sent.part("note").unwrap().text(), "Signed copy");

    let listed = app.get::<Transfers>().list();
    assert_eq!(listed.len(), 1);
    let transfer = &listed[0];
    assert_eq!(transfer.direction, Direction::Upload);
    assert_eq!(transfer.name, "contract.pdf");
    assert_eq!(transfer.state, TransferState::Done);
    assert_eq!(transfer.sent, 15);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_put_with_a_file_is_a_post_with_method_and_a_key() {
    let dir = folder("put");
    let file = dir.join("contract.pdf");
    std::fs::write(&file, "v2").unwrap();
    let fake = HttpFake::new().post(format!("{BASE}/api/customers/7/contract"), 200, json!({}));
    let app = app(&fake);
    let _: Value = app
        .invoke_ok(
            "customers_replace_contract",
            (7, ChosenFile::from_path(&file)),
        )
        .await;
    let sent = fake.assert_sent("POST", &format!("{BASE}/api/customers/7/contract"));
    assert_eq!(sent.part("_method").unwrap().text(), "PUT");
    assert_eq!(sent.parts[0].name, "_method", "first, before the file");
    assert!(sent
        .header("idempotency-key")
        .is_some_and(|k| k.len() == 32));
    assert_eq!(app.get::<Transfers>().list()[0].name, "Contract for Acme");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn laravels_422_and_the_servers_413_are_on_the_field() {
    let dir = folder("refused");
    let file = dir.join("scan.tiff");
    std::fs::write(&file, "tiff").unwrap();
    let url = format!("{BASE}/api/customers/7/attachments");

    let laravel = HttpFake::new().post(
        &url,
        422,
        json!({ "message": "The file must be a PDF.", "errors": { "file": ["The file must be a PDF."] } }),
    );
    let errors = app(&laravel)
        .invoke_validation_errors("customers_attach", (7, ChosenFile::from_path(&file)))
        .await;
    assert_eq!(
        errors
            .as_ref()
            .and_then(|e| e.get("file"))
            .and_then(|m| m.first())
            .map(String::as_str),
        Some("The file must be a PDF.")
    );

    let nginx = HttpFake::new().post(&url, 413, json!({}));
    let app = app(&nginx);
    let errors = app
        .invoke_validation_errors("customers_attach", (7, ChosenFile::from_path(&file)))
        .await;
    assert_eq!(
        errors
            .as_ref()
            .and_then(|e| e.get("file"))
            .and_then(|m| m.first())
            .map(String::as_str),
        Some("The file is too large for the server.")
    );
    let failed = &app.get::<Transfers>().list()[0];
    assert_eq!(failed.state, TransferState::Failed);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_file_over_max_size_isnt_sent() {
    let dir = folder("max");
    let file = dir.join("video.mov");
    std::fs::write(&file, vec![0u8; 2048]).unwrap();
    let fake = HttpFake::new().post(
        format!("{BASE}/api/customers/7/attachments"),
        201,
        json!({}),
    );
    let errors = app(&fake)
        .invoke_validation_errors("customers_attach", (7, ChosenFile::from_path(&file)))
        .await;
    assert_eq!(
        errors
            .as_ref()
            .and_then(|e| e.get("file"))
            .and_then(|m| m.first())
            .map(String::as_str),
        Some("The file may not be larger than 1 KB.")
    );
    fake.assert_not_sent("POST", &format!("{BASE}/api/customers/7/attachments"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_download_goes_where_the_user_chose() {
    let dir = folder("download");
    let fake = HttpFake::new()
        .get_bytes(
            format!("{BASE}/api/invoices/7/pdf"),
            200,
            b"%PDF".to_vec(),
            "invoice-7.pdf",
        )
        .get_bytes(
            format!("{BASE}/api/exports/42"),
            200,
            b"a,b".to_vec(),
            "export.csv",
        );
    let app = app(&fake);

    let target = dir.join("Invoice 7.pdf");
    let saved: String = app
        .invoke_ok("invoices_pdf", (7, ChosenFile::for_saving(&target)))
        .await;
    assert_eq!(PathBuf::from(saved), target);
    assert_eq!(std::fs::read(&target).unwrap(), b"%PDF");
    let sent = fake.assert_sent("GET", &format!("{BASE}/api/invoices/7/pdf"));
    assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));

    let saved: String = app
        .invoke_ok("exports_into", (ChosenFile::folder(&dir),))
        .await;
    assert_eq!(PathBuf::from(saved), dir.join("export.csv"));
    assert_eq!(names(&dir), ["Invoice 7.pdf", "export.csv"]);

    let downloads: Vec<_> = app.get::<Transfers>().list();
    assert!(downloads
        .iter()
        .all(|t| t.direction == Direction::Download && t.state == TransferState::Done));
    assert_eq!(downloads[1].name, "export.csv");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_file_chosen_to_read_isnt_overwritten() {
    let dir = folder("wrong-kind");
    let opened = dir.join("mine.pdf");
    std::fs::write(&opened, "the user's own").unwrap();
    let fake = HttpFake::new().get_bytes(
        format!("{BASE}/api/invoices/7/pdf"),
        200,
        b"%PDF".to_vec(),
        "x.pdf",
    );
    let app = app(&fake);
    let err = app
        .invoke_err("invoices_pdf", (7, ChosenFile::from_path(&opened)))
        .await;
    assert!(err.contains("wasn't chosen to be saved to"), "{err}");
    let err = app
        .invoke_err("exports_into", (ChosenFile::from_path(&opened),))
        .await;
    assert!(err.contains("isn't a folder"), "{err}");
    assert_eq!(std::fs::read(&opened).unwrap(), b"the user's own");
    fake.assert_not_sent("GET", &format!("{BASE}/api/invoices/7/pdf"));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_signed_url_is_fetched_without_the_token() {
    let dir = folder("signed");
    let signed = "https://bucket.s3.test/exports/42.csv?X-Amz-Signature=abc";
    let fake = HttpFake::new()
        .get(
            format!("{BASE}/api/exports/42/link"),
            200,
            json!({ "url": signed }),
        )
        .get_bytes(
            "https://bucket.s3.test/exports/42.csv",
            200,
            b"a,b".to_vec(),
            "42.csv",
        );
    let app = app(&fake);
    let saved: String = app
        .invoke_ok("signed_download", (ChosenFile::folder(&dir),))
        .await;
    assert_eq!(PathBuf::from(saved), dir.join("42.csv"));
    let sent = fake.assert_sent("GET", "https://bucket.s3.test/exports/42.csv");
    assert_eq!(sent.header("authorization"), None);
    assert_eq!(sent.header("x-socket-id"), None);
    let _ = std::fs::remove_dir_all(dir);
}

/// A server that sends a download's head and a first chunk, then waits.
async fn slow_download() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let _ = stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nContent-Disposition: attachment; filename=\"big.zip\"\r\n\r\n")
                    .await;
                let _ = stream.write_all(&[b'z'; 1000]).await;
                let _ = stream.flush().await;
                tokio::time::sleep(Duration::from_secs(30)).await;
            });
        }
    });
    base
}

#[tokio::test]
async fn a_cancelled_download_stops_and_leaves_nothing() {
    let dir = folder("cancel");
    let base = slow_download().await;
    let app = app_with(&base, None);
    let transfers = app.get::<Transfers>();
    let running = {
        let app_dir = ChosenFile::folder(&dir);
        let backend = app.get::<Backend>();
        tokio::spawn(async move { backend.get("/big.zip").download_into(&app_dir).await })
    };
    // Wait until it's under way.
    let mut id = None;
    for _ in 0..100 {
        if let Some(t) = transfers.list().into_iter().find(|t| t.sent > 0) {
            id = Some(t.id);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let id = id.expect("the download started");
    assert!(names(&dir).iter().any(|n| n.ends_with(".part")));
    assert!(transfers.cancel(&id));
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .expect("it stops")
        .unwrap();
    assert!(
        matches!(outcome, Err(elyra::backend::BackendError::Cancelled)),
        "{outcome:?}"
    );
    assert!(names(&dir).is_empty(), "no .part left: {:?}", names(&dir));
    assert_eq!(transfers.list()[0].state, TransferState::Cancelled);
    let err: elyra::Error = elyra::backend::BackendError::Cancelled.into();
    assert_eq!(err.kind(), Some("cancelled"));
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_download_is_quarantined_on_macos() {
    let dir = folder("quarantine");
    let fake = HttpFake::new().get_bytes(
        format!("{BASE}/api/exports/42"),
        200,
        b"a,b".to_vec(),
        "export.csv",
    );
    let app = app(&fake);
    let saved: String = app
        .invoke_ok("exports_into", (ChosenFile::folder(&dir),))
        .await;
    let out = std::process::Command::new("xattr")
        .args(["-p", "com.apple.quarantine", &saved])
        .output()
        .unwrap();
    let value = String::from_utf8_lossy(&out.stdout);
    assert!(
        value.starts_with("0081;") && value.contains(";Elyra;"),
        "{value:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(windows)]
#[tokio::test]
async fn a_download_has_the_mark_of_the_web_on_windows() {
    let dir = folder("motw");
    let fake = HttpFake::new().get_bytes(
        format!("{BASE}/api/exports/42"),
        200,
        b"a,b".to_vec(),
        "export.csv",
    );
    let app = app(&fake);
    let saved: String = app
        .invoke_ok("exports_into", (ChosenFile::folder(&dir),))
        .await;
    let mark = std::fs::read_to_string(format!("{saved}:Zone.Identifier")).unwrap();
    assert!(mark.contains("ZoneId=3"), "{mark}");
    assert!(mark.contains("HostUrl=https://crm.test/"), "{mark}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A route under `/__auth/`, as the runtime calls it.
async fn route(shell: &elyra::testing::TestShell, op: &str, body: Value) -> Value {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__auth/{op}"))
        .header("x-elyra-token", shell.token())
        .body(rmp_serde::to_vec_named(&body).unwrap())
        .unwrap();
    let response = shell.handle(request).await;
    assert_ne!(
        response
            .headers()
            .get("x-elyra-status")
            .map(|v| v.as_bytes()),
        Some(&b"error"[..]),
        "{op} failed"
    );
    rmp_serde::from_slice(response.body()).unwrap()
}

#[tokio::test]
async fn the_frontend_lists_transfers_and_cancels_one() {
    let dir = folder("routes");
    let base = slow_download().await;
    let prepared = App::new().backend(Backend::new(&base)).prepare();
    let ctx = prepared.ctx.clone();
    let shell = elyra::testing::TestShell::new(prepared);
    assert_eq!(route(&shell, "transfers", Value::Null).await, json!([]));

    let backend = ctx.get::<Backend>();
    let into = ChosenFile::folder(&dir);
    let running = tokio::spawn(async move { backend.get("/big.zip").download_into(&into).await });
    let mut id = String::new();
    for _ in 0..100 {
        let listed = route(&shell, "transfers", Value::Null).await;
        if let Some(t) = listed.as_array().and_then(|a| a.first()) {
            if t["sent"].as_u64().unwrap_or(0) > 0 {
                assert_eq!(t["direction"], "download");
                assert_eq!(t["state"], "running");
                assert_eq!(t["total"], 1_000_000);
                id = t["id"].as_str().unwrap().to_owned();
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!id.is_empty(), "listed while it ran");
    let after = route(&shell, "transfer-cancel", json!({ "id": id })).await;
    let outcome = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        outcome,
        Err(elyra::backend::BackendError::Cancelled)
    ));
    let state = after[0]["state"].as_str().unwrap().to_owned();
    assert!(state == "running" || state == "cancelled", "{state}");
    assert_eq!(
        route(&shell, "transfers", Value::Null).await[0]["state"],
        "cancelled"
    );
    let _ = std::fs::remove_dir_all(dir);
}
