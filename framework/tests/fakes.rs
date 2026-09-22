//! Fakes through the real wiring: `App::swap` replaces what providers bound,
//! and the code under test never knows.

use std::sync::Arc;

use elyra::queue::{Queue, QueueProvider};
use elyra::storage::{Storage, StorageProvider};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Container, Ctx, Dispatcher, Provider};
use parking_lot::Mutex;
use serde_json::json;

#[derive(Clone, serde::Serialize, specta::Type)]
struct ReportExported {
    path: String,
}

/// The code under test: writes a file, queues follow-up work, announces it.
#[command]
async fn export(ctx: Ctx, name: String) -> elyra::Result<()> {
    let path = format!("exports/{name}.csv");
    ctx.get::<Storage>()
        .put_str(&path, "id,total\n1,42\n")
        .map_err(|e| elyra::Error::command(e.to_string()))?;
    ctx.get::<Queue>().push("upload", json!({ "path": path }));
    ctx.dispatch(ReportExported { path }).await
}

fn app() -> App {
    App::new()
        // The production wiring, untouched…
        .provider(StorageProvider::at("/definitely/not/used"))
        .provider(QueueProvider::new())
        .listen(|_: ReportExported, _ctx: Ctx| async {
            panic!("a faked dispatcher must not run listeners")
        })
        // …and the fakes swapped in over it.
        .swap(Storage::fake())
        .swap(Queue::fake())
        .swap(Dispatcher::fake())
        .commands(commands![export])
}

#[tokio::test]
async fn a_command_is_tested_against_fakes_without_touching_real_services() {
    let app = TestApp::new(app());
    app.invoke_ok::<()>("export", ("q3",)).await;

    let disk = app.get::<Storage>();
    assert_ne!(
        disk.root(),
        std::path::Path::new("/definitely/not/used"),
        "swap must win over StorageProvider"
    );
    disk.assert_contents("exports/q3.csv", "id,total\n1,42\n");

    let queue = app.get::<Queue>();
    queue.assert_pushed_times("upload", 1);
    queue.assert_pushed_with("upload", |p| p["path"] == "exports/q3.csv");

    let events = app.get::<Dispatcher>();
    events.assert_dispatched_with(|e: &ReportExported| e.path == "exports/q3.csv");
}

#[tokio::test]
async fn nothing_happens_when_the_command_fails_early() {
    #[command]
    async fn refuse(_ctx: Ctx) -> elyra::Result<()> {
        Err(elyra::Error::command("no data"))
    }
    let app = TestApp::new(
        App::new()
            .swap(Queue::fake())
            .swap(Dispatcher::fake())
            .commands(commands![refuse]),
    );
    assert_eq!(app.invoke_err("refuse", ()).await, "no data");
    app.get::<Queue>().assert_nothing_pushed();
    app.get::<Dispatcher>().assert_nothing_dispatched();
}

// --- swapping a trait-object binding -------------------------------------------

trait Mailer: Send + Sync {
    fn send(&self, to: &str);
}

struct Smtp;
impl Mailer for Smtp {
    fn send(&self, _to: &str) {
        panic!("the real mailer must not run in a test");
    }
}

#[derive(Default)]
struct FakeMailer {
    sent: Mutex<Vec<String>>,
}
impl Mailer for FakeMailer {
    fn send(&self, to: &str) {
        self.sent.lock().push(to.to_owned());
    }
}

struct MailProvider;
impl Provider for MailProvider {
    fn register(&self, c: &mut Container) {
        c.bind_as::<dyn Mailer>(Arc::new(Smtp));
    }
}

#[command]
async fn invite(ctx: Ctx, email: String) {
    ctx.get::<dyn Mailer>().send(&email);
}

#[tokio::test]
async fn swap_as_replaces_a_trait_binding_a_provider_made() {
    let fake = Arc::new(FakeMailer::default());
    let app = TestApp::new(
        App::new()
            .provider(MailProvider)
            .swap_as::<dyn Mailer>(fake.clone())
            .commands(commands![invite]),
    );
    app.invoke_ok::<()>("invite", ("ada@example.test",)).await;
    assert_eq!(*fake.sent.lock(), vec!["ada@example.test"]);
}
