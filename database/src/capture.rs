//! Every write seen (RFC 0043): the database says which tables changed, however
//! they were changed — raw SQL, a migration, a sidecar, another process — and
//! the [`ChangeHub`] hears it as it hears the model layer's writes.
//!
//! - **SQLite and MySQL**: a trigger per table and operation bumps the table's
//!   row in `_elyra_changes`; the watcher reads the counts every 250 ms and
//!   reports each table whose count moved.
//! - **Postgres**: a statement trigger per table says its name with
//!   `NOTIFY elyra_changes`; the watcher listens on a connection of its own.
//!
//! Turned on with [`Database::capture_changes`](crate::Database::capture_changes);
//! nothing is installed or watched without it.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sqlx::AnyPool;

use crate::error::Result;
use crate::live::ChangeHub;
use crate::Driver;

/// How often the counts are read (SQLite, MySQL) by default.
pub const DEFAULT_EVERY: Duration = Duration::from_millis(250);

/// Never more often than this.
pub const FASTEST: Duration = Duration::from_millis(50);

/// The table the counts are kept in.
const TABLE: &str = "_elyra_changes";

/// The Postgres channel, and its trigger function.
const CHANNEL: &str = "elyra_changes";
const FUNCTION: &str = "_elyra_notify";

/// The framework's own tables: written only by the app, and never what a live
/// query reads.
const OWN: &[&str] = &[
    "elyra_backend_cache",
    "elyra_outbox",
    "elyra_outbox_failed",
    "elyra_jobs",
    "elyra_failed_jobs",
];

/// What change capture is doing, for the inspector and the app's own checks.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct CaptureStatus {
    /// Turned on.
    pub on: bool,
    /// How often the counts are read (SQLite, MySQL), in milliseconds.
    pub every_ms: u64,
    /// The tables with triggers, as of the last install.
    pub tables: Vec<String>,
    /// When the watcher last looked (SQLite, MySQL) or heard (Postgres), in
    /// milliseconds since the epoch.
    pub last_look_ms: Option<i64>,
    /// The last thing that went wrong, if the watcher is retrying.
    pub error: Option<String>,
}

struct State {
    on: bool,
    every: Duration,
    tables: Vec<String>,
    /// The counts as they were before this install's triggers (SQLite,
    /// MySQL): the first look's baseline, so a write between the triggers and
    /// that look is news, not the starting point.
    baseline: Option<HashMap<String, i64>>,
    last_look: Option<i64>,
    error: Option<String>,
}

/// Shared by every clone of a `Database`.
pub(crate) struct Capture {
    pool: AnyPool,
    driver: Driver,
    url: String,
    hub: Arc<ChangeHub>,
    state: Mutex<State>,
    /// A watcher is running.
    running: AtomicBool,
    /// Bumped when it's turned off, so a running watcher stops.
    generation: AtomicU64,
    /// One install at a time: the watcher's first and a migration's may
    /// overlap, and the later one's list of tables is the one to keep.
    installing: tokio::sync::Mutex<()>,
}

impl Capture {
    pub(crate) fn new(pool: AnyPool, driver: Driver, url: &str, hub: Arc<ChangeHub>) -> Self {
        Self {
            pool,
            driver,
            url: url.to_owned(),
            hub,
            state: Mutex::new(State {
                on: false,
                every: DEFAULT_EVERY,
                tables: Vec::new(),
                baseline: None,
                last_look: None,
                error: None,
            }),
            running: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            installing: tokio::sync::Mutex::new(()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn is_on(&self) -> bool {
        self.state().on
    }

    pub(crate) fn turn_on(&self) {
        self.state().on = true;
    }

    pub(crate) fn every(&self, every: Duration) {
        self.state().every = every.max(FASTEST);
    }

    pub(crate) fn status(&self) -> CaptureStatus {
        let state = self.state();
        CaptureStatus {
            on: state.on,
            every_ms: state.every.as_millis() as u64,
            tables: state.tables.clone(),
            last_look_ms: state.last_look,
            error: state.error.clone(),
        }
    }

    fn looked(&self, error: Option<String>) {
        let mut state = self.state();
        state.last_look = Some(now_ms());
        state.error = error;
    }

    /// Start the watcher in the current runtime, unless one runs. Without a
    /// runtime, the next [`watch`](Self::watch) starts it.
    pub(crate) fn start(self: &Arc<Self>) {
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        let generation = self.generation.load(Ordering::SeqCst);
        handle.spawn(watch(Arc::downgrade(self), generation));
    }

    /// Install what the driver needs for every table there is now. The
    /// tables watched.
    pub(crate) async fn install(&self) -> Result<Vec<String>> {
        let _one = self.installing.lock().await;
        let tables = self.tables().await?;
        match self.driver {
            Driver::Sqlite | Driver::MySql => {
                self.run(&format!(
                    "CREATE TABLE IF NOT EXISTS {TABLE} (table_name VARCHAR(191) PRIMARY KEY, version BIGINT NOT NULL)"
                ))
                .await?;
                // Before the triggers: whatever counts after this is a write
                // the first look reports.
                let baseline = self.counts().await?;
                self.state().baseline = Some(baseline);
            }
            Driver::Postgres => {
                self.run(&format!(
                    "CREATE OR REPLACE FUNCTION {FUNCTION}() RETURNS trigger AS $$ \
                     BEGIN PERFORM pg_notify('{CHANNEL}', TG_TABLE_NAME); RETURN NULL; END \
                     $$ LANGUAGE plpgsql"
                ))
                .await?;
            }
        }
        for table in &tables {
            for statement in self.triggers(table) {
                self.run(&statement).await?;
            }
        }
        self.state().tables = tables.clone();
        Ok(tables)
    }

    /// Take out every trigger, the counts and the function, and stop
    /// watching.
    pub(crate) async fn uninstall(&self) -> Result<()> {
        let _one = self.installing.lock().await;
        {
            let mut state = self.state();
            state.on = false;
            state.tables.clear();
        }
        // A running watcher stops at its next turn; the next `start` makes a new one.
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.running.store(false, Ordering::SeqCst);
        match self.driver {
            Driver::Sqlite => {
                let names: Vec<(String,)> = sqlx::query_as(
                    "SELECT name FROM sqlite_master WHERE type = 'trigger' AND name LIKE '\\_elyra\\_%' ESCAPE '\\'",
                )
                .fetch_all(&self.pool)
                .await?;
                for (name,) in names {
                    self.run(&format!(
                        "DROP TRIGGER IF EXISTS {}",
                        quote(self.driver, &name)
                    ))
                    .await?;
                }
                self.run(&format!("DROP TABLE IF EXISTS {TABLE}")).await?;
            }
            Driver::MySql => {
                let names: Vec<(String,)> = sqlx::query_as(
                    "SELECT CAST(trigger_name AS CHAR) FROM information_schema.triggers \
                     WHERE trigger_schema = DATABASE() AND trigger_name LIKE '\\_elyra\\_%'",
                )
                .fetch_all(&self.pool)
                .await?;
                for (name,) in names {
                    self.run(&format!(
                        "DROP TRIGGER IF EXISTS {}",
                        quote(self.driver, &name)
                    ))
                    .await?;
                }
                self.run(&format!("DROP TABLE IF EXISTS {TABLE}")).await?;
            }
            Driver::Postgres => {
                let tables: Vec<(String,)> = sqlx::query_as(
                    "SELECT DISTINCT CAST(event_object_table AS TEXT) FROM information_schema.triggers \
                     WHERE trigger_name = '_elyra_changes' AND event_object_schema = current_schema()",
                )
                .fetch_all(&self.pool)
                .await?;
                for (table,) in tables {
                    self.run(&format!(
                        "DROP TRIGGER IF EXISTS _elyra_changes ON {}",
                        quote(self.driver, &table)
                    ))
                    .await?;
                }
                self.run(&format!("DROP FUNCTION IF EXISTS {FUNCTION}()"))
                    .await?;
            }
        }
        Ok(())
    }

    async fn run(&self, sql: &str) -> Result<()> {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_owned()))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// The app's tables: not the framework's, the migrations', or the
    /// database's own.
    async fn tables(&self) -> Result<Vec<String>> {
        let sql = match self.driver {
            Driver::Sqlite => "SELECT name FROM sqlite_master WHERE type = 'table'",
            Driver::MySql => {
                "SELECT CAST(table_name AS CHAR) FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_type = 'BASE TABLE'"
            }
            Driver::Postgres => {
                "SELECT CAST(tablename AS TEXT) FROM pg_tables WHERE schemaname = current_schema()"
            }
        };
        let rows: Vec<(String,)> = sqlx::query_as(sql).fetch_all(&self.pool).await?;
        let mut tables: Vec<String> = rows
            .into_iter()
            .map(|(name,)| name)
            .filter(|name| watched(name))
            .collect();
        tables.sort();
        Ok(tables)
    }

    /// The statements that make `table` say its changes, idempotently.
    fn triggers(&self, table: &str) -> Vec<String> {
        let on = quote(self.driver, table);
        let literal = literal(table);
        match self.driver {
            Driver::Sqlite => ["INSERT", "UPDATE", "DELETE"]
                .iter()
                .map(|op| {
                    format!(
                        "CREATE TRIGGER IF NOT EXISTS {name} AFTER {op} ON {on} BEGIN \
                         INSERT INTO {TABLE} (table_name, version) VALUES ({literal}, 1) \
                         ON CONFLICT(table_name) DO UPDATE SET version = version + 1; END",
                        name = quote(self.driver, &trigger_name(table, op)),
                    )
                })
                .collect(),
            Driver::MySql => ["INSERT", "UPDATE", "DELETE"]
                .iter()
                .flat_map(|op| {
                    let name = quote(self.driver, &trigger_name(table, op));
                    [
                        format!("DROP TRIGGER IF EXISTS {name}"),
                        format!(
                            "CREATE TRIGGER {name} AFTER {op} ON {on} FOR EACH ROW \
                             INSERT INTO {TABLE} (table_name, version) VALUES ({literal}, 1) \
                             ON DUPLICATE KEY UPDATE version = version + 1"
                        ),
                    ]
                })
                .collect(),
            Driver::Postgres => vec![
                format!("DROP TRIGGER IF EXISTS _elyra_changes ON {on}"),
                format!(
                    "CREATE TRIGGER _elyra_changes AFTER INSERT OR UPDATE OR DELETE OR TRUNCATE \
                     ON {on} FOR EACH STATEMENT EXECUTE FUNCTION {FUNCTION}()"
                ),
            ],
        }
    }

    /// The counts as they are now (SQLite, MySQL).
    async fn counts(&self) -> Result<HashMap<String, i64>> {
        let rows: Vec<(String, i64)> =
            sqlx::query_as("SELECT table_name, version FROM _elyra_changes")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }

    fn report_all(&self) {
        let tables = self.state().tables.clone();
        for table in tables {
            self.hub.report(&table);
        }
    }
}

/// Whether `table` is the app's, to be watched.
fn watched(table: &str) -> bool {
    !(table.starts_with("_elyra")
        || table.starts_with("sqlite_")
        || table.starts_with("_sqlx")
        || OWN.contains(&table))
}

/// A trigger's name: `_elyra_<table>_<op>`, short enough for MySQL's 64
/// characters (a hash of the table's name when it isn't).
fn trigger_name(table: &str, op: &str) -> String {
    let op = op.to_ascii_lowercase();
    let name = format!("_elyra_{table}_{op}");
    if name.len() <= 60 {
        return name;
    }
    // FNV-1a: stable across runs and builds, unlike the std hasher.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in table.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("_elyra_{hash:016x}_{op}")
}

/// An identifier, quoted for `driver`.
fn quote(driver: Driver, name: &str) -> String {
    match driver {
        Driver::MySql => format!("`{}`", name.replace('`', "``")),
        Driver::Sqlite | Driver::Postgres => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// A string literal.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// How long the watcher waits after something went wrong.
const RETRY: Duration = Duration::from_secs(5);

/// The watcher: installs, then looks (SQLite, MySQL) or listens (Postgres)
/// until the `Database` is gone or capture is turned off.
async fn watch(capture: Weak<Capture>, generation: u64) {
    let alive = |capture: &Weak<Capture>| {
        capture
            .upgrade()
            .filter(|c| c.generation.load(Ordering::SeqCst) == generation && c.is_on())
    };
    // Postgres: listening before the triggers exist, so nothing they say
    // before the loop below listens is lost.
    #[allow(unused_mut)]
    let mut early = None;
    if let Some(c) = alive(&capture).filter(|c| c.driver == Driver::Postgres) {
        if let Ok(mut l) = sqlx::postgres::PgListener::connect(&c.url).await {
            if l.listen(CHANNEL).await.is_ok() {
                early = Some(l);
            }
        }
    }
    // Installed first; retried until it works.
    loop {
        let Some(c) = alive(&capture) else {
            return stopped(&capture, generation);
        };
        match c.install().await {
            Ok(_) => break,
            Err(e) => {
                c.looked(Some(format!("installing: {e}")));
                drop(c);
                tokio::time::sleep(RETRY).await;
            }
        }
    }
    let driver = match alive(&capture) {
        Some(c) => c.driver,
        None => return stopped(&capture, generation),
    };
    match driver {
        Driver::Sqlite | Driver::MySql => look(capture, generation, alive).await,
        Driver::Postgres => listen(capture, generation, alive, early).await,
    }
}

/// This watcher is done: another may start, unless one of a later
/// generation already has.
fn stopped(capture: &Weak<Capture>, generation: u64) {
    if let Some(c) = capture.upgrade() {
        if c.generation.load(Ordering::SeqCst) == generation {
            c.running.store(false, Ordering::SeqCst);
        }
    }
}

async fn look(
    capture: Weak<Capture>,
    generation: u64,
    alive: impl Fn(&Weak<Capture>) -> Option<Arc<Capture>>,
) {
    // What `install` counted before its triggers; without it, the first look
    // only remembers.
    let mut seen: Option<HashMap<String, i64>> =
        alive(&capture).and_then(|c| c.state().baseline.take());
    loop {
        let Some(c) = alive(&capture) else {
            return stopped(&capture, generation);
        };
        let every = c.state().every;
        match c.counts().await {
            Ok(now) => {
                // The first look only remembers: what changed before isn't news.
                if let Some(before) = &seen {
                    for (table, version) in &now {
                        if before.get(table) != Some(version) {
                            c.hub.report(table);
                        }
                    }
                }
                seen = Some(now);
                c.looked(None);
            }
            Err(e) => {
                c.looked(Some(e.to_string()));
                // The table may have gone (`unwatch_tables` from elsewhere, a
                // restored file): install again before the next look.
                let _ = c.install().await;
            }
        }
        drop(c);
        tokio::time::sleep(every).await;
    }
}

async fn listen(
    capture: Weak<Capture>,
    generation: u64,
    alive: impl Fn(&Weak<Capture>) -> Option<Arc<Capture>>,
    early: Option<sqlx::postgres::PgListener>,
) {
    use sqlx::postgres::PgListener;
    // Listening since before the triggers, when that worked.
    let mut listener: Option<PgListener> = early;
    if listener.is_some() {
        if let Some(c) = alive(&capture) {
            c.looked(None);
        }
    }
    // A connection made from here on came after the triggers, or after one
    // was lost: what was said meanwhile is lost, so every table is said.
    let mut connected = true;
    loop {
        let Some(c) = alive(&capture) else {
            return stopped(&capture, generation);
        };
        if listener.is_none() {
            match PgListener::connect(&c.url).await {
                Ok(mut l) => match l.listen(CHANNEL).await {
                    Ok(()) => {
                        listener = Some(l);
                        c.looked(None);
                        // What was said while nobody listened is lost.
                        if std::mem::replace(&mut connected, true) {
                            c.report_all();
                        }
                    }
                    Err(e) => c.looked(Some(e.to_string())),
                },
                Err(e) => c.looked(Some(e.to_string())),
            }
            if listener.is_none() {
                drop(c);
                tokio::time::sleep(RETRY).await;
                continue;
            }
        }
        let l = listener.as_mut().expect("connected above");
        drop(c);
        // A second at most, to notice the Database going away.
        let heard = tokio::time::timeout(Duration::from_secs(1), l.try_recv()).await;
        let Some(c) = alive(&capture) else {
            return stopped(&capture, generation);
        };
        match heard {
            Ok(Ok(Some(notification))) => {
                c.hub.report(notification.payload());
                c.looked(None);
            }
            // The connection was lost and made again: say every table.
            Ok(Ok(None)) => c.report_all(),
            Ok(Err(e)) => {
                c.looked(Some(e.to_string()));
                listener = None;
            }
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_fit_and_quote() {
        assert_eq!(trigger_name("orders", "INSERT"), "_elyra_orders_insert");
        let long = "a".repeat(64);
        let name = trigger_name(&long, "UPDATE");
        assert!(name.len() <= 64, "{name}");
        assert_eq!(name, trigger_name(&long, "UPDATE"));
        assert_eq!(quote(Driver::MySql, "we`ird"), "`we``ird`");
        assert_eq!(quote(Driver::Postgres, "we\"ird"), "\"we\"\"ird\"");
        assert_eq!(literal("o'brien"), "'o''brien'");
    }

    #[test]
    fn only_the_app_s_tables() {
        for own in [
            "_elyra_changes",
            "_elyra_migrations",
            "sqlite_sequence",
            "_sqlx_migrations",
            "elyra_outbox",
        ] {
            assert!(!watched(own), "{own}");
        }
        for app in ["orders", "elyra_widgets", "customers"] {
            assert!(watched(app), "{app}");
        }
    }
}
