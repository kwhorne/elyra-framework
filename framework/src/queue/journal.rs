//! The database journal behind a durable [`Queue`](super::Queue).
//!
//! Two tables in the app's own database — Laravel's `jobs` and `failed_jobs`:
//!
//! * `elyra_jobs` — every job from push until it succeeds or finally fails.
//!   A retry updates the row's attempt and `available_at` in place.
//! * `elyra_failed_jobs` — jobs that exhausted their attempts, kept until
//!   `retry_failed` or `clear_failed`.
//!
//! Both are created on first use (`CREATE TABLE IF NOT EXISTS`), so there is no
//! migration to run. The very first operation also snapshots the rows a previous
//! run left behind; [`Journal::recovered`] hands that snapshot to the queue once
//! the app has booted. Because the snapshot is taken before this process has
//! inserted anything, a job pushed during this run is never delivered twice.

use std::sync::Arc;

use elyra_db::model::placeholder;
use elyra_db::sqlx::{self, Row};
use elyra_db::{Database, Driver};
use parking_lot::Mutex;
use serde_json::Value;
use tokio::sync::OnceCell;

use super::{FailedJob, Journal, JournalFuture, Recovered, StoredJob};

/// How many failed jobs the recovered snapshot carries into memory. The table
/// itself keeps them all.
const RECOVERED_FAILED: i64 = 100;

pub(crate) struct DbJournal {
    db: Arc<Database>,
    ready: OnceCell<()>,
    snapshot: Mutex<Option<Recovered>>,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("queue journal: {e}")
}

impl DbJournal {
    pub(crate) fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            ready: OnceCell::new(),
            snapshot: Mutex::new(None),
        }
    }

    fn ph(&self, n: usize) -> String {
        placeholder(self.db.driver(), n)
    }

    /// Create the tables and take the startup snapshot — exactly once, before
    /// any other operation touches the tables.
    async fn ready(&self) -> Result<(), String> {
        self.ready
            .get_or_try_init(|| async {
                for ddl in ddl(self.db.driver()) {
                    sqlx::raw_sql(ddl)
                        .execute(self.db.pool())
                        .await
                        .map_err(err)?;
                }
                let pending = self.load_pending().await?;
                let failed = self.load_failed().await?;
                *self.snapshot.lock() = Some(Recovered { pending, failed });
                Ok::<(), String>(())
            })
            .await
            .map(|_| ())
    }

    async fn load_pending(&self) -> Result<Vec<StoredJob>, String> {
        let rows = sqlx::query(
            "SELECT id, job, payload, attempt, available_at FROM elyra_jobs ORDER BY id",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(err)?;
        rows.iter()
            .map(|row| {
                let payload: String = row.try_get("payload").map_err(err)?;
                Ok(StoredJob {
                    id: row.try_get("id").map_err(err)?,
                    name: row.try_get("job").map_err(err)?,
                    payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                    attempt: row.try_get::<i64, _>("attempt").map_err(err)? as u32,
                    available_at_ms: row.try_get::<i64, _>("available_at").map_err(err)? as u64,
                })
            })
            .collect()
    }

    async fn load_failed(&self) -> Result<Vec<(i64, FailedJob)>, String> {
        let sql = format!(
            "SELECT id, job, payload, error, attempts, failed_at FROM elyra_failed_jobs \
             ORDER BY id DESC LIMIT {RECOVERED_FAILED}"
        );
        let rows = sqlx::query(sqlx::AssertSqlSafe(sql))
            .fetch_all(self.db.pool())
            .await
            .map_err(err)?;
        let mut failed = rows
            .iter()
            .map(|row| {
                let payload: String = row.try_get("payload").map_err(err)?;
                Ok((
                    row.try_get::<i64, _>("id").map_err(err)?,
                    FailedJob {
                        job: row.try_get("job").map_err(err)?,
                        payload: serde_json::from_str(&payload).unwrap_or(Value::Null),
                        error: row.try_get("error").map_err(err)?,
                        attempts: row.try_get::<i64, _>("attempts").map_err(err)? as u32,
                        failed_at: row.try_get::<i64, _>("failed_at").map_err(err)? as u64,
                    },
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        failed.reverse(); // oldest first, like the in-memory history
        Ok(failed)
    }

    /// `INSERT … RETURNING id` where the driver has it, `last_insert_id` on MySQL.
    async fn insert_returning_id<'c, E>(
        &self,
        executor: E,
        sql: String,
        args: sqlx::any::AnyArguments,
    ) -> Result<i64, String>
    where
        E: sqlx::Executor<'c, Database = sqlx::Any>,
    {
        match self.db.driver() {
            Driver::MySql => {
                let result = sqlx::query_with(sqlx::AssertSqlSafe(sql), args)
                    .execute(executor)
                    .await
                    .map_err(err)?;
                result
                    .last_insert_id()
                    .ok_or_else(|| err("MySQL returned no insert id"))
            }
            _ => {
                let row =
                    sqlx::query_with(sqlx::AssertSqlSafe(format!("{sql} RETURNING id")), args)
                        .fetch_one(executor)
                        .await
                        .map_err(err)?;
                row.try_get::<i64, _>("id").map_err(err)
            }
        }
    }
}

fn bind<'t, T>(args: &mut sqlx::any::AnyArguments, value: T) -> Result<(), String>
where
    T: sqlx::Encode<'t, sqlx::Any> + sqlx::Type<sqlx::Any>,
{
    sqlx::Arguments::add(args, value).map_err(err)
}

fn now_ms() -> u64 {
    super::now_ms()
}

impl Journal for DbJournal {
    fn insert<'a>(
        &'a self,
        job: &'a str,
        payload: &'a Value,
        available_at_ms: u64,
    ) -> JournalFuture<'a, i64> {
        Box::pin(async move {
            self.ready().await?;
            let mut args = sqlx::any::AnyArguments::default();
            bind(&mut args, job.to_string())?;
            bind(&mut args, payload.to_string())?;
            bind(&mut args, 1i64)?;
            bind(&mut args, available_at_ms as i64)?;
            bind(&mut args, now_ms() as i64)?;
            let sql = format!(
                "INSERT INTO elyra_jobs (job, payload, attempt, available_at, created_at) \
                 VALUES ({}, {}, {}, {}, {})",
                self.ph(1),
                self.ph(2),
                self.ph(3),
                self.ph(4),
                self.ph(5)
            );
            self.insert_returning_id(self.db.pool(), sql, args).await
        })
    }

    fn reschedule(&self, id: i64, attempt: u32, available_at_ms: u64) -> JournalFuture<'_, ()> {
        Box::pin(async move {
            self.ready().await?;
            let sql = format!(
                "UPDATE elyra_jobs SET attempt = {}, available_at = {} WHERE id = {}",
                self.ph(1),
                self.ph(2),
                self.ph(3)
            );
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(attempt as i64)
                .bind(available_at_ms as i64)
                .bind(id)
                .execute(self.db.pool())
                .await
                .map_err(err)?;
            Ok(())
        })
    }

    fn complete(&self, id: i64) -> JournalFuture<'_, ()> {
        Box::pin(async move {
            self.ready().await?;
            let sql = format!("DELETE FROM elyra_jobs WHERE id = {}", self.ph(1));
            sqlx::query(sqlx::AssertSqlSafe(sql))
                .bind(id)
                .execute(self.db.pool())
                .await
                .map_err(err)?;
            Ok(())
        })
    }

    fn fail<'a>(&'a self, id: Option<i64>, failed: &'a FailedJob) -> JournalFuture<'a, i64> {
        Box::pin(async move {
            self.ready().await?;
            // Move, don't copy: the job row and the failed row change together.
            let mut tx = self.db.begin().await.map_err(err)?;
            if let Some(id) = id {
                let sql = format!("DELETE FROM elyra_jobs WHERE id = {}", self.ph(1));
                sqlx::query(sqlx::AssertSqlSafe(sql))
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(err)?;
            }
            let mut args = sqlx::any::AnyArguments::default();
            bind(&mut args, failed.job.clone())?;
            bind(&mut args, failed.payload.to_string())?;
            bind(&mut args, failed.error.clone())?;
            bind(&mut args, failed.attempts as i64)?;
            bind(&mut args, failed.failed_at as i64)?;
            let sql = format!(
                "INSERT INTO elyra_failed_jobs (job, payload, error, attempts, failed_at) \
                 VALUES ({}, {}, {}, {}, {})",
                self.ph(1),
                self.ph(2),
                self.ph(3),
                self.ph(4),
                self.ph(5)
            );
            let failed_id = self.insert_returning_id(&mut *tx, sql, args).await?;
            tx.commit().await.map_err(err)?;
            Ok(failed_id)
        })
    }

    fn forget_failed(&self, ids: Option<Vec<i64>>) -> JournalFuture<'_, ()> {
        Box::pin(async move {
            self.ready().await?;
            match ids {
                None => {
                    sqlx::query("DELETE FROM elyra_failed_jobs")
                        .execute(self.db.pool())
                        .await
                        .map_err(err)?;
                }
                Some(ids) if ids.is_empty() => {}
                Some(ids) => {
                    let phs: Vec<String> = (1..=ids.len()).map(|i| self.ph(i)).collect();
                    let sql = format!(
                        "DELETE FROM elyra_failed_jobs WHERE id IN ({})",
                        phs.join(", ")
                    );
                    let mut query = sqlx::query(sqlx::AssertSqlSafe(sql));
                    for id in ids {
                        query = query.bind(id);
                    }
                    query.execute(self.db.pool()).await.map_err(err)?;
                }
            }
            Ok(())
        })
    }

    fn recovered(&self) -> JournalFuture<'_, Recovered> {
        Box::pin(async move {
            self.ready().await?;
            Ok(self.snapshot.lock().take().unwrap_or_default())
        })
    }
}

/// `CREATE TABLE IF NOT EXISTS` for both tables, per driver.
fn ddl(driver: Driver) -> [&'static str; 2] {
    match driver {
        Driver::MySql => [
            "CREATE TABLE IF NOT EXISTS elyra_jobs (\
                id BIGINT AUTO_INCREMENT PRIMARY KEY, job VARCHAR(255) NOT NULL, \
                payload LONGTEXT NOT NULL, attempt BIGINT NOT NULL, \
                available_at BIGINT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_failed_jobs (\
                id BIGINT AUTO_INCREMENT PRIMARY KEY, job VARCHAR(255) NOT NULL, \
                payload LONGTEXT NOT NULL, error LONGTEXT NOT NULL, \
                attempts BIGINT NOT NULL, failed_at BIGINT NOT NULL)",
        ],
        Driver::Postgres => [
            "CREATE TABLE IF NOT EXISTS elyra_jobs (\
                id BIGSERIAL PRIMARY KEY, job TEXT NOT NULL, payload TEXT NOT NULL, \
                attempt BIGINT NOT NULL, available_at BIGINT NOT NULL, created_at BIGINT NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_failed_jobs (\
                id BIGSERIAL PRIMARY KEY, job TEXT NOT NULL, payload TEXT NOT NULL, \
                error TEXT NOT NULL, attempts BIGINT NOT NULL, failed_at BIGINT NOT NULL)",
        ],
        Driver::Sqlite => [
            "CREATE TABLE IF NOT EXISTS elyra_jobs (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, job TEXT NOT NULL, payload TEXT NOT NULL, \
                attempt INTEGER NOT NULL, available_at INTEGER NOT NULL, created_at INTEGER NOT NULL)",
            "CREATE TABLE IF NOT EXISTS elyra_failed_jobs (\
                id INTEGER PRIMARY KEY AUTOINCREMENT, job TEXT NOT NULL, payload TEXT NOT NULL, \
                error TEXT NOT NULL, attempts INTEGER NOT NULL, failed_at INTEGER NOT NULL)",
        ],
    }
}
