//! The outbox (RFC 0007 step 2): writes marked `.queue_offline()` that got no
//! answer, kept in the app's database and sent in order once the server is
//! back — each with its own `Idempotency-Key`, so a write that did reach the
//! server isn't made twice.
//!
//! Two tables, created on first use:
//!
//! * `elyra_outbox` — writes waiting to be sent, per user, in order;
//! * `elyra_outbox_failed` — writes the server refused (a `422`, a `403`, a
//!   conflict), until the user retries or discards them.
//!
//! A write never overtakes an earlier one. One the server can't take *now*
//! (no answer, a `5xx`, a `429`) waits, with backoff, and the rest wait behind
//! it; one it refuses moves aside, and the rest go on.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use elyra_db::sqlx::{self, Row};
use elyra_db::Driver;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::OnceCell;

use super::{err, now_ms, Offline};
use crate::backend::{Backend, BackendError};
use crate::http::{Method, Request, Response};

/// The longest a write waits before it's tried again.
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Headers a queued write doesn't keep: the token is the one in the keychain
/// when it's sent, and the rest are set again then.
const NOT_KEPT: &[&str] = &[
    "authorization",
    "x-socket-id",
    "content-type",
    "idempotency-key",
];

/// The outbox's part of [`Offline`].
#[derive(Default)]
pub(crate) struct State {
    ready: OnceCell<()>,
    /// One sender at a time: a write never overtakes an earlier one.
    flushing: tokio::sync::Mutex<()>,
    /// One count at a time: a slow one mustn't overwrite a newer one.
    counting: tokio::sync::Mutex<()>,
    pending: AtomicU32,
    failed: AtomicU32,
    /// A wake-up is scheduled for a write that's backing off.
    waking: AtomicBool,
}

impl State {
    pub(crate) fn counts(&self) -> (u32, u32) {
        (
            self.pending.load(Ordering::SeqCst),
            self.failed.load(Ordering::SeqCst),
        )
    }
}

/// A write, as the outbox keeps it.
pub(crate) struct Entry {
    method: Method,
    /// The path and query, relative to the backend's URL.
    target: String,
    body: Option<String>,
    headers: Vec<(String, String)>,
    key: String,
}

impl Entry {
    pub(crate) fn new(method: Method, path: &str, request: &Request, key: String) -> Self {
        let target = match request.query_string() {
            Some(query) => format!("{path}?{query}"),
            None => path.to_owned(),
        };
        Self {
            method,
            target,
            body: request
                .body_bytes()
                .and_then(|b| String::from_utf8(b.to_vec()).ok()),
            headers: request
                .headers()
                .iter()
                .filter(|(name, _)| !NOT_KEPT.contains(&name.to_ascii_lowercase().as_str()))
                .cloned()
                .collect(),
            key,
        }
    }

    /// The idempotency key it's sent with, every time.
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    pub(crate) fn method(&self) -> Method {
        self.method
    }

    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    pub(crate) fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }

    pub(crate) fn headers(&self) -> &[(String, String)] {
        &self.headers
    }
}

/// A random key, UUID v4-shaped, for `Idempotency-Key`.
pub(crate) fn idempotency_key() -> String {
    let hex = crate::security::random_token();
    let mut chars: Vec<char> = hex.chars().take(32).collect();
    chars[12] = '4';
    chars[16] = ['8', '9', 'a', 'b'][(chars[16].to_digit(16).unwrap_or(0) % 4) as usize];
    let s: String = chars.into_iter().collect();
    format!(
        "{}-{}-{}-{}-{}",
        &s[0..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    )
}

fn method_from(name: &str) -> Option<Method> {
    Some(match name {
        "POST" => Method::Post,
        "PUT" => Method::Put,
        "PATCH" => Method::Patch,
        "DELETE" => Method::Delete,
        "GET" => Method::Get,
        _ => return None,
    })
}

/// Whether a write waits for the network or is refused by the server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum OutboxStatus {
    Pending,
    Failed,
}

/// A write in the outbox, as the frontend lists it.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct OutboxItem {
    pub id: i64,
    pub method: String,
    /// The path and query, relative to the backend.
    pub path: String,
    pub body: Option<Value>,
    pub status: OutboxStatus,
    /// Times it was tried and the server couldn't take it.
    pub attempts: u32,
    /// Why the server refused it.
    pub error: Option<String>,
    /// A `422`'s errors, per field.
    pub errors: Option<Value>,
    /// When it was saved, in milliseconds since the Unix epoch.
    pub created_at: i64,
}

/// A queued write reached the server — dispatched as a domain event:
/// `App::listen(|e: OutboxSent, ctx| …)`. `body` is the server's answer.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct OutboxSent {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub body: Value,
}

/// The server refused a queued write: it waits in the outbox for the user.
#[derive(Clone, Debug, Serialize, specta::Type)]
pub struct OutboxFailed {
    pub id: i64,
    pub method: String,
    pub path: String,
    pub status: u16,
    pub error: String,
}

/// The writes waiting for the network, and those the server refused —
/// resolvable as `ctx.get::<Outbox>()` once the backend works offline.
#[derive(Clone)]
pub struct Outbox {
    offline: Arc<Offline>,
}

fn store_error(e: String) -> BackendError {
    BackendError::Other {
        status: 0,
        message: e,
    }
}

impl Outbox {
    pub(crate) fn new(offline: Arc<Offline>) -> Self {
        Self { offline }
    }

    /// Writes waiting to be sent, oldest first.
    pub async fn pending(&self) -> Vec<OutboxItem> {
        self.offline.items(false).await
    }

    /// Writes the server refused, oldest first.
    pub async fn failed(&self) -> Vec<OutboxItem> {
        self.offline.items(true).await
    }

    /// Both, in the order they were saved.
    pub async fn items(&self) -> Vec<OutboxItem> {
        let mut all = self.pending().await;
        all.extend(self.failed().await);
        all.sort_by_key(|item| item.id);
        all
    }

    /// How many writes wait to be sent, and how many the server refused.
    pub fn counts(&self) -> (u32, u32) {
        self.offline.outbox.counts()
    }

    /// Send a refused write again — with a new `body`, say, once the user has
    /// fixed what the server's `422` said.
    pub async fn retry(&self, id: i64, body: Option<Value>) -> Result<(), BackendError> {
        self.offline.retry(id, body).await?;
        self.offline.spawn_flush();
        Ok(())
    }

    /// Drop a write, pending or refused: it's never sent.
    pub async fn discard(&self, id: i64) -> Result<(), BackendError> {
        self.offline.discard(id).await
    }

    /// Send what's waiting now, without waiting out a backoff.
    pub async fn flush(&self) {
        self.offline.flush(true).await;
    }
}

impl Offline {
    async fn outbox_ready(&self) -> Result<(), String> {
        self.outbox
            .ready
            .get_or_try_init(|| async {
                for ddl in outbox_ddl(self.db.driver()) {
                    sqlx::raw_sql(ddl)
                        .execute(self.db.pool())
                        .await
                        .map_err(err)?;
                }
                Ok::<(), String>(())
            })
            .await
            .map(|_| ())
    }

    /// Writes are waiting: a new one goes behind them. Asked of the table,
    /// not the count the windows see — order can't rest on a count.
    pub(crate) async fn waiting(&self) -> bool {
        let Some(owner) = self.owner() else {
            return false;
        };
        if self.outbox_ready().await.is_err() {
            return false;
        }
        let sql = format!(
            "SELECT id FROM elyra_outbox WHERE owner = {} LIMIT 1",
            self.ph(1)
        );
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(owner)
            .fetch_optional(self.db.pool())
            .await
            .map(|row| row.is_some())
            .unwrap_or(false)
    }

    /// Keep `entry` for later: the answer to the write is `Queued`.
    pub(crate) async fn enqueue(self: &Arc<Self>, entry: Entry) -> Result<Response, BackendError> {
        let Some(owner) = self.owner() else {
            return Err(BackendError::Unreachable(
                "no answer, and no signed-in user to keep the write for".into(),
            ));
        };
        let id = self
            .insert_outbox(&owner, &entry)
            .await
            .map_err(store_error)?;
        crate::info!(
            target: "elyra::offline",
            "{} {} kept in the outbox (#{id})",
            entry.method.as_str(),
            without_query(&entry.target)
        );
        self.refresh().await;
        if !self.is_offline() {
            self.spawn_flush();
        }
        Err(BackendError::Queued(id))
    }

    async fn insert_outbox(&self, owner: &str, entry: &Entry) -> Result<i64, String> {
        self.outbox_ready().await?;
        let headers = serde_json::to_string(&entry.headers).map_err(err)?;
        let now = now_ms();
        let sql = format!(
            "INSERT INTO elyra_outbox (owner, method, target, body, headers, \
             idempotency_key, attempts, available_at, created_at) \
             VALUES ({}, {}, {}, {}, {}, {}, 0, {}, {})",
            self.ph(1),
            self.ph(2),
            self.ph(3),
            self.ph(4),
            self.ph(5),
            self.ph(6),
            self.ph(7),
            self.ph(8)
        );
        let query = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
            .bind(owner.to_owned())
            .bind(entry.method.as_str().to_owned())
            .bind(entry.target.clone())
            .bind(entry.body.clone())
            .bind(headers.clone())
            .bind(entry.key.clone())
            .bind(now)
            .bind(now);
        match self.db.driver() {
            Driver::MySql => {
                let result = query.execute(self.db.pool()).await.map_err(err)?;
                result
                    .last_insert_id()
                    .ok_or_else(|| err("MySQL returned no insert id"))
            }
            _ => {
                let row = sqlx::query(sqlx::AssertSqlSafe(format!("{sql} RETURNING id")))
                    .bind(owner.to_owned())
                    .bind(entry.method.as_str().to_owned())
                    .bind(entry.target.clone())
                    .bind(entry.body.clone())
                    .bind(headers)
                    .bind(entry.key.clone())
                    .bind(now)
                    .bind(now)
                    .fetch_one(self.db.pool())
                    .await
                    .map_err(err)?;
                row.try_get::<i64, _>("id").map_err(err)
            }
        }
    }

    /// Count what's waiting and refused, and tell the windows if it changed.
    pub(crate) async fn refresh(&self) {
        let _counting = self.outbox.counting.lock().await;
        let (pending, failed) = match self.owner() {
            Some(owner) => {
                let count = |table: &'static str| {
                    let owner = owner.clone();
                    async move {
                        let sql = format!(
                            "SELECT COUNT(*) AS n FROM {table} WHERE owner = {}",
                            self.ph(1)
                        );
                        sqlx::query(sqlx::AssertSqlSafe(sql))
                            .bind(owner)
                            .fetch_one(self.db.pool())
                            .await
                            .ok()
                            .and_then(|row| row.try_get::<i64, _>("n").ok())
                            .unwrap_or(0) as u32
                    }
                };
                if self.outbox_ready().await.is_err() {
                    return;
                }
                (
                    count("elyra_outbox").await,
                    count("elyra_outbox_failed").await,
                )
            }
            None => (0, 0),
        };
        let before = self.outbox.counts();
        self.outbox.pending.store(pending, Ordering::SeqCst);
        self.outbox.failed.store(failed, Ordering::SeqCst);
        if before != (pending, failed) {
            self.emit();
        }
    }

    /// Writes waiting or refused, counted now: signing out refuses while
    /// there are any.
    pub(crate) async fn unsent(&self) -> u32 {
        self.refresh().await;
        let (pending, failed) = self.outbox.counts();
        pending + failed
    }

    /// Drop every write of the signed-in user: they chose to sign out anyway.
    pub(crate) async fn discard_all(&self) {
        let Some(owner) = self.owner() else {
            return;
        };
        if self.outbox_ready().await.is_ok() {
            for table in ["elyra_outbox", "elyra_outbox_failed"] {
                let sql = format!("DELETE FROM {table} WHERE owner = {}", self.ph(1));
                if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(owner.clone())
                    .execute(self.db.pool())
                    .await
                {
                    crate::warn!(target: "elyra::offline", "{}", err(e));
                }
            }
        }
        self.refresh().await;
    }

    /// Signed in — the same user again after a `401`, say: carry on sending.
    pub(crate) fn resume(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            this.refresh().await;
            this.flush(false).await;
        });
    }

    async fn items(&self, failed: bool) -> Vec<OutboxItem> {
        let Some(owner) = self.owner() else {
            return Vec::new();
        };
        if self.outbox_ready().await.is_err() {
            return Vec::new();
        }
        let sql = if failed {
            format!(
                "SELECT id, method, target, body, 0 AS attempts, error, errors, created_at \
                 FROM elyra_outbox_failed WHERE owner = {} ORDER BY id",
                self.ph(1)
            )
        } else {
            format!(
                "SELECT id, method, target, body, attempts, created_at \
                 FROM elyra_outbox WHERE owner = {} ORDER BY id",
                self.ph(1)
            )
        };
        let rows = match sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(owner)
            .fetch_all(self.db.pool())
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                crate::warn!(target: "elyra::offline", "{}", err(e));
                return Vec::new();
            }
        };
        rows.iter()
            .filter_map(|row| {
                let body: Option<String> = row.try_get("body").ok()?;
                Some(OutboxItem {
                    id: row.try_get("id").ok()?,
                    method: row.try_get("method").ok()?,
                    path: row.try_get("target").ok()?,
                    body: body.and_then(|b| serde_json::from_str(&b).ok()),
                    status: if failed {
                        OutboxStatus::Failed
                    } else {
                        OutboxStatus::Pending
                    },
                    attempts: row.try_get::<i64, _>("attempts").unwrap_or(0) as u32,
                    error: if failed {
                        row.try_get("error").ok()
                    } else {
                        None
                    },
                    errors: if failed {
                        row.try_get::<Option<String>, _>("errors")
                            .ok()
                            .flatten()
                            .and_then(|e| serde_json::from_str(&e).ok())
                    } else {
                        None
                    },
                    created_at: row.try_get("created_at").ok()?,
                })
            })
            .collect()
    }

    /// A refused write goes back in line, where it was.
    async fn retry(&self, id: i64, body: Option<Value>) -> Result<(), BackendError> {
        let owner = self.owner().ok_or(BackendError::NotFound)?;
        self.outbox_ready().await.map_err(store_error)?;
        let select = format!(
            "SELECT method, target, body, headers, idempotency_key, created_at \
             FROM elyra_outbox_failed WHERE id = {} AND owner = {}",
            self.ph(1),
            self.ph(2)
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(select))
            .bind(id)
            .bind(owner.clone())
            .fetch_optional(self.db.pool())
            .await
            .map_err(|e| store_error(err(e)))?
            .ok_or(BackendError::NotFound)?;
        let get = |name: &str| {
            row.try_get::<String, _>(name)
                .map_err(|e| store_error(err(e)))
        };
        let kept_body: Option<String> = row.try_get("body").map_err(|e| store_error(err(e)))?;
        let body = match body {
            Some(body) => Some(serde_json::to_string(&body).map_err(|e| store_error(err(e)))?),
            None => kept_body,
        };
        let created: i64 = row.try_get("created_at").map_err(|e| store_error(err(e)))?;
        let mut tx = self
            .db
            .pool()
            .begin()
            .await
            .map_err(|e| store_error(err(e)))?;
        let insert = format!(
            "INSERT INTO elyra_outbox (id, owner, method, target, body, headers, \
             idempotency_key, attempts, available_at, created_at) \
             VALUES ({}, {}, {}, {}, {}, {}, {}, 0, {}, {})",
            self.ph(1),
            self.ph(2),
            self.ph(3),
            self.ph(4),
            self.ph(5),
            self.ph(6),
            self.ph(7),
            self.ph(8),
            self.ph(9)
        );
        sqlx::query(sqlx::AssertSqlSafe(insert))
            .bind(id)
            .bind(owner.clone())
            .bind(get("method")?)
            .bind(get("target")?)
            .bind(body)
            .bind(get("headers")?)
            .bind(get("idempotency_key")?)
            .bind(now_ms())
            .bind(created)
            .execute(&mut *tx)
            .await
            .map_err(|e| store_error(err(e)))?;
        let delete = format!("DELETE FROM elyra_outbox_failed WHERE id = {}", self.ph(1));
        sqlx::query(sqlx::AssertSqlSafe(delete))
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| store_error(err(e)))?;
        tx.commit().await.map_err(|e| store_error(err(e)))?;
        self.refresh().await;
        Ok(())
    }

    async fn discard(&self, id: i64) -> Result<(), BackendError> {
        let owner = self.owner().ok_or(BackendError::NotFound)?;
        self.outbox_ready().await.map_err(store_error)?;
        let mut gone = 0;
        for table in ["elyra_outbox", "elyra_outbox_failed"] {
            let sql = format!(
                "DELETE FROM {table} WHERE id = {} AND owner = {}",
                self.ph(1),
                self.ph(2)
            );
            gone += sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .bind(owner.clone())
                .execute(self.db.pool())
                .await
                .map_err(|e| store_error(err(e)))?
                .rows_affected();
        }
        self.refresh().await;
        if gone == 0 {
            return Err(BackendError::NotFound);
        }
        Ok(())
    }

    pub(crate) fn spawn_flush(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move { this.flush(false).await });
    }

    /// Send the oldest write, and the next, until the outbox is empty or the
    /// server can't take one now. `now` sends a write that's backing off too.
    pub(crate) async fn flush(self: &Arc<Self>, now: bool) {
        let _sending = self.outbox.flushing.lock().await;
        let Some(ctx) = self.ctx() else {
            return;
        };
        let Some(backend) = ctx.try_get::<Backend>() else {
            return;
        };
        loop {
            // Signed out, or the token expired (a `401`): the outbox waits for
            // the same user to sign in again.
            if !backend.has_token() {
                return;
            }
            let Some(owner) = self.owner() else {
                return;
            };
            let Some(row) = self.oldest(&owner).await else {
                return;
            };
            // Known offline: the probe says when it's worth trying. Asked
            // now, after the read: a write kept meanwhile was kept *because*
            // there was no answer.
            if self.is_offline() && !now {
                return;
            }
            let wait = row.available_at - now_ms();
            if wait > 0 && !now {
                self.wake_in(Duration::from_millis(wait as u64));
                return;
            }
            let Some(method) = method_from(&row.method) else {
                self.drop_row(row.id).await;
                continue;
            };
            let entry = Entry {
                method,
                target: row.target.clone(),
                body: row.body.clone(),
                headers: serde_json::from_str(&row.headers).unwrap_or_default(),
                key: row.key.clone(),
            };
            let outcome = backend.replay(&entry).send().await;
            match outcome {
                Ok(response) => {
                    let body = response.json::<Value>().unwrap_or(Value::Null);
                    self.sent(&ctx, &row, response.status(), body).await;
                }
                // Gone either way.
                Err(BackendError::NotFound) if method == Method::Delete => {
                    self.sent(&ctx, &row, 404, Value::Null).await;
                }
                // The token is gone: kept, untried, until the user signs in.
                Err(BackendError::Unauthenticated | BackendError::Expired) => {
                    crate::info!(
                        target: "elyra::offline",
                        "the outbox waits for the user to sign in again"
                    );
                    return;
                }
                Err(BackendError::TooManyRequests { retry_after }) => {
                    let wait = retry_after
                        .map(Duration::from_secs)
                        .unwrap_or_else(|| backoff(row.attempts));
                    self.later(&row, wait).await;
                    return;
                }
                Err(BackendError::Server(_) | BackendError::Unreachable(_)) => {
                    self.later(&row, backoff(row.attempts)).await;
                    return;
                }
                Err(refused) => {
                    self.refused(&ctx, &backend, &row, refused).await;
                }
            }
        }
    }

    async fn oldest(&self, owner: &str) -> Option<Row_> {
        self.outbox_ready().await.ok()?;
        let sql = format!(
            "SELECT id, method, target, body, headers, idempotency_key, attempts, \
             available_at, created_at FROM elyra_outbox WHERE owner = {} ORDER BY id LIMIT 1",
            self.ph(1)
        );
        let row = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(owner.to_owned())
            .fetch_optional(self.db.pool())
            .await
            .map_err(|e| crate::warn!(target: "elyra::offline", "{}", err(e)))
            .ok()??;
        Some(Row_ {
            id: row.try_get("id").ok()?,
            owner: owner.to_owned(),
            method: row.try_get("method").ok()?,
            target: row.try_get("target").ok()?,
            body: row.try_get("body").ok()?,
            headers: row.try_get("headers").ok()?,
            key: row.try_get("idempotency_key").ok()?,
            attempts: row.try_get::<i64, _>("attempts").ok()? as u32,
            available_at: row.try_get("available_at").ok()?,
            created_at: row.try_get("created_at").ok()?,
        })
    }

    async fn drop_row(&self, id: i64) {
        let sql = format!("DELETE FROM elyra_outbox WHERE id = {}", self.ph(1));
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(id)
            .execute(self.db.pool())
            .await
        {
            crate::warn!(target: "elyra::offline", "{}", err(e));
        }
        self.refresh().await;
    }

    async fn sent(&self, ctx: &crate::Ctx, row: &Row_, status: u16, body: Value) {
        self.drop_row(row.id).await;
        crate::info!(
            target: "elyra::offline",
            "{} {} sent from the outbox (#{}) → {status}",
            row.method,
            without_query(&row.target),
            row.id
        );
        ctx.dispatch_background(OutboxSent {
            id: row.id,
            method: row.method.clone(),
            path: row.target.clone(),
            status,
            body,
        });
    }

    /// The server can't take it now: try again after `wait`; the rest wait
    /// behind it.
    async fn later(self: &Arc<Self>, row: &Row_, wait: Duration) {
        let sql = format!(
            "UPDATE elyra_outbox SET attempts = {}, available_at = {} WHERE id = {}",
            self.ph(1),
            self.ph(2),
            self.ph(3)
        );
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(sql))
            .bind(i64::from(row.attempts + 1))
            .bind(now_ms() + wait.as_millis() as i64)
            .bind(row.id)
            .execute(self.db.pool())
            .await
        {
            crate::warn!(target: "elyra::offline", "{}", err(e));
        }
        self.wake_in(wait);
    }

    /// The server refused it: aside, for the user, and on with the next.
    async fn refused(&self, ctx: &crate::Ctx, backend: &Backend, row: &Row_, why: BackendError) {
        let (status, errors) = match &why {
            BackendError::Validation(bag) => (422, serde_json::to_string(bag).ok()),
            BackendError::Forbidden(_) => (403, None),
            BackendError::NotFound => (404, None),
            BackendError::Other { status, .. } => (*status, None),
            _ => (0, None),
        };
        let error = why.to_string();
        let moved = async {
            let mut tx = self.db.pool().begin().await.map_err(err)?;
            let insert = format!(
                "INSERT INTO elyra_outbox_failed (id, owner, method, target, body, headers, \
                 idempotency_key, status, error, errors, failed_at, created_at) \
                 VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                self.ph(1),
                self.ph(2),
                self.ph(3),
                self.ph(4),
                self.ph(5),
                self.ph(6),
                self.ph(7),
                self.ph(8),
                self.ph(9),
                self.ph(10),
                self.ph(11),
                self.ph(12)
            );
            sqlx::query(sqlx::AssertSqlSafe(insert))
                .bind(row.id)
                .bind(row.owner.clone())
                .bind(row.method.clone())
                .bind(row.target.clone())
                .bind(row.body.clone())
                .bind(row.headers.clone())
                .bind(row.key.clone())
                .bind(i64::from(status))
                .bind(error.clone())
                .bind(errors)
                .bind(now_ms())
                .bind(row.created_at)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            let delete = format!("DELETE FROM elyra_outbox WHERE id = {}", self.ph(1));
            sqlx::query(sqlx::AssertSqlSafe(delete))
                .bind(row.id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            tx.commit().await.map_err(err)
        }
        .await;
        if let Err(e) = moved {
            crate::warn!(target: "elyra::offline", "{e}");
        }
        self.refresh().await;
        crate::info!(
            target: "elyra::offline",
            "{} {} refused by the server (#{}): {status}",
            row.method,
            without_query(&row.target),
            row.id
        );
        // The user sees the server's version of what they changed.
        backend.touched(&crate::backend::resource_key(&row.target));
        ctx.dispatch_background(OutboxFailed {
            id: row.id,
            method: row.method.clone(),
            path: row.target.clone(),
            status,
            error,
        });
    }

    /// Flush again after `wait` (once: a wake-up already scheduled stands).
    fn wake_in(self: &Arc<Self>, wait: Duration) {
        if self.outbox.waking.swap(true, Ordering::SeqCst) {
            return;
        }
        let this = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if let Some(this) = this.upgrade() {
                this.outbox.waking.store(false, Ordering::SeqCst);
                this.flush(false).await;
            }
        });
    }
}

/// A row of `elyra_outbox`.
struct Row_ {
    id: i64,
    owner: String,
    method: String,
    target: String,
    body: Option<String>,
    headers: String,
    key: String,
    attempts: u32,
    available_at: i64,
    created_at: i64,
}

/// 1 s, 2 s, 4 s … up to 5 minutes.
fn backoff(attempts: u32) -> Duration {
    Duration::from_secs(1u64 << attempts.min(9)).min(MAX_BACKOFF)
}

/// A path for the log: never its query, which may carry what the user typed.
fn without_query(target: &str) -> &str {
    target.split('?').next().unwrap_or(target)
}

fn outbox_ddl(driver: Driver) -> [&'static str; 2] {
    match driver {
        Driver::MySql => [
            "CREATE TABLE IF NOT EXISTS elyra_outbox (\
                id BIGINT AUTO_INCREMENT PRIMARY KEY, owner CHAR(64) NOT NULL, \
                method VARCHAR(10) NOT NULL, target LONGTEXT NOT NULL, body LONGTEXT NULL, \
                headers LONGTEXT NOT NULL, idempotency_key VARCHAR(64) NOT NULL, \
                attempts BIGINT NOT NULL, available_at BIGINT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_outbox_failed (\
                id BIGINT PRIMARY KEY, owner CHAR(64) NOT NULL, method VARCHAR(10) NOT NULL, \
                target LONGTEXT NOT NULL, body LONGTEXT NULL, headers LONGTEXT NOT NULL, \
                idempotency_key VARCHAR(64) NOT NULL, status BIGINT NOT NULL, \
                error LONGTEXT NOT NULL, errors LONGTEXT NULL, failed_at BIGINT NOT NULL, \
                created_at BIGINT NOT NULL)",
        ],
        Driver::Postgres => [
            "CREATE TABLE IF NOT EXISTS elyra_outbox (\
                id BIGSERIAL PRIMARY KEY, owner TEXT NOT NULL, method TEXT NOT NULL, \
                target TEXT NOT NULL, body TEXT NULL, headers TEXT NOT NULL, \
                idempotency_key TEXT NOT NULL, attempts BIGINT NOT NULL, \
                available_at BIGINT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_outbox_failed (\
                id BIGINT PRIMARY KEY, owner TEXT NOT NULL, method TEXT NOT NULL, \
                target TEXT NOT NULL, body TEXT NULL, headers TEXT NOT NULL, \
                idempotency_key TEXT NOT NULL, status BIGINT NOT NULL, error TEXT NOT NULL, \
                errors TEXT NULL, failed_at BIGINT NOT NULL, created_at BIGINT NOT NULL)",
        ],
        Driver::Sqlite => [
            "CREATE TABLE IF NOT EXISTS elyra_outbox (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, owner TEXT NOT NULL, method TEXT NOT NULL, \
                target TEXT NOT NULL, body TEXT NULL, headers TEXT NOT NULL, \
                idempotency_key TEXT NOT NULL, attempts INTEGER NOT NULL, \
                available_at INTEGER NOT NULL, created_at INTEGER NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_outbox_failed (\
                id INTEGER PRIMARY KEY, owner TEXT NOT NULL, method TEXT NOT NULL, \
                target TEXT NOT NULL, body TEXT NULL, headers TEXT NOT NULL, \
                idempotency_key TEXT NOT NULL, status INTEGER NOT NULL, error TEXT NOT NULL, \
                errors TEXT NULL, failed_at INTEGER NOT NULL, created_at INTEGER NOT NULL)",
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idempotency_key_is_a_v4_uuid() {
        let key = idempotency_key();
        assert_eq!(key.len(), 36);
        let parts: Vec<&str> = key.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
        assert!(parts[2].starts_with('4'));
        assert!(matches!(
            parts[3].chars().next(),
            Some('8' | '9' | 'a' | 'b')
        ));
        assert_ne!(idempotency_key(), key);
    }

    #[test]
    fn backoff_doubles_up_to_five_minutes() {
        assert_eq!(backoff(0), Duration::from_secs(1));
        assert_eq!(backoff(3), Duration::from_secs(8));
        assert_eq!(backoff(30), MAX_BACKOFF);
    }
}
