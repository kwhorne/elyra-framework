//! RFC 0009 against a real Laravel app (scripts/laravel-backend.sh): a
//! customer's contract goes up as an upload beside the form's fields, comes
//! back as a download, is refused on its field by Laravel's `422`, and an
//! upload made with no network waits
//! with a copy of the file and is sent once it's back. Skipped unless
//! `ELYRA_LARAVEL_URL` says where the app is; CI starts one.
#![cfg(all(feature = "backend", feature = "database"))]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::auth::Auth;
use elyra::backend::{Backend, BackendError};
use elyra::files::ChosenFile;
use elyra::offline::Outbox;
use elyra::testing::TestApp;
use elyra::{App, Database};
use serde_json::{json, Value};
use tokio::net::{TcpListener, TcpStream};

/// A TCP proxy to Laravel whose plug can be pulled (as in laravel_offline.rs).
struct Plug {
    addr: SocketAddr,
    up: Arc<AtomicBool>,
    open: Arc<Mutex<Vec<tokio::task::JoinHandle<()>>>>,
}

impl Plug {
    async fn to(target: SocketAddr) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let up = Arc::new(AtomicBool::new(true));
        let open = Arc::new(Mutex::new(Vec::new()));
        let (accepting, tasks) = (up.clone(), open.clone());
        tokio::spawn(async move {
            while let Ok((mut inbound, _)) = listener.accept().await {
                if !accepting.load(Ordering::SeqCst) {
                    continue;
                }
                tasks.lock().unwrap().push(tokio::spawn(async move {
                    if let Ok(mut outbound) = TcpStream::connect(target).await {
                        let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
                    }
                }));
            }
        });
        Plug { addr, up, open }
    }

    fn pull(&self) {
        self.up.store(false, Ordering::SeqCst);
        for task in self.open.lock().unwrap().drain(..) {
            task.abort();
        }
    }

    fn push_in(&self) {
        self.up.store(true, Ordering::SeqCst);
    }
}

fn folder(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("elyra-laravel-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A small but real PDF, different for each `tag`.
fn pdf(tag: &str) -> Vec<u8> {
    format!(
        "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\n% {tag}\n\
         trailer << /Root 1 0 R >>\n%%EOF\n"
    )
    .into_bytes()
}

async fn app(base: String, dir: &std::path::Path) -> TestApp {
    let db = Database::connect(&elyra::db::sqlite_url(dir.join("app.db")))
        .await
        .unwrap();
    let app = TestApp::new(
        App::new().bind(db).backend(
            Backend::new(base)
                .offline()
                .offline_upload_folder(dir.join("queued")),
        ),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");
    app
}

fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[tokio::test]
async fn a_contract_goes_up_and_comes_back() {
    let Ok(url) = std::env::var("ELYRA_LARAVEL_URL") else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
        return;
    };
    let dir = folder("online");
    let app = app(url, &dir).await;
    let backend = app.get::<Backend>();
    let stamp = stamp();

    // Created with its contract, the fields beside it.
    let v1 = dir.join("contract.pdf");
    std::fs::write(&v1, pdf("v1")).unwrap();
    let created: Value = backend
        .post("/api/customers")
        .fields(&json!({ "name": format!("Files {stamp}"), "email": format!("files{stamp}@example.com") }))
        .attach("contract", ChosenFile::from_path(&v1))
        .json()
        .await
        .expect("created");
    let id = created["id"].as_i64().unwrap();
    assert_eq!(created["contract"], "contract.pdf");
    assert!(created.get("contract_path").is_none(), "hidden");

    // Replaced with a PUT: sent as a POST with `_method`, as PHP needs.
    let v2 = dir.join("signed.pdf");
    std::fs::write(&v2, pdf("v2")).unwrap();
    let updated: Value = backend
        .put(format!("/api/customers/{id}"))
        .field("name", format!("Files {stamp} (signed)"))
        .attach("contract", &v2)
        .json()
        .await
        .expect("updated");
    assert_eq!(updated["contract"], "signed.pdf");
    assert_eq!(updated["name"], format!("Files {stamp} (signed)"));

    // Downloaded where the user chose, as Laravel's Storage::download names it.
    let to = dir.join("saved.pdf");
    let saved = backend
        .get(format!("/api/customers/{id}/contract"))
        .download_to(&ChosenFile::for_saving(&to))
        .await
        .expect("downloaded");
    assert_eq!(std::fs::read(&saved).unwrap(), pdf("v2"));
    let into = dir.join("downloads");
    std::fs::create_dir_all(&into).unwrap();
    let named = backend
        .get(format!("/api/customers/{id}/contract"))
        .download_into(&ChosenFile::folder(&into))
        .await
        .expect("downloaded into the folder");
    assert_eq!(named, into.join("signed.pdf"));

    // Laravel's rule on the file, on its field.
    let text = dir.join("notes.txt");
    std::fs::write(&text, "not a pdf").unwrap();
    let refused = backend
        .put(format!("/api/customers/{id}"))
        .attach("contract", &text)
        .json::<Value>()
        .await
        .unwrap_err();
    let BackendError::Validation(errors) = refused else {
        panic!("a 422, not {refused:?}");
    };
    assert!(errors.first("contract").is_some(), "{errors:?}");

    // A body over PHP's `post_max_size` is Laravel's `413` behind php-fpm,
    // but PHP's built-in server (`artisan serve`) prints its warning before
    // Laravel can answer. backend_files.rs covers the `413` with a fake.

    backend
        .delete(format!("/api/customers/{id}"))
        .send()
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_made_offline_is_sent_from_its_copy() {
    let Ok(url) = std::env::var("ELYRA_LARAVEL_URL") else {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
        return;
    };
    let target: SocketAddr = url
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .parse()
        .expect("ELYRA_LARAVEL_URL as http://<ip>:<port>");
    let plug = Plug::to(target).await;
    let dir = folder("offline");
    let app = app(format!("http://{}", plug.addr), &dir).await;
    let backend = app.get::<Backend>();
    let stamp = stamp();
    let name = format!("Offline files {stamp}");

    let original = dir.join("contract.pdf");
    std::fs::write(&original, pdf("offline")).unwrap();
    plug.pull();
    let queued = backend
        .post("/api/customers")
        .fields(&json!({ "name": name, "email": format!("offline{stamp}@example.com") }))
        .attach("contract", ChosenFile::from_path(&original))
        .queue_offline()
        .json::<Value>()
        .await
        .unwrap_err();
    assert!(matches!(queued, BackendError::Queued(_)), "{queued:?}");
    // The user deletes the original meanwhile: the copy is what's sent.
    std::fs::remove_file(&original).unwrap();

    plug.push_in();
    let outbox = app.get::<Outbox>();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        outbox.flush().await;
        if outbox.items().await.is_empty() {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "never sent: {:?}",
            outbox.items().await
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    let page: Value = backend
        .get("/api/customers")
        .query(&json!({ "search": name }))
        .json()
        .await
        .unwrap();
    assert_eq!(page["total"], 1, "sent once: {page}");
    let customer = &page["data"][0];
    assert_eq!(customer["contract"], "contract.pdf");
    let id = customer["id"].as_i64().unwrap();
    let to = dir.join("back.pdf");
    backend
        .get(format!("/api/customers/{id}/contract"))
        .download_to(&ChosenFile::for_saving(&to))
        .await
        .unwrap();
    assert_eq!(std::fs::read(&to).unwrap(), pdf("offline"));
    assert!(
        std::fs::read_dir(dir.join("queued"))
            .map(|mut d| d.next().is_none())
            .unwrap_or(true)
            || walk_empty(&dir.join("queued")),
        "the copy is gone"
    );
    backend
        .delete(format!("/api/customers/{id}"))
        .send()
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// No files anywhere under `dir` (empty folders may stay).
fn walk_empty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .all(|e| {
            let path = e.path();
            path.is_dir() && walk_empty(&path)
        })
}
