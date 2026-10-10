//! The data side of live queries (RFC 0002): which keys a piece of code
//! *read*, and which keys a write *changed*.
//!
//! - **Reads** are recorded into a task-local read set while code runs inside
//!   [`track`]: every `Query` execution adds its table and joined tables. Code
//!   the model layer can't see declares its own with [`depends_on`].
//! - **Writes** are reported to the [`ChangeHub`] every `Database` clone
//!   shares: the generated `insert` / `update` / `delete`, `Query`'s bulk
//!   writes and the pivot operations report their table once the write is
//!   durable. Inside [`Database::transaction`](crate::Database::transaction)
//!   reports are held and sent on commit; a rollback sends nothing. Raw SQL
//!   reports with [`Database::touch`](crate::Database::touch).
//!
//! A key is a string: a table's key is its name (`customers`), and anything
//! else (`settings:theme`) can be invalidated and depended on the same way.
//! Table names can't contain `:`, so the two never collide.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::error::{Error, Result};

/// How many unread changes a slow subscriber may fall behind by before it's
/// told it lagged (and should treat everything as changed).
const CAPACITY: usize = 1024;

/// The broadcast of changed keys, shared by every clone of a `Database`.
pub struct ChangeHub {
    sender: broadcast::Sender<Arc<str>>,
}

impl Default for ChangeHub {
    fn default() -> Self {
        Self {
            sender: broadcast::channel(CAPACITY).0,
        }
    }
}

impl ChangeHub {
    /// A receiver of every key changed from now on. A receiver that falls
    /// more than a thousand changes behind gets `RecvError::Lagged`: it missed
    /// some, and should treat every key as changed.
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<str>> {
        self.sender.subscribe()
    }

    /// Announce that `key` changed — now, or on commit inside a transaction.
    pub fn report(&self, key: &str) {
        let held = PENDING.try_with(|pending| {
            if let Some(keys) = pending.borrow_mut().as_mut() {
                keys.insert(key.to_owned());
                true
            } else {
                false
            }
        });
        if !held.unwrap_or(false) {
            self.send(key);
        }
    }

    fn send(&self, key: &str) {
        // No receivers is fine: nobody is watching.
        let _ = self.sender.send(Arc::from(key));
    }
}

/// What a tracked run is allowed to do.
#[derive(Default)]
struct Tracking {
    reads: BTreeSet<String>,
    /// `Some(label)`: a live command's run, which must not write.
    read_only: Option<String>,
}

tokio::task_local! {
    static TRACKING: RefCell<Tracking>;
    /// Keys reported inside a transaction, sent on commit.
    static PENDING: RefCell<Option<BTreeSet<String>>>;
}

/// Run `fut`, recording the keys it reads. With `read_only = Some(label)`,
/// a write inside it fails with an error naming `label` — what a live
/// command's re-run uses, since a write there would re-trigger itself.
///
/// Reads on a task spawned from `fut` aren't recorded (the read set is
/// task-local); declare those with [`depends_on`].
pub async fn track<F: Future>(read_only: Option<&str>, fut: F) -> (F::Output, BTreeSet<String>) {
    let tracking = RefCell::new(Tracking {
        reads: BTreeSet::new(),
        read_only: read_only.map(str::to_owned),
    });
    TRACKING
        .scope(tracking, async move {
            let out = fut.await;
            let reads = TRACKING.with(|t| std::mem::take(&mut t.borrow_mut().reads));
            (out, reads)
        })
        .await
}

/// Declare that the running code depends on `key` — a table read with raw
/// SQL, or any other key (`settings:theme`). A no-op outside [`track`].
pub fn depends_on(key: &str) {
    let _ = TRACKING.try_with(|t| {
        t.borrow_mut().reads.insert(key.to_owned());
    });
}

/// Whether the current code runs inside [`track`].
pub fn is_tracking() -> bool {
    TRACKING.try_with(|_| ()).is_ok()
}

/// Refuse a write to `table` (or any other key) inside a read-only tracked
/// run — a live command's re-run. Model writes check this on their own; a
/// write elsewhere (to a server, say) can ask too.
pub fn check_write(table: &str) -> Result<()> {
    let refused = TRACKING
        .try_with(|t| t.borrow().read_only.clone())
        .ok()
        .flatten();
    match refused {
        Some(label) => Err(Error::Query(format!(
            "`{label}` is a live command and must only read, but it wrote to `{table}`"
        ))),
        None => Ok(()),
    }
}

/// Run a transaction body with change reports held back; `commit` decides
/// whether they're sent. Nested transactions join the outer one's buffer.
pub(crate) async fn hold<F, T>(hub: &ChangeHub, fut: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    let nested = PENDING.try_with(|p| p.borrow().is_some()).unwrap_or(false);
    if nested {
        return fut.await;
    }
    PENDING
        .scope(RefCell::new(Some(BTreeSet::new())), async move {
            let out = fut.await;
            let keys = PENDING.with(|p| p.borrow_mut().take().unwrap_or_default());
            if out.is_ok() {
                for key in &keys {
                    hub.send(key);
                }
            }
            out
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reads_are_recorded_only_inside_track() {
        depends_on("ignored"); // outside: a no-op, not a panic
        assert!(!is_tracking());
        let ((), reads) = track(None, async {
            assert!(is_tracking());
            depends_on("customers");
            depends_on("settings:theme");
            depends_on("customers");
        })
        .await;
        assert_eq!(
            reads.into_iter().collect::<Vec<_>>(),
            ["customers", "settings:theme"]
        );
    }

    #[tokio::test]
    async fn a_read_only_run_refuses_writes() {
        assert!(check_write("customers").is_ok());
        let (refused, _) = track(Some("customers_index"), async { check_write("customers") }).await;
        let err = refused.unwrap_err().to_string();
        assert!(
            err.contains("`customers_index` is a live command") && err.contains("`customers`"),
            "{err}"
        );
        let (allowed, _) = track(None, async { check_write("customers") }).await;
        assert!(allowed.is_ok(), "tracking alone doesn't forbid writes");
    }

    #[tokio::test]
    async fn reports_inside_a_transaction_wait_for_commit() {
        let hub = ChangeHub::default();
        let mut rx = hub.subscribe();

        hub.report("now");
        assert_eq!(&*rx.try_recv().unwrap(), "now");

        let out: Result<()> = hold(&hub, async {
            hub.report("teams");
            hub.report("teams");
            assert!(rx.try_recv().is_err(), "held until commit");
            Ok(())
        })
        .await;
        out.unwrap();
        assert_eq!(&*rx.try_recv().unwrap(), "teams", "sent once, on commit");
        assert!(rx.try_recv().is_err());

        let rolled: Result<()> = hold(&hub, async {
            hub.report("customers");
            Err(Error::Query("boom".into()))
        })
        .await;
        assert!(rolled.is_err());
        assert!(rx.try_recv().is_err(), "a rollback sends nothing");
    }

    #[tokio::test]
    async fn a_nested_transaction_joins_the_outer_one() {
        let hub = ChangeHub::default();
        let mut rx = hub.subscribe();
        let out: Result<()> = hold(&hub, async {
            hold(&hub, async {
                hub.report("inner");
                Ok(())
            })
            .await?;
            assert!(
                rx.try_recv().is_err(),
                "still held by the outer transaction"
            );
            Ok(())
        })
        .await;
        out.unwrap();
        assert_eq!(&*rx.try_recv().unwrap(), "inner");
    }
}
