//! Structured logging — Elyra's `Log` facade.
//!
//! The framework used to print diagnostics with bare `eprintln!`: no levels, no
//! timestamps, nothing on disk, and nothing a user could attach to a bug report.
//! This module gives app *and* framework code one place to log to:
//!
//! ```ignore
//! use elyra::log;
//!
//! log::info!("sync finished in {}ms", elapsed);
//! log::warn!(target: "sync", "retrying after {e}");
//! log::error!("import failed: {e}");
//! ```
//!
//! ## Sinks and levels
//! By default logs go to stderr at [`Level::Info`] and above. Add a file sink
//! (with size-based rotation) via [`LogProvider`]:
//!
//! ```ignore
//! App::new().provider(LogProvider::default().level(Level::Debug).to_app_dir("MyApp"))
//! ```
//!
//! The level can be overridden at runtime with `ELYRA_LOG`
//! (`error` / `warn` / `info` / `debug` / `trace`, or `off`), which is read once
//! on first use — the same reflex as `RUST_LOG` without pulling in a filter DSL.
//!
//! ## Command spans
//! The shell logs every command dispatch at debug level with its name, decoded
//! byte size, and duration, and any panic at error level. That trace is what makes
//! "why is this command slow / silently failing" answerable in a released build.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

/// Severity of a log record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl Level {
    /// The uppercase name used in output.
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    fn parse(value: &str) -> Option<Level> {
        match value.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Level::Error),
            "warn" | "warning" => Some(Level::Warn),
            "info" => Some(Level::Info),
            "debug" => Some(Level::Debug),
            "trace" => Some(Level::Trace),
            _ => None,
        }
    }
}

/// `0` = off; otherwise a [`Level`] discriminant.
static LEVEL: AtomicU8 = AtomicU8::new(0);
static INITIALIZED: OnceLock<()> = OnceLock::new();

/// The optional file sink.
static FILE: OnceLock<Mutex<FileSink>> = OnceLock::new();

struct FileSink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
    max_bytes: u64,
    keep: usize,
}

impl FileSink {
    fn write_line(&mut self, line: &str) {
        if self.file.is_none() {
            if let Some(dir) = self.path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            self.written = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
            self.file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if writeln!(file, "{line}").is_ok() {
            self.written += line.len() as u64 + 1;
        }
        if self.written >= self.max_bytes {
            self.rotate();
        }
    }

    /// `app.log` -> `app.log.1` -> `app.log.2` … dropping the oldest.
    fn rotate(&mut self) {
        self.file = None;
        for index in (1..=self.keep).rev() {
            let from = numbered(&self.path, index - 1);
            let to = numbered(&self.path, index);
            if from.exists() {
                if index == self.keep {
                    let _ = std::fs::remove_file(&from);
                } else {
                    let _ = std::fs::rename(&from, &to);
                }
            }
        }
        self.written = 0;
    }
}

fn numbered(path: &Path, index: usize) -> PathBuf {
    if index == 0 {
        return path.to_path_buf();
    }
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

/// Set the minimum level that is emitted. `None` turns logging off.
pub fn set_level(level: Option<Level>) {
    let _ = INITIALIZED.set(());
    LEVEL.store(level.map(|l| l as u8).unwrap_or(0), Ordering::Relaxed);
}

/// The current minimum level (`None` = logging is off).
pub fn level() -> Option<Level> {
    // First use resolves the default: `ELYRA_LOG`, else Info.
    if INITIALIZED.get().is_none() {
        let default = std::env::var("ELYRA_LOG")
            .ok()
            .and_then(|v| {
                if v.trim().eq_ignore_ascii_case("off") {
                    Some(None)
                } else {
                    Level::parse(&v).map(Some)
                }
            })
            .unwrap_or(Some(Level::Info));
        set_level(default);
    }
    match LEVEL.load(Ordering::Relaxed) {
        0 => None,
        1 => Some(Level::Error),
        2 => Some(Level::Warn),
        3 => Some(Level::Info),
        4 => Some(Level::Debug),
        _ => Some(Level::Trace),
    }
}

/// Whether a record at `level` would be emitted (cheap; call before formatting).
pub fn enabled(level: Level) -> bool {
    self::level().map(|min| level <= min).unwrap_or(false)
}

/// Send every record to `path`, rotating at `max_bytes` and keeping `keep` files.
/// Called by [`LogProvider`]; safe to call once.
pub fn to_file(path: impl Into<PathBuf>, max_bytes: u64, keep: usize) {
    let _ = FILE.set(Mutex::new(FileSink {
        path: path.into(),
        file: None,
        written: 0,
        max_bytes,
        keep: keep.max(1),
    }));
}

/// The active log file, if a file sink is configured — hand this to a
/// "send us your log" button.
pub fn log_path() -> Option<PathBuf> {
    FILE.get().map(|sink| sink.lock().path.clone())
}

/// Emit a record. Prefer the [`info!`](crate::info) / [`warn!`](crate::warn) macros.
pub fn log(level: Level, target: &str, message: &str) {
    if !enabled(level) {
        return;
    }
    let line = format!(
        "{} {:<5} [{}] {}",
        timestamp(),
        level.label(),
        target,
        message
    );
    eprintln!("{line}");
    if let Some(sink) = FILE.get() {
        sink.lock().write_line(&line);
    }
}

/// `YYYY-MM-DDTHH:MM:SS.mmmZ`, computed without a date dependency.
fn timestamp() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() as i64;
    let millis = now.subsec_millis();
    let (y, mo, d, h, mi, s) = civil_from_unix(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
}

/// Days-from-civil algorithm (Howard Hinnant), inverted: UTC calendar time.
fn civil_from_unix(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Log at error level: `log::error!("failed: {e}")`, or with an explicit target.
#[macro_export]
macro_rules! error {
    (target: $target:expr, $($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Error, $target, &::std::format!($($arg)+))
    };
    ($($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Error, ::std::module_path!(), &::std::format!($($arg)+))
    };
}

/// Log at warn level. See [`error!`](crate::error).
#[macro_export]
macro_rules! warn {
    (target: $target:expr, $($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Warn, $target, &::std::format!($($arg)+))
    };
    ($($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Warn, ::std::module_path!(), &::std::format!($($arg)+))
    };
}

/// Log at info level. See [`error!`](crate::error).
#[macro_export]
macro_rules! info {
    (target: $target:expr, $($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Info, $target, &::std::format!($($arg)+))
    };
    ($($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Info, ::std::module_path!(), &::std::format!($($arg)+))
    };
}

/// Log at debug level. See [`error!`](crate::error).
#[macro_export]
macro_rules! debug {
    (target: $target:expr, $($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Debug, $target, &::std::format!($($arg)+))
    };
    ($($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Debug, ::std::module_path!(), &::std::format!($($arg)+))
    };
}

/// Log at trace level. See [`error!`](crate::error).
#[macro_export]
macro_rules! trace {
    (target: $target:expr, $($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Trace, $target, &::std::format!($($arg)+))
    };
    ($($arg:tt)+) => {
        $crate::log::log($crate::log::Level::Trace, ::std::module_path!(), &::std::format!($($arg)+))
    };
}

/// Default rotation size (5 MiB).
pub const DEFAULT_MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// Default number of rotated files kept.
pub const DEFAULT_KEEP: usize = 3;

/// A [`Provider`](crate::Provider) that configures the logger.
///
/// ```no_run
/// use elyra::App;
/// use elyra::log::{Level, LogProvider};
/// App::new()
///     .provider(LogProvider::new().level(Level::Debug).to_app_dir("My App"))
///     .run()
///     .unwrap();
/// ```
#[derive(Default)]
pub struct LogProvider {
    level: Option<Level>,
    file: Option<PathBuf>,
    max_bytes: Option<u64>,
    keep: Option<usize>,
}

impl LogProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Minimum level to emit (`ELYRA_LOG` still overrides this if set).
    pub fn level(mut self, level: Level) -> Self {
        self.level = Some(level);
        self
    }

    /// Write to an explicit file path.
    pub fn to_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.file = Some(path.into());
        self
    }

    /// Write to `<os-log-or-config-dir>/<app>/<app>.log`.
    pub fn to_app_dir(mut self, app: &str) -> Self {
        if let Some(dir) = crate::winstate::app_dir(app) {
            self.file = Some(dir.join("app.log"));
        }
        self
    }

    /// Rotate at `max_bytes`, keeping `keep` older files.
    pub fn rotate(mut self, max_bytes: u64, keep: usize) -> Self {
        self.max_bytes = Some(max_bytes);
        self.keep = Some(keep);
        self
    }
}

impl crate::Provider for LogProvider {
    fn register(&self, _container: &mut crate::Container) {
        // `ELYRA_LOG` wins over the compiled-in default, like RUST_LOG.
        if std::env::var("ELYRA_LOG").is_err() {
            if let Some(level) = self.level {
                set_level(Some(level));
            }
        }
        if let Some(path) = &self.file {
            to_file(
                path.clone(),
                self.max_bytes.unwrap_or(DEFAULT_MAX_LOG_BYTES),
                self.keep.unwrap_or(DEFAULT_KEEP),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_order_from_error_to_trace() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert!(Level::Debug < Level::Trace);
        assert_eq!(Level::parse("WARNING"), Some(Level::Warn));
        assert_eq!(Level::parse("nonsense"), None);
    }

    #[test]
    fn timestamps_are_iso8601_utc() {
        let ts = timestamp();
        assert_eq!(ts.len(), 24, "{ts}");
        assert!(ts.ends_with('Z'));
        assert_eq!(&ts[4..5], "-");
        assert_eq!(&ts[10..11], "T");
    }

    #[test]
    fn civil_from_unix_matches_known_dates() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil_from_unix(1_000_000_000), (2001, 9, 9, 1, 46, 40));
        // A leap day.
        assert_eq!(civil_from_unix(1_709_164_800), (2024, 2, 29, 0, 0, 0));
    }

    #[test]
    fn file_sink_writes_and_rotates() {
        let dir = std::env::temp_dir().join(format!("elyra-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.log");

        let mut sink = FileSink {
            path: path.clone(),
            file: None,
            written: 0,
            max_bytes: 200,
            keep: 2,
        };
        for i in 0..40 {
            sink.write_line(&format!("line {i} ------------------------"));
        }

        assert!(path.exists(), "the active log must exist");
        assert!(
            numbered(&path, 1).exists(),
            "one rotation must have happened"
        );
        // `keep = 2` means at most app.log + app.log.1 + app.log.2.
        assert!(!numbered(&path, 3).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn numbered_paths_are_suffixed() {
        let p = Path::new("/tmp/app.log");
        assert_eq!(numbered(p, 0), PathBuf::from("/tmp/app.log"));
        assert_eq!(numbered(p, 2), PathBuf::from("/tmp/app.log.2"));
    }

    #[test]
    fn enabled_follows_the_configured_level() {
        set_level(Some(Level::Warn));
        assert!(enabled(Level::Error));
        assert!(enabled(Level::Warn));
        assert!(!enabled(Level::Info));

        set_level(None);
        assert!(!enabled(Level::Error));

        set_level(Some(Level::Info)); // restore the default for other tests
    }
}
