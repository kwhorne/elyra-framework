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
    }
}
