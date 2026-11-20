//! Edit it in its own app (RFC 0038): a file on the server downloaded into
//! the app's folder and opened, each save sent back checked against its
//! version, a conflict never over the user's file, only the last save
//! waiting offline, and sign-out refused while one waits.
#![cfg(all(feature = "backend", feature = "system"))]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::editing::{EditState, Editing, Editings};
use elyra::http::{Http, HttpFake, Method, Sent};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use parking_lot::Mutex;
use serde_json::{json, Value};

const BASE: &str = "https://crm.test";
const FILE: &str = "https://crm.test/api/attachments/7/file";
const OPENED_AT: &str = "Wed, 19 Nov 2026 10:00:00 GMT";

#[command]
async fn attachments_edit(ctx: Ctx, id: i64) -> elyra::Result<Editing> {
    Ok(ctx
        .get::<Backend>()
        .edit(format!("/api/attachments/{id}/file"))
        .open()
        .await?)
}

#[command]
async fn attachments_edit_word(ctx: Ctx, id: i64) -> elyra::Result<Editing> {
    Ok(ctx
        .get::<Backend>()
        .edit(format!("/api/attachments/{id}/file"))
        .only(["docx"])
        .open()
        .await?)
}

struct Rig {
    app: TestApp,
    fake: HttpFake,
    root: PathBuf,
    opened: Arc<Mutex<Vec<PathBuf>>>,
    /// Leave the folder, as when the app quits.
    keep: bool,
}

impl Rig {
    async fn new(name: &str, fake: HttpFake) -> Self {
        let root =
            std::env::temp_dir().join(format!("elyra-editing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Self::at(root, fake).await
    }

    async fn at(root: PathBuf, fake: HttpFake) -> Self {
        let app = TestApp::new(
            App::new()
                .backend(Backend::new(BASE).editing_folder(&root))
                .token_store(Arc::new(MemoryTokens::new()))
                .swap(Http::fake(fake.clone()))
                .commands(commands![attachments_edit, attachments_edit_word]),
        );
        app.get::<Auth>()
            .sign_in_with_token(BASE, "t0ken", json!({ "id": 7, "name": "Ada" }))
            .await
            .unwrap();
        let opened = Arc::new(Mutex::new(Vec::new()));
        let seen = opened.clone();
        app.get::<Editings>().opening_with(move |path: &Path| {
            seen.lock().push(path.to_path_buf());
            Ok(())
        });
        Self {
            app,
            fake,
            root,
            opened,
            keep: false,
        }
    }

    fn editings(&self) -> Arc<Editings> {
        self.app.get::<Editings>()
    }

    async fn open(&self) -> Value {
        self.app.invoke_ok("attachments_edit", (7,)).await
    }

    fn path(&self, id: &str) -> PathBuf {
        self.editings().path(id).expect("being edited")
    }

    fn state(&self) -> EditState {
        self.editings().list()[0].state
    }

    fn saves(&self) -> Vec<Sent> {
        self.fake.sent_to("POST", FILE)
    }

    /// Look until `done`, as the app does once a second.
    async fn look_until(&self, what: &str, done: impl Fn(&Self) -> bool) {
        let backend = self.app.get::<Backend>();
        for _ in 0..200 {
            self.editings().look(&backend).await;
            if done(self) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("never {what}: {:?}", self.editings().list());
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn served(fake: HttpFake, name: &str, bytes: &[u8]) -> HttpFake {
    let disposition = format!("attachment; filename=\"{name}\"");
    fake.on_bytes(
        Method::Get,
        FILE,
        200,
        &[
            ("content-type", "application/octet-stream"),
            ("content-disposition", &disposition),
            ("last-modified", OPENED_AT),
        ],
        bytes.to_vec(),
    )
}

fn word() -> HttpFake {
    served(HttpFake::new(), "Contract Acme.docx", b"version 1")
}

/// Write the file as an editor would: a new size, so the change is seen
/// even where modified times are coarse.
fn save(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[tokio::test]
async fn it_is_downloaded_into_the_app_s_folder_and_opened_in_its_app() {
    let rig = Rig::new("open", word()).await;
    let editing = rig.open().await;

    assert_eq!(editing["name"], "Contract Acme.docx");
    assert_eq!(editing["state"], "open");
    let path = rig.path(editing["id"].as_str().unwrap());
    assert!(path.starts_with(&rig.root));
    assert_eq!(path.file_name().unwrap(), "Contract Acme.docx");
    assert_eq!(std::fs::read(&path).unwrap(), b"version 1");
    assert_eq!(*rig.opened.lock(), std::slice::from_ref(&path));
    let sent = rig.fake.assert_sent("GET", FILE);
    assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));

    // The folder names neither the server nor the user.
    let shown = path
        .strip_prefix(&rig.root)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(!shown.contains("crm") && !shown.contains("Ada"));
}

#[tokio::test]
async fn each_save_is_sent_back_checked_against_its_version() {
    let fake = word().post(
        FILE,
        200,
        json!({ "data": { "id": 7, "updated_at": "2026-11-20T09:00:00.000000Z" } }),
    );
    let rig = Rig::new("save", fake).await;
    let editing = rig.open().await;
    let path = rig.path(editing["id"].as_str().unwrap());

    save(&path, "version 2, by Ada");
    rig.look_until("sent", |r| {
        r.saves().len() == 1 && r.state() == EditState::Saved
    })
    .await;
    let sent = &rig.saves()[0];
    assert_eq!(sent.part("_method").unwrap().text(), "PUT");
    let file = sent.part("file").unwrap();
    assert_eq!(file.text(), "version 2, by Ada");
    assert_eq!(file.file_name.as_deref(), Some("Contract Acme.docx"));
    assert_eq!(
        file.content_type.as_deref(),
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    );
    assert_eq!(sent.header("if-unmodified-since"), Some(OPENED_AT));
    assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));
    assert!(rig.editings().list()[0].saved_at.is_some());

    // The version moved on: saving again isn't a conflict with yourself.
    save(&path, "version 3, by Ada again");
    rig.look_until("sent again", |r| {
        r.saves().len() == 2 && r.state() == EditState::Saved
    })
    .await;
    assert_eq!(
        rig.saves()[1].header("if-unmodified-since"),
        Some("2026-11-20T09:00:00.000000Z")
    );
}

#[tokio::test]
async fn a_file_touched_but_not_changed_is_not_sent() {
    let rig = Rig::new("touched", word().post(FILE, 200, json!({}))).await;
    let editing = rig.open().await;
    let path = rig.path(editing["id"].as_str().unwrap());

    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(5))
        .unwrap();
    drop(file);
    let backend = rig.app.get::<Backend>();
    for _ in 0..5 {
        rig.editings().look(&backend).await;
    }
    assert!(rig.saves().is_empty());
    assert_eq!(rig.state(), EditState::Open);
}

#[tokio::test]
async fn opening_it_again_opens_the_same_copy() {
    let rig = Rig::new("again", word()).await;
    let first = rig.open().await;
    let path = rig.path(first["id"].as_str().unwrap());
    save(&path, "unsaved work, not sent yet");

    let second = rig.open().await;
    assert_eq!(first["id"], second["id"]);
    assert_eq!(rig.fake.sent_to("GET", FILE).len(), 1);
    assert_eq!(*rig.opened.lock(), [path.clone(), path.clone()]);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "unsaved work, not sent yet"
    );
}

#[tokio::test]
async fn a_program_or_a_web_page_is_never_opened() {
    for name in ["setup.exe", "Invoice.html", "Report.url"] {
        let rig = Rig::new("refused", served(HttpFake::new(), name, b"MZ")).await;
        let error = rig.app.invoke_err("attachments_edit", (7,)).await;
        assert!(error.contains("could run something"), "{name}: {error}");
        assert!(rig.opened.lock().is_empty());
        assert!(rig.editings().list().is_empty());
        let left: Vec<_> = walk(&rig.root);
        assert!(left.is_empty(), "{name} left {left:?}");
    }

    let rig = Rig::new("only", served(HttpFake::new(), "scan.pdf", b"%PDF")).await;
    let error = rig.app.invoke_err("attachments_edit_word", (7,)).await;
    assert!(
        error.contains("isn't a kind of file this app edits"),
        "{error}"
    );
    assert!(rig.opened.lock().is_empty());
}

#[tokio::test]
async fn a_conflict_takes_theirs_beside_the_user_s_file_never_over_it() {
    let fake = word().on(
        Method::Post,
        FILE,
        412,
        json!({ "message": "Bob changed it." }),
    );
    let rig = Rig::new("theirs", fake).await;
    let editing = rig.open().await;
    let id = editing["id"].as_str().unwrap().to_owned();
    let mine = rig.path(&id);

    save(&mine, "version 2, by Ada");
    rig.look_until("a conflict", |r| r.state() == EditState::Conflict)
        .await;
    assert_eq!(
        rig.editings().list()[0].error.as_deref(),
        Some("Bob changed it.")
    );

    // Further saves wait behind it.
    save(&mine, "version 3, by Ada, still");
    let backend = rig.app.get::<Backend>();
    for _ in 0..3 {
        rig.editings().look(&backend).await;
    }
    assert_eq!(rig.saves().len(), 1);

    let _ = served(rig.fake.clone(), "Contract Acme.docx", b"version 2, by Bob");
    rig.editings().take_theirs(&backend, &id).await.unwrap();
    let theirs = rig.path(&id);
    assert_eq!(
        theirs.file_name().unwrap(),
        "Contract Acme (from the server).docx"
    );
    assert_eq!(
        std::fs::read_to_string(&theirs).unwrap(),
        "version 2, by Bob"
    );
    assert_eq!(
        std::fs::read_to_string(&mine).unwrap(),
        "version 3, by Ada, still"
    );
    assert_eq!(rig.opened.lock().last(), Some(&theirs));
    assert_eq!(rig.state(), EditState::Open);
    assert_eq!(
        rig.editings().list()[0].name,
        "Contract Acme (from the server).docx"
    );
}

#[tokio::test]
async fn a_conflict_keeps_mine_by_sending_it_without_the_check() {
    let fake = word().on(
        Method::Post,
        FILE,
        412,
        json!({ "message": "Bob changed it." }),
    );
    let rig = Rig::new("mine", fake).await;
    let editing = rig.open().await;
    let id = editing["id"].as_str().unwrap().to_owned();

    save(&rig.path(&id), "version 2, by Ada");
    rig.look_until("a conflict", |r| r.state() == EditState::Conflict)
        .await;

    let _ = rig.fake.clone().post(FILE, 200, json!({}));
    let backend = rig.app.get::<Backend>();
    rig.editings().keep_mine(&backend, &id).await.unwrap();
    assert_eq!(rig.state(), EditState::Saved);
    let saves = rig.saves();
    assert_eq!(saves.len(), 2);
    assert_eq!(saves[1].header("if-unmodified-since"), None);
    assert_eq!(saves[1].part("file").unwrap().text(), "version 2, by Ada");
}

#[tokio::test]
async fn offline_only_the_last_save_waits_and_it_goes_when_the_server_is_back() {
    let fake = word().unreachable(Method::Post, FILE);
    let rig = Rig::new("offline", fake).await;
    let editing = rig.open().await;
    let id = editing["id"].as_str().unwrap().to_owned();
    let path = rig.path(&id);
    let waiting = rig.root.clone();

    save(&path, "version 2");
    rig.look_until("waiting", |r| r.state() == EditState::Waiting)
        .await;
    save(&path, "version 3, on the train");
    rig.look_until("the newer one waiting", |r| {
        r.state() == EditState::Waiting && r.saves().len() >= 2
    })
    .await;
    // One copy waits: the last.
    let copies: Vec<PathBuf> = walk(&waiting).into_iter().filter(|p| in_saved(p)).collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(
        std::fs::read_to_string(&copies[0]).unwrap(),
        "version 3, on the train"
    );

    let _ = rig.fake.clone().post(FILE, 200, json!({}));
    rig.editings().retry_now();
    rig.look_until("sent", |r| r.state() == EditState::Saved)
        .await;
    let last = rig.saves().pop().unwrap();
    assert_eq!(last.part("file").unwrap().text(), "version 3, on the train");
    assert!(!walk(&waiting).iter().any(|p| in_saved(p)));
}

#[tokio::test]
async fn sign_out_is_refused_while_a_save_waits_and_ends_the_editing_after() {
    let fake = word().unreachable(Method::Post, FILE);
    let rig = Rig::new("sign-out", fake).await;
    let editing = rig.open().await;
    let id = editing["id"].as_str().unwrap().to_owned();
    let folder = rig.path(&id).parent().unwrap().to_path_buf();

    save(&rig.path(&id), "version 2");
    rig.look_until("waiting", |r| r.state() == EditState::Waiting)
        .await;
    let auth = rig.app.get::<Auth>();
    let refused = auth.sign_out().await.unwrap_err();
    assert!(refused.to_string().contains("1 change"), "{refused}");
    assert!(rig
        .editings()
        .finish(&id, false)
        .unwrap_err()
        .to_string()
        .contains("1 change"));

    auth.sign_out_discarding().await.unwrap();
    assert!(rig.editings().list().is_empty());
    assert!(!folder.exists());
}

#[tokio::test]
async fn signing_out_ends_every_editing_and_removes_its_copy() {
    let rig = Rig::new("signed-out", word()).await;
    let editing = rig.open().await;
    let folder = rig
        .path(editing["id"].as_str().unwrap())
        .parent()
        .unwrap()
        .to_path_buf();

    rig.app.get::<Auth>().sign_out().await.unwrap();
    assert!(rig.editings().list().is_empty());
    assert!(!folder.exists());
}

#[tokio::test]
async fn finishing_removes_the_copy() {
    let rig = Rig::new("finish", word()).await;
    let editing = rig.open().await;
    let id = editing["id"].as_str().unwrap().to_owned();
    let folder = rig.path(&id).parent().unwrap().to_path_buf();
    rig.editings().finish(&id, false).unwrap();
    assert!(rig.editings().list().is_empty());
    assert!(!folder.exists());
}

#[tokio::test]
async fn an_editing_outlives_the_app_and_a_save_made_meanwhile_is_sent() {
    let root = std::env::temp_dir().join(format!("elyra-editing-restart-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let fake = word().post(FILE, 200, json!({}));
    let path = {
        let mut rig = Rig::at(root.clone(), fake.clone()).await;
        let editing = rig.open().await;
        let path = rig.path(editing["id"].as_str().unwrap());
        rig.keep = true;
        path
    };
    // The first app's loop has ended.
    tokio::time::sleep(Duration::from_millis(1200)).await;
    save(&path, "saved while the app was closed");

    let rig = Rig::at(root, fake).await;
    rig.look_until("sent", |r| r.saves().len() == 1).await;
    assert_eq!(
        rig.saves()[0].part("file").unwrap().text(),
        "saved while the app was closed"
    );
    assert_eq!(
        rig.saves()[0].header("if-unmodified-since"),
        Some(OPENED_AT)
    );
}

fn in_saved(path: &Path) -> bool {
    path.parent().is_some_and(|d| d.ends_with(".saved"))
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}
