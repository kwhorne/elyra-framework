//! A durable queue across **real** restarts: two app runs over one SQLite file,
//! each in its own tokio runtime.
//!
//! The runtime matters. An app's workers, in-flight jobs and pending delays are
//! tasks on its runtime; shutting that runtime down kills them all, exactly as
//! quitting — or crashing — would. Two apps on one shared runtime would both keep
//! working the journal at once, which is not a restart but two instances (and
//! the queue is explicitly one instance per database).
#![cfg(feature = "database")]

use std::future::Future;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use elyra::db::sqlx::{self, Row};
use elyra::queue::{JobOptions, Queue, QueueProvider};
use elyra::testing::TestApp;
use elyra::{App, Ctx, Database, Provider};
use serde_json::json;

/// Run one "launch" of the app to completion, then kill everything it spawned.
fn run<T>(launch: impl Future<Output = T>) -> T {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let out = rt.block_on(launch);
    rt.shutdown_timeout(Duration::from_millis(500));
    out
}

fn db_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::AtomicU32;
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path =
        std::env::temp_dir().join(format!("elyra-durable-{tag}-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

async fn connect(path: &std::path::Path) -> Database {
    Database::connect(&elyra::db::sqlite_url(path))
        .await
        .unwrap()
}

/// An app with a durable queue over `path`, plus whatever `extra` wires up.
async fn app(path: &std::path::Path, extra: impl FnOnce(App) -> App) -> TestApp {
    TestApp::new(extra(
        App::new()
            .bind(connect(path).await)
            .provider(QueueProvider::new().durable()),
    ))
}

/// Poll `check` for up to `limit`. Generous: CI runners can be slow, and a
/// test here asserts *eventual* behaviour, never a deadline.
async fn eventually_within(limit: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    check()
}

async fn eventually(check: impl FnMut() -> bool) -> bool {
    eventually_within(Duration::from_secs(10), check).await
}

async fn rows(db: &Database, table: &str) -> Option<i64> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT COUNT(*) AS n FROM {table}"
    )))
    .fetch_one(db.pool())
    .await
    .ok()
    .map(|row| row.get("n"))
}

/// Poll until `table` holds `want` rows. The journal creates its tables on
/// first write, which a plain `push` does a moment *after* returning — so a
/// missing table just means "not yet".
async fn eventually_rows(db: &Database, table: &str, want: i64) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        if rows(db, table).await == Some(want) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    false
}

/// Registers a handler that never finishes — the job stays "in flight" until
/// the run is killed, as it would if the app were quit mid-job.
struct Stuck;
impl Provider for Stuck {
    fn boot(&self, ctx: &Ctx) {
        ctx.get::<Queue>()
            .on("export", |_| std::future::pending::<Result<(), String>>());
    }
}

/// Counts how often `export` ran. Registered *after* `QueueProvider`, which is
/// the order that would lose recovered jobs if recovery ran in `boot`.
struct Counting(Arc<AtomicUsize>);
impl Provider for Counting {
    fn boot(&self, ctx: &Ctx) {
        let runs = self.0.clone();
        ctx.get::<Queue>().on("export", move |_| {
            let runs = runs.clone();
            async move {
                runs.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        });
    }
}

#[tokio::test]
async fn a_confirmed_push_is_on_disk_before_it_runs() {
    let path = db_path("confirm");
    let first = app(&path, |a| a.provider(Stuck)).await;
    let queue = first.get::<Queue>();
    assert!(queue.is_durable());
    queue
        .push_confirmed("export", json!({"n": 1}))
        .await
        .unwrap();

    let db = connect(&path).await;
    assert_eq!(rows(&db, "elyra_jobs").await, Some(1));
    let _ = std::fs::remove_file(path);
}

#[test]
fn pending_jobs_survive_a_restart_and_run_once_handled() {
    let path = db_path("restart");

    // Run 1: one job is stuck in its handler, two more wait behind it; then quit.
    run(async {
        let first = app(&path, |a| a.provider(Stuck)).await;
        let queue = first.get::<Queue>();
        for n in 0..3 {
            assert!(queue.push("export", json!({ "n": n })));
        }
        let db = connect(&path).await;
        assert!(
            eventually_rows(&db, "elyra_jobs", 3).await,
            "the backlog must be on disk, not only in memory"
        );
    });

    // Run 2: a working handler, registered by a provider that boots after the
    // queue's. Every job runs — including the one that was mid-flight.
    let runs = Arc::new(AtomicUsize::new(0));
    run(async {
        let r = runs.clone();
        let _second = app(&path, move |a| a.provider(Counting(r))).await;
        assert!(eventually(|| runs.load(Ordering::SeqCst) == 3).await);
        let db = connect(&path).await;
        assert!(
            eventually_rows(&db, "elyra_jobs", 0).await,
            "completed jobs are removed"
        );
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn jobs_pushed_this_run_are_not_also_recovered() {
    // Recovery snapshots the journal before this process writes to it, so a job
    // pushed during boot must run exactly once, next to the recovered one.
    let path = db_path("no-dup");
    run(async {
        let first = app(&path, |a| a.provider(Stuck)).await;
        first
            .get::<Queue>()
            .push_confirmed("export", json!({}))
            .await
            .unwrap();
    });

    struct PushDuringBoot;
    impl Provider for PushDuringBoot {
        fn boot(&self, ctx: &Ctx) {
            ctx.get::<Queue>().push("export", json!({"fresh": true}));
        }
    }
    let runs = Arc::new(AtomicUsize::new(0));
    run(async {
        let r = runs.clone();
        let _second = app(&path, move |a| {
            a.provider(Counting(r)).provider(PushDuringBoot)
        })
        .await;
        assert!(eventually(|| runs.load(Ordering::SeqCst) >= 2).await);
        // Settle, then make sure nothing ran a third time.
        let db = connect(&path).await;
        assert!(eventually_rows(&db, "elyra_jobs", 0).await);
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    assert_eq!(
        runs.load(Ordering::SeqCst),
        2,
        "one recovered + one fresh, no duplicate"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_failed_job_moves_to_the_failed_table_and_survives_a_restart() {
    let path = db_path("failed");
    struct AlwaysFails;
    impl Provider for AlwaysFails {
        fn boot(&self, ctx: &Ctx) {
            ctx.get::<Queue>().on_with(
                "export",
                JobOptions::default()
                    .attempts(2)
                    .retry_base(Duration::from_millis(5)),
                |_| async { Err("disk full".to_string()) },
            );
        }
    }
    run(async {
        let first = app(&path, |a| a.provider(AlwaysFails)).await;
        let queue = first.get::<Queue>();
        queue.push("export", json!({"file": "a.csv"}));
        assert!(eventually(|| queue.failed().len() == 1).await);
        let db = connect(&path).await;
        assert!(eventually_rows(&db, "elyra_failed_jobs", 1).await);
        assert_eq!(rows(&db, "elyra_jobs").await, Some(0), "moved, not copied");
        let row = sqlx::query("SELECT error, attempts FROM elyra_failed_jobs")
            .fetch_one(db.pool())
            .await
            .unwrap();
        assert_eq!(row.get::<String, _>("error"), "disk full");
        assert_eq!(row.get::<i64, _>("attempts"), 2);
    });

    // Next launch: the failure is still listed, and retrying it clears the row.
    let runs = Arc::new(AtomicUsize::new(0));
    run(async {
        let r = runs.clone();
        let second = app(&path, move |a| a.provider(Counting(r))).await;
        let queue = second.get::<Queue>();
        assert!(eventually(|| queue.failed().len() == 1).await);
        assert_eq!(queue.failed()[0].payload, json!({"file": "a.csv"}));
        assert_eq!(queue.retry_failed(), 1);
        assert!(eventually(|| runs.load(Ordering::SeqCst) == 1).await);
        let db = connect(&path).await;
        assert!(eventually_rows(&db, "elyra_failed_jobs", 0).await);
        assert!(eventually_rows(&db, "elyra_jobs", 0).await);
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_recovered_job_without_a_handler_fails_instead_of_vanishing() {
    let path = db_path("unhandled");
    // Run 1 holds its single worker on a stuck `export`, so the unknown job is
    // still pending when the run is killed.
    run(async {
        let first = app(&path, |a| a.provider(Stuck)).await;
        let queue = first.get::<Queue>();
        queue.push_confirmed("export", json!({})).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        queue
            .push_confirmed("retired-job", json!({}))
            .await
            .unwrap();
    });
    // Run 2 has no handler for either: both land in the failed table, once.
    run(async {
        let second = app(&path, |a| a).await;
        let queue = second.get::<Queue>();
        assert!(eventually(|| queue.failed().len() == 2).await);
        assert!(queue
            .failed()
            .iter()
            .all(|f| f.error.contains("no handler registered")));
        let db = connect(&path).await;
        assert!(eventually_rows(&db, "elyra_failed_jobs", 2).await);
        assert!(eventually_rows(&db, "elyra_jobs", 0).await);
    });
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_delay_survives_a_restart() {
    // A long delay, so the check that it hasn't run early has seconds of margin
    // even on a slow runner; the test asserts ordering, never a deadline.
    const DELAY: Duration = Duration::from_secs(3);
    let path = db_path("delay");
    let pushed_at = Instant::now();
    run(async {
        let first = app(&path, |a| a.provider(Stuck)).await;
        first.get::<Queue>().push_later(DELAY, "export", json!({}));
        let db = connect(&path).await;
        assert!(eventually_rows(&db, "elyra_jobs", 1).await);
    });

    let runs = Arc::new(AtomicUsize::new(0));
    run(async {
        let r = runs.clone();
        let _second = app(&path, move |a| a.provider(Counting(r))).await;
        // The invariant, race-free: whenever it *has* run, its due time has
        // passed. Read the counter first, then the clock — never the reverse.
        let start = Instant::now();
        while start.elapsed() < DELAY * 3 {
            let ran = runs.load(Ordering::SeqCst);
            assert!(
                ran == 0 || pushed_at.elapsed() >= DELAY,
                "ran before its due time across the restart"
            );
            if ran == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(runs.load(Ordering::SeqCst), 1, "the delayed job ran once");
    });
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn an_in_memory_queue_is_unchanged() {
    let app = TestApp::new(App::new().provider(QueueProvider::new()));
    let queue = app.get::<Queue>();
    assert!(!queue.is_durable());
    assert!(queue.push("x", json!(1)));
    queue.push_confirmed("x", json!(2)).await.unwrap();
}
