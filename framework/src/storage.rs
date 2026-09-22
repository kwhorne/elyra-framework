//! An ergonomic filesystem **storage** facade — the desktop-side counterpart to
//! Laravel's `Storage::` (local disk). One ecosystem: the same
//! `put` / `get` / `exists` / `delete` / `url` surface as the Askr/Laravel side,
//! over a local directory here.
//!
//! Every path is **jailed** to the disk root: `..`, absolute paths, and drive
//! prefixes are rejected, so a caller can't read or write outside the root.
//!
//! Add [`StorageProvider`] to bind it, then resolve `ctx.get::<Storage>()` from
//! commands or reach it from the frontend via `@elyra/runtime`'s `storage`.

use std::io::{self, ErrorKind};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// A local filesystem disk rooted at a directory.
#[derive(Clone)]
pub struct Storage {
    root: PathBuf,
    /// Set for [`Storage::fake`]: removes the temporary root when the last clone
    /// of the disk is dropped.
    _temp: Option<Arc<TempRoot>>,
}

/// Deletes a fake disk's directory on drop.
struct TempRoot(PathBuf);

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl Storage {
    /// A disk rooted at `root` (created on first write).
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            _temp: None,
        }
    }

    /// A disk in a fresh temporary directory, deleted when the last clone is
    /// dropped — Laravel's `Storage::fake()`. Every call gets its own directory,
    /// so parallel tests don't see each other's files. Put it in place with
    /// `App::swap(Storage::fake())`:
    ///
    /// ```
    /// # use elyra::storage::Storage;
    /// let disk = Storage::fake();
    /// disk.put_str("exports/report.csv", "a,b").unwrap();
    /// disk.assert_exists("exports/report.csv");
    /// disk.assert_missing("exports/old.csv");
    /// ```
    pub fn fake() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "elyra-fake-disk-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        Self {
            _temp: Some(Arc::new(TempRoot(root.clone()))),
            root,
        }
    }

    /// Assert `rel` exists on this disk.
    #[track_caller]
    pub fn assert_exists(&self, rel: &str) {
        assert!(self.exists(rel), "expected `{rel}` to exist on the disk");
    }

    /// Assert `rel` does not exist on this disk.
    #[track_caller]
    pub fn assert_missing(&self, rel: &str) {
        assert!(
            !self.exists(rel),
            "expected `{rel}` not to exist on the disk"
        );
    }

    /// Assert `rel` holds exactly `expected` (as UTF-8).
    #[track_caller]
    pub fn assert_contents(&self, rel: &str, expected: &str) {
        match self.get_str(rel) {
            Ok(actual) => assert_eq!(actual, expected, "contents of `{rel}`"),
            Err(e) => panic!("expected `{rel}` to hold {expected:?}, but reading it failed: {e}"),
        }
    }

    /// The disk root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a relative path to an absolute one, jailed within the root.
    fn resolve(&self, rel: &str) -> io::Result<PathBuf> {
        let mut path = self.root.clone();
        for component in Path::new(rel).components() {
            match component {
                Component::Normal(c) => path.push(c),
                Component::CurDir => {}
                _ => {
                    return Err(io::Error::new(
                        ErrorKind::PermissionDenied,
                        "path escapes the storage root",
                    ))
                }
            }
        }
        Ok(path)
    }

    /// The absolute path for `rel` (jailed). Useful for passing to other APIs.
    pub fn path(&self, rel: &str) -> io::Result<PathBuf> {
        self.resolve(rel)
    }

    /// Write bytes, creating parent directories as needed.
    pub fn put(&self, rel: &str, contents: &[u8]) -> io::Result<()> {
        let path = self.resolve(rel)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, contents)
    }

    /// Write a string (see [`put`](Storage::put)).
    pub fn put_str(&self, rel: &str, contents: &str) -> io::Result<()> {
        self.put(rel, contents.as_bytes())
    }

    /// Read bytes.
    pub fn get(&self, rel: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.resolve(rel)?)
    }

    /// Read a UTF-8 string.
    pub fn get_str(&self, rel: &str) -> io::Result<String> {
        std::fs::read_to_string(self.resolve(rel)?)
    }

    /// Append bytes, creating the file/dirs if needed.
    pub fn append(&self, rel: &str, contents: &[u8]) -> io::Result<()> {
        use std::io::Write;
        let path = self.resolve(rel)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(contents)
    }

    /// Whether a file or directory exists.
    pub fn exists(&self, rel: &str) -> bool {
        self.resolve(rel).map(|p| p.exists()).unwrap_or(false)
    }

    /// Delete a file (ignores a missing file).
    pub fn delete(&self, rel: &str) -> io::Result<()> {
        let path = self.resolve(rel)?;
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// File size in bytes.
    pub fn size(&self, rel: &str) -> io::Result<u64> {
        Ok(std::fs::metadata(self.resolve(rel)?)?.len())
    }

    /// Create a directory (and parents).
    pub fn make_directory(&self, rel: &str) -> io::Result<()> {
        std::fs::create_dir_all(self.resolve(rel)?)
    }

    /// File names directly inside `rel` (non-recursive; directories skipped).
    pub fn files(&self, rel: &str) -> io::Result<Vec<String>> {
        let dir = self.resolve(rel)?;
        let mut names = Vec::new();
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// A `file://` URL for `rel` (to open in the OS).
    pub fn url(&self, rel: &str) -> io::Result<String> {
        Ok(format!("file://{}", self.resolve(rel)?.display()))
    }
}

/// Conformance to the shared [`substrate_core::Storage`] contract.
impl substrate_core::Storage for Storage {
    fn put(&self, path: &str, contents: &[u8]) -> substrate_core::Result<()> {
        Storage::put(self, path, contents).map_err(|e| substrate_core::Error::new(e.to_string()))
    }
    fn get(&self, path: &str) -> substrate_core::Result<Vec<u8>> {
        Storage::get(self, path).map_err(|e| substrate_core::Error::new(e.to_string()))
    }
    fn exists(&self, path: &str) -> bool {
        Storage::exists(self, path)
    }
    fn delete(&self, path: &str) -> substrate_core::Result<()> {
        Storage::delete(self, path).map_err(|e| substrate_core::Error::new(e.to_string()))
    }
    fn size(&self, path: &str) -> substrate_core::Result<u64> {
        Storage::size(self, path).map_err(|e| substrate_core::Error::new(e.to_string()))
    }
    fn files(&self, dir: &str) -> substrate_core::Result<Vec<String>> {
        Storage::files(self, dir).map_err(|e| substrate_core::Error::new(e.to_string()))
    }
}

/// A [`Provider`](crate::Provider) that binds a [`Storage`] disk.
///
/// ```no_run
/// use elyra::App;
/// use elyra::storage::StorageProvider;
/// App::new().provider(StorageProvider::at("/path/to/data")).run().unwrap();
/// ```
pub struct StorageProvider {
    root: PathBuf,
}

impl StorageProvider {
    /// Root the disk at `root`.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

impl Default for StorageProvider {
    /// Defaults to `./storage` (relative to the working directory). Prefer
    /// [`at`](StorageProvider::at) with an explicit app-data path.
    fn default() -> Self {
        Self::at("storage")
    }
}

impl crate::Provider for StorageProvider {
    fn register(&self, container: &mut crate::Container) {
        container.bind(Storage::new(self.root.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fake_disk_is_private_and_cleans_up_after_its_last_clone() {
        let a = Storage::fake();
        let b = Storage::fake();
        assert_ne!(a.root(), b.root(), "each fake gets its own directory");

        a.put_str("x/report.csv", "a,b").unwrap();
        a.assert_exists("x/report.csv");
        a.assert_contents("x/report.csv", "a,b");
        b.assert_missing("x/report.csv");

        let root = a.root().to_path_buf();
        let clone = a.clone();
        drop(a);
        assert!(root.exists(), "a live clone keeps the directory");
        drop(clone);
        assert!(!root.exists(), "the last clone removes it");
    }

    #[test]
    #[should_panic(expected = "expected `nope.txt` to exist")]
    fn assert_exists_names_the_missing_path() {
        Storage::fake().assert_exists("nope.txt");
    }

    fn temp_disk() -> Storage {
        // A unique dir per call: tests run in parallel and would otherwise share a
        // pid-keyed dir, so one test's setup `remove_dir_all` could wipe another's
        // files mid-run.
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "elyra-storage-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        Storage::new(dir)
    }

    #[test]
    fn put_get_exists_delete_size() {
        let s = temp_disk();
        assert!(!s.exists("a/b.txt"));
        s.put_str("a/b.txt", "hello").unwrap();
        assert!(s.exists("a/b.txt"));
        assert_eq!(s.get_str("a/b.txt").unwrap(), "hello");
        assert_eq!(s.size("a/b.txt").unwrap(), 5);
        s.delete("a/b.txt").unwrap();
        assert!(!s.exists("a/b.txt"));
        s.delete("a/b.txt").unwrap(); // missing is ok
    }

    #[test]
    fn append_and_list_files() {
        let s = temp_disk();
        s.put_str("log.txt", "one\n").unwrap();
        s.append("log.txt", b"two\n").unwrap();
        assert_eq!(s.get_str("log.txt").unwrap(), "one\ntwo\n");
        s.put_str("dir/x.txt", "x").unwrap();
        s.put_str("dir/y.txt", "y").unwrap();
        assert_eq!(s.files("dir").unwrap(), vec!["x.txt", "y.txt"]);
    }

    #[test]
    fn path_is_jailed() {
        let s = temp_disk();
        assert!(s.get("../../etc/passwd").is_err());
        assert!(s.put_str("../escape.txt", "no").is_err());
        assert!(s.path("/abs").is_err());
    }
}
