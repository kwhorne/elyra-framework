//! Files over HTTP (RFC 0009 step 1): multipart parts streamed from disk,
//! downloads streamed to it, the progress of both, and the timeout that
//! counts the time without progress rather than the whole transfer.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures_util::{Stream, StreamExt};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
// tokio's clock, so a test with paused time sees the idle time pass.
use tokio::time::Instant;

use super::HttpError;

/// How much of a file is read, or written, at a time.
const CHUNK: usize = 64 * 1024;

/// The longest file name a download is given, in bytes.
const MAX_NAME: usize = 200;

/// How far a transfer has got: bytes sent (an upload's files) or received (a
/// download), and the total when it's known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub done: u64,
    pub total: Option<u64>,
}

pub(crate) type OnProgress = Arc<dyn Fn(Progress) + Send + Sync>;

/// A multipart part's content.
#[derive(Debug, Clone)]
pub(crate) enum PartBody {
    /// Read from disk while it's sent.
    File(PathBuf),
    Bytes(Vec<u8>),
    Text(String),
}

/// A part of a `multipart/form-data` body.
#[derive(Debug, Clone)]
pub(crate) struct Part {
    pub name: String,
    pub body: PartBody,
    pub file_name: Option<String>,
    pub content_type: Option<String>,
}

impl Part {
    /// A file, named as it is on disk, its type guessed from its extension.
    pub fn file(name: String, path: &Path) -> Self {
        Self {
            name,
            file_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            content_type: Some(guess_type(path)),
            body: PartBody::File(path.to_owned()),
        }
    }

    pub fn text(name: String, value: String) -> Self {
        Self {
            name,
            body: PartBody::Text(value),
            file_name: None,
            content_type: None,
        }
    }

    /// A file or bytes, as opposed to a field.
    pub fn is_payload(&self) -> bool {
        !matches!(self.body, PartBody::Text(_))
    }
}

/// The MIME type for a file's extension, `application/octet-stream` when
/// it's unknown.
pub(crate) fn guess_type(path: &Path) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .essence_str()
        .to_owned()
}

/// Stops a transfer when [`cancel`](Cancel::cancel)led: the request is
/// dropped where it is, and a download's `.part` file removed.
#[derive(Default)]
pub(crate) struct Cancel {
    cancelled: std::sync::atomic::AtomicBool,
    notify: tokio::sync::Notify,
}

impl Cancel {
    #[cfg_attr(not(feature = "backend"), allow(dead_code))]
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Resolves once cancelled.
    pub async fn cancelled(&self) {
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}

/// One attempt's progress, and what went wrong with a file on the way.
pub(crate) struct Meter {
    started: Instant,
    /// Milliseconds after `started` of the last progress.
    last: AtomicU64,
    done: AtomicU64,
    /// `u64::MAX` while unknown.
    total: AtomicU64,
    on_progress: Option<OnProgress>,
    /// Set by a part's stream: reqwest only sees an I/O error.
    failed: Mutex<Option<HttpError>>,
}

impl Meter {
    pub fn new(total: Option<u64>, on_progress: Option<OnProgress>) -> Arc<Self> {
        Arc::new(Self {
            started: Instant::now(),
            last: AtomicU64::new(0),
            done: AtomicU64::new(0),
            total: AtomicU64::new(total.unwrap_or(u64::MAX)),
            on_progress,
            failed: Mutex::new(None),
        })
    }

    pub fn advance(&self, bytes: u64) {
        let done = self.done.fetch_add(bytes, Ordering::Relaxed) + bytes;
        self.touch();
        if let Some(report) = &self.on_progress {
            report(Progress {
                done,
                total: self.total(),
            });
        }
    }

    /// A download's total, once its headers say.
    pub fn set_total(&self, total: Option<u64>) {
        self.total
            .store(total.unwrap_or(u64::MAX), Ordering::Relaxed);
    }

    fn total(&self) -> Option<u64> {
        Some(self.total.load(Ordering::Relaxed)).filter(|&t| t != u64::MAX)
    }

    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    fn touch(&self) {
        let now = self.started.elapsed().as_millis() as u64;
        self.last.store(now, Ordering::Relaxed);
    }

    fn idle(&self) -> Duration {
        let last = Duration::from_millis(self.last.load(Ordering::Relaxed));
        self.started.elapsed().saturating_sub(last)
    }

    fn fail(&self, error: HttpError) {
        self.failed.lock().get_or_insert(error);
    }

    /// What a part's stream ran into, which reqwest reports only as an I/O
    /// error.
    pub fn failure(&self) -> Option<HttpError> {
        self.failed.lock().clone()
    }
}

/// Run `work`, giving up once `idle` passes with no progress on `meter`.
/// `None` is that timeout.
pub(crate) async fn within_idle<F: std::future::Future>(
    meter: &Meter,
    idle: Duration,
    work: F,
) -> Option<F::Output> {
    tokio::pin!(work);
    loop {
        let quiet = meter.idle();
        if quiet >= idle {
            return None;
        }
        tokio::select! {
            out = &mut work => return Some(out),
            () = tokio::time::sleep(idle - quiet) => {}
        }
    }
}

/// A file's size and modification time, to tell whether it changed while it
/// was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub len: u64,
    modified: Option<SystemTime>,
}

pub(crate) async fn snapshot(path: &Path) -> Result<Snapshot, HttpError> {
    let meta = tokio::fs::metadata(path)
        .await
        .map_err(|e| HttpError::File(format!("{}: {e}", display_name(path))))?;
    if !meta.is_file() {
        return Err(HttpError::File(format!(
            "{} isn't a file",
            display_name(path)
        )));
    }
    Ok(Snapshot {
        len: meta.len(),
        modified: meta.modified().ok(),
    })
}

/// The file's name alone, for errors: the path can hold a user name.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "the file".into())
}

enum Reading {
    Open,
    At(tokio::fs::File, u64),
    Done,
}

/// The file at `path`, in chunks, as it's sent. It ends in an error if the
/// file isn't what `expected` says any more: a different size, or modified.
pub(crate) fn file_stream(
    path: PathBuf,
    expected: Snapshot,
    meter: Arc<Meter>,
) -> impl Stream<Item = io::Result<Vec<u8>>> + Send + 'static {
    futures_util::stream::unfold(
        (Reading::Open, path, meter),
        move |(state, path, meter)| async move {
            let (mut file, read) = match state {
                Reading::Done => return None,
                Reading::Open => match tokio::fs::File::open(&path).await {
                    Ok(file) => (file, 0),
                    Err(e) => {
                        meter.fail(HttpError::File(format!("{}: {e}", display_name(&path))));
                        return Some((Err(e), (Reading::Done, path, meter)));
                    }
                },
                Reading::At(file, read) => (file, read),
            };
            let changed = |meter: &Meter, path: &Path| {
                meter.fail(HttpError::FileChanged(display_name(path)));
                io::Error::other("the file changed while it was being sent")
            };
            let mut buf = vec![0; CHUNK];
            match file.read(&mut buf).await {
                Err(e) => {
                    meter.fail(HttpError::File(format!("{}: {e}", display_name(&path))));
                    Some((Err(e), (Reading::Done, path, meter)))
                }
                Ok(0) => {
                    let now = snapshot(&path).await.ok();
                    if read != expected.len || now != Some(expected) {
                        let e = changed(&meter, &path);
                        return Some((Err(e), (Reading::Done, path, meter)));
                    }
                    None
                }
                Ok(n) => {
                    if read + n as u64 > expected.len {
                        let e = changed(&meter, &path);
                        return Some((Err(e), (Reading::Done, path, meter)));
                    }
                    buf.truncate(n);
                    meter.advance(n as u64);
                    Some((Ok(buf), (Reading::At(file, read + n as u64), path, meter)))
                }
            }
        },
    )
}

/// Bytes, in chunks, counted as they're sent.
pub(crate) fn bytes_stream(
    bytes: Vec<u8>,
    meter: Arc<Meter>,
) -> impl Stream<Item = io::Result<Vec<u8>>> + Send + 'static {
    let chunks: Vec<Vec<u8>> = bytes.chunks(CHUNK).map(<[u8]>::to_vec).collect();
    futures_util::stream::iter(chunks).map(move |chunk| {
        meter.advance(chunk.len() as u64);
        Ok(chunk)
    })
}

/// Where a download goes.
#[derive(Debug, Clone)]
pub(crate) enum SaveTo {
    /// This file, replaced if it's there: the user chose it in a save dialog.
    File(PathBuf),
    /// A new file in this folder, under the server's name.
    Into(PathBuf),
}

/// A download's body, as it arrives.
pub(crate) enum Incoming {
    Real(reqwest::Response),
    /// The fake's whole body, at once, and its `Content-Disposition`.
    Fake(Option<Vec<u8>>, Option<String>),
}

impl Incoming {
    fn disposition(&self) -> Option<String> {
        match self {
            Incoming::Real(res) => res
                .headers()
                .get("content-disposition")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned),
            Incoming::Fake(_, disposition) => disposition.clone(),
        }
    }

    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, HttpError> {
        match self {
            Incoming::Real(res) => Ok(res
                .chunk()
                .await
                .map_err(super::unreachable)?
                .map(|b| b.to_vec())),
            Incoming::Fake(body, _) => Ok(body.take()),
        }
    }
}

/// Write `incoming` to `<target>.part`, then rename it to the target, so a
/// download that stops halfway leaves nothing that looks finished. Returns
/// where it was saved.
pub(crate) async fn save(
    mut incoming: Incoming,
    to: &SaveTo,
    url: &reqwest::Url,
    meter: &Meter,
) -> Result<PathBuf, HttpError> {
    let server_name = &server_file_name(incoming.disposition().as_deref(), url);
    let (dir, target) = match to {
        SaveTo::File(path) => (
            path.parent().map(Path::to_owned).unwrap_or_default(),
            path.clone(),
        ),
        SaveTo::Into(dir) => (dir.clone(), dir.join(unique_name(dir, server_name))),
    };
    let part = part_path(&target);
    let shown = display_name(&target);
    let file_error = |e: io::Error| HttpError::File(format!("{shown}: {e}"));
    // Removes the `.part` file if this is dropped halfway: cancelled, or
    // timed out. Declared before `file`, so it's dropped after the file is
    // closed (Windows won't remove an open file).
    let mut guard = PartGuard(Some(part.clone()));
    let mut file = tokio::fs::File::create(&part).await.map_err(file_error)?;
    let written = async {
        while let Some(chunk) = incoming.chunk().await? {
            file.write_all(&chunk).await.map_err(file_error)?;
            meter.advance(chunk.len() as u64);
        }
        file.flush().await.map_err(file_error)?;
        file.sync_all().await.map_err(file_error)
    }
    .await;
    drop(file);
    if let Err(e) = written {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e);
    }
    // Into a folder: another file may have taken the name meanwhile.
    let target = match to {
        SaveTo::Into(_) if tokio::fs::try_exists(&target).await.unwrap_or(false) => {
            dir.join(unique_name(&dir, server_name))
        }
        _ => target,
    };
    if let Err(e) = tokio::fs::rename(&part, &target).await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(file_error(e));
    }
    guard.0 = None;
    mark_downloaded(&target, url);
    Ok(target)
}

/// Removes a download's `.part` file when the download stops before it's
/// renamed.
struct PartGuard(Option<PathBuf>);

impl Drop for PartGuard {
    fn drop(&mut self) {
        if let Some(part) = self.0.take() {
            let _ = std::fs::remove_file(part);
        }
    }
}

/// Mark a downloaded file as coming from the internet, as a browser does, so
/// the OS checks it before it's opened: the quarantine attribute on macOS,
/// the Mark of the Web on Windows. Elsewhere there's no such mark. A failure
/// (a file system without the attribute) leaves the file as it is.
pub(crate) fn mark_downloaded(path: &Path, url: &reqwest::Url) {
    #[cfg(target_os = "macos")]
    {
        let _ = url;
        let _ = quarantine(path);
    }
    #[cfg(windows)]
    {
        // The origin, never the whole URL: a signed URL carries a secret.
        let origin = url.origin().ascii_serialization();
        let mark = format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={origin}/\r\n");
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = std::fs::write(stream, mark);
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    let _ = (path, url);
}

/// The value Safari and Chrome write: flags, the time, the agent.
#[cfg(target_os = "macos")]
pub(crate) fn quarantine_value() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("0081;{now:08x};Elyra;")
}

#[cfg(target_os = "macos")]
fn quarantine(path: &Path) -> io::Result<()> {
    use std::ffi::{c_char, c_int, c_void, CString};
    use std::os::unix::ffi::OsStrExt;
    // Avoid a `libc` dependency for one call.
    extern "C" {
        fn setxattr(
            path: *const c_char,
            name: *const c_char,
            value: *const c_void,
            size: usize,
            position: u32,
            options: c_int,
        ) -> c_int;
    }
    let path = CString::new(path.as_os_str().as_bytes()).map_err(io::Error::other)?;
    let value = quarantine_value();
    // SAFETY: both strings are NUL-terminated and live across the call, and
    // `value` is valid for `size` bytes.
    let done = unsafe {
        setxattr(
            path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if done == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `<target>.part`.
pub(crate) fn part_path(target: &Path) -> PathBuf {
    let mut name: OsString = target.file_name().map(OsString::from).unwrap_or_default();
    name.push(".part");
    target.with_file_name(name)
}

/// `name` in `dir`, or `name (1)`, `name (2)`, … when it's taken (by a file,
/// or a download still writing it).
pub(crate) fn unique_name(dir: &Path, name: &str) -> String {
    let free = |candidate: &str| {
        let path = dir.join(candidate);
        !path.exists() && !part_path(&path).exists()
    };
    if free(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => name.split_at(at),
        _ => (name, ""),
    };
    (1..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| free(candidate))
        .expect("an unused name")
}

/// The file name the server gives a download: `Content-Disposition`'s
/// `filename*`, then its `filename`, then the URL's last segment — cleaned.
pub(crate) fn server_file_name(disposition: Option<&str>, url: &reqwest::Url) -> String {
    let named = disposition.and_then(disposition_file_name).or_else(|| {
        url.path_segments()?
            .rev()
            .find(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(&percent_decode(s)).into_owned())
    });
    clean_file_name(&named.unwrap_or_default())
}

/// The file name in a `Content-Disposition` header, `filename*` (RFC 6266 /
/// 8187) over `filename`.
fn disposition_file_name(header: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    let mut rest = header.split_once(';').map_or("", |(_, rest)| rest);
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let key = key.trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, escaped)) = chars.next() {
                            value.push(escaped);
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => value.push(c),
                }
            }
            let next = quoted[end..].split_once(';').map_or("", |(_, r)| r);
            (value, next)
        } else {
            let (value, next) = after.split_once(';').unwrap_or((after, ""));
            (value.trim().to_owned(), next)
        };
        match key.as_str() {
            "filename" => plain = Some(value),
            "filename*" => extended = decode_extended(&value),
            _ => {}
        }
        rest = next;
    }
    extended.or(plain).filter(|n| !n.is_empty())
}

/// RFC 8187's `charset'language'percent-encoded`.
fn decode_extended(value: &str) -> Option<String> {
    let mut parts = value.splitn(3, '\'');
    let charset = parts.next()?.to_ascii_lowercase();
    let _language = parts.next()?;
    let bytes = percent_decode(parts.next()?);
    match charset.as_str() {
        "utf-8" => String::from_utf8(bytes).ok(),
        "iso-8859-1" => Some(bytes.into_iter().map(char::from).collect()),
        _ => None,
    }
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1), bytes.get(i + 2)) {
            (b'%', Some(&h), Some(&l)) if hex(h).is_some() && hex(l).is_some() => {
                out.push((hex(h).unwrap() * 16 + hex(l).unwrap()) as u8);
                i += 3;
            }
            (b, _, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

/// A file name from the server, made safe to create: no folders, no
/// characters a file system refuses, no leading dot (a hidden file), not
/// one of Windows' device names, and not too long.
pub(crate) fn clean_file_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let kept: String = last
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    let mut name = kept
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .trim()
        .to_owned();
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        name.insert(0, '_');
    }
    if name.len() > MAX_NAME {
        let ext = name
            .rfind('.')
            .map(|at| name[at..].to_owned())
            .filter(|ext| ext.len() <= 16)
            .unwrap_or_default();
        let mut cut = MAX_NAME - ext.len();
        while !name.is_char_boundary(cut) {
            cut -= 1;
        }
        name = format!("{}{ext}", &name[..cut]);
    }
    if name.is_empty() {
        "download".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_servers_file_name_is_cleaned() {
        assert_eq!(clean_file_name("invoice-7.pdf"), "invoice-7.pdf");
        assert_eq!(clean_file_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_file_name("..\\..\\boot.ini"), "boot.ini");
        assert_eq!(clean_file_name(".bashrc"), "bashrc");
        assert_eq!(clean_file_name(".."), "download");
        assert_eq!(clean_file_name(""), "download");
        assert_eq!(clean_file_name("a<b>c:d\"e|f?g*h\u{7}.txt"), "abcdefgh.txt");
        assert_eq!(clean_file_name("report. "), "report");
        assert_eq!(clean_file_name("CON"), "_CON");
        assert_eq!(clean_file_name("nul.txt"), "_nul.txt");
        assert_eq!(clean_file_name("com1.log"), "_com1.log");
        assert_eq!(clean_file_name("console.log"), "console.log");
        assert_eq!(
            clean_file_name("Blåbær kvittering.pdf"),
            "Blåbær kvittering.pdf"
        );
        let long = format!("{}.pdf", "æ".repeat(150));
        let cleaned = clean_file_name(&long);
        assert!(
            cleaned.len() <= MAX_NAME && cleaned.ends_with(".pdf"),
            "{cleaned}"
        );
    }

    #[test]
    fn the_name_comes_from_content_disposition_then_the_url() {
        let url =
            reqwest::Url::parse("https://crm.test/api/invoices/7/Faktura%207.pdf?x=1").unwrap();
        let name = |h: Option<&str>| server_file_name(h, &url);
        assert_eq!(name(None), "Faktura 7.pdf");
        assert_eq!(
            name(Some("attachment; filename=\"invoice 7.pdf\"")),
            "invoice 7.pdf"
        );
        assert_eq!(name(Some("attachment; filename=plain.pdf")), "plain.pdf");
        assert_eq!(
            name(Some(
                "attachment; filename=\"fallback.pdf\"; filename*=UTF-8''bl%C3%A5b%C3%A6r.pdf"
            )),
            "blåbær.pdf"
        );
        assert_eq!(
            name(Some("attachment; filename*=iso-8859-1'en'%E6%F8%E5.txt")),
            "æøå.txt"
        );
        assert_eq!(
            name(Some(
                "attachment; filename=\"a \\\"quoted\\\"; name.pdf\"; size=3"
            )),
            "a quoted; name.pdf"
        );
        assert_eq!(name(Some("attachment; filename=\"../../x.sh\"")), "x.sh");
        assert_eq!(name(Some("inline")), "Faktura 7.pdf");
        let bare = reqwest::Url::parse("https://crm.test/").unwrap();
        assert_eq!(server_file_name(None, &bare), "download");
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("elyra-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(unique_name(&dir, "a.pdf"), "a.pdf");
        std::fs::write(dir.join("a.pdf"), "").unwrap();
        assert_eq!(unique_name(&dir, "a.pdf"), "a (1).pdf");
        std::fs::write(dir.join("a (1).pdf.part"), "").unwrap();
        assert_eq!(unique_name(&dir, "a.pdf"), "a (2).pdf");
        std::fs::write(dir.join("README"), "").unwrap();
        assert_eq!(unique_name(&dir, "README"), "README (1)");
        assert_eq!(part_path(&dir.join("a.pdf")), dir.join("a.pdf.part"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn the_timeout_counts_time_without_progress() {
        let meter = Meter::new(None, None);
        let busy = {
            let meter = meter.clone();
            async move {
                for _ in 0..10 {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    meter.advance(1);
                }
                "done"
            }
        };
        // 300 ms in all, longer than the limit, but never quiet for 250 ms.
        let started = Instant::now();
        assert_eq!(
            within_idle(&meter, Duration::from_millis(250), busy).await,
            Some("done")
        );
        assert!(started.elapsed() >= Duration::from_millis(250));
        let stalled = Meter::new(None, None);
        let never = std::future::pending::<()>();
        assert_eq!(
            within_idle(&stalled, Duration::from_millis(50), never).await,
            None
        );
    }
}
