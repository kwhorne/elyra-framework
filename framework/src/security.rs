//! The IPC security policy: origin handling, the per-run IPC token, and the
//! allowlists that gate the dangerous native operations.
//!
//! Everything under `elyra://localhost/__*` is a native capability: commands,
//! the filesystem storage facade, the settings store, the clipboard, sidecar
//! processes, the updater. Any script running in the webview can reach those
//! endpoints, so the shell must decide *who* is allowed to call them — not just
//! *what* the call does.
//!
//! Three mechanisms, all enforced in [`crate::shell`]:
//!
//! 1. **Token** — a random, per-run secret injected into the webview before any
//!    page script runs (`window.__ELYRA__.token`) and required on every `/__*`
//!    request. A page the app never loaded (a remote iframe, an injected
//!    `<script src>` fetching cross-origin) doesn't have it.
//! 2. **Origin** — CORS headers are only emitted for the dev server's exact
//!    origin, and only when `ELYRA_DEV_URL` is set. In a production build there
//!    are no `Access-Control-Allow-*` headers at all, so a foreign origin can't
//!    read a response even if it manages to issue the request.
//! 3. **Allowlists** — `shell.open` accepts only approved URL schemes (and
//!    refuses executable files), and `sidecar` refuses to spawn any program the
//!    app hasn't explicitly allowed.
//!
//! See `App::allow_open_schemes` and `App::sidecar_allow` to widen the last one.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A native capability the **frontend** can ask the shell for.
///
/// Each `/__*` route maps to one of these (see [`Capability::for_route`]). The
/// everyday ones are granted by default; the destructive or expensive ones are
/// opt-in through `App::allow_frontend`, so a stray script can't wipe a user's
/// settings or trigger a binary replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Invoke `#[command]` handlers (`/__cmd/*`).
    Commands,
    /// Window control (`/__window/*`).
    Window,
    /// Read/write the settings store (`/__store/{get,set,delete,all}`).
    Store,
    /// Wipe the whole settings store (`/__store/clear`). **Opt-in.**
    StoreClear,
    /// Read/write the cache (`/__cache/*` except `flush`).
    Cache,
    /// Drop the entire cache (`/__cache/flush`). **Opt-in.**
    CacheFlush,
    /// Read from the storage disk (`get` / `exists` / `size` / `url` / `files`).
    StorageRead,
    /// Write to the storage disk (`put`).
    StorageWrite,
    /// Delete files on the storage disk. **Opt-in.**
    StorageDelete,
    /// Enqueue background jobs (`/__queue/push`).
    Queue,
    /// Spawn/write/kill sidecar processes (also program-allowlisted).
    Sidecar,
    /// Native dialogs, clipboard, notifications, paths (`/__sys/*`).
    System,
    /// Launch-at-login control (`/__autostart/*`).
    Autostart,
    /// Check for updates (`/__update/check`).
    Updater,
    /// Download + apply an update and relaunch. **Opt-in.**
    UpdaterInstall,
}

impl Capability {
    /// The capability a request path + op requires, or `None` for the routes that
    /// are always available (`/__events`, `/__about`, `/__deeplink/*`, `/__cancel`).
    pub fn for_route(path: &str) -> Option<Capability> {
        // `op` is the last path segment, which distinguishes e.g. a cache read
        // from a cache flush.
        let op = path.rsplit('/').next().unwrap_or_default();
        if path.starts_with("/__cmd/") {
            return Some(Capability::Commands);
        }
        if path.starts_with("/__window/") {
            return Some(Capability::Window);
        }
        if path.starts_with("/__store/") {
            return Some(match op {
                "clear" => Capability::StoreClear,
                _ => Capability::Store,
            });
        }
        if path.starts_with("/__cache/") {
            return Some(match op {
                "flush" => Capability::CacheFlush,
                _ => Capability::Cache,
            });
        }
        if path.starts_with("/__storage/") {
            return Some(match op {
                "put" => Capability::StorageWrite,
                "delete" => Capability::StorageDelete,
                _ => Capability::StorageRead,
            });
        }
        if path.starts_with("/__queue/") {
            return Some(Capability::Queue);
        }
        if path.starts_with("/__sidecar/") {
            return Some(Capability::Sidecar);
        }
        if path.starts_with("/__sys/") {
            return Some(Capability::System);
        }
        if path.starts_with("/__autostart/") {
            return Some(Capability::Autostart);
        }
        if path == "/__update/install" {
            return Some(Capability::UpdaterInstall);
        }
        if path.starts_with("/__update/") {
            return Some(Capability::Updater);
        }
        None
    }

    /// Granted to the frontend unless the app revokes them.
    pub fn defaults() -> &'static [Capability] {
        &[
            Capability::Commands,
            Capability::Window,
            Capability::Store,
            Capability::Cache,
            Capability::StorageRead,
            Capability::StorageWrite,
            Capability::Queue,
            Capability::Sidecar,
            Capability::System,
            Capability::Autostart,
            Capability::Updater,
        ]
    }

    /// How many calls per window this capability tolerates, if it's gated.
    /// Keeps a runaway loop (or a hostile script) from hammering an expensive,
    /// side-effecting operation.
    fn rate_limit(self) -> Option<(u32, Duration)> {
        match self {
            // One install attempt per 10s: it downloads, verifies and relaunches.
            Capability::UpdaterInstall => Some((1, Duration::from_secs(10))),
            Capability::Sidecar => Some((30, Duration::from_secs(60))),
            Capability::Updater => Some((6, Duration::from_secs(60))),
            _ => None,
        }
    }
}

/// A tiny sliding-window counter for the rate-limited capabilities.
#[derive(Default)]
struct RateGate {
    hits: Mutex<HashMap<Capability, Vec<Instant>>>,
}

impl RateGate {
    /// Record a call, returning `false` when the limit is exceeded.
    fn allow(&self, capability: Capability) -> bool {
        let Some((max, window)) = capability.rate_limit() else {
            return true;
        };
        let mut hits = self.hits.lock();
        let entry = hits.entry(capability).or_default();
        entry.retain(|t| t.elapsed() < window);
        if entry.len() as u32 >= max {
            return false;
        }
        entry.push(Instant::now());
        true
    }
}

/// URL schemes `shell.open` accepts unless the app widens the list.
const DEFAULT_OPEN_SCHEMES: &[&str] = &["http", "https", "mailto"];

/// Extensions that are executable (or executable-adjacent) on some platform.
/// `shell.open` refuses these outright: "open this file with the OS default
/// handler" must never become "run this program".
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe",
    "bat",
    "cmd",
    "com",
    "msi",
    "msix",
    "scr",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "lnk",
    "reg",
    "sh",
    "bash",
    "zsh",
    "command",
    "app",
    "scpt",
    "applescript",
    "jar",
    "pkg",
    "dmg",
    "deb",
    "rpm",
    "run",
    "appimage",
    "desktop",
    "so",
    "dylib",
    "dll",
    "py",
    "rb",
    "pl",
    "php",
];

/// The app's IPC security policy. Built by [`crate::App`], held by the shell,
/// and bound in the container so commands can consult it.
#[derive(Clone)]
pub struct Policy {
    /// Random per-run secret required on every `/__*` request.
    token: String,
    /// The dev server's origin (from `ELYRA_DEV_URL`), if we're running under
    /// `rata dev`. `None` in a production build — and then no CORS at all.
    dev_origin: Option<String>,
    /// Extra URL schemes `shell.open` may hand to the OS.
    open_schemes: Vec<String>,
    /// Whether `shell.open` may open local paths (non-executable ones).
    open_paths: bool,
    /// Programs the frontend is allowed to spawn as sidecars.
    sidecar_allow: Vec<String>,
    /// Native capabilities the frontend is granted.
    capabilities: HashSet<Capability>,
    /// Sliding-window limiter for the expensive capabilities.
    rate_gate: Arc<RateGate>,
    /// Maximum accepted request body, in bytes.
    max_body: usize,
}

impl Policy {
    /// Build the policy for this run: a fresh token, plus the app's allowlists.
    pub(crate) fn new(
        open_schemes: Vec<String>,
        open_paths: bool,
        sidecar_allow: Vec<String>,
        capabilities: HashSet<Capability>,
        max_body: usize,
    ) -> Self {
        Self {
            token: random_token(),
            dev_origin: dev_origin(),
            open_schemes,
            open_paths,
            sidecar_allow,
            capabilities,
            rate_gate: Arc::new(RateGate::default()),
            max_body,
        }
    }

    /// Whether the frontend may use `capability`.
    pub fn grants(&self, capability: Capability) -> bool {
        self.capabilities.contains(&capability)
    }

    /// Check a request path against the capability set + rate limits.
    ///
    /// `Ok(())` for the always-available routes and for granted capabilities that
    /// are under their limit; `Err(reason)` otherwise.
    pub fn allows_route(&self, path: &str) -> Result<(), RouteDenied> {
        let Some(capability) = Capability::for_route(path) else {
            return Ok(());
        };
        if !self.grants(capability) {
            return Err(RouteDenied::NotGranted(capability));
        }
        if !self.rate_gate.allow(capability) {
            return Err(RouteDenied::RateLimited(capability));
        }
        Ok(())
    }

    /// The maximum request body this app accepts on `/__*`.
    pub fn max_body(&self) -> usize {
        self.max_body
    }

    /// A policy for tests: known token, no dev origin, nothing allowed.
    #[cfg(test)]
    pub(crate) fn test_policy() -> Self {
        Self {
            token: "test-token".into(),
            dev_origin: None,
            open_schemes: Vec::new(),
            open_paths: true,
            sidecar_allow: Vec::new(),
            capabilities: Capability::defaults().iter().copied().collect(),
            rate_gate: Arc::new(RateGate::default()),
            max_body: crate::wire::DEFAULT_MAX_BODY,
        }
    }

    /// A test policy that behaves as if `ELYRA_DEV_URL` were set.
    #[cfg(test)]
    pub(crate) fn test_policy_with_dev_origin(origin: &str) -> Self {
        Self {
            dev_origin: Some(origin.to_owned()),
            ..Self::test_policy()
        }
    }

    /// The per-run IPC token.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The dev-server origin CORS is allowed for, if any.
    pub fn dev_origin(&self) -> Option<&str> {
        self.dev_origin.as_deref()
    }

    /// Whether `token` matches this run's token (length-independent compare, so
    /// a wrong-length guess costs the same as a wrong-value one).
    pub fn token_matches(&self, token: Option<&str>) -> bool {
        let Some(token) = token else {
            return false;
        };
        constant_time_eq(token.as_bytes(), self.token.as_bytes())
    }

    /// The JS injected before any page script, handing the frontend its token.
    /// Runs in every frame the webview creates, which is fine: the token only
    /// unlocks what the policy already allows.
    pub(crate) fn init_script(&self) -> String {
        // The token is a hex string, so it can't break out of the literal.
        format!(
            "globalThis.__ELYRA__ = Object.freeze({{ token: \"{}\" }});",
            self.token
        )
    }

    /// Whether `shell.open` may hand `target` to the OS.
    ///
    /// Accepts approved URL schemes, and (when enabled) local paths that exist
    /// and aren't executables. Everything else is refused.
    pub fn allows_open(&self, target: &str) -> Result<(), String> {
        let target = target.trim();
        if target.is_empty() {
            return Err("shell.open: empty target".into());
        }

        if let Some(scheme) = url_scheme(target) {
            let allowed = DEFAULT_OPEN_SCHEMES.contains(&scheme.as_str())
                || self.open_schemes.iter().any(|s| s == &scheme);
            if !allowed {
                return Err(format!(
                    "shell.open: scheme `{scheme}` is not allowed by policy \
                     (add it with App::allow_open_schemes)"
                ));
            }
            if scheme == "file" {
                return Err("shell.open: `file:` URLs are not allowed; pass a path instead".into());
            }
            return Ok(());
        }

        if !self.open_paths {
            return Err("shell.open: opening local paths is disabled by policy".into());
        }
        let path = Path::new(target);
        if !path.is_absolute() {
            return Err("shell.open: only absolute paths are allowed".into());
        }
        if is_executable_path(path) {
            return Err("shell.open: refusing to open an executable file".into());
        }
        if !path.exists() {
            return Err("shell.open: no such file or directory".into());
        }
        Ok(())
    }

    /// Whether the **frontend** may spawn `program` as a sidecar. Default deny:
    /// an app must name the programs it ships with.
    pub fn allows_sidecar(&self, program: &str) -> Result<(), String> {
        if self.sidecar_allow.is_empty() {
            return Err("sidecar: no programs are allowed for frontend spawn \
                 (allow one with App::sidecar_allow)"
                .into());
        }
        let requested = Path::new(program);
        let requested_name = requested.file_name().and_then(|n| n.to_str());
        let allowed = self.sidecar_allow.iter().any(|entry| {
            if entry == program {
                return true;
            }
            // An allowlist entry may be a bare program name; match it against the
            // requested file name so `/usr/bin/ffmpeg` matches `ffmpeg`.
            match (
                Path::new(entry).file_name().and_then(|n| n.to_str()),
                requested_name,
            ) {
                (Some(a), Some(b)) => a == b && !entry.contains('/') && !entry.contains('\\'),
                _ => false,
            }
        });
        if allowed {
            Ok(())
        } else {
            Err(format!(
                "sidecar: `{program}` is not in the allowlist \
                 (allow it with App::sidecar_allow)"
            ))
        }
    }
}

/// Why a `/__*` route was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDenied {
    /// The app hasn't granted this capability to the frontend.
    NotGranted(Capability),
    /// Granted, but called too often.
    RateLimited(Capability),
}

impl std::fmt::Display for RouteDenied {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouteDenied::NotGranted(c) => write!(
                f,
                "capability {c:?} is not granted to the frontend \
                 (grant it with App::allow_frontend)"
            ),
            RouteDenied::RateLimited(c) => {
                write!(
                    f,
                    "capability {c:?} was called too often; try again shortly"
                )
            }
        }
    }
}

/// The default Content-Security-Policy served with HTML responses.
///
/// Local-app shaped: only our own origin may supply code, inline styles are
/// allowed (component frameworks inject them), `data:`/`blob:` images are fine,
/// and plugins/embedding are off. Override with `App::csp`, or turn it off
/// entirely with `App::csp_disabled`.
pub const DEFAULT_CSP: &str = "default-src 'self' elyra:; \
     script-src 'self' elyra:; \
     style-src 'self' elyra: 'unsafe-inline'; \
     img-src 'self' elyra: data: blob:; \
     font-src 'self' elyra: data:; \
     media-src 'self' elyra: blob:; \
     connect-src 'self' elyra:; \
     object-src 'none'; \
     base-uri 'none'; \
     frame-ancestors 'none'";

/// The dev server's origin, when running under `rata dev`.
fn dev_origin() -> Option<String> {
    let url = std::env::var("ELYRA_DEV_URL").ok()?;
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    // scheme://host[:port] — drop any path so the value can be compared to an
    // `Origin` header verbatim.
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split('/').next()?;
    Some(format!("{scheme}://{host}"))
}

/// The scheme of `target` if it looks like a URL (`scheme://…` or `mailto:…`),
/// lowercased. Windows drive letters (`C:\…`) are not schemes.
fn url_scheme(target: &str) -> Option<String> {
    let (scheme, rest) = target.split_once(':')?;
    if scheme.len() < 2 || rest.is_empty() {
        return None; // `C:\path` — a Windows path, not a URL
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.')
    {
        return None;
    }
    if !scheme.chars().next()?.is_ascii_alphabetic() {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Whether the path's extension is executable on any supported platform.
fn is_executable_path(path: &Path) -> bool {
    // `.app` and `.appimage` are directories/bundles, so check every component:
    // `/Applications/Evil.app/Contents/MacOS/evil` must not slip through.
    path.components().any(|c| {
        Path::new(c.as_os_str())
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| EXECUTABLE_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    })
}

/// Compare two byte strings without an early return on the first difference.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u8;
    let n = a.len().min(b.len());
    for i in 0..n {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// A 256-bit random hex token, shared with the single-instance handshake.
pub(crate) fn random_hex_token() -> String {
    random_token()
}

/// A 256-bit random hex token.
///
/// Uses `RandomState`, whose keys come from OS entropy (the same source
/// `HashMap` relies on for HashDoS resistance) — no extra dependency, and the
/// value only has to be unguessable for the lifetime of this process.
fn random_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};

    let mut token = String::with_capacity(64);
    for round in 0..4u64 {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u64(std::process::id() as u64);
        hasher.write_u64(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(round),
        );
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            open_schemes: vec!["slack".into()],
            sidecar_allow: vec!["ffmpeg".into(), "/opt/tools/helper".into()],
            ..Policy::test_policy()
        }
    }

    #[test]
    fn routes_map_to_capabilities() {
        assert_eq!(
            Capability::for_route("/__cmd/greet"),
            Some(Capability::Commands)
        );
        assert_eq!(
            Capability::for_route("/__store/set"),
            Some(Capability::Store)
        );
        assert_eq!(
            Capability::for_route("/__store/clear"),
            Some(Capability::StoreClear)
        );
        assert_eq!(
            Capability::for_route("/__cache/get"),
            Some(Capability::Cache)
        );
        assert_eq!(
            Capability::for_route("/__cache/flush"),
            Some(Capability::CacheFlush)
        );
        assert_eq!(
            Capability::for_route("/__storage/get"),
            Some(Capability::StorageRead)
        );
        assert_eq!(
            Capability::for_route("/__storage/put"),
            Some(Capability::StorageWrite)
        );
        assert_eq!(
            Capability::for_route("/__storage/delete"),
            Some(Capability::StorageDelete)
        );
        assert_eq!(
            Capability::for_route("/__update/check"),
            Some(Capability::Updater)
        );
        assert_eq!(
            Capability::for_route("/__update/install"),
            Some(Capability::UpdaterInstall)
        );
        // Always-available routes need no capability.
        assert_eq!(Capability::for_route("/__events"), None);
        assert_eq!(Capability::for_route("/__about"), None);
        assert_eq!(Capability::for_route("/__cancel"), None);
        assert_eq!(Capability::for_route("/__deeplink/initial"), None);
    }

    #[test]
    fn destructive_capabilities_are_opt_in() {
        let p = policy();
        assert!(p.allows_route("/__store/set").is_ok());
        assert!(p.allows_route("/__cache/get").is_ok());
        assert!(p.allows_route("/__storage/put").is_ok());
        assert!(p.allows_route("/__events").is_ok());

        assert_eq!(
            p.allows_route("/__store/clear"),
            Err(RouteDenied::NotGranted(Capability::StoreClear))
        );
        assert_eq!(
            p.allows_route("/__cache/flush"),
            Err(RouteDenied::NotGranted(Capability::CacheFlush))
        );
        assert_eq!(
            p.allows_route("/__storage/delete"),
            Err(RouteDenied::NotGranted(Capability::StorageDelete))
        );
        assert_eq!(
            p.allows_route("/__update/install"),
            Err(RouteDenied::NotGranted(Capability::UpdaterInstall))
        );
    }

    #[test]
    fn granting_a_capability_opens_the_route() {
        let mut p = policy();
        p.capabilities.insert(Capability::StoreClear);
        assert!(p.allows_route("/__store/clear").is_ok());
    }

    #[test]
    fn expensive_capabilities_are_rate_limited() {
        let mut p = policy();
        p.capabilities.insert(Capability::UpdaterInstall);
        assert!(p.allows_route("/__update/install").is_ok());
        // One install per 10s — the second attempt is throttled, not denied.
        assert_eq!(
            p.allows_route("/__update/install"),
            Err(RouteDenied::RateLimited(Capability::UpdaterInstall))
        );
        // An unrelated route is unaffected.
        assert!(p.allows_route("/__store/set").is_ok());
    }

    #[test]
    fn tokens_are_random_and_long() {
        let a = random_token();
        let b = random_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn token_must_match_exactly() {
        let p = policy();
        assert!(p.token_matches(Some("test-token")));
        assert!(!p.token_matches(Some("test-tokeN")));
        assert!(!p.token_matches(Some("test-toke")));
        assert!(!p.token_matches(Some("")));
        assert!(!p.token_matches(None));
    }

    #[test]
    fn open_allows_web_schemes_and_extra_schemes() {
        let p = policy();
        assert!(p.allows_open("https://example.com").is_ok());
        assert!(p.allows_open("http://localhost:5173").is_ok());
        assert!(p.allows_open("mailto:a@b.c").is_ok());
        assert!(p.allows_open("slack://channel?id=1").is_ok());
    }

    #[test]
    fn open_rejects_unapproved_schemes_and_executables() {
        let p = policy();
        assert!(p.allows_open("smb://share/x").is_err());
        assert!(p.allows_open("javascript:alert(1)").is_err());
        assert!(p.allows_open("file:///etc/passwd").is_err());
        assert!(p.allows_open("").is_err());
        // Executables, by extension anywhere in the path.
        assert!(p.allows_open("/Applications/Evil.app").is_err());
        assert!(p
            .allows_open("/Applications/Evil.app/Contents/MacOS/evil")
            .is_err());
        assert!(p.allows_open("/tmp/payload.sh").is_err());
        assert!(p.allows_open("C:\\Windows\\System32\\cmd.exe").is_err());
        // Relative paths are refused even when they exist.
        assert!(p.allows_open("Cargo.toml").is_err());
        // A non-existent (but otherwise fine) path is refused too.
        assert!(p.allows_open("/definitely/not/here.pdf").is_err());
    }

    #[test]
    fn open_allows_an_existing_harmless_file() {
        let p = policy();
        let file = std::env::temp_dir().join("elyra-policy-open-test.txt");
        std::fs::write(&file, b"x").unwrap();
        assert!(p.allows_open(&file.display().to_string()).is_ok());
        let _ = std::fs::remove_file(file);
    }

    #[test]
    fn open_paths_can_be_disabled() {
        let mut p = policy();
        p.open_paths = false;
        assert!(p.allows_open("/tmp").is_err());
        assert!(p.allows_open("https://example.com").is_ok());
    }

    #[test]
    fn sidecar_defaults_to_deny() {
        let mut p = policy();
        p.sidecar_allow.clear();
        assert!(p.allows_sidecar("ffmpeg").is_err());
        assert!(p.allows_sidecar("/bin/sh").is_err());
    }

    #[test]
    fn sidecar_matches_names_and_exact_paths() {
        let p = policy();
        assert!(p.allows_sidecar("ffmpeg").is_ok());
        assert!(p.allows_sidecar("/usr/local/bin/ffmpeg").is_ok());
        assert!(p.allows_sidecar("/opt/tools/helper").is_ok());
        // An allowlist entry given as a path must match exactly.
        assert!(p.allows_sidecar("/tmp/helper").is_err());
        assert!(p.allows_sidecar("bash").is_err());
        assert!(p.allows_sidecar("/bin/sh").is_err());
    }

    #[test]
    fn dev_origin_is_scheme_and_host_only() {
        std::env::set_var("ELYRA_DEV_URL", "http://localhost:5173/some/path");
        assert_eq!(dev_origin().as_deref(), Some("http://localhost:5173"));
        std::env::remove_var("ELYRA_DEV_URL");
        assert_eq!(dev_origin(), None);
    }
}
