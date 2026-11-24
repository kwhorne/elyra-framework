//! Documents the OS hands the app (RFC 0052): double-clicked in Finder or
//! Explorer, dropped on the Dock icon, chosen in "Open With", or named on the
//! command line.
//!
//! [`App::opens`](crate::App::opens) says which extensions the app takes;
//! `rata bundle` registers the types from `[[bundle.documents]]` so the OS
//! offers the app for them. A file arrives here from macOS's open-documents
//! event, from argv at launch, or from a second launch through
//! single-instance. It's accepted when it's a regular file with one of
//! those extensions, granted as a [`ChosenFile`] of kind
//! [`FileChoice::Document`](crate::files::FileChoice::Document), and handed
//! to the page: queued until `documents.initial()` takes the launch's
//! files, then on `elyra:open-documents`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::files::{ChosenFile, FileChoice};
use crate::Ctx;

/// The event each batch opened after launch goes out on.
pub(crate) const CHANNEL: &str = "elyra:open-documents";

/// [`App::on_open`](crate::App::on_open)'s callback.
pub(crate) type OnOpen = Arc<dyn Fn(&Ctx, Vec<ChosenFile>) + Send + Sync>;

/// What the app opens, and the files waiting for the page.
pub(crate) struct Documents {
    extensions: Vec<String>,
    /// The launch's files, until the page takes them; `None` after.
    waiting: Mutex<Option<Vec<ChosenFile>>>,
    on_open: Option<OnOpen>,
}

impl Documents {
    pub(crate) fn new(extensions: Vec<String>, on_open: Option<OnOpen>) -> Self {
        Self {
            extensions,
            waiting: Mutex::new(Some(Vec::new())),
            on_open,
        }
    }

    /// `path` as a document this app opens: absolute, a regular file there
    /// now, with one of its extensions (any case).
    pub(crate) fn accepts(&self, path: &Path) -> Option<PathBuf> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        if !self.extensions.contains(&ext) {
            return None;
        }
        let path = plain(std::fs::canonicalize(path).ok()?);
        std::fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|_| path)
    }

    /// Grant the accepted ones among `paths` and hand them over: to the
    /// queue before the page has asked, on [`CHANNEL`] after; and to
    /// `on_open`. Returns how many were accepted.
    pub(crate) fn open(&self, ctx: &Ctx, paths: impl IntoIterator<Item = PathBuf>) -> usize {
        let files: Vec<ChosenFile> = paths
            .into_iter()
            .filter_map(|p| self.accepts(&p))
            .map(|p| ChosenFile::grant(p, FileChoice::Document))
            .collect();
        if files.is_empty() {
            return 0;
        }
        let count = files.len();
        if let Some(on_open) = &self.on_open {
            on_open(ctx, files.clone());
        }
        let mut waiting = self.waiting.lock();
        match waiting.as_mut() {
            Some(queue) => queue.extend(files),
            None => {
                drop(waiting);
                if let Some(bus) = ctx.try_get::<crate::EventBus>() {
                    let _ = bus.emit(CHANNEL, &files);
                }
            }
        }
        count
    }

    /// The launch's files, once: what `documents.initial()` returns. Files
    /// opened after this go out on [`CHANNEL`].
    pub(crate) fn initial(&self) -> Vec<ChosenFile> {
        self.waiting.lock().take().unwrap_or_default()
    }
}

/// A canonical path as the user knows it: on Windows, `\\?\C:\…` without
/// the `\\?\` that `canonicalize` adds (a share's `\\?\UNC\…` keeps it).
fn plain(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    if let Some(rest) = path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        if rest.as_bytes().get(1) == Some(&b':') {
            return PathBuf::from(rest);
        }
    }
    path
}

/// The files among this process's arguments: not flags, not a deep link's
/// URL, paths or `file://` URLs (Linux's `%U`), made absolute against the
/// current directory. Whether each is a document the app opens is
/// [`Documents::accepts`]'s to say.
pub(crate) fn in_args(
    args: impl IntoIterator<Item = String>,
    scheme: Option<&str>,
) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_default();
    args.into_iter()
        .filter(|a| !a.is_empty() && !a.starts_with('-'))
        .filter(|a| scheme.is_none_or(|s| !a.starts_with(&format!("{s}://"))))
        .filter_map(|a| {
            if a.starts_with("file://") {
                file_url_path(&a)
            } else if a.contains("://") {
                None
            } else {
                Some(PathBuf::from(a))
            }
        })
        .map(|p| if p.is_absolute() { p } else { cwd.join(p) })
        .collect()
}

/// `file:///home/ada/a%20b.kver` → `/home/ada/a b.kver`; `None` for another
/// host or a broken escape.
pub(crate) fn file_url_path(url: &str) -> Option<PathBuf> {
    let rest = url.strip_prefix("file://")?;
    let path = rest.strip_prefix("localhost").unwrap_or(rest);
    if !path.starts_with('/') {
        return None;
    }
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let path = String::from_utf8(out).ok()?;
    // `file:///C:/Users/…` on Windows.
    #[cfg(windows)]
    let path = path
        .strip_prefix('/')
        .filter(|p| p.as_bytes().get(1) == Some(&b':'))
        .map(str::to_owned)
        .unwrap_or(path);
    Some(PathBuf::from(path))
}

/// What a later launch hands the running app through single-instance: the
/// deep link it was given and the files. Sent as JSON; a plain string (from
/// an app on an Elyra before 0.54, or a launch with only a URL) is the URL.
#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Launch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PathBuf>,
}

impl Launch {
    /// The payload: the URL alone as it always was, JSON when there are
    /// files.
    pub(crate) fn payload(&self) -> String {
        if self.files.is_empty() {
            self.url.clone().unwrap_or_default()
        } else {
            serde_json::to_string(self).unwrap_or_default()
        }
    }

    pub(crate) fn read(payload: &str) -> Self {
        if payload.starts_with('{') {
            if let Ok(launch) = serde_json::from_str::<Launch>(payload) {
                return launch;
            }
        }
        Self {
            url: (!payload.is_empty()).then(|| payload.to_owned()),
            files: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test.
    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("elyra-docs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn docs() -> Documents {
        Documents::new(vec!["kver".into(), "md".into()], None)
    }

    #[test]
    fn only_a_regular_file_with_an_extension_it_opens_is_accepted() {
        let dir = folder("accepts");
        let note = dir.join("Plan.KVER");
        std::fs::write(&note, "{}").unwrap();
        let other = dir.join("plan.txt");
        std::fs::write(&other, "").unwrap();
        let sub = dir.join("folder.kver");
        std::fs::create_dir(&sub).unwrap();
        let d = docs();
        // Canonical, as the user knows it: no verbatim prefix on Windows.
        let accepted = d.accepts(&note).unwrap();
        assert_eq!(accepted, plain(std::fs::canonicalize(&note).unwrap()));
        assert!(!accepted.to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(d.accepts(&other), None);
        assert_eq!(d.accepts(&sub), None);
        assert_eq!(d.accepts(&dir.join("gone.kver")), None);
        assert_eq!(d.accepts(&dir.join("noext")), None);
    }

    #[test]
    fn launch_files_wait_for_the_page_and_later_ones_go_out_as_an_event() {
        let dir = folder("queue");
        let a = dir.join("a.kver");
        let b = dir.join("b.md");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let told = seen.clone();
        let d = Documents::new(
            vec!["kver".into(), "md".into()],
            Some(Arc::new(move |_: &Ctx, files: Vec<ChosenFile>| {
                told.lock()
                    .extend(files.into_iter().map(|f| f.name().to_owned()))
            })),
        );
        let ctx = Ctx::new(Arc::new(crate::Container::new()));
        assert_eq!(d.open(&ctx, [a.clone(), dir.join("c.txt")]), 1);
        let first = d.initial();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].name(), "a.kver");
        assert_eq!(first[0].kind(), FileChoice::Document);
        // Handed out once.
        assert!(d.initial().is_empty());
        // After: not queued (no bus here to emit on), but on_open hears it.
        assert_eq!(d.open(&ctx, [b]), 1);
        assert!(d.initial().is_empty());
        assert_eq!(*seen.lock(), ["a.kver", "b.md"]);
    }

    #[test]
    fn arguments_are_paths_and_file_urls_not_flags_or_links() {
        let cwd = std::env::current_dir().unwrap();
        // An absolute path and `file://` URLs, as each platform writes them.
        #[cfg(not(windows))]
        let (abs, url, url_host, from_url, from_host) = (
            "/abs/e.kver",
            "file:///tmp/a%20b.kver",
            "file://localhost/tmp/c.kver",
            "/tmp/a b.kver",
            "/tmp/c.kver",
        );
        #[cfg(windows)]
        let (abs, url, url_host, from_url, from_host) = (
            r"C:\abs\e.kver",
            "file:///C:/tmp/a%20b.kver",
            "file://localhost/C:/tmp/c.kver",
            "C:/tmp/a b.kver",
            "C:/tmp/c.kver",
        );
        let args = [
            "--mcp",
            "notes://open/1",
            "https://example.com/x.kver",
            url,
            url_host,
            "rel/d.kver",
            abs,
            "",
        ]
        .map(String::from);
        let found = in_args(args, Some("notes"));
        assert_eq!(
            found,
            [
                PathBuf::from(from_url),
                PathBuf::from(from_host),
                cwd.join("rel/d.kver"),
                PathBuf::from(abs),
            ]
        );
        assert_eq!(file_url_path("file://server/share/x"), None);
        assert_eq!(file_url_path("file:///bad%zz"), None);
    }

    #[test]
    fn the_payload_is_the_url_alone_unless_there_are_files() {
        let url_only = Launch {
            url: Some("notes://x".into()),
            files: vec![],
        };
        assert_eq!(url_only.payload(), "notes://x");
        assert_eq!(Launch::read("notes://x"), url_only);
        assert_eq!(Launch::read(""), Launch::default());
        let both = Launch {
            url: Some("notes://x".into()),
            files: vec![PathBuf::from("/tmp/a b.kver")],
        };
        let payload = both.payload();
        assert!(payload.starts_with('{') && !payload.contains('\n'));
        assert_eq!(Launch::read(&payload), both);
        // Not JSON after all: a URL that happens to start with a brace.
        assert_eq!(Launch::read("{oops").url.as_deref(), Some("{oops"));
    }
}
