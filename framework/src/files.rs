//! A file the user chose (RFC 0009): what a dialog or a drop hands the
//! frontend, and what a command takes from it.
//!
//! The frontend never names a file by itself. When the user picks one in a
//! dialog or drops one on a window, the shell grants it for this run and
//! hands the frontend a [`ChosenFile`]: an opaque id, with the path and name
//! for display. A command argument of type `ChosenFile` is decoded from that
//! id alone, so a path the user didn't choose doesn't decode, and a path the
//! frontend changed is ignored.
//!
//! ```ignore
//! #[command(can = "customers.update")]
//! async fn customers_attach(ctx: Ctx, id: i64, file: ChosenFile) -> elyra::Result<Attachment> {
//!     Ok(ctx.get::<Backend>()
//!         .post(format!("/api/customers/{id}/attachments"))
//!         .attach("file", &file)
//!         .json()
//!         .await?)
//! }
//! ```
//!
//! Rust code isn't limited: [`ChosenFile::from_path`] grants a file the app
//! knows itself, as do [`for_saving`](ChosenFile::for_saving) and
//! [`folder`](ChosenFile::folder). Grants last for the run and are never
//! stored.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use parking_lot::Mutex;
use serde::{Deserialize, Deserializer, Serialize};

/// The most grants kept; the oldest go first.
const MAX_GRANTS: usize = 4096;

/// How the user chose it, which is what it may be used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FileChoice {
    /// Picked in an open dialog, or dropped: it may be read.
    Open,
    /// Picked in a save dialog: it may be written, replacing what's there.
    Save,
    /// A folder picked in an open dialog: a new file may be written in it.
    Folder,
    /// Opened from the OS (RFC 0052): double-clicked, dropped on the app's
    /// icon, or named on its command line. It may be read, and saved back to.
    Document,
}

impl FileChoice {
    /// Whether a file chosen this way may be read: opened, dropped, or a
    /// document the OS opened.
    pub fn readable(self) -> bool {
        matches!(self, FileChoice::Open | FileChoice::Document)
    }

    /// Whether a file chosen this way may be written: chosen to save to, or
    /// a document the OS opened, saved back.
    pub fn writable(self) -> bool {
        matches!(self, FileChoice::Save | FileChoice::Document)
    }
}

/// A file (or folder) the user chose in this run. Decoded from the
/// frontend only when the shell granted it; see the [module](self).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct ChosenFile {
    /// The grant: random, and meaningless after the run.
    id: String,
    path: String,
    name: String,
    kind: FileChoice,
}

impl ChosenFile {
    /// A file the app itself may read, granted so it can go to the frontend
    /// and come back, and to stand in for the user's choice in tests.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self::grant(path.into(), FileChoice::Open)
    }

    /// A path the app may write, as if chosen in a save dialog.
    pub fn for_saving(path: impl Into<PathBuf>) -> Self {
        Self::grant(path.into(), FileChoice::Save)
    }

    /// A document the app may read and save back to, as if the OS had
    /// opened it with the app (RFC 0052).
    pub fn document(path: impl Into<PathBuf>) -> Self {
        Self::grant(path.into(), FileChoice::Document)
    }

    /// A folder a new file may be written in, as if chosen in a dialog.
    pub fn folder(path: impl Into<PathBuf>) -> Self {
        Self::grant(path.into(), FileChoice::Folder)
    }

    pub(crate) fn grant(path: PathBuf, kind: FileChoice) -> Self {
        let id = crate::security::random_token()[..32].to_owned();
        let file = Self {
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            path: path.display().to_string(),
            kind,
            id,
        };
        GRANTS.lock().insert(file.clone(), MAX_GRANTS);
        file
    }

    pub fn path(&self) -> &Path {
        Path::new(&self.path)
    }

    /// The file's name, for display.
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> FileChoice {
        self.kind
    }

    /// The file's size now, if it's a file that's there.
    pub fn size(&self) -> Option<u64> {
        std::fs::metadata(self.path())
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len())
    }

    /// Look a grant up by its id.
    fn granted(id: &str) -> Option<Self> {
        GRANTS.lock().by_id.get(id).cloned()
    }
}

/// Write `bytes` where the user chose to save, from the page (RFC 0060): only
/// to a file chosen in a save dialog in this run, and only once, so the page
/// can't write anywhere the user didn't just pick. The path written.
// The system feature's `/__sys/file.write` is its one caller.
#[cfg_attr(not(feature = "system"), allow(dead_code))]
pub(crate) fn write_saved(id: &str, bytes: &[u8]) -> Result<String, String> {
    let file = {
        let mut grants = GRANTS.lock();
        let file = grants
            .by_id
            .get(id)
            .cloned()
            .ok_or("not a file the user chose in this run")?;
        if file.kind != FileChoice::Save {
            return Err("only a file chosen in a save dialog can be written from the page".into());
        }
        // Once: the grant is spent.
        grants.by_id.remove(id);
        grants.order.retain(|kept| kept != id);
        file
    };
    std::fs::write(file.path(), bytes).map_err(|e| format!("{}: {e}", file.path))?;
    Ok(file.path)
}

impl AsRef<Path> for ChosenFile {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

#[derive(Default)]
struct Grants {
    by_id: HashMap<String, ChosenFile>,
    order: VecDeque<String>,
}

impl Grants {
    fn insert(&mut self, file: ChosenFile, max: usize) {
        self.order.push_back(file.id.clone());
        self.by_id.insert(file.id.clone(), file);
        while self.order.len() > max {
            if let Some(old) = self.order.pop_front() {
                self.by_id.remove(&old);
            }
        }
    }
}

static GRANTS: LazyLock<Mutex<Grants>> = LazyLock::new(Mutex::default);

/// What the frontend sends: the `ChosenFile` it was given (a map, or the
/// compact array Rust encodes), or its id. Only the id is read.
#[derive(Deserialize)]
#[serde(untagged)]
enum Wire {
    Id(String),
    File {
        id: String,
    },
    Compact(
        String,
        serde::de::IgnoredAny,
        serde::de::IgnoredAny,
        serde::de::IgnoredAny,
    ),
}

impl<'de> Deserialize<'de> for ChosenFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = match Wire::deserialize(deserializer)? {
            Wire::Id(id) | Wire::File { id } | Wire::Compact(id, ..) => id,
        };
        Self::granted(&id).ok_or_else(|| {
            serde::de::Error::custom(
                "not a file the user chose in this run: a ChosenFile comes from a dialog or a drop",
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_writes_a_save_grant_once_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("elyra-write-saved-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("chart.png");
        let saving = ChosenFile::for_saving(&target);
        assert_eq!(
            write_saved(&saving.id, b"png").unwrap(),
            target.display().to_string()
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"png");
        assert!(write_saved(&saving.id, b"again").is_err(), "once");
        assert_eq!(std::fs::read(&target).unwrap(), b"png");

        let opened = ChosenFile::from_path(dir.join("read.txt"));
        assert!(write_saved(&opened.id, b"x")
            .unwrap_err()
            .contains("save dialog"));
        assert!(write_saved("nonsense", b"x").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
    use serde_json::json;

    #[test]
    fn a_granted_file_decodes_by_its_id_alone() {
        let file = ChosenFile::from_path("/Users/ada/Desktop/contract.pdf");
        assert_eq!(file.name(), "contract.pdf");
        assert_eq!(file.kind(), FileChoice::Open);
        let sent = serde_json::to_value(&file).unwrap();
        assert_eq!(sent["kind"], "open");
        let back: ChosenFile = serde_json::from_value(sent).unwrap();
        assert_eq!(back, file);
        // Its id alone is enough.
        let by_id: ChosenFile = serde_json::from_value(json!(file.id)).unwrap();
        assert_eq!(by_id.path(), Path::new("/Users/ada/Desktop/contract.pdf"));
    }

    #[test]
    fn a_path_changed_by_the_frontend_is_ignored() {
        let file = ChosenFile::from_path("/Users/ada/Desktop/contract.pdf");
        let forged = json!({ "id": file.id, "path": "/Users/ada/.ssh/id_ed25519", "name": "x", "kind": "save" });
        let back: ChosenFile = serde_json::from_value(forged).unwrap();
        assert_eq!(back.path(), Path::new("/Users/ada/Desktop/contract.pdf"));
        assert_eq!(back.kind(), FileChoice::Open);
    }

    #[test]
    fn a_file_nobody_chose_doesnt_decode() {
        let forged = json!({ "id": "0123456789abcdef0123456789abcdef", "path": "/etc/passwd" });
        let err = serde_json::from_value::<ChosenFile>(forged).unwrap_err();
        assert!(
            err.to_string().contains("not a file the user chose"),
            "{err}"
        );
        assert!(serde_json::from_value::<ChosenFile>(json!("/etc/passwd")).is_err());
        assert!(serde_json::from_value::<ChosenFile>(json!({ "path": "/etc/passwd" })).is_err());
    }

    #[test]
    fn grants_are_unique_and_the_oldest_go_first() {
        let first = ChosenFile::from_path("/tmp/first.txt");
        let again = ChosenFile::from_path("/tmp/first.txt");
        assert_ne!(first.id, again.id);
        // A list of its own, so the other tests' grants stay.
        let mut grants = Grants::default();
        grants.insert(first.clone(), 3);
        for i in 0..3 {
            grants.insert(ChosenFile::from_path(format!("/tmp/{i}.txt")), 3);
        }
        assert!(!grants.by_id.contains_key(&first.id));
        assert_eq!((grants.by_id.len(), grants.order.len()), (3, 3));
        let save = ChosenFile::for_saving("/tmp/out.pdf");
        assert_eq!(
            ChosenFile::granted(&save.id).unwrap().kind(),
            FileChoice::Save
        );
        assert_eq!(ChosenFile::folder("/tmp").kind(), FileChoice::Folder);
        assert_eq!(
            ChosenFile::document("/tmp/a.kver").kind(),
            FileChoice::Document
        );
        assert!(FileChoice::Document.readable() && FileChoice::Document.writable());
        assert!(FileChoice::Open.readable() && !FileChoice::Open.writable());
        assert!(!FileChoice::Save.readable() && FileChoice::Save.writable());
        assert!(!FileChoice::Folder.readable() && !FileChoice::Folder.writable());
    }
}
