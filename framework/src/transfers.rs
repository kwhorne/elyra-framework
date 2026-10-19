//! Uploads and downloads to the backend while they happen (RFC 0009): how
//! far each has got, and cancelling one. Behind the `backend` feature, in
//! the container as [`Transfers`], and published on `elyra:transfers` for
//! the frontend's `transfers` store.
//!
//! A [`BackendRequest`](crate::backend::BackendRequest) with a file attached,
//! or a download, is a transfer by itself; `.label(..)` names it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;

use crate::event::EventBus;
use crate::http::{Cancel, HttpError, Progress};

/// The channel the list is published on.
pub const CHANNEL: &str = "elyra:transfers";

/// How long a finished transfer stays in the list.
const KEPT: Duration = Duration::from_secs(60);

/// The most often progress is published.
const EVERY: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    /// Waiting in the outbox for the network.
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
}

/// An upload or download, as the frontend shows it.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
pub struct Transfer {
    pub id: String,
    pub direction: Direction,
    /// The label, or the file's name.
    pub name: String,
    /// Bytes sent or received so far.
    pub sent: u64,
    pub total: Option<u64>,
    pub state: TransferState,
    /// Why it failed.
    pub error: Option<String>,
}

struct Entry {
    transfer: Transfer,
    cancel: Arc<Cancel>,
    finished: Option<Instant>,
}

struct Inner {
    entries: Mutex<Vec<Entry>>,
    bus: Option<EventBus>,
    published: Mutex<Option<Instant>>,
}

/// The transfers running, and those that finished in the last minute.
#[derive(Clone)]
pub struct Transfers {
    inner: Arc<Inner>,
}

impl Transfers {
    pub(crate) fn new(bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                entries: Mutex::new(Vec::new()),
                bus,
                published: Mutex::new(None),
            }),
        }
    }

    /// What's running, queued, or finished in the last minute, oldest first.
    pub fn list(&self) -> Vec<Transfer> {
        let mut entries = self.inner.entries.lock();
        entries.retain(|e| e.finished.is_none_or(|at| at.elapsed() < KEPT));
        entries.iter().map(|e| e.transfer.clone()).collect()
    }

    /// Stop a transfer: the command that started it gets a `cancelled`
    /// error. Whether there was one to stop.
    pub fn cancel(&self, id: &str) -> bool {
        let cancel = self
            .inner
            .entries
            .lock()
            .iter()
            .find(|e| e.transfer.id == id && e.finished.is_none())
            .map(|e| e.cancel.clone());
        match cancel {
            Some(cancel) => {
                cancel.cancel();
                true
            }
            None => false,
        }
    }

    /// A transfer begins.
    pub(crate) fn start(&self, direction: Direction, name: String, total: Option<u64>) -> Handle {
        let cancel = Arc::new(Cancel::default());
        let id = crate::security::random_token()[..16].to_owned();
        self.inner.entries.lock().push(Entry {
            transfer: Transfer {
                id: id.clone(),
                direction,
                name,
                sent: 0,
                total,
                state: TransferState::Running,
                error: None,
            },
            cancel: cancel.clone(),
            finished: None,
        });
        self.publish(true);
        Handle {
            transfers: self.clone(),
            id,
            cancel,
        }
    }

    fn update(&self, id: &str, change: impl FnOnce(&mut Transfer), now: bool) {
        if let Some(entry) = self
            .inner
            .entries
            .lock()
            .iter_mut()
            .find(|e| e.transfer.id == id)
        {
            change(&mut entry.transfer);
            if entry.transfer.state != TransferState::Running
                && entry.transfer.state != TransferState::Queued
            {
                entry.finished.get_or_insert_with(Instant::now);
            }
        }
        self.publish(now);
    }

    /// Publish the list: at once for a change of state, at most every
    /// 100 ms for progress.
    fn publish(&self, now: bool) {
        let Some(bus) = &self.inner.bus else {
            return;
        };
        {
            let mut published = self.inner.published.lock();
            if !now && published.is_some_and(|at| at.elapsed() < EVERY) {
                return;
            }
            *published = Some(Instant::now());
        }
        let _ = bus.emit(CHANNEL, &self.list());
    }
}

/// One transfer, while it runs.
pub(crate) struct Handle {
    transfers: Transfers,
    id: String,
    cancel: Arc<Cancel>,
}

impl Handle {
    pub fn cancel(&self) -> Arc<Cancel> {
        self.cancel.clone()
    }

    /// Report progress (the request's `on_progress`).
    pub fn reporter(&self) -> impl Fn(Progress) + Send + Sync + 'static {
        let transfers = self.transfers.clone();
        let id = self.id.clone();
        move |p: Progress| {
            transfers.update(
                &id,
                |t| {
                    t.sent = p.done;
                    t.total = p.total.or(t.total);
                },
                false,
            );
        }
    }

    pub fn rename(&self, name: String) {
        self.transfers.update(&self.id, |t| t.name = name, true);
    }

    /// It's over: done, failed, or cancelled.
    pub fn finish<T>(self, outcome: &Result<T, HttpError>) {
        self.transfers.update(
            &self.id,
            |t| match outcome {
                Ok(_) => {
                    t.state = TransferState::Done;
                    if let Some(total) = t.total {
                        t.sent = total;
                    }
                }
                Err(HttpError::Cancelled) => t.state = TransferState::Cancelled,
                Err(e) => {
                    t.state = TransferState::Failed;
                    t.error = Some(e.to_string());
                }
            },
            true,
        );
    }

    /// It's over, and the server refused it.
    pub fn refused(self, why: String) {
        self.transfers.update(
            &self.id,
            |t| {
                t.state = TransferState::Failed;
                t.error = Some(why);
            },
            true,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transfer_is_listed_counted_and_kept_a_while() {
        let transfers = Transfers::new(None);
        let handle = transfers.start(Direction::Upload, "contract.pdf".into(), Some(10));
        let report = handle.reporter();
        report(Progress {
            done: 4,
            total: Some(10),
        });
        let listed = transfers.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            (listed[0].sent, listed[0].state),
            (4, TransferState::Running)
        );
        handle.finish(&Ok::<(), HttpError>(()));
        let done = &transfers.list()[0];
        assert_eq!((done.sent, done.state), (10, TransferState::Done));
    }

    #[tokio::test]
    async fn cancelling_stops_only_a_running_one() {
        let transfers = Transfers::new(None);
        let handle = transfers.start(Direction::Download, "export.csv".into(), None);
        let cancel = handle.cancel();
        let id = transfers.list()[0].id.clone();
        assert!(!transfers.cancel("nope"));
        assert!(transfers.cancel(&id));
        cancel.cancelled().await;
        handle.finish(&Err::<(), _>(HttpError::Cancelled));
        assert_eq!(transfers.list()[0].state, TransferState::Cancelled);
        assert!(!transfers.cancel(&id), "it's over");
    }

    #[test]
    fn a_failure_says_why() {
        let transfers = Transfers::new(None);
        let handle = transfers.start(Direction::Upload, "a".into(), None);
        handle.finish(&Err::<(), _>(HttpError::FileChanged("a".into())));
        let failed = &transfers.list()[0];
        assert_eq!(failed.state, TransferState::Failed);
        assert!(failed.error.as_deref().unwrap().contains("changed"));
    }
}
