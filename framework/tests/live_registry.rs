//! Live queries end to end through `TestApp` (RFC 0002 step 2): subscribe to a
//! `#[command(live)]`, write, and get the new result pushed — coalesced, only
//! when it changed, only for what the command read.
#![cfg(feature = "database")]

use std::sync::Mutex;
use std::time::Duration;

use elyra::db::sqlx;
use elyra::live::LiveRegistry;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx, Database, Model};
use serde::{Deserialize, Serialize};

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type, PartialEq)]
#[model(table = "teams")]
struct Team {
    id: i64,
    name: String,
}

#[derive(Model, Debug, Clone, Default, Serialize, Deserialize, specta::Type)]
#[model(table = "notes")]
struct Note {
    id: i64,
    body: String,
}

#[command(live)]
async fn teams_index(ctx: Ctx) -> elyra::Result<Vec<Team>> {
    Ok(Team::query()
        .order_by("id")
        .get(&ctx.get::<Database>())
        .await?)
}

#[command(live)]
async fn teams_count(ctx: Ctx) -> elyra::Result<i64> {
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// Fails once there are more than two teams — a re-run can fail.
#[command(live)]
async fn few_teams(ctx: Ctx) -> elyra::Result<i64> {
    let n = Team::query().count(&ctx.get::<Database>()).await?;
    if n > 2 {
        return Err(elyra::Error::command("too many teams"));
    }
    Ok(n)
}

#[command]
async fn teams_store(ctx: Ctx, name: String) -> elyra::Result<Team> {
    let mut team = Team { id: 0, name };
    team.insert(&ctx.get::<Database>()).await?;
    Ok(team)
}

#[command]
async fn teams_rename_all(ctx: Ctx, name: String) -> elyra::Result<u64> {
    Ok(Team::query()
        .update(&ctx.get::<Database>(), &[("name", name.into())])
        .await?)
}

#[command]
async fn notes_store(ctx: Ctx) -> elyra::Result<()> {
    let mut note = Note {
        id: 0,
        body: "x".into(),
    };
    Ok(note.insert(&ctx.get::<Database>()).await?)
}

/// Writes — which a live command must not.
#[command(live)]
async fn sneaky(ctx: Ctx) -> elyra::Result<i64> {
    let mut team = Team {
        id: 0,
        name: "sneaky".into(),
    };
    team.insert(&ctx.get::<Database>()).await?;
    Ok(team.id)
}

static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Counts its runs, so a test can tell "re-ran, unchanged" from "never re-ran".
#[command(live)]
async fn teams_counted(ctx: Ctx) -> elyra::Result<i64> {
    RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    Ok(Team::query().count(&ctx.get::<Database>()).await?)
}

/// Not live.
#[command]
async fn plain(_ctx: Ctx) -> i64 {
    1
}

static THEME: Mutex<String> = Mutex::new(String::new());

static LEVEL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Depends on the `level` key, for timing the batch window without I/O.
#[command(live)]
async fn current_level(ctx: Ctx) -> i64 {
    ctx.depends_on("level");
    LEVEL.load(std::sync::atomic::Ordering::SeqCst)
}

/// Depends on a key that isn't a table.
#[command(live)]
async fn current_theme(ctx: Ctx) -> String {
    ctx.depends_on("settings:theme");
    THEME.lock().unwrap().clone()
}

#[command]
async fn set_theme(ctx: Ctx, theme: String) {
    *THEME.lock().unwrap() = theme;
    ctx.invalidate("settings:theme");
}

async fn app(window: Duration) -> (TestApp, std::path::PathBuf) {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("elyra-liver-{}-{n}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL);\
         CREATE TABLE notes (id INTEGER PRIMARY KEY AUTOINCREMENT, body TEXT NOT NULL);",
    )
    .execute(db.pool())
    .await
    .unwrap();
    let app = TestApp::new(
        App::new()
            .bind(db)
            .live_batch_window(window)
            .commands(commands![
                teams_index,
                teams_count,
                teams_counted,
                few_teams,
                teams_store,
                teams_rename_all,
                notes_store,
                sneaky,
                plain,
                current_theme,
                current_level,
                set_theme
            ]),
    );
    (app, path)
}

const QUIET: Duration = Duration::from_millis(150);

#[tokio::test]
async fn a_write_pushes_the_new_result() {
    let (app, path) = app(Duration::ZERO).await;
    let mut list = app.live::<Vec<Team>>("teams_index", ()).await;
    assert!(list.value().is_empty());

    app.invoke_ok::<Team>("teams_store", ("core".to_string(),))
        .await;
    let teams = list.next().await;
    assert_eq!(teams.len(), 1);
    assert_eq!(teams[0].name, "core");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn every_window_watching_gets_it() {
    let (app, path) = app(Duration::ZERO).await;
    let mut a = app.live::<i64>("teams_count", ()).await;
    let mut b = app.live::<i64>("teams_count", ()).await;
    app.invoke_ok::<Team>("teams_store", ("core".to_string(),))
        .await;
    assert_eq!(*a.next().await, 1);
    assert_eq!(*b.next().await, 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_burst_of_writes_is_coalesced() {
    // Coalescing promises fewer updates than writes — not exactly one, since a
    // slow disk can stretch the burst past the window — and the final state.
    let (app, path) = app(Duration::from_millis(400)).await;
    let mut count = app.live::<i64>("teams_count", ()).await;
    for i in 0..10 {
        app.invoke_ok::<Team>("teams_store", (format!("t{i}"),)).await;
    }
    let mut updates = 0;
    while *count.value() != 10 {
        count.next().await;
        updates += 1;
    }
    assert!(updates < 10, "{updates} updates for 10 writes");
    assert!(!count.updated_within(QUIET).await, "and nothing after the last");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn changes_within_the_batch_window_are_one_update() {
    // Five changes 50 ms apart all fall inside a 400 ms window: one re-run,
    // with the last value. (No I/O, so a slow machine doesn't stretch it.)
    let (app, path) = app(Duration::from_millis(400)).await;
    let mut level = app.live::<i64>("current_level", ()).await;
    let live = app.get::<LiveRegistry>();
    for n in 1..=5 {
        LEVEL.store(n, std::sync::atomic::Ordering::SeqCst);
        live.invalidate("level");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(*level.next().await, 5);
    assert!(!level.updated_within(QUIET).await, "one update, not five");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn nothing_is_pushed_when_the_result_is_unchanged() {
    let (app, path) = app(Duration::ZERO).await;
    app.invoke_ok::<Team>("teams_store", ("core".to_string(),))
        .await;
    let mut count = app.live::<i64>("teams_count", ()).await;
    // `teams` changed, so the count re-runs — but it's still 1.
    app.invoke_ok::<u64>("teams_rename_all", ("renamed".to_string(),))
        .await;
    assert!(!count.updated_within(QUIET).await);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_write_to_a_table_it_didnt_read_is_ignored() {
    let (app, path) = app(Duration::ZERO).await;
    let mut list = app.live::<i64>("teams_counted", ()).await;
    let runs = RUNS.load(std::sync::atomic::Ordering::SeqCst);
    app.invoke_ok::<()>("notes_store", ()).await;
    assert!(!list.updated_within(QUIET).await);
    assert_eq!(
        RUNS.load(std::sync::atomic::Ordering::SeqCst),
        runs,
        "not even re-run: it never read `notes`"
    );
    // While a write to what it read does re-run it.
    app.invoke_ok::<Team>("teams_store", ("core".to_string(),))
        .await;
    assert_eq!(*list.next().await, 1);
    assert_eq!(RUNS.load(std::sync::atomic::Ordering::SeqCst), runs + 1);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn any_key_can_be_invalidated() {
    let (app, path) = app(Duration::ZERO).await;
    *THEME.lock().unwrap() = "light".into();
    let mut theme = app.live::<String>("current_theme", ()).await;
    assert_eq!(theme.value(), "light");
    app.invoke_ok::<()>("set_theme", ("dark".to_string(),))
        .await;
    assert_eq!(theme.next().await, "dark");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_failing_re_run_is_pushed_as_an_error() {
    let (app, path) = app(Duration::ZERO).await;
    let mut few = app.live::<i64>("few_teams", ()).await;
    for i in 0..3 {
        app.invoke_ok::<Team>("teams_store", (format!("t{i}"),))
            .await;
    }
    // Updates to 1 and 2 may coalesce; the third write makes it fail.
    let mut last = Ok(());
    for _ in 0..3 {
        last = few.next_update().await;
        if last.is_err() {
            break;
        }
    }
    assert_eq!(last.unwrap_err(), "too many teams");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn only_live_commands_can_be_subscribed_and_they_cant_write() {
    let (app, path) = app(Duration::ZERO).await;
    let err = app.try_live::<i64>("plain", ()).await.err().unwrap();
    assert!(err.to_string().contains("isn't a live command"), "{err}");
    let err = app.try_live::<i64>("nope", ()).await.err().unwrap();
    assert!(err.to_string().contains("isn't a live command"), "{err}");

    let err = app.try_live::<i64>("sneaky", ()).await.err().unwrap();
    assert!(err.to_string().contains("must only read"), "{err}");
    assert_eq!(
        app.invoke_ok::<i64>("teams_count", ()).await,
        0,
        "nothing was written"
    );
    // The same command called once (not live) is an ordinary call.
    assert!(app.invoke_ok::<i64>("sneaky", ()).await > 0);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn subscriptions_are_bounded_and_owned_by_their_window() {
    let (app, path) = app(Duration::ZERO).await;
    let live = app.get::<LiveRegistry>();
    let mut ids = Vec::new();
    for _ in 0..elyra::live::DEFAULT_LIMIT {
        ids.push(
            live.subscribe("w1", "teams_count", &[0x90])
                .await
                .unwrap()
                .id,
        );
    }
    let over = live.subscribe("w1", "teams_count", &[0x90]).await;
    assert!(over.err().unwrap().to_string().contains("limit"));
    assert!(
        live.subscribe("w2", "teams_count", &[0x90]).await.is_ok(),
        "the limit is per window"
    );

    assert!(
        !live.unsubscribe("w2", &ids[0]),
        "another window can't end it"
    );
    assert!(live.unsubscribe("w1", &ids[0]));
    assert!(!live.unsubscribe("w1", &ids[0]), "already gone");

    // Dropping a TestApp handle ends its subscription.
    let before = live.len();
    drop(app.live::<i64>("teams_count", ()).await);
    assert_eq!(live.len(), before);
    let _ = std::fs::remove_file(path);
}
