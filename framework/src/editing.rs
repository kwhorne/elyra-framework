//! Edit it in its own app (RFC 0038): a file on a record, downloaded into a
//! folder of the app's own, opened in the user's default app for it, and
//! sent back to the server every time they save. Behind the `backend` and
//! `system` features, in the container as [`Editings`], and published on
//! `elyra:editing` for the frontend's `editing` store.
//!
//! ```ignore
//! #[command(can = "attachments.update")]
//! async fn attachments_edit(ctx: Ctx, id: i64) -> elyra::Result<Editing> {
//!     Ok(ctx.get::<Backend>()
//!         .edit(format!("/api/attachments/{id}/file"))
//!         .open()
//!         .await?)
//! }
//! ```
//!
//! A save is noticed by looking at the file once a second (its size and
//! modified time), sent once the file is still, and only when its content
//! changed. It's checked against the version the server gave, so someone
//! else's change in the meantime is a conflict, never overwritten. With no
//! answer it waits, and only the last save is sent when the server is back.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backend::{Backend, BackendError, BackendRequest};
use crate::event::EventBus;
use crate::http::{HttpError, Method, Response};

/// The channel the list is published on.
pub const CHANNEL: &str = "elyra:editing";

/// How often the files being edited are looked at.
const EVERY: Duration = Duration::from_secs(1);

/// How often a save that got no answer is tried again.
const RETRY: Duration = Duration::from_secs(15);

/// An editing with no change for this long, and nothing waiting, ends at
/// the next start.
const IDLE: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Each editing's own record, beside its file.
const RECORD: &str = "editing.json";

/// Where a save's copy waits while it's sent.
const SAVED: &str = ".saved";

/// Kinds that run something when they're opened, besides the executables
/// `shell.openExternal` refuses: a shortcut, a web page, a terminal.
const NOT_OPENED: &[&str] = &[
    "url",
    "website",
    "scf",
    "pif",
    "cpl",
    "inf",
    "library-ms",
    "search-ms",
    "searchconnector-ms",
    "settingcontent-ms",
    "webloc",
    "inetloc",
    "fileloc",
    "terminal",
    "tool",
    "workflow",
    "html",
    "htm",
    "xhtml",
    "shtml",
    "mht",
    "mhtml",
    "svg",
    "svgz",
];

/// Where an editing is, as the frontend shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum EditState {
    /// Open in its app; nothing to send.
    Open,
    /// A save is being sent.
    Saving,
    /// The last save reached the server.
    Saved,
    /// A save waits for the server, or for the user to sign in to it.
    Waiting,
    /// The file changed on the server since this copy's version: keep mine
    /// or take theirs.
    Conflict,
    /// The server said no (`403`).
    Denied,
    /// The server refused the save, with its message.
    Failed,
}

/// A file being edited in its own app.
#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Editing {
    pub id: String,
    /// The file's name, as its app shows it.
    pub name: String,
    pub state: EditState,
    /// When the last save reached the server, in milliseconds since 1970.
    pub saved_at: Option<i64>,
    /// Why it's waiting, refused or a conflict.
    pub error: Option<String>,
}

/// What the next save is checked against: the header, and its value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Version {
    header: String,
    value: String,
}

/// An editing, as it's kept beside its file.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    id: String,
    /// The server and the user (and workspace) it was opened as.
    base: String,
    identity: String,
    /// Where it was read from, and where it's saved.
    get: String,
    save: String,
    method: String,
    /// The file's name in its folder.
    name: String,
    version: Option<Version>,
    /// The content last downloaded or sent.
    sent: Option<String>,
    /// The file's size and modified time when last looked at.
    seen: Option<(u64, i128)>,
    state: EditState,
    saved_at: Option<i64>,
    error: Option<String>,
    /// The last change or open, for ending it after seven idle days.
    touched: i64,
    /// A save not yet sent: its content's hash, its copy in `.saved/`.
    waiting: Option<String>,
}

impl Record {
    fn shown(&self) -> Editing {
        Editing {
            id: self.id.clone(),
            name: self.name.clone(),
            state: self.state,
            saved_at: self.saved_at,
            error: self.error.clone(),
        }
    }

    /// A save that hasn't reached the server.
    fn unsent(&self) -> bool {
        self.waiting.is_some()
    }
}

/// An editing while the app runs.
struct Live {
    record: Record,
    folder: PathBuf,
    /// Changed at the last look: sent once it's still.
    changing: bool,
    /// When a waiting save was last tried.
    tried: Option<Instant>,
}

impl Live {
    fn file(&self) -> PathBuf {
        self.folder.join(&self.record.name)
    }

    fn copy(&self, hash: &str) -> PathBuf {
        self.folder.join(SAVED).join(hash)
    }
}

type Opener = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;
type Tokens = Arc<dyn Fn(&str, &str) -> Option<String> + Send + Sync>;

struct Inner {
    /// `<data>/editing`; `None` when the system has no data folder.
    root: Option<PathBuf>,
    live: Mutex<Vec<Live>>,
    bus: Option<EventBus>,
    opener: Mutex<Opener>,
    /// The token of a server and user, active or not (RFC 0022, 0031).
    tokens: OnceLock<Tokens>,
    loaded: AtomicBool,
    started: AtomicBool,
    /// One look at a time.
    looking: tokio::sync::Mutex<()>,
}

/// The files being edited in their own apps.
#[derive(Clone)]
pub struct Editings {
    inner: Arc<Inner>,
}

impl Editings {
    pub(crate) fn new(root: Option<PathBuf>, bus: Option<EventBus>) -> Self {
        Self {
            inner: Arc::new(Inner {
                root,
                live: Mutex::new(Vec::new()),
                bus,
                opener: Mutex::new(Arc::new(|path: &Path| {
                    open::that(path).map_err(|e| e.to_string())
                })),
                tokens: OnceLock::new(),
                loaded: AtomicBool::new(false),
                started: AtomicBool::new(false),
                looking: tokio::sync::Mutex::new(()),
            }),
        }
    }

    /// Open files with `opener` instead of the OS's default app: for tests,
    /// which see what would be opened and open nothing.
    #[doc(hidden)]
    pub fn opening_with(
        &self,
        opener: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static,
    ) {
        *self.inner.opener.lock() = Arc::new(opener);
    }

    /// How a save finds the token of the server and user it was opened as.
    pub(crate) fn tokens_from(
        &self,
        tokens: impl Fn(&str, &str) -> Option<String> + Send + Sync + 'static,
    ) {
        let _ = self.inner.tokens.set(Arc::new(tokens));
    }

    /// The files being edited, oldest first.
    pub fn list(&self) -> Vec<Editing> {
        self.inner
            .live
            .lock()
            .iter()
            .map(|l| l.record.shown())
            .collect()
    }

    /// Where an editing's file is: for an app's own tests and tools. The
    /// page never gets it.
    pub fn path(&self, id: &str) -> Option<PathBuf> {
        self.inner
            .live
            .lock()
            .iter()
            .find(|l| l.record.id == id)
            .map(Live::file)
    }

    /// Open it in its app again.
    pub fn show(&self, id: &str) -> Result<(), String> {
        let path = self.path(id).ok_or_else(|| "no such editing".to_owned())?;
        self.open_path(&path)
    }

    /// Show its folder, in Finder or Explorer.
    pub fn reveal(&self, id: &str) -> Result<(), String> {
        let folder = self
            .inner
            .live
            .lock()
            .iter()
            .find(|l| l.record.id == id)
            .map(|l| l.folder.clone())
            .ok_or_else(|| "no such editing".to_owned())?;
        self.open_path(&folder)
    }

    /// Stop editing it: its folder is removed. Refused with
    /// `PendingChanges` while a save of it hasn't reached the server, unless
    /// `discard`.
    pub fn finish(&self, id: &str, discard: bool) -> Result<(), BackendError> {
        let mut live = self.inner.live.lock();
        let Some(at) = live.iter().position(|l| l.record.id == id) else {
            return Ok(());
        };
        if live[at].record.unsent() && !discard {
            return Err(BackendError::PendingChanges(1));
        }
        let gone = live.remove(at);
        drop(live);
        remove_folder(&gone.folder);
        self.publish();
        Ok(())
    }

    /// Saves of `identity` on `base` that haven't reached the server.
    pub(crate) fn unsent_of(&self, base: &str, identity: &str) -> u32 {
        self.load();
        self.inner
            .live
            .lock()
            .iter()
            .filter(|l| l.record.base == base && l.record.identity == identity && l.record.unsent())
            .count() as u32
    }

    /// End every editing on `base` (or only `identity`'s): signed out.
    pub(crate) fn end_on(&self, base: &str, identity: Option<&str>) {
        self.load();
        self.end_where(|r| r.base == base && identity.is_none_or(|id| r.identity == id));
    }

    /// End the editings of everyone but `owners` (server, identity): another
    /// user signed in.
    pub(crate) fn keep_only(&self, owners: &[(String, String)]) {
        self.load();
        self.end_where(|r| {
            !owners
                .iter()
                .any(|(base, id)| r.base == *base && r.identity == *id)
        });
    }

    fn end_where(&self, ended: impl Fn(&Record) -> bool) {
        let mut gone = Vec::new();
        self.inner.live.lock().retain_mut(|l| {
            if ended(&l.record) {
                gone.push(l.folder.clone());
                false
            } else {
                true
            }
        });
        if gone.is_empty() {
            return;
        }
        for folder in gone {
            remove_folder(&folder);
        }
        self.publish();
    }

    fn open_path(&self, path: &Path) -> Result<(), String> {
        let opener = self.inner.opener.lock().clone();
        opener(path)
    }

    fn publish(&self) {
        if let Some(bus) = &self.inner.bus {
            let _ = bus.emit(CHANNEL, &self.list());
        }
    }

    /// Keep an editing's record beside its file.
    fn keep(&self, id: &str) {
        let found = self
            .inner
            .live
            .lock()
            .iter()
            .find(|l| l.record.id == id)
            .map(|l| (l.folder.join(RECORD), l.record.clone()));
        if let Some((path, record)) = found {
            write_record(&path, &record);
        }
    }

    fn change(&self, id: &str, f: impl FnOnce(&mut Live)) {
        if let Some(live) = self
            .inner
            .live
            .lock()
            .iter_mut()
            .find(|l| l.record.id == id)
        {
            f(live);
        }
        self.keep(id);
        self.publish();
    }

    /// The editings kept from an earlier run, once: one idle for seven days
    /// with nothing waiting ends; one that was being sent waits.
    pub(crate) fn load(&self) {
        if self.inner.loaded.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(root) = &self.inner.root else {
            return;
        };
        let mut found = Vec::new();
        for owner in read_dirs(root) {
            for folder in read_dirs(&owner) {
                let Some(mut record) = std::fs::read(folder.join(RECORD))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
                else {
                    continue;
                };
                let idle = now_ms() - record.touched > IDLE.as_millis() as i64;
                if idle && !record.unsent() {
                    remove_folder(&folder);
                    continue;
                }
                if record.state == EditState::Saving {
                    record.state = EditState::Waiting;
                }
                found.push(Live {
                    record,
                    folder,
                    changing: false,
                    tried: None,
                });
            }
        }
        found.sort_by_key(|l| l.record.touched);
        let any = !found.is_empty();
        self.inner.live.lock().extend(found);
        if any {
            self.publish();
        }
    }

    /// Look at the files once a second, from now on. Needs a Tokio runtime;
    /// without one it starts with the first editing.
    pub(crate) fn start(&self, backend: &Backend) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        self.load();
        // Held weakly, and the backend without its editings: the loop ends
        // with the app.
        let weak = Arc::downgrade(&self.inner);
        let backend = backend.without_editings();
        tokio::spawn(async move {
            loop {
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                Editings { inner }.look(&backend).await;
                tokio::time::sleep(EVERY).await;
            }
        });
    }

    /// Try every waiting save at the next look, without waiting the 15
    /// seconds between tries: for tests.
    #[doc(hidden)]
    pub fn retry_now(&self) {
        for live in self.inner.live.lock().iter_mut() {
            live.tried = None;
        }
    }

    /// Look at every file being edited once: a change is sent once the file
    /// is still, and a save that got no answer is tried again.
    #[doc(hidden)]
    pub async fn look(&self, backend: &Backend) {
        let _one = self.inner.looking.lock().await;
        let ids: Vec<String> = self
            .inner
            .live
            .lock()
            .iter()
            .map(|l| l.record.id.clone())
            .collect();
        for id in ids {
            if let Some(hash) = self.noticed(&id).await {
                self.send(backend, &id, &hash).await;
            }
        }
    }

    /// What's to be sent for `id` now, if anything: a new save, or one that
    /// waited long enough to be tried again. The file is read and copied on
    /// a blocking thread, with the editings unlocked: a large file doesn't
    /// hold up the list, or the runtime.
    async fn noticed(&self, id: &str) -> Option<String> {
        let file = {
            let mut live = self.inner.live.lock();
            let l = live.iter_mut().find(|l| l.record.id == id)?;
            // Further saves wait behind a conflict.
            if l.record.state == EditState::Conflict {
                return None;
            }
            match stat(&l.file()) {
                Some(now) if l.record.seen != Some(now) => {
                    l.record.seen = Some(now);
                    l.changing = true;
                    return None;
                }
                Some(_) if l.changing => {
                    l.changing = false;
                    l.file()
                }
                _ => {
                    // A save that got no answer, tried again now and then.
                    return match (&l.record.waiting, l.record.state) {
                        (Some(hash), EditState::Waiting)
                            if l.tried.is_none_or(|at| at.elapsed() >= RETRY) =>
                        {
                            Some(hash.clone())
                        }
                        _ => None,
                    };
                }
            }
        };

        // Still: read it, and send it if it's not what the server has.
        let hash = {
            let file = file.clone();
            tokio::task::spawn_blocking(move || hash_file(&file))
                .await
                .ok()
                .flatten()
        };
        let copy = {
            let mut live = self.inner.live.lock();
            let l = live.iter_mut().find(|l| l.record.id == id)?;
            if l.record.state == EditState::Conflict {
                return None;
            }
            let Some(hash) = hash.clone() else {
                // Held by its editor: the next look tries again.
                l.record.seen = None;
                return None;
            };
            l.record.touched = now_ms();
            if l.record.sent.as_deref() == Some(hash.as_str()) {
                // An editor that touched it without changing it, or saved
                // what was already sent.
                if let Some(old) = l.record.waiting.take() {
                    let _ = std::fs::remove_file(l.copy(&old));
                    l.record.state = EditState::Saved;
                    l.record.error = None;
                }
                let path = l.folder.join(RECORD);
                let record = l.record.clone();
                drop(live);
                write_record(&path, &record);
                self.publish();
                return None;
            }
            if l.record.waiting.as_deref() == Some(hash.as_str()) {
                return Some(hash);
            }
            (l.copy(&hash), l.folder.clone())
        };
        let (copy, folder) = copy;

        // Only the last save waits: kept as a copy, so the editor's next
        // save doesn't change what's sent.
        let copied = {
            let (file, copy) = (file.clone(), copy.clone());
            tokio::task::spawn_blocking(move || {
                if let Some(parent) = copy.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(file, copy).is_ok()
            })
            .await
            .unwrap_or(false)
        };
        let hash = hash?;
        let mut live = self.inner.live.lock();
        let Some(l) = live.iter_mut().find(|l| l.record.id == id) else {
            // Done with, or signed out of, while it was copied: its folder
            // went, and the copy may have made it again. Its own folder, by
            // its id: nobody else's.
            drop(live);
            remove_folder(&folder);
            return None;
        };
        if !copied {
            // Held by its editor: the next look tries again.
            l.record.seen = None;
            return None;
        }
        if let Some(old) = l.record.waiting.replace(hash.clone()) {
            if old != hash {
                let _ = std::fs::remove_file(l.copy(&old));
            }
        }
        Some(hash)
    }

    /// Send the save `hash` of `id`, checked against its version.
    async fn send(&self, backend: &Backend, id: &str, hash: &str) {
        let Some((record, copy)) = self.with(id, |l| {
            l.record.state = EditState::Saving;
            l.tried = Some(Instant::now());
            (l.record.clone(), l.copy(hash))
        }) else {
            return;
        };
        self.publish();
        let token = self
            .inner
            .tokens
            .get()
            .and_then(|tokens| tokens(&record.base, &record.identity));
        let Some(token) = token else {
            self.change(id, |l| {
                l.record.state = EditState::Waiting;
                l.record.error = Some("waits for you to sign in again".into());
            });
            return;
        };
        let method = if record.method == "post" {
            Method::Post
        } else {
            Method::Put
        };
        let mut request = backend
            .request_as(&record.base, &token, method, record.save.clone())
            .attach_as(
                "file",
                &copy,
                record.name.clone(),
                crate::http::guess_type(Path::new(&record.name)),
            )
            .label(record.name.clone());
        if let Some(version) = &record.version {
            request = request.header(version.header.clone(), version.value.clone());
        }
        let outcome = request.send().await;
        self.answered(id, hash, &copy, outcome);
    }

    fn answered(&self, id: &str, hash: &str, copy: &Path, outcome: Result<Response, BackendError>) {
        self.change(id, |l| {
            // A newer save came while this one was sent: it goes next.
            let newer = l.record.waiting.as_deref() != Some(hash);
            match outcome {
                Ok(response) => {
                    if let Some(version) = version_of(&response) {
                        l.record.version = Some(version);
                    }
                    l.record.sent = Some(hash.to_owned());
                    l.record.saved_at = Some(now_ms());
                    l.record.error = None;
                    if newer {
                        l.record.state = EditState::Waiting;
                        l.tried = None;
                    } else {
                        l.record.waiting = None;
                        l.record.state = EditState::Saved;
                    }
                    let _ = std::fs::remove_file(copy);
                }
                Err(e) => {
                    let (state, error) = match &e {
                        BackendError::Conflict { message, .. } => (
                            EditState::Conflict,
                            Some(if message.is_empty() {
                                "changed on the server since you opened it".into()
                            } else {
                                message.clone()
                            }),
                        ),
                        BackendError::Forbidden(message) => {
                            (EditState::Denied, Some(message.clone()))
                        }
                        BackendError::Unreachable(_)
                        | BackendError::Server(_)
                        | BackendError::TooManyRequests { .. } => {
                            (EditState::Waiting, Some(e.to_string()))
                        }
                        BackendError::Unauthenticated | BackendError::Expired => (
                            EditState::Waiting,
                            Some("waits for you to sign in again".into()),
                        ),
                        _ => (EditState::Failed, Some(e.to_string())),
                    };
                    l.record.state = if newer && state != EditState::Conflict {
                        EditState::Waiting
                    } else {
                        state
                    };
                    if newer {
                        l.tried = None;
                    }
                    l.record.error = error;
                }
            }
        });
    }

    fn with<T>(&self, id: &str, f: impl FnOnce(&mut Live) -> T) -> Option<T> {
        self.inner
            .live
            .lock()
            .iter_mut()
            .find(|l| l.record.id == id)
            .map(f)
    }

    /// A conflict, resolved as **keep mine**: the save is sent again without
    /// the check, replacing the server's version.
    pub async fn keep_mine(&self, backend: &Backend, id: &str) -> Result<(), BackendError> {
        let hash = self
            .with(id, |l| {
                (l.record.state == EditState::Conflict)
                    .then(|| {
                        l.record.version = None;
                        l.record.waiting.clone()
                    })
                    .flatten()
            })
            .flatten()
            .ok_or_else(|| not_a_conflict(id))?;
        self.send(backend, id, &hash).await;
        Ok(())
    }

    /// A conflict, resolved as **take theirs**: the server's version is
    /// downloaded beside the user's file, never over it, and opened; the
    /// editing carries on with it. The user's file stays where it was.
    pub async fn take_theirs(&self, backend: &Backend, id: &str) -> Result<(), BackendError> {
        let (record, folder) = self
            .with(id, |l| {
                (l.record.state == EditState::Conflict)
                    .then(|| (l.record.clone(), l.folder.clone()))
            })
            .flatten()
            .ok_or_else(|| not_a_conflict(id))?;
        let token = self
            .inner
            .tokens
            .get()
            .and_then(|tokens| tokens(&record.base, &record.identity))
            .ok_or(BackendError::Unauthenticated)?;
        let theirs = folder.join(beside(&folder, &record.name));
        let response = backend
            .request_as(&record.base, &token, Method::Get, record.get.clone())
            .label(record.name.clone())
            .save_to_path(theirs.clone())
            .download_answer()
            .await?;
        let name = file_name(&theirs);
        let hash = hash_file(&theirs);
        let seen = stat(&theirs);
        self.change(id, |l| {
            if let Some(old) = l.record.waiting.take() {
                let _ = std::fs::remove_file(l.copy(&old));
            }
            l.record.name = name;
            l.record.version = version_of(&response);
            l.record.sent = hash;
            l.record.seen = seen;
            l.record.state = EditState::Open;
            l.record.error = None;
            l.record.touched = now_ms();
            l.changing = false;
        });
        self.open_path(&theirs).map_err(open_failed)?;
        Ok(())
    }

    /// Download `get` and open it, or open the copy already being edited.
    pub(crate) async fn open(&self, edit: EditRequest) -> Result<Editing, BackendError> {
        let EditRequest {
            backend,
            request,
            get,
            save,
            only,
        } = edit;
        self.load();
        self.start(&backend);
        let base = backend.base_url().ok_or(BackendError::NoServer)?;
        let identity = backend
            .current_identity()
            .ok_or(BackendError::Unauthenticated)?;
        // Already being edited: its copy, with what's unsaved and unsent.
        let open = self
            .inner
            .live
            .lock()
            .iter()
            .find(|l| l.record.base == base && l.record.identity == identity && l.record.get == get)
            .map(|l| (l.record.shown(), l.file()));
        if let Some((shown, file)) = open {
            self.open_path(&file).map_err(open_failed)?;
            return Ok(shown);
        }
        let root = self.inner.root.clone().ok_or_else(|| {
            BackendError::Http(HttpError::File(
                "this system has no folder for the app's data".into(),
            ))
        })?;
        let id = crate::security::random_token()[..16].to_owned();
        let folder = root.join(owner_folder(&base, &identity)).join(&id);
        std::fs::create_dir_all(&folder)
            .map_err(|e| BackendError::Http(HttpError::File(e.to_string())))?;
        let response = match request.save_into(folder.clone()).download_answer().await {
            Ok(response) => response,
            Err(e) => {
                remove_folder(&folder);
                return Err(e);
            }
        };
        let Some(path) = response.saved_to().map(Path::to_path_buf) else {
            remove_folder(&folder);
            return Err(BackendError::Http(HttpError::File(
                "the answer wasn't saved".into(),
            )));
        };
        let name = file_name(&path);
        if let Err(why) = may_open(&name, only.as_deref()) {
            remove_folder(&folder);
            return Err(BackendError::Http(HttpError::File(why)));
        }
        let (method, save) = save.unwrap_or((Method::Put, get.clone()));
        let record = Record {
            id: id.clone(),
            base,
            identity,
            get,
            save,
            method: if method == Method::Post {
                "post"
            } else {
                "put"
            }
            .into(),
            name,
            version: version_of(&response),
            sent: hash_file(&path),
            seen: stat(&path),
            state: EditState::Open,
            saved_at: None,
            error: None,
            touched: now_ms(),
            waiting: None,
        };
        let shown = record.shown();
        write_record(&folder.join(RECORD), &record);
        self.inner.live.lock().push(Live {
            record,
            folder: folder.clone(),
            changing: false,
            tried: None,
        });
        self.publish();
        if let Err(e) = self.open_path(&path) {
            let _ = self.finish(&id, true);
            return Err(open_failed(e));
        }
        Ok(shown)
    }
}

/// `Backend::edit(..)`: a file on the server, to be opened in its own app.
pub struct EditRequest {
    backend: Backend,
    request: BackendRequest,
    get: String,
    save: Option<(Method, String)>,
    only: Option<Vec<String>>,
}

impl EditRequest {
    pub(crate) fn new(backend: Backend, get: String) -> Self {
        let request = backend.get(get.clone()).no_offline();
        Self {
            backend,
            request,
            get,
            save: None,
            only: None,
        }
    }

    /// Where a save is sent: a `PUT` (as a `POST` with `_method`, which is
    /// how PHP takes a file), with the file as `file`. Without it, a save
    /// goes to the path it was read from.
    pub fn save_back(self, path: impl Into<String>) -> Self {
        self.save_back_with(Method::Put, path)
    }

    /// Where a save is sent, and how: `Method::Post` for a server that keeps
    /// each save as a new version.
    pub fn save_back_with(mut self, method: Method, path: impl Into<String>) -> Self {
        self.save = Some((method, path.into()));
        self
    }

    /// Only files of these kinds (`["docx", "xlsx", "pdf"]`) are opened;
    /// another is refused before it's opened. Programs, shortcuts and web
    /// pages are always refused.
    pub fn only<E: Into<String>>(mut self, extensions: impl IntoIterator<Item = E>) -> Self {
        self.only = Some(
            extensions
                .into_iter()
                .map(|e| e.into().trim_start_matches('.').to_ascii_lowercase())
                .collect(),
        );
        self
    }

    /// A header on the download.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request = self.request.header(name, value);
        self
    }

    /// What `transfers` calls the download.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.request = self.request.label(label);
        self
    }

    /// Download it and open it in its app; the same file already being
    /// edited is opened again instead. It's in `editing` from now on.
    pub async fn open(self) -> Result<Editing, BackendError> {
        let editings = self.backend.editings().cloned().ok_or_else(|| {
            BackendError::Http(HttpError::File(
                "editing needs the app: Backend::edit is used from a command".into(),
            ))
        })?;
        editings.open(self).await
    }
}

/// Whether a file of this name may be opened for editing: never a program,
/// a shortcut or a web page; with `only`, only those kinds.
pub(crate) fn may_open(name: &str, only: Option<&[String]>) -> Result<(), String> {
    let extension = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if crate::security::is_executable_path(Path::new(name))
        || NOT_OPENED.contains(&extension.as_str())
    {
        return Err(format!(
            "{name} isn't opened for editing: it could run something"
        ));
    }
    if let Some(only) = only {
        if !only.contains(&extension) {
            return Err(format!("{name} isn't a kind of file this app edits"));
        }
    }
    Ok(())
}

fn not_a_conflict(id: &str) -> BackendError {
    BackendError::Http(HttpError::File(format!("editing {id} isn't a conflict")))
}

fn open_failed(why: String) -> BackendError {
    BackendError::Http(HttpError::File(format!("couldn't open it: {why}")))
}

/// What the next save is checked against, from an answer: `Last-Modified`,
/// else the `ETag`, else a JSON body's `updated_at` (Laravel's model).
fn version_of(response: &Response) -> Option<Version> {
    if let Some(value) = response.header("last-modified") {
        return Some(Version {
            header: "If-Unmodified-Since".into(),
            value: value.to_owned(),
        });
    }
    if let Some(value) = response.header("etag") {
        return Some(Version {
            header: "If-Match".into(),
            value: value.to_owned(),
        });
    }
    let body: serde_json::Value = response.json().ok()?;
    let updated = body
        .get("data")
        .unwrap_or(&body)
        .get("updated_at")?
        .as_str()?;
    Some(Version {
        header: "If-Unmodified-Since".into(),
        value: updated.to_owned(),
    })
}

/// "Contract (from the server).docx", numbered when that's taken.
fn beside(folder: &Path, name: &str) -> String {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let stem = stem
        .strip_suffix(" (from the server)")
        .unwrap_or(stem)
        .to_owned();
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            format!("{stem} (from the server){extension}")
        } else {
            format!("{stem} (from the server {n}){extension}")
        };
        if !folder.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

/// A folder per server and user, named by neither.
fn owner_folder(base: &str, identity: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(base.as_bytes());
    hasher.update([0]);
    hasher.update(identity.as_bytes());
    hex(&hasher.finalize()[..8])
}

fn hash_file(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path).ok()?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let n = file.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Some(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A file's size and modified time, in nanoseconds.
fn stat(path: &Path) -> Option<(u64, i128)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    Some((meta.len(), modified))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn read_dirs(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

/// Write the record through a temporary file, so a crash leaves the old one.
fn write_record(path: &Path, record: &Record) {
    let Ok(bytes) = serde_json::to_vec_pretty(record) else {
        return;
    };
    let temporary = path.with_extension("json.part");
    if std::fs::write(&temporary, bytes).is_ok() {
        let _ = std::fs::rename(&temporary, path);
    }
}

fn remove_folder(folder: &Path) {
    if let Err(e) = std::fs::remove_dir_all(folder) {
        if e.kind() != std::io::ErrorKind::NotFound {
            crate::debug!(target: "elyra::editing", "couldn't remove an editing's folder: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn programs_shortcuts_and_web_pages_are_never_opened() {
        for name in [
            "setup.exe",
            "run.sh",
            "Report.html",
            "site.url",
            "x.webloc",
            "logo.svg",
            "Shortcut.lnk",
            "tool.jar",
        ] {
            assert!(may_open(name, None).is_err(), "{name} was allowed");
        }
        for name in [
            "Contract.docx",
            "Budget.xlsx",
            "scan.pdf",
            "photo.JPG",
            "README",
        ] {
            assert!(may_open(name, None).is_ok(), "{name} was refused");
        }
    }

    #[test]
    fn only_narrows_the_kinds() {
        let only = vec!["docx".to_owned(), "xlsx".to_owned()];
        assert!(may_open("a.docx", Some(&only)).is_ok());
        assert!(may_open("a.XLSX", Some(&only)).is_ok());
        assert!(may_open("a.pdf", Some(&only)).is_err());
        assert!(may_open("a.exe", Some(&["exe".to_owned()])).is_err());
    }

    #[test]
    fn theirs_goes_beside_and_is_numbered() {
        let dir = std::env::temp_dir().join(format!("elyra-beside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(
            beside(&dir, "Contract.docx"),
            "Contract (from the server).docx"
        );
        std::fs::write(dir.join("Contract (from the server).docx"), b"x").unwrap();
        assert_eq!(
            beside(&dir, "Contract.docx"),
            "Contract (from the server 2).docx"
        );
        assert_eq!(
            beside(&dir, "Contract (from the server).docx"),
            "Contract (from the server 2).docx"
        );
        assert_eq!(beside(&dir, "README"), "README (from the server)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_owner_s_folder_names_neither_server_nor_user() {
        let folder = owner_folder("https://crm.example.com", "7");
        assert_eq!(folder.len(), 16);
        assert!(!folder.contains("crm"));
        assert_ne!(folder, owner_folder("https://crm.example.com", "8"));
    }
}
