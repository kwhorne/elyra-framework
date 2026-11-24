//! Diagnostics the user can send (RFC 0033): one plain-text file that says
//! what the app is, where it runs, how it's connected, and what happened
//! just before — for the user to read, save and attach to an email or a
//! ticket.
//!
//! ```ignore
//! App::new()
//!     .diagnostics()                    // Help → Save Diagnostics…
//!     .diagnostics_with(|d| {
//!         d.section("Sync", "last full sync: 14:02");
//!     })
//! ```
//!
//! From the page, `diagnostics.save()` and `diagnostics.text()` in
//! `@elyra/runtime`.
//!
//! What's never in it: tokens and secrets (every log line is
//! [redacted](crate::log::redact) as it's written), the user's data (no
//! records, no outbox payloads, nothing from the clipboard or the window),
//! who the user is (only whether they're signed in), and the user's home
//! folder (written `~`). Nothing is sent: the file is where the user saved
//! it, or the text is handed to the page that asked.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::Deserialize;

use crate::container::Ctx;

/// The menu item's id, in the app's menu and the tray's.
pub(crate) const MENU_ID: &str = "__elyra_diagnostics";
/// What the item says: a file is saved, nothing is sent.
pub(crate) const MENU_LABEL: &str = "Save Diagnostics…";

/// Lines of the log the file holds.
pub const LOG_LINES: usize = 300;
/// The page's errors kept.
pub const PAGE_ERRORS: usize = 50;

/// What an app adds with [`App::diagnostics_with`](crate::App::diagnostics_with):
/// its own sections, after the framework's.
#[derive(Default)]
pub struct Diagnostics {
    sections: Vec<(String, String)>,
}

impl Diagnostics {
    /// A section titled `title`. Its text is redacted like the log, and the
    /// home folder becomes `~`, but it's the app's to keep the user's data
    /// out of it.
    pub fn section(&mut self, title: impl Into<String>, text: impl Into<String>) -> &mut Self {
        self.sections.push((title.into(), text.into()));
        self
    }
}

type Extra = Arc<dyn Fn(&mut Diagnostics) + Send + Sync>;

/// Bound by [`App::diagnostics`](crate::App::diagnostics): what the file says
/// of the app, and the app's own sections.
#[derive(Clone, Default)]
pub(crate) struct Setup {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) identifier: String,
    pub(crate) extra: Vec<Extra>,
}

impl Setup {
    pub(crate) fn add(&mut self, f: impl Fn(&mut Diagnostics) + Send + Sync + 'static) {
        self.extra.push(Arc::new(f));
    }
}

/// An uncaught error or unhandled rejection in a page, as `@elyra/runtime`
/// reports it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(crate) struct PageError {
    /// `TypeError: Cannot read properties of undefined (reading 'id')`.
    pub(crate) message: String,
    /// The first frame of its stack: `CustomerCard.svelte:41`.
    #[serde(default)]
    pub(crate) at: Option<String>,
    /// `error` or `rejection`.
    #[serde(default)]
    pub(crate) kind: Option<String>,
}

/// The last [`PAGE_ERRORS`] of the app's pages, bound in every app: each is
/// also written to the log, with the target `page`.
#[derive(Default)]
pub(crate) struct PageErrors {
    kept: Mutex<VecDeque<(SystemTime, String, PageError)>>,
    /// Per window: when its minute started, and how many came in it.
    budget: Mutex<std::collections::HashMap<String, (Instant, u32)>>,
}

/// How many errors a window may report a minute. `@elyra/runtime` sends no
/// more than this; a script calling `/__page-error` itself can't fill the
/// log either.
pub(crate) const PAGE_ERRORS_A_MINUTE: u32 = 20;

impl PageErrors {
    /// Whether the window `window` may report one more error at `now`. The
    /// first one refused in a minute is logged, once.
    pub(crate) fn admit(&self, window: &str, now: Instant) -> bool {
        let mut budget = self.budget.lock();
        let (start, count) = budget.entry(window.to_owned()).or_insert((now, 0));
        if now.duration_since(*start) >= Duration::from_secs(60) {
            (*start, *count) = (now, 0);
        }
        *count += 1;
        if *count == PAGE_ERRORS_A_MINUTE + 1 {
            crate::warn!(
                target: "page",
                "more than {PAGE_ERRORS_A_MINUTE} errors in `{window}` in a minute: the rest of it aren't kept"
            );
        }
        *count <= PAGE_ERRORS_A_MINUTE
    }

    /// Keep `error` from the window `window`, and log it.
    pub(crate) fn record(&self, window: &str, mut error: PageError) {
        error.message = clip(&error.message, 2_000);
        error.at = error.at.map(|at| clip(&at, 300));
        let place = error
            .at
            .as_deref()
            .map(|at| format!(" (at {at})"))
            .unwrap_or_default();
        let what = match error.kind.as_deref() {
            Some("rejection") => "an unhandled rejection",
            _ => "an uncaught error",
        };
        crate::error!(target: "page", "{what} in `{window}`: {}{place}", error.message);
        let mut kept = self.kept.lock();
        if kept.len() == PAGE_ERRORS {
            kept.pop_front();
        }
        kept.push_back((SystemTime::now(), window.to_owned(), error));
    }

    fn all(&self) -> Vec<(SystemTime, String, PageError)> {
        self.kept.lock().iter().cloned().collect()
    }
}

fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

/// What the event loop sees: the windows, the screens and the theme.
#[derive(Clone, Debug, Default)]
pub(crate) struct Seen {
    pub(crate) windows: Vec<WindowSeen>,
    /// Each screen's scale factor.
    pub(crate) screens: Vec<f64>,
    pub(crate) dark: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowSeen {
    pub(crate) label: String,
    pub(crate) visible: bool,
    pub(crate) focused: bool,
    /// The page's path: `/customers/7`.
    pub(crate) path: Option<String>,
}

/// A page's URL as the Windows section names it: its path, and a hash
/// route's path, without a query (`?token=…` has no place in the file).
pub(crate) fn page_path(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (before, hash) = match rest.split_once('#') {
        Some((before, hash)) => (before, Some(hash)),
        None => (rest, None),
    };
    let path = before.find('/').map_or("/", |at| &before[at..]);
    let path = path.split('?').next().unwrap_or(path);
    let hash = hash
        .filter(|h| h.starts_with('/'))
        .map(|h| h.split('?').next().unwrap_or(h));
    Some(match hash {
        Some(hash) => format!("{path}#{hash}"),
        None => path.to_owned(),
    })
}

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Note when the app started, for the file's uptime.
pub(crate) fn started() {
    let _ = STARTED.get_or_init(Instant::now);
}

/// The file's name: `acme-notes-diagnostics-2026-11-19-1402.txt`.
#[cfg_attr(not(feature = "system"), allow(dead_code))]
pub(crate) fn file_name(setup: &Setup) -> String {
    // `Acme Notes` → `acme-notes`, the identifier's rules.
    let slug = crate::app_id::of(None, &setup.name);
    format!(
        "{slug}-diagnostics-{}.txt",
        local(SystemTime::now(), "%Y-%m-%d-%H%M")
    )
}

/// What the file would hold, as `diagnostics.text()` hands it to the page:
/// for an app's own "Report a problem" screen, or a test. `None` without
/// [`App::diagnostics`](crate::App::diagnostics).
pub async fn text(ctx: &Ctx) -> Option<String> {
    let setup = ctx.try_get::<Setup>()?;
    let seen = match ctx.try_get::<crate::window::Windows>() {
        Some(windows) => windows.seen().await,
        None => None,
    };
    Some(text_with(ctx, &setup, seen).await)
}

/// The whole file, from what `ctx` holds and the event loop saw.
pub(crate) async fn text_with(ctx: &Ctx, setup: &Setup, seen: Option<Seen>) -> String {
    let mut out = String::new();
    let now = SystemTime::now();
    let _ = writeln!(out, "Elyra diagnostics — {} {}", setup.name, setup.version);
    let _ = writeln!(out, "Saved {}", local(now, "%Y-%m-%d %H:%M %:z"));

    section(&mut out, "App", &app(setup));
    section(&mut out, "System", &system(seen.as_ref()));
    section(&mut out, "Connection", &connection(ctx));
    section(&mut out, "Outbox", &outbox(ctx).await);
    section(&mut out, "Updates", &updates(ctx));
    section(&mut out, "Windows", &windows(seen.as_ref()));
    section(
        &mut out,
        "The page",
        &page(
            ctx.try_get::<PageErrors>()
                .map(|p| p.all())
                .unwrap_or_default(),
        ),
    );
    section(
        &mut out,
        &format!("Log (the last {LOG_LINES} lines)"),
        &log_lines(LOG_LINES).join("\n"),
    );

    let mut extra = Diagnostics::default();
    for add in &setup.extra {
        add(&mut extra);
    }
    if !extra.sections.is_empty() {
        let _ = write!(out, "\n## From the app\n");
        for (title, text) in &extra.sections {
            section(&mut out, title, text);
        }
    }

    tidy(&out)
}

/// Make the file, ask the user where to save it, write it there and show it
/// in Finder or Explorer: its path, or `None` when the user cancelled.
#[cfg(feature = "system")]
pub(crate) async fn save(
    ctx: &Ctx,
    setup: &Setup,
    seen: Option<Seen>,
) -> Result<Option<std::path::PathBuf>, String> {
    let text = text_with(ctx, setup, seen).await;
    let chosen = crate::system::save_dialog(crate::system::SaveDialog {
        title: Some("Save Diagnostics".into()),
        default_name: Some(file_name(setup)),
        filters: vec![crate::system::Filter {
            name: "Text".into(),
            extensions: vec!["txt".into()],
        }],
        start_dir: dirs::download_dir()
            .or_else(dirs::desktop_dir)
            .map(|d| d.display().to_string()),
    })
    .await;
    let Some(path) = chosen.map(std::path::PathBuf::from) else {
        return Ok(None);
    };
    let to = path.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&to, text))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    crate::info!(target: "elyra::diagnostics", "saved to {}", tidy(&path.display().to_string()));
    reveal(&path);
    Ok(Some(path))
}

/// Show `path` in Finder or Explorer, selected; on Linux, its folder.
#[cfg(feature = "system")]
fn reveal(path: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let shown = std::process::Command::new("open")
        .arg("-R")
        .arg(path)
        .spawn()
        .map(|_| ());
    #[cfg(target_os = "windows")]
    let shown = {
        let mut select = std::ffi::OsString::from("/select,");
        select.push(path);
        std::process::Command::new("explorer")
            .arg(select)
            .spawn()
            .map(|_| ())
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let shown = path.parent().map_or(Ok(()), open::that_detached);
    if let Err(e) = shown {
        crate::warn!(target: "elyra::diagnostics", "couldn't show the file: {e}");
    }
}

/// Redacted, and the home folder as `~`, all of it: the app's sections and a
/// log file written before 0.48 too.
fn tidy(text: &str) -> String {
    tidy_with(text, home().as_deref())
}

/// Redacted, with `home` written `~`: on Windows also where the path has
/// forward slashes (`C:/Users/kari`, as in a `sqlite:` or `file:` URL).
fn tidy_with(text: &str, home: Option<&str>) -> String {
    let text = crate::log::redact(text);
    let Some(home) = home else {
        return text.into_owned();
    };
    let text = text.replace(home, "~");
    let forward = home.replace('\\', "/");
    if forward == home {
        text
    } else {
        text.replace(&forward, "~")
    }
}

fn home() -> Option<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let home = home.trim_end_matches(['/', '\\']).to_owned();
    // `/` or `C:` alone would turn every path into `~`.
    (home.len() > 3).then_some(home)
}

fn section(out: &mut String, title: &str, body: &str) {
    let _ = write!(out, "\n## {title}\n");
    if body.trim().is_empty() {
        out.push_str("(nothing)\n");
    } else {
        out.push_str(body.trim_end());
        out.push('\n');
    }
}

fn local(at: SystemTime, format: &str) -> String {
    jiff::Timestamp::try_from(at)
        .map(|t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime(format)
                .to_string()
        })
        .unwrap_or_default()
}

fn app(setup: &Setup) -> String {
    let build = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    format!(
        "name: {} · version: {} · identifier: {}\nelyra: {} · build: {build} · features: {}",
        setup.name,
        setup.version,
        setup.identifier,
        env!("CARGO_PKG_VERSION"),
        features().join(", "),
    )
}

/// elyra's features this app was built with.
fn features() -> Vec<&'static str> {
    let all = [
        ("ai", cfg!(feature = "ai")),
        ("autostart", cfg!(feature = "autostart")),
        ("backend", cfg!(feature = "backend")),
        ("database", cfg!(feature = "database")),
        ("http", cfg!(feature = "http")),
        ("microphone", cfg!(feature = "microphone")),
        ("secrets", cfg!(feature = "secrets")),
        ("shortcuts", cfg!(feature = "shortcuts")),
        ("sidecar", cfg!(feature = "sidecar")),
        ("system", cfg!(feature = "system")),
        ("tray", cfg!(feature = "tray")),
        ("updater", cfg!(feature = "updater")),
    ];
    let on: Vec<&str> = all.iter().filter(|(_, on)| *on).map(|(n, _)| *n).collect();
    if on.is_empty() {
        vec!["none"]
    } else {
        on
    }
}

fn system(seen: Option<&Seen>) -> String {
    let locale = sys_locale::get_locale().unwrap_or_else(|| "unknown".into());
    #[cfg(feature = "gui")]
    let webview = wry::webview_version().unwrap_or_else(|_| "unknown".into());
    #[cfg(not(feature = "gui"))]
    let webview = String::from("none (built without a window)");
    let mut out = format!(
        "os: {} · arch: {} · locale: {locale}\nwebview: {} {webview}",
        os_version(),
        std::env::consts::ARCH,
        webview_name(),
    );
    if let Some(seen) = seen {
        let scales: Vec<String> = seen.screens.iter().map(|s| format!("{s}x")).collect();
        let _ = write!(out, " · screens: {}", seen.screens.len());
        if !scales.is_empty() {
            let _ = write!(out, " ({})", scales.join(", "));
        }
        if let Some(dark) = seen.dark {
            let _ = write!(out, " · dark mode: {}", if dark { "on" } else { "off" });
        }
    }
    if let Some(started) = STARTED.get() {
        let _ = write!(out, "\nuptime: {}", duration(started.elapsed()));
    }
    if let Some(bytes) = memory() {
        // The webview draws in processes of its own, not counted here.
        let _ = write!(
            out,
            "\nmemory: {} (the app's own; the webview's processes aren't counted)",
            megabytes(bytes)
        );
    }
    out
}

fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes + 512 * 1024) / (1024 * 1024))
}

/// What happened at `at`, as the file says it: `14:02:11, 3 min ago`.
#[cfg_attr(not(feature = "backend"), allow(dead_code))]
fn when(at: SystemTime) -> String {
    let ago = SystemTime::now().duration_since(at).unwrap_or_default();
    format!("{}, {} ago", local(at, "%H:%M:%S"), duration(ago))
}

/// The memory the app's own process has in RAM now (its resident set), in
/// bytes; `None` where it can't be read.
fn memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let kb: u64 = status
            .lines()
            .find_map(|l| l.strip_prefix("VmRSS:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok()?;
        Some(kb * 1024)
    }
    #[cfg(target_os = "macos")]
    {
        // mach/task_info.h's MACH_TASK_BASIC_INFO, packed to 4 as there.
        #[repr(C, packed(4))]
        #[derive(Default)]
        struct BasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time: [i32; 2],
            system_time: [i32; 2],
            policy: i32,
            suspend_count: i32,
        }
        const MACH_TASK_BASIC_INFO: i32 = 20;
        extern "C" {
            static mach_task_self_: u32;
            fn task_info(task: u32, flavor: i32, info: *mut i32, count: *mut u32) -> i32;
        }
        let mut info = BasicInfo::default();
        let mut count = (std::mem::size_of::<BasicInfo>() / std::mem::size_of::<i32>()) as u32;
        // SAFETY: `info` is the struct the flavour fills, `count` its size in
        // `natural_t`s, and `mach_task_self_` is this task's port.
        let kr = unsafe {
            task_info(
                mach_task_self_,
                MACH_TASK_BASIC_INFO,
                (&raw mut info).cast(),
                &mut count,
            )
        };
        (kr == 0).then_some(info.resident_size)
    }
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;
        let mut counters = PROCESS_MEMORY_COUNTERS::default();
        let size = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: `counters` is the struct of `size` bytes the call fills,
        // for this process's own handle.
        let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        ok.as_bool().then_some(counters.WorkingSetSize as u64)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn webview_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "WKWebView"
    } else if cfg!(target_os = "windows") {
        "WebView2"
    } else {
        "WebKitGTK"
    }
}

fn duration(d: Duration) -> String {
    let minutes = d.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => format!("{} s", d.as_secs()),
        (0, m) => format!("{m} min"),
        (h, m) => format!("{h} h {m} min"),
    }
}

/// `macOS 16.1 (25B78)`, `Ubuntu 24.04.1 LTS (6.8.0-48-generic)`,
/// `Windows 10.0.22631.4460`.
fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let plist = std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
            .unwrap_or_default();
        let value = |key: &str| {
            let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
            let start = after.find("<string>")? + "<string>".len();
            let end = after.find("</string>")?;
            after.get(start..end).map(str::to_owned)
        };
        match (value("ProductVersion"), value("ProductBuildVersion")) {
            (Some(v), Some(b)) => format!("macOS {v} ({b})"),
            (Some(v), None) => format!("macOS {v}"),
            _ => "macOS".into(),
        }
    }
    #[cfg(target_os = "linux")]
    {
        let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let name = release
            .lines()
            .find_map(|l| l.strip_prefix("PRETTY_NAME="))
            .map(|v| v.trim_matches('"').to_owned())
            .unwrap_or_else(|| "Linux".into());
        match std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            Ok(kernel) => format!("{name} ({})", kernel.trim()),
            Err(_) => name,
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let ver = std::process::Command::new("cmd")
            .args(["/C", "ver"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        // `Microsoft Windows [Version 10.0.22631.4460]`
        match ver
            .split("Version ")
            .nth(1)
            .and_then(|v| v.split(']').next())
        {
            Some(v) => format!("Windows {}", v.trim()),
            None => "Windows".into(),
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        std::env::consts::OS.to_owned()
    }
}

fn connection(ctx: &Ctx) -> String {
    #[cfg(feature = "backend")]
    if let Some(backend) = ctx.try_get::<crate::backend::Backend>() {
        let mut parts = Vec::new();
        #[cfg(feature = "database")]
        if let Some(state) = backend.connection() {
            let online = matches!(state.state, crate::offline::Connection::Online);
            let mut online = if online {
                "yes".to_owned()
            } else {
                "no".to_owned()
            };
            if let Some(since) = state.since {
                let since = SystemTime::UNIX_EPOCH + Duration::from_millis(since.max(0) as u64);
                let _ = write!(online, ", since {}", local(since, "%H:%M:%S"));
            }
            if state.simulated {
                online.push_str(" (the inspector's switch)");
            }
            parts.push(format!("online: {online}"));
        }
        let accounts = ctx
            .try_get::<crate::auth::Auth>()
            .map(|auth| auth.accounts())
            .unwrap_or_default();
        let server = backend.base_url().unwrap_or_else(|| "none chosen".into());
        let server = if accounts.len() > 1 {
            let at = accounts.iter().position(|a| a.active).map_or(0, |i| i + 1);
            format!("{server} ({at} of {})", accounts.len())
        } else {
            server
        };
        parts.push(format!("server: {server}"));
        parts.push(format!(
            "signed in: {}",
            if backend.has_token() { "yes" } else { "no" }
        ));
        let mut out = parts.join(" · ");
        let _ = write!(
            out,
            "\nlast answer: {}",
            backend.last_answer().map_or("none this run".into(), when)
        );
        if let Some(broadcaster) = ctx.try_get::<crate::broadcasting::Broadcaster>() {
            let state = match broadcaster.state() {
                crate::broadcasting::State::Connecting => "connecting",
                crate::broadcasting::State::Connected => "connected",
                crate::broadcasting::State::Offline => "not connected",
            };
            let _ = write!(out, "\nlive channel: {state}");
        }
        return out;
    }
    let _ = ctx;
    "no backend".into()
}

async fn outbox(ctx: &Ctx) -> String {
    #[cfg(all(feature = "backend", feature = "database"))]
    if let Some(outbox) = ctx.try_get::<crate::offline::Outbox>() {
        let mut text = outbox_text(&outbox.pending().await, &outbox.failed().await);
        if let Some(at) = outbox.last_sent() {
            let _ = write!(text, "\nlast sent: {}", when(at));
        }
        return text;
    }
    let _ = ctx;
    "no outbox".into()
}

/// The counts, by resource, and each refused write: its method, path and the
/// server's answer — never its body.
#[cfg(all(feature = "backend", feature = "database"))]
fn outbox_text(
    pending: &[crate::offline::OutboxItem],
    failed: &[crate::offline::OutboxItem],
) -> String {
    let by_resource = |items: &[crate::offline::OutboxItem]| {
        let mut counts: Vec<(String, usize)> = Vec::new();
        for item in items {
            let key = crate::backend::resource_key(&item.path);
            let name = key
                .rsplit('/')
                .next()
                .filter(|n| !n.is_empty())
                .unwrap_or("/")
                .to_owned();
            match counts.iter_mut().find(|(n, _)| *n == name) {
                Some((_, count)) => *count += 1,
                None => counts.push((name, 1)),
            }
        }
        counts
            .iter()
            .map(|(n, c)| format!("{n}: {c}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut out = format!("waiting: {}", pending.len());
    if !pending.is_empty() {
        let _ = write!(out, " ({})", by_resource(pending));
    }
    let _ = write!(out, " · refused: {}", failed.len());
    for item in failed.iter().rev().take(20).rev() {
        let path = item.path.split('?').next().unwrap_or(&item.path);
        let _ = write!(out, "\nrefused: {} {path}", item.method);
        if let Some(reason) = &item.reason {
            let _ = write!(out, " — {reason}");
        }
        if let Some(said) = refusal(item) {
            let _ = write!(out, ": {:?}", clip(&said, 300));
        }
    }
    if failed.len() > 20 {
        let _ = write!(out, "\n(and {} earlier)", failed.len() - 20);
    }
    out
}

/// What the server said of a refused write: a `422`'s first message per
/// field (`due: The due date must be…`), else its message.
#[cfg(all(feature = "backend", feature = "database"))]
fn refusal(item: &crate::offline::OutboxItem) -> Option<String> {
    let bag = item.errors.clone().or_else(|| {
        item.error
            .as_deref()
            .and_then(|e| serde_json::from_str::<serde_json::Value>(e).ok())
    });
    if let Some(serde_json::Value::Object(fields)) = bag {
        let said: Vec<String> = fields
            .iter()
            .filter_map(|(field, messages)| {
                let first = messages.as_array()?.first()?.as_str()?;
                Some(format!("{field}: {first}"))
            })
            .collect();
        if !said.is_empty() {
            return Some(said.join("; "));
        }
    }
    item.error.clone()
}

fn updates(ctx: &Ctx) -> String {
    #[cfg(feature = "updater")]
    if let Some(updater) = ctx.try_get::<crate::updater::UpdaterRuntime>() {
        let mut out = format!("from: {}", updater.source());
        match updater.last_check() {
            None => out.push_str(" · checked: not this run"),
            Some(check) => {
                let _ = write!(out, " · checked: {}", local(check.at, "%H:%M"));
                match check.found {
                    Ok(None) => out.push_str(" · up to date"),
                    Ok(Some(version)) => {
                        let _ = write!(out, " · newest: {version}");
                    }
                    Err(e) => {
                        let _ = write!(out, " · failed: {}", clip(&e, 300));
                    }
                }
            }
        }
        if let Some(version) = updater.installed() {
            let _ = write!(out, "\ninstalled: {version}, waiting for a restart");
        }
        return out;
    }
    let _ = ctx;
    "no updater".into()
}

fn windows(seen: Option<&Seen>) -> String {
    let Some(seen) = seen else {
        return "not known (no event loop)".into();
    };
    seen.windows
        .iter()
        .map(|w| {
            let mut state = vec![if w.visible { "shown" } else { "hidden" }];
            if w.focused {
                state.push("focused");
            }
            match &w.path {
                Some(path) => format!("{} ({}, {path})", w.label, state.join(", ")),
                None => format!("{} ({})", w.label, state.join(", ")),
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn page(errors: Vec<(SystemTime, String, PageError)>) -> String {
    if errors.is_empty() {
        return "no errors".into();
    }
    let mut out = String::new();
    for (at, window, error) in errors {
        let kind = match error.kind.as_deref() {
            Some("rejection") => "Rejection",
            _ => "Error",
        };
        let _ = writeln!(
            out,
            "{} {kind:<9} [{window}] {}",
            local(at, "%H:%M:%S"),
            error.message
        );
        if let Some(place) = &error.at {
            let _ = writeln!(out, "                   at {place}");
        }
    }
    out
}

/// The last `count` lines of the log: from the log file when there is one,
/// which reaches back further, else the records kept in memory.
pub(crate) fn log_lines(count: usize) -> Vec<String> {
    if let Some(lines) = crate::log::log_path().and_then(|p| file_tail(&p, count)) {
        if !lines.is_empty() {
            return lines;
        }
    }
    crate::log::recent(count)
        .into_iter()
        .map(|r| {
            format!(
                "{} {:<5} [{}] {}",
                local(r.at, "%m-%d %H:%M:%S"),
                r.level.label(),
                r.target,
                r.message
            )
        })
        .collect()
}

fn file_tail(path: &std::path::Path, count: usize) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = text.lines().collect();
    let from = lines.len().saturating_sub(count);
    Some(lines[from..].iter().map(|l| local_line(l)).collect())
}

/// A file line's UTC timestamp in the user's own time, as the rest of the
/// file is.
fn local_line(line: &str) -> String {
    let stamp = line
        .get(..24)
        .and_then(|s| s.parse::<jiff::Timestamp>().ok());
    match stamp {
        Some(at) => format!(
            "{}{}",
            at.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%m-%d %H:%M:%S"),
            &line[24..]
        ),
        None => line.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Setup {
        Setup {
            name: "Acme Notes".into(),
            version: "2.3.1".into(),
            identifier: "com.acme.notes".into(),
            extra: Vec::new(),
        }
    }

    fn ctx() -> Ctx {
        Ctx::new(Arc::new(crate::container::Container::new()))
    }

    #[test]
    fn names_the_file_after_the_app_and_the_minute() {
        let name = file_name(&setup());
        assert!(name.starts_with("acme-notes-diagnostics-"), "{name}");
        assert!(name.ends_with(".txt"));
        // acme-notes-diagnostics-2026-11-19-1402.txt
        assert_eq!(
            name.len(),
            "acme-notes-diagnostics-2026-11-19-1402.txt".len()
        );
    }

    #[tokio::test]
    async fn every_section_is_there_without_a_backend() {
        let text = text_with(&ctx(), &setup(), None).await;
        assert!(
            text.starts_with("Elyra diagnostics — Acme Notes 2.3.1\nSaved "),
            "{text}"
        );
        for heading in [
            "## App",
            "## System",
            "## Connection",
            "## Outbox",
            "## Updates",
            "## Windows",
            "## The page",
            "## Log (the last 300 lines)",
        ] {
            assert!(text.contains(heading), "{heading} in {text}");
        }
        assert!(text.contains("identifier: com.acme.notes"));
        assert!(text.contains(&format!("elyra: {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("no backend"));
        assert!(text.contains("no outbox"));
        assert!(text.contains(&format!("arch: {}", std::env::consts::ARCH)));
        assert!(!text.contains("## From the app"));
    }

    #[tokio::test]
    async fn the_system_section_names_the_os() {
        let text = text_with(&ctx(), &setup(), None).await;
        let os = if cfg!(target_os = "macos") {
            "os: macOS"
        } else if cfg!(target_os = "windows") {
            "os: Windows"
        } else {
            "os: "
        };
        assert!(text.contains(os), "{text}");
    }

    #[tokio::test]
    async fn the_apps_sections_come_last_and_are_redacted() {
        let mut setup = setup();
        setup.add(|d| {
            d.section("Sync", "last full sync: 14:02 with token=abc123");
        });
        let text = text_with(&ctx(), &setup, None).await;
        let at = text.find("## From the app").expect("the app's part");
        assert!(at > text.find("## Log").unwrap());
        assert!(
            text.contains("## Sync\nlast full sync: 14:02 with token=•••"),
            "{text}"
        );
        assert!(!text.contains("abc123"));
    }

    #[tokio::test]
    async fn the_home_folder_is_a_tilde() {
        let Some(home) = home() else { return };
        let mut setup = setup();
        let file = format!("{home}/Documents/notes.db");
        setup.add(move |d| {
            d.section("Files", format!("database: {file}"));
        });
        let text = text_with(&ctx(), &setup, None).await;
        assert!(text.contains("database: ~/Documents/notes.db"), "{text}");
        assert!(!text.contains(&home));
    }

    #[test]
    fn a_window_reports_so_many_errors_a_minute_and_then_again() {
        let _lock = crate::log::TEST_LOCK.lock();
        let errors = PageErrors::default();
        let start = Instant::now();
        for _ in 0..PAGE_ERRORS_A_MINUTE {
            assert!(errors.admit("main", start));
        }
        assert!(!errors.admit("main", start + Duration::from_secs(30)));
        assert!(
            errors.admit("settings", start + Duration::from_secs(30)),
            "per window"
        );
        assert!(
            errors.admit("main", start + Duration::from_secs(61)),
            "a new minute"
        );
    }

    #[test]
    fn home_is_written_as_a_tilde_with_either_slash() {
        let home = r"C:\Users\kari";
        let text = r"database: sqlite:///C:/Users/kari/AppData/app.db, log: C:\Users\kari\app.log";
        assert_eq!(
            tidy_with(text, Some(home)),
            r"database: sqlite:///~/AppData/app.db, log: ~\app.log"
        );
        assert_eq!(
            tidy_with("/Users/kari/notes.db", Some("/Users/kari")),
            "~/notes.db"
        );
    }

    #[test]
    fn the_page_errors_are_kept_and_logged() {
        let _lock = crate::log::TEST_LOCK.lock();
        crate::log::set_level(Some(crate::log::Level::Info));
        let errors = PageErrors::default();
        for i in 0..PAGE_ERRORS + 3 {
            errors.record(
                "main",
                PageError {
                    message: format!("TypeError: boom {i}"),
                    at: Some("CustomerCard.svelte:41".into()),
                    kind: None,
                },
            );
        }
        errors.record(
            "main",
            PageError {
                message: "an answer was not JSON".into(),
                at: None,
                kind: Some("rejection".into()),
            },
        );
        assert_eq!(errors.all().len(), PAGE_ERRORS);
        let text = page(errors.all());
        assert!(!text.contains("boom 2\n"), "the oldest are dropped");
        assert!(
            text.contains("Error     [main] TypeError: boom 52"),
            "{text}"
        );
        assert!(text.contains("at CustomerCard.svelte:41"));
        assert!(text.contains("Rejection [main] an answer was not JSON"));
        // And in the log, for a log file and `rata mcp dev`'s last_error.
        assert!(crate::log::recent(crate::log::KEPT)
            .iter()
            .any(|r| r.target == "page"
                && r.level == crate::log::Level::Error
                && r.message == "an unhandled rejection in `main`: an answer was not JSON"));
    }

    #[tokio::test]
    async fn what_the_event_loop_saw() {
        let seen = Seen {
            windows: vec![
                WindowSeen {
                    label: "main".into(),
                    visible: true,
                    focused: true,
                    path: Some("/customers/7".into()),
                },
                WindowSeen {
                    label: "settings".into(),
                    visible: false,
                    focused: false,
                    path: None,
                },
            ],
            screens: vec![1.0, 2.0],
            dark: Some(true),
        };
        let text = text_with(&ctx(), &setup(), Some(seen)).await;
        assert!(
            text.contains("main (shown, focused, /customers/7) · settings (hidden)"),
            "{text}"
        );
        assert!(
            text.contains("screens: 2 (1x, 2x) · dark mode: on"),
            "{text}"
        );
    }

    #[test]
    fn the_log_is_there_redacted_whatever_the_sinks() {
        let _lock = crate::log::TEST_LOCK.lock();
        crate::log::set_level(Some(crate::log::Level::Info));
        crate::log::log(
            crate::log::Level::Warn,
            "elyra::backend",
            "GET /api/customers/7: 500 with Authorization: Bearer sekrit",
        );
        // All that is kept: other tests log at the same time.
        let lines = log_lines(crate::log::KEPT);
        let line = lines
            .iter()
            .rev()
            .find(|l| l.contains("GET /api/customers/7"))
            .expect("the line");
        assert!(
            line.contains(
                "WARN  [elyra::backend] GET /api/customers/7: 500 with Authorization: •••"
            ),
            "{line}"
        );
    }

    #[test]
    fn file_lines_are_in_local_time() {
        let line = local_line("2026-11-19T13:02:00.000Z INFO  [elyra::auth] signed in");
        assert!(line.ends_with(" INFO  [elyra::auth] signed in"), "{line}");
        assert!(
            line.starts_with("11-19 ") || line.starts_with("11-20 ") || line.starts_with("11-18 "),
            "{line}"
        );
        assert_eq!(local_line("  a continuation"), "  a continuation");
    }

    #[test]
    fn a_page_is_named_by_its_path_without_its_query() {
        assert_eq!(
            page_path("elyra://localhost/customers/7").as_deref(),
            Some("/customers/7")
        );
        assert_eq!(
            page_path("http://elyra.localhost/files?signature=abc").as_deref(),
            Some("/files")
        );
        assert_eq!(page_path("http://localhost:5173/").as_deref(), Some("/"));
        assert_eq!(
            page_path("elyra://localhost/#/customers/7?tab=notes").as_deref(),
            Some("/#/customers/7")
        );
        assert_eq!(page_path("elyra://localhost").as_deref(), Some("/"));
    }

    #[test]
    fn the_app_s_memory_is_read_and_said_in_megabytes() {
        let bytes = memory().expect("read on macOS, Linux and Windows");
        // A test process: more than a megabyte, less than a lot.
        assert!(
            bytes > 1024 * 1024 && bytes < 64 * 1024 * 1024 * 1024,
            "{bytes}"
        );
        eprintln!("memory: {}", megabytes(bytes));
        assert_eq!(megabytes(150 * 1024 * 1024 + 600 * 1024), "151 MB");
        assert_eq!(megabytes(150 * 1024 * 1024 + 100 * 1024), "150 MB");
    }

    #[test]
    fn durations_read_as_hours_and_minutes() {
        assert_eq!(duration(Duration::from_secs(42)), "42 s");
        assert_eq!(duration(Duration::from_secs(5 * 60 + 3)), "5 min");
        assert_eq!(
            duration(Duration::from_secs(3 * 3600 + 12 * 60)),
            "3 h 12 min"
        );
    }

    #[cfg(all(feature = "backend", feature = "database"))]
    #[test]
    fn the_outbox_says_what_waits_and_why_a_write_was_refused_without_its_body() {
        use crate::offline::{OutboxItem, OutboxStatus};
        let item = |id, method: &str, path: &str, failed: bool| OutboxItem {
            id,
            method: method.into(),
            path: path.into(),
            body: Some(serde_json::json!({ "email": "kari@example.no" })),
            status: if failed {
                OutboxStatus::Failed
            } else {
                OutboxStatus::Pending
            },
            attempts: 1,
            error: failed
                .then(|| r#"{"due":["The due date must be after the issue date."]}"#.into()),
            errors: None,
            reason: failed.then(|| "validation".into()),
            created_at: 0,
        };
        let text = outbox_text(
            &[
                item(1, "POST", "/api/customers", false),
                item(2, "PATCH", "/api/customers/4", false),
                item(3, "POST", "/api/invoices", false),
            ],
            &[item(4, "PATCH", "/api/invoices/881?expand=lines", true)],
        );
        assert_eq!(
            text,
            "waiting: 3 (customers: 2, invoices: 1) · refused: 1\n\
             refused: PATCH /api/invoices/881 — validation: \"due: The due date must be after the issue date.\""
        );
        assert!(!text.contains("kari@"));
    }
}
