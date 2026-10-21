//! Working offline (RFC 0007): the backend's connection state, and a copy of
//! the last answer to each `GET`, read when the server can't be reached.
//! Behind the `backend` and `database` features, turned on with
//! [`Backend::offline`](crate::backend::Backend::offline).
//!
//! The copy lives in the app's own database, in `elyra_backend_cache`, created
//! on first use — no migration to run. It's per user: a row is keyed by a hash
//! of the user and the URL, it's cleared when the user signs out, and when
//! someone else signs in.
//!
//! Writes marked `.queue_offline()` that get no answer wait in the
//! [`Outbox`], and are sent in order once the server is back.

mod outbox;
mod uploads;

pub(crate) use outbox::{idempotency_key, Entry};
pub(crate) use uploads::Kept as KeptPart;

/// An upload's parts, if this outbox body is one (RFC 0009).
pub(crate) fn upload_manifest(body: Option<&str>) -> Option<Vec<KeptPart>> {
    uploads::manifest(body).map(|m| m.parts)
}
pub use outbox::{
    Outbox, OutboxFailed, OutboxItem, OutboxSent, OutboxStatus, CHANNEL as OUTBOX_CHANNEL,
};

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use elyra_db::model::placeholder;
use elyra_db::sqlx::{self, Row};
use elyra_db::{Database, Driver};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::sync::OnceCell;

use crate::container::{Ctx, WeakCtx};
use crate::event::EventBus;
use crate::http::{Http, HttpError, Response};

/// The channel the connection state is emitted on.
pub const CHANNEL: &str = "elyra:connection";

/// The largest answer kept: past it, a read isn't copied.
pub(crate) const MAX_ANSWER: usize = 2 * 1024 * 1024;

/// The copy's total size by default; the oldest answers go first past it.
pub(crate) const DEFAULT_TOTAL: usize = 100 * 1024 * 1024;

/// The longest wait between two probes of the health route.
const MAX_PROBE_WAIT: Duration = Duration::from_secs(30);

/// Whether the backend answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Connection {
    Online,
    Offline,
}

/// What `elyra:connection` carries: the state, since when it's offline, and
/// the writes waiting in the outbox.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct ConnectionState {
    pub state: Connection,
    /// When it went offline, in milliseconds since the Unix epoch.
    pub since: Option<i64>,
    /// Writes waiting to be sent.
    pub pending: u32,
    /// Writes the server refused, waiting for the user.
    pub failed: u32,
}

/// What `Backend::offline()` and its options set.
#[derive(Clone, Debug)]
pub(crate) struct Config {
    /// Probed while offline: Laravel's `/up` by default.
    pub health: String,
    /// The copy's total size, in bytes.
    pub total: usize,
    /// Where queued uploads keep their files (RFC 0009); the app's data
    /// folder unless set.
    pub upload_folder: Option<std::path::PathBuf>,
    /// The most a user's queued uploads may keep, in bytes.
    pub upload_limit: u64,
}

/// 500 MB of files waiting to be sent, per user.
pub(crate) const DEFAULT_UPLOAD_LIMIT: u64 = 500 * 1024 * 1024;

impl Default for Config {
    fn default() -> Self {
        Self {
            health: "/up".into(),
            total: DEFAULT_TOTAL,
            upload_folder: None,
            upload_limit: DEFAULT_UPLOAD_LIMIT,
        }
    }
}

type Back = Arc<dyn Fn(&[String]) + Send + Sync>;

/// The connection state and the copy, shared by every clone of a `Backend`.
pub(crate) struct Offline {
    db: Arc<Database>,
    ready: OnceCell<()>,
    config: Config,
    http: Http,
    base: String,
    /// Whose copy it is: a hash of the user's id, once known.
    owner: RwLock<Option<String>>,
    /// `Some(when)` while offline.
    since: RwLock<Option<i64>>,
    probing: AtomicBool,
    bus: Option<EventBus>,
    /// Told the top resource keys read, when the connection comes back.
    back: RwLock<Vec<Back>>,
    /// The top resource key of each path read (`backend:/api`).
    tops: Mutex<HashSet<String>>,
    /// Writes waiting for the network (step 2).
    outbox: outbox::State,
    /// The context `OutboxSent` / `OutboxFailed` are dispatched in.
    ctx: OnceLock<WeakCtx>,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("offline copy: {e}")
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl Offline {
    pub(crate) fn new(
        db: Arc<Database>,
        config: Config,
        http: Http,
        base: String,
        bus: Option<EventBus>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            ready: OnceCell::new(),
            config,
            http,
            base,
            owner: RwLock::new(None),
            since: RwLock::new(None),
            probing: AtomicBool::new(false),
            bus,
            back: RwLock::new(Vec::new()),
            tops: Mutex::new(HashSet::new()),
            outbox: outbox::State::default(),
            ctx: OnceLock::new(),
        })
    }

    /// Hand over the app's context, and send what the last run left behind.
    pub(crate) fn attach(self: &Arc<Self>, ctx: &Ctx) {
        let _ = self.ctx.set(ctx.downgrade());
        let this = self.clone();
        tokio::spawn(async move {
            this.refresh().await;
            this.flush(false).await;
        });
    }

    fn ctx(&self) -> Option<Ctx> {
        self.ctx.get().and_then(WeakCtx::upgrade)
    }

    /// Whose copy and outbox it is.
    pub(crate) fn owner(&self) -> Option<String> {
        self.owner.read().clone()
    }

    fn ph(&self, n: usize) -> String {
        placeholder(self.db.driver(), n)
    }

    async fn ready(&self) -> Result<(), String> {
        self.ready
            .get_or_try_init(|| async {
                sqlx::raw_sql(ddl(self.db.driver()))
                    .execute(self.db.pool())
                    .await
                    .map_err(err)?;
                Ok::<(), String>(())
            })
            .await
            .map(|_| ())
    }

    // -- whose copy -------------------------------------------------------

    /// The signed-in user's id (`GET /api/user`'s `id`), or `None`.
    pub(crate) fn use_identity(&self, identity: Option<&str>) {
        *self.owner.write() = identity.map(|id| hash(&[&self.base, id]));
    }

    /// Forget the current user's copy: they signed out.
    pub(crate) async fn forget_owner(&self) {
        let Some(owner) = self.owner.write().take() else {
            return;
        };
        let sql = format!(
            "DELETE FROM elyra_backend_cache WHERE owner = {}",
            self.ph(1)
        );
        self.run(sql, |q| q.bind(owner)).await;
    }

    /// Forget everyone else's copy: this user signed in.
    pub(crate) async fn forget_others(&self) {
        let Some(owner) = self.owner.read().clone() else {
            return;
        };
        let sql = format!(
            "DELETE FROM elyra_backend_cache WHERE owner <> {}",
            self.ph(1)
        );
        self.run(sql, |q| q.bind(owner)).await;
    }

    async fn run<'q>(
        &self,
        sql: String,
        bind: impl FnOnce(
            sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments>,
        ) -> sqlx::query::Query<'q, sqlx::Any, sqlx::any::AnyArguments>,
    ) {
        let outcome = async {
            self.ready().await?;
            bind(sqlx::query(sqlx::AssertSqlSafe(sql)))
                .execute(self.db.pool())
                .await
                .map_err(err)
        }
        .await;
        if let Err(e) = outcome {
            crate::warn!(target: "elyra::offline", "{e}");
        }
    }

    // -- the copy ---------------------------------------------------------

    /// Keep `response` as the last answer to `url`, for the current user.
    pub(crate) async fn store(&self, url: &str, response: &Response) {
        let Some(owner) = self.owner.read().clone() else {
            return;
        };
        let limit = MAX_ANSWER.min(self.config.total);
        if response.bytes().len() > limit {
            return;
        }
        // JSON is text; anything else isn't kept.
        let Ok(body) = std::str::from_utf8(response.bytes()) else {
            return;
        };
        let id = hash(&[&owner, url]);
        let outcome = async {
            self.ready().await?;
            let delete = format!("DELETE FROM elyra_backend_cache WHERE id = {}", self.ph(1));
            sqlx::query(sqlx::AssertSqlSafe(delete))
                .bind(id.clone())
                .execute(self.db.pool())
                .await
                .map_err(err)?;
            let insert = format!(
                "INSERT INTO elyra_backend_cache (id, owner, status, body, size, stored_at) \
                 VALUES ({}, {}, {}, {}, {}, {})",
                self.ph(1),
                self.ph(2),
                self.ph(3),
                self.ph(4),
                self.ph(5),
                self.ph(6)
            );
            sqlx::query(sqlx::AssertSqlSafe(insert))
                .bind(id)
                .bind(owner)
                .bind(i64::from(response.status()))
                .bind(body.to_owned())
                .bind(body.len() as i64)
                .bind(now_ms())
                .execute(self.db.pool())
                .await
                .map_err(err)?;
            self.trim().await
        }
        .await;
        if let Err(e) = outcome {
            crate::warn!(target: "elyra::offline", "{e}");
        }
    }

    /// Past the total, the oldest answers go.
    async fn trim(&self) -> Result<(), String> {
        let rows = sqlx::query("SELECT id, size FROM elyra_backend_cache ORDER BY stored_at DESC")
            .fetch_all(self.db.pool())
            .await
            .map_err(err)?;
        let mut kept = 0usize;
        let mut over = Vec::new();
        for row in &rows {
            let size: i64 = row.try_get("size").map_err(err)?;
            kept += size.max(0) as usize;
            if kept > self.config.total {
                over.push(row.try_get::<String, _>("id").map_err(err)?);
            }
        }
        for id in over {
            let delete = format!("DELETE FROM elyra_backend_cache WHERE id = {}", self.ph(1));
            sqlx::query(sqlx::AssertSqlSafe(delete))
                .bind(id)
                .execute(self.db.pool())
                .await
                .map_err(err)?;
        }
        Ok(())
    }

    /// The current user's last answer to `url`, marked with when it was kept.
    pub(crate) async fn lookup(&self, url: &str) -> Option<Response> {
        let owner = self.owner.read().clone()?;
        let id = hash(&[&owner, url]);
        let outcome = async {
            self.ready().await?;
            let select = format!(
                "SELECT status, body, stored_at FROM elyra_backend_cache WHERE id = {}",
                self.ph(1)
            );
            let row = sqlx::query(sqlx::AssertSqlSafe(select))
                .bind(id)
                .fetch_optional(self.db.pool())
                .await
                .map_err(err)?;
            let Some(row) = row else {
                return Ok(None);
            };
            let status: i64 = row.try_get("status").map_err(err)?;
            let body: String = row.try_get("body").map_err(err)?;
            let stored: i64 = row.try_get("stored_at").map_err(err)?;
            Ok::<_, String>(Some(Response::copy(
                status as u16,
                body.into_bytes(),
                UNIX_EPOCH + Duration::from_millis(stored.max(0) as u64),
            )))
        }
        .await;
        outcome.unwrap_or_else(|e| {
            crate::warn!(target: "elyra::offline", "{e}");
            None
        })
    }

    // -- the connection ---------------------------------------------------

    /// Re-run what read under `top` when the connection comes back.
    pub(crate) fn seen(&self, top: String) {
        self.tops.lock().insert(top);
    }

    pub(crate) fn on_back(&self, f: impl Fn(&[String]) + Send + Sync + 'static) {
        self.back.write().push(Arc::new(f));
    }

    pub(crate) fn is_offline(&self) -> bool {
        self.since.read().is_some()
    }

    pub(crate) fn state(&self) -> ConnectionState {
        let since = *self.since.read();
        ConnectionState {
            state: if since.is_some() {
                Connection::Offline
            } else {
                Connection::Online
            },
            since,
            pending: self.outbox.counts().0,
            failed: self.outbox.counts().1,
        }
    }

    /// No answer: offline, until the health route (or anything) answers.
    pub(crate) fn went_offline(self: &Arc<Self>) {
        {
            let mut since = self.since.write();
            if since.is_some() {
                return;
            }
            *since = Some(now_ms());
        }
        crate::info!(target: "elyra::offline", "the backend is unreachable: offline");
        self.emit();
        self.probe();
    }

    /// An answer: back online, the outbox is sent, and everything read re-runs.
    pub(crate) fn came_back(self: &Arc<Self>) {
        if self.since.write().take().is_none() {
            return;
        }
        crate::info!(target: "elyra::offline", "the backend answers again: online");
        self.emit();
        self.spawn_flush();
        let back = self.back.read().clone();
        if !back.is_empty() {
            let tops: Vec<String> = self.tops.lock().iter().cloned().collect();
            for back in back {
                back(&tops);
            }
        }
    }

    fn emit(&self) {
        if let Some(bus) = &self.bus {
            let _ = bus.emit(CHANNEL, &self.state());
        }
    }

    /// Ask the health route, backing off from 1 s to 30 s, until it answers —
    /// any answer: a `500` is a server that's there.
    fn probe(self: &Arc<Self>) {
        if self.probing.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak: Weak<Self> = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut wait = Duration::from_secs(1);
            loop {
                tokio::time::sleep(wait).await;
                let Some(this) = weak.upgrade() else {
                    return;
                };
                if !this.is_offline() {
                    this.probing.store(false, Ordering::SeqCst);
                    return;
                }
                let url = format!("{}{}", this.base, this.config.health);
                let answer = this
                    .http
                    .get(&url)
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await;
                if !matches!(answer, Err(HttpError::Unreachable(_))) {
                    this.probing.store(false, Ordering::SeqCst);
                    this.came_back();
                    return;
                }
                wait = (wait * 2).min(MAX_PROBE_WAIT);
            }
        });
    }
}

/// `CREATE TABLE IF NOT EXISTS`, per driver. `owner` and `id` are hashes: the
/// user's id and the URL never reach the table in the clear.
fn ddl(driver: Driver) -> &'static str {
    match driver {
        Driver::MySql => {
            "CREATE TABLE IF NOT EXISTS elyra_backend_cache (\
                id CHAR(64) PRIMARY KEY, owner CHAR(64) NOT NULL, status BIGINT NOT NULL, \
                body LONGTEXT NOT NULL, size BIGINT NOT NULL, stored_at BIGINT NOT NULL)"
        }
        Driver::Postgres => {
            "CREATE TABLE IF NOT EXISTS elyra_backend_cache (\
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, status BIGINT NOT NULL, \
                body TEXT NOT NULL, size BIGINT NOT NULL, stored_at BIGINT NOT NULL)"
        }
        Driver::Sqlite => {
            "CREATE TABLE IF NOT EXISTS elyra_backend_cache (\
                id TEXT PRIMARY KEY, owner TEXT NOT NULL, status INTEGER NOT NULL, \
                body TEXT NOT NULL, size INTEGER NOT NULL, stored_at INTEGER NOT NULL)"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hash_separates_its_parts() {
        assert_ne!(hash(&["ab", "c"]), hash(&["a", "bc"]));
        assert_eq!(hash(&["a"]).len(), 64);
    }
}
