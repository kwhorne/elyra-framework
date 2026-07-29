//! Auto-updater (behind the `updater` feature).
//!
//! The security model mirrors Tauri's: releases are published as an artifact
//! plus an **ed25519 signature**, listed in a JSON manifest. The app ships the
//! matching public key. An update is only ever installed after its downloaded
//! bytes verify against that key — so a compromised update server still can't
//! push a malicious binary.
//!
//! ```ignore
//! let updater = Updater::new(PUBLIC_KEY_B64, env!("CARGO_PKG_VERSION"))?;
//! if let UpdateStatus::Available(info) =
//!     updater.check("https://releases.example.com/latest.json", &Updater::current_target())?
//! {
//!     let staged = updater.download_verified(&info)?; // signature-checked
//!     // ...then apply + relaunch (platform-specific; see `apply`).
//! }
//! ```
//!
//! ## What's verified here vs. what needs infra
//! `evaluate` (manifest parse + semver) and `verify` (ed25519) are pure and
//! unit-tested. `check` / `download_verified` do real HTTP. Replacing the
//! running binary and relaunching is inherently environment-specific and is
//! provided as a documented helper, not exercised in tests.

use std::collections::HashMap;
use std::path::PathBuf;

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};

/// Updater errors.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid public key")]
    PublicKey,
    #[error("invalid version: {0}")]
    Version(String),
    #[error("invalid manifest: {0}")]
    Manifest(String),
    #[error("no release for target `{0}`")]
    NoTarget(String),
    #[error("signature verification failed")]
    Signature,
    #[error("base64 decode failed")]
    Base64,
    #[error("http error: {0}")]
    Http(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("insecure url (https required): {0}")]
    Insecure(String),
    #[error("artifact is larger than the {limit} byte limit")]
    TooLarge { limit: u64 },
}

type Result<T> = std::result::Result<T, Error>;

/// Default cap on a downloaded artifact (1 GiB).
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

/// Details of an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub url: String,
    /// Base64 ed25519 signature of the artifact at `url`.
    pub signature: String,
}

/// Result of an update check.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    UpToDate,
    Available(UpdateInfo),
}

/// Serializable summary sent to the frontend by the `/__update/check` endpoint
/// and the `elyra:update` event (signature + URL stay server-side).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl From<UpdateStatus> for UpdateCheck {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::UpToDate => UpdateCheck {
                available: false,
                version: None,
                notes: None,
                error: None,
            },
            UpdateStatus::Available(info) => UpdateCheck {
                available: true,
                version: Some(info.version),
                notes: info.notes,
                error: None,
            },
        }
    }
}

/// Configuration for the framework's built-in update flow (`App::updater`).
#[derive(Clone)]
pub struct UpdaterConfig {
    /// Base64 ed25519 public key the app was built with.
    pub public_key: String,
    /// URL of the JSON manifest listing the latest release per platform.
    pub manifest_url: String,
    /// The running app's version (typically `env!("CARGO_PKG_VERSION")`).
    pub current_version: String,
    /// Check for updates silently on startup (default: false — opt in with
    /// [`auto_check`](UpdaterConfig::auto_check)).
    pub auto_check: bool,
    /// Allow plain `http://` for the manifest/artifact. **Off by default**: the
    /// ed25519 signature protects integrity, but plain HTTP still lets a network
    /// attacker hide that an update exists at all.
    pub allow_insecure: bool,
    /// Refuse an artifact larger than this (default 1 GiB), so a hostile or
    /// broken server can't make the app fill the disk.
    pub max_artifact_bytes: u64,
}

impl UpdaterConfig {
    /// Create a config from the public key, manifest URL, and current version.
    pub fn new(
        public_key: impl Into<String>,
        manifest_url: impl Into<String>,
        current_version: impl Into<String>,
    ) -> Self {
        Self {
            public_key: public_key.into(),
            manifest_url: manifest_url.into(),
            current_version: current_version.into(),
            auto_check: false,
            allow_insecure: false,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
        }
    }

    /// Permit plain `http://` URLs (test servers, intranet mirrors).
    pub fn allow_insecure(mut self, yes: bool) -> Self {
        self.allow_insecure = yes;
        self
    }

    /// Cap the accepted artifact size.
    pub fn max_artifact_bytes(mut self, bytes: u64) -> Self {
        self.max_artifact_bytes = bytes;
        self
    }

    /// Toggle the silent startup check (default: false). When on, the shell
    /// checks the manifest on launch and shows the toast if a newer release
    /// exists.
    pub fn auto_check(mut self, yes: bool) -> Self {
        self.auto_check = yes;
        self
    }

    /// Build the [`Updater`] this config describes.
    pub fn build(&self) -> Result<Updater> {
        Ok(Updater::new(&self.public_key, &self.current_version)?
            .allow_insecure(self.allow_insecure)
            .max_artifact_bytes(self.max_artifact_bytes))
    }
}

/// The assembled update runtime, bound in the container by [`crate::App`] and
/// used by the shell's `/__update/*` endpoints and startup auto-check.
pub struct UpdaterRuntime {
    pub updater: Updater,
    pub manifest_url: String,
    pub target: String,
    pub auto_check: bool,
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    notes: Option<String>,
    platforms: HashMap<String, PlatformRelease>,
}

#[derive(Deserialize)]
struct PlatformRelease {
    url: String,
    signature: String,
}

/// Checks for and verifies updates against a bundled public key.
pub struct Updater {
    public_key: VerifyingKey,
    current: Version,
    allow_insecure: bool,
    max_artifact_bytes: u64,
}

impl Updater {
    /// Create an updater from a base64 ed25519 public key and the current version.
    pub fn new(public_key_b64: &str, current_version: &str) -> Result<Self> {
        let bytes = b64(public_key_b64)?;
        let arr: [u8; 32] = bytes.as_slice().try_into().map_err(|_| Error::PublicKey)?;
        let public_key = VerifyingKey::from_bytes(&arr).map_err(|_| Error::PublicKey)?;
        let current = Version::parse(current_version).map_err(|e| Error::Version(e.to_string()))?;
        Ok(Self {
            public_key,
            current,
            allow_insecure: false,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
        })
    }

    /// Permit plain `http://` URLs (off by default).
    pub fn allow_insecure(mut self, yes: bool) -> Self {
        self.allow_insecure = yes;
        self
    }

    /// Cap the accepted artifact size (default 1 GiB).
    pub fn max_artifact_bytes(mut self, bytes: u64) -> Self {
        self.max_artifact_bytes = bytes;
        self
    }

    /// Reject a URL that isn't HTTPS, unless the app opted in to plain HTTP.
    fn check_url(&self, url: &str) -> Result<()> {
        let lower = url.trim().to_ascii_lowercase();
        if lower.starts_with("https://") {
            return Ok(());
        }
        // A loopback dev server is not a network attacker.
        let local = lower.starts_with("http://127.0.0.1")
            || lower.starts_with("http://localhost")
            || lower.starts_with("http://[::1]");
        if self.allow_insecure || local {
            return Ok(());
        }
        Err(Error::Insecure(url.to_owned()))
    }

    /// The current platform target string, e.g. `"macos-aarch64"`.
    pub fn current_target() -> String {
        format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
    }

    /// Parse a manifest and decide whether a newer release exists for `target`.
    /// Pure — no network, no crypto.
    pub fn evaluate(&self, manifest_json: &str, target: &str) -> Result<UpdateStatus> {
        let manifest: Manifest =
            serde_json::from_str(manifest_json).map_err(|e| Error::Manifest(e.to_string()))?;
        let version =
            Version::parse(&manifest.version).map_err(|e| Error::Version(e.to_string()))?;

        if version <= self.current {
            return Ok(UpdateStatus::UpToDate);
        }

        let release = manifest
            .platforms
            .get(target)
            .ok_or_else(|| Error::NoTarget(target.to_owned()))?;

        Ok(UpdateStatus::Available(UpdateInfo {
            version: manifest.version,
            notes: manifest.notes,
            url: release.url.clone(),
            signature: release.signature.clone(),
        }))
    }

    /// Verify an ed25519 signature (base64) over `data` with the bundled key.
    pub fn verify(&self, data: &[u8], signature_b64: &str) -> Result<()> {
        let bytes = b64(signature_b64)?;
        let arr: [u8; 64] = bytes.as_slice().try_into().map_err(|_| Error::Signature)?;
        let signature = Signature::from_bytes(&arr);
        self.public_key
            .verify_strict(data, &signature)
            .map_err(|_| Error::Signature)
    }

    /// Fetch the manifest over HTTP(S) and evaluate it.
    pub fn check(&self, manifest_url: &str, target: &str) -> Result<UpdateStatus> {
        self.check_url(manifest_url)?;
        let body = http_get(manifest_url)?
            .into_body()
            .read_to_string()
            .map_err(|e| Error::Http(e.to_string()))?;
        self.evaluate(&body, target)
    }

    /// Download the update artifact, verify its signature, and stage it to a
    /// temp file. Never returns an unverified artifact.
    pub fn download_verified(&self, info: &UpdateInfo) -> Result<PathBuf> {
        self.download_verified_with_progress(info, |_, _| {})
    }

    /// Like [`download_verified`], but reports progress as `(downloaded, total)`
    /// where `total` is `None` when the server omits `Content-Length`.
    ///
    /// The body is **streamed to disk** in 64 KiB chunks rather than buffered in
    /// memory, so a large artifact no longer costs its own size in RAM while
    /// downloading, and `max_artifact_bytes` stops a runaway response. The staged
    /// file is only kept if its ed25519 signature verifies; on any failure it is
    /// removed before returning.
    ///
    /// [`download_verified`]: Updater::download_verified
    pub fn download_verified_with_progress<F: FnMut(u64, Option<u64>)>(
        &self,
        info: &UpdateInfo,
        mut on_progress: F,
    ) -> Result<PathBuf> {
        use std::io::{Read, Write};

        self.check_url(&info.url)?;
        let resp = http_get(&info.url)?;
        let total = resp
            .headers()
            .get(ureq::http::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());

        if let Some(total) = total {
            if total > self.max_artifact_bytes {
                return Err(Error::TooLarge {
                    limit: self.max_artifact_bytes,
                });
            }
        }

        let (staged, mut file) = staged_file(&info.version)?;
        let mut reader = resp.into_body().into_reader();
        let mut buf = vec![0u8; 64 * 1024];
        let mut downloaded = 0u64;
        on_progress(0, total);

        let outcome = (|| -> Result<()> {
            loop {
                let n = reader
                    .read(&mut buf)
                    .map_err(|e| Error::Http(e.to_string()))?;
                if n == 0 {
                    break;
                }
                downloaded += n as u64;
                if downloaded > self.max_artifact_bytes {
                    return Err(Error::TooLarge {
                        limit: self.max_artifact_bytes,
                    });
                }
                file.write_all(&buf[..n])
                    .map_err(|e| Error::Io(e.to_string()))?;
                on_progress(downloaded, total);
            }
            file.flush().map_err(|e| Error::Io(e.to_string()))?;
            file.sync_all().map_err(|e| Error::Io(e.to_string()))?;
            Ok(())
        })();

        if let Err(e) = outcome {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        drop(file);

        // ed25519 (unlike ed25519ph) verifies over the whole message, so the
        // staged file is read back once for the signature check. Peak memory is
        // one copy instead of two, and nothing unverified is ever returned.
        if let Err(e) = self.verify_file(&staged, &info.signature) {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        Ok(staged)
    }

    /// Verify a staged file's signature.
    pub fn verify_file(&self, path: &std::path::Path, signature_b64: &str) -> Result<()> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(e.to_string()))?;
        self.verify(&bytes, signature_b64)
    }

    /// Install a verified, staged update and relaunch. Never returns on success
    /// — the process re-execs; returns `Err` only if the swap or relaunch fails.
    ///
    /// Two shapes are supported, chosen by where the running executable lives:
    ///
    /// * **Inside a macOS `.app`** — the artifact must be a **zip of the whole
    ///   signed bundle**. The bundle is replaced as a unit, because a code
    ///   signature seals `Info.plist` and every file under `Contents/`: dropping a
    ///   new executable into a signed bundle breaks the seal and Gatekeeper then
    ///   refuses to launch the app at all ("the application can't be opened").
    ///   A bare-binary artifact is **rejected** here rather than applied.
    /// * **A loose executable** — the artifact is the executable itself, swapped
    ///   in place. Fine for unsigned/single-binary distribution.
    pub fn apply_and_relaunch(staged: &std::path::Path) -> Result<()> {
        let exe = std::env::current_exe().map_err(|e| Error::Io(e.to_string()))?;

        #[cfg(target_os = "macos")]
        if let Some(bundle) = bundle_root(&exe) {
            return apply_bundle(&bundle, staged);
        }

        Self::apply_executable(&exe, staged)
    }

    /// Swap a loose executable and relaunch it.
    fn apply_executable(exe: &std::path::Path, staged: &std::path::Path) -> Result<()> {
        let backup = exe.with_extension("old");
        let _ = std::fs::remove_file(&backup);
        std::fs::rename(exe, &backup).map_err(|e| Error::Io(e.to_string()))?;
        if let Err(e) = std::fs::copy(staged, exe) {
            let _ = std::fs::rename(&backup, exe); // roll back the swap
            return Err(Error::Io(e.to_string()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
        }
        std::process::Command::new(exe)
            .spawn()
            .map_err(|e| Error::Io(e.to_string()))?;
        std::process::exit(0);
    }
}

/// The `.app` bundle the executable belongs to, if any: a macOS bundle always
/// lays out as `Foo.app/Contents/MacOS/foo`.
#[cfg(target_os = "macos")]
fn bundle_root(exe: &std::path::Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    if macos.file_name()? != "MacOS" {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name()? != "Contents" {
        return None;
    }
    let app = contents.parent()?;
    if app.extension()? != "app" {
        return None;
    }
    Some(app.to_path_buf())
}

/// Local zip files start with the `PK\x03\x04` local-file-header magic.
// Only the macOS bundle-apply path inspects the artifact's container format.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

/// Replace a whole `.app` bundle with the one inside `staged` (a zip), verifying
/// its code signature and identity before anything is moved.
#[cfg(target_os = "macos")]
fn apply_bundle(bundle: &std::path::Path, staged: &std::path::Path) -> Result<()> {
    use std::process::Command;

    let mut magic = [0u8; 4];
    {
        use std::io::Read;
        let mut f = std::fs::File::open(staged).map_err(|e| Error::Io(e.to_string()))?;
        let _ = f.read(&mut magic).map_err(|e| Error::Io(e.to_string()))?;
    }
    if !is_zip(&magic) {
        // Applying a bare binary here would break the bundle's signature and
        // leave an app macOS refuses to open. Refuse instead of bricking it.
        return Err(Error::Io(
            "update artifact is not a .app zip; refusing to modify a signed bundle".into(),
        ));
    }

    let work = temp_dir("elyra-update")?;
    // `ditto` is Apple's own tool and preserves extended attributes and the code
    // signature; plain unzip can drop both.
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(staged)
        .arg(&work)
        .status()
        .map_err(|e| Error::Io(e.to_string()))?;
    if !status.success() {
        let _ = std::fs::remove_dir_all(&work);
        return Err(Error::Io("could not expand the update archive".into()));
    }

    let new_app = find_app(&work).ok_or_else(|| {
        let _ = std::fs::remove_dir_all(&work);
        Error::Io("update archive contains no .app bundle".into())
    })?;

    // The ed25519 manifest signature already proves the archive came from us;
    // this additionally proves the bundle inside is intact and still sealed, so
    // we never install something Gatekeeper would reject.
    if let Err(reason) = verify_signature(&new_app) {
        let _ = std::fs::remove_dir_all(&work);
        return Err(Error::Io(format!(
            "the update's code signature does not verify ({reason}); not installing"
        )));
    }
    if let (Some(a), Some(b)) = (bundle_identifier(bundle), bundle_identifier(&new_app)) {
        if a != b {
            let _ = std::fs::remove_dir_all(&work);
            return Err(Error::Io(format!(
                "the update is a different application ({b}, expected {a})"
            )));
        }
    }

    // Swap inside the bundle's own directory so the renames stay on one
    // filesystem, and keep the outgoing copy *outside* the bundle: an extra file
    // under Contents/ would itself invalidate the signature.
    let parent = bundle
        .parent()
        .ok_or_else(|| Error::Io("bundle has no parent directory".into()))?;
    let name = bundle
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Io("bundle has no name".into()))?;
    let outgoing = parent.join(format!(".{name}.old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outgoing);

    std::fs::rename(bundle, &outgoing).map_err(|e| {
        Error::Io(format!(
            "could not move the current app aside: {e} (is it in /Applications and writable?)"
        ))
    })?;
    if let Err(e) = std::fs::rename(&new_app, bundle) {
        let _ = std::fs::rename(&outgoing, bundle); // roll back
        let _ = std::fs::remove_dir_all(&work);
        return Err(Error::Io(format!("could not install the update: {e}")));
    }
    let _ = std::fs::remove_dir_all(&outgoing);
    let _ = std::fs::remove_dir_all(&work);

    // `open` re-registers the bundle with LaunchServices; spawning the binary
    // directly would keep the old registration.
    Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .spawn()
        .map_err(|e| Error::Io(e.to_string()))?;
    std::process::exit(0);
}

/// Check a bundle's code signature with Apple's own verifier, returning
/// codesign's reason on failure ("a sealed resource is missing or invalid" and
/// "invalid Info.plist" mean very different things to whoever debugs it).
///
/// Note: `codesign` has **no** `--quiet` flag — passing one makes it exit 2 with
/// "unrecognized option", which would silently turn this into "reject every
/// update". Hence [`accepts_a_correctly_signed_bundle`](tests), which fails if the
/// invocation itself is broken.
#[cfg(target_os = "macos")]
fn verify_signature(app: &std::path::Path) -> std::result::Result<(), String> {
    let out = std::process::Command::new("/usr/bin/codesign")
        .args(["--verify", "--strict"])
        .arg(app)
        .output()
        .map_err(|e| format!("could not run codesign: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let reason = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if reason.is_empty() {
        format!("codesign exited with {}", out.status)
    } else {
        reason
    })
}

/// The first `*.app` directory directly inside `dir`.
#[cfg(target_os = "macos")]
fn find_app(dir: &std::path::Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path();
        (p.is_dir() && p.extension().map(|x| x == "app").unwrap_or(false)).then_some(p)
    })
}

#[cfg(target_os = "macos")]
fn bundle_identifier(app: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// A fresh, private temp directory with an unpredictable name.
// Used by the macOS bundle-apply path (and the tests around it).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn temp_dir(prefix: &str) -> Result<PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| Error::Io(e.to_string()))?;
    Ok(dir)
}

fn b64(input: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|_| Error::Base64)
}

/// Create a fresh, private temp file (0600 on Unix) with an unpredictable name,
/// refusing to open a pre-existing path (`O_EXCL`). This avoids symlink attacks
/// and collisions on shared / multi-user machines.
fn staged_file(version: &str) -> Result<(PathBuf, std::fs::File)> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let name = format!(
        "elyra-update-{version}-{}-{nanos}-{n}.bin",
        std::process::id()
    );
    let path = std::env::temp_dir().join(name);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let file = opts.open(&path).map_err(|e| Error::Io(e.to_string()))?;
    Ok((path, file))
}

/// Write bytes to a staged temp file (test helper for the apply/bundle paths).
#[cfg(test)]
#[allow(dead_code)]
fn stage_bytes(version: &str, bytes: &[u8]) -> Result<PathBuf> {
    use std::io::Write;
    let (path, mut file) = staged_file(version)?;
    file.write_all(bytes)
        .map_err(|e| Error::Io(e.to_string()))?;
    Ok(path)
}

fn http_get(url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    ureq::get(url)
        .call()
        .map_err(|e| Error::Http(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn keypair() -> (SigningKey, String) {
        // Deterministic key from fixed seed (test only).
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let public_b64 =
            base64::engine::general_purpose::STANDARD.encode(signing.verifying_key().to_bytes());
        (signing, public_b64)
    }

    fn manifest(version: &str, url: &str, sig: &str) -> String {
        format!(
            r#"{{"version":"{version}","notes":"hi","platforms":{{"macos-aarch64":{{"url":"{url}","signature":"{sig}"}}}}}}"#
        )
    }

    #[test]
    fn newer_version_is_available() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();
        let m = manifest("1.2.0", "https://x/app.bin", "sig");
        match updater.evaluate(&m, "macos-aarch64").unwrap() {
            UpdateStatus::Available(info) => {
                assert_eq!(info.version, "1.2.0");
                assert_eq!(info.url, "https://x/app.bin");
            }
            _ => panic!("expected an update"),
        }
    }

    #[test]
    fn same_or_older_is_up_to_date() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "2.0.0").unwrap();
        assert!(matches!(
            updater
                .evaluate(&manifest("2.0.0", "u", "s"), "macos-aarch64")
                .unwrap(),
            UpdateStatus::UpToDate
        ));
        assert!(matches!(
            updater
                .evaluate(&manifest("1.9.9", "u", "s"), "macos-aarch64")
                .unwrap(),
            UpdateStatus::UpToDate
        ));
    }

    #[test]
    fn missing_target_errors() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();
        assert!(updater
            .evaluate(&manifest("1.1.0", "u", "s"), "windows-x86_64")
            .is_err());
    }

    #[test]
    fn config_builds_updater_and_defaults_auto_check_off() {
        let (_, pk) = keypair();
        let cfg = UpdaterConfig::new(&pk, "https://x/latest.json", "1.0.0");
        assert!(!cfg.auto_check, "auto_check should be opt-in");
        assert!(cfg.build().is_ok());
        assert!(cfg.auto_check(true).auto_check);
    }

    #[test]
    fn update_check_is_derived_from_status() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();

        let available: UpdateCheck = updater
            .evaluate(&manifest("1.2.0", "u", "s"), "macos-aarch64")
            .unwrap()
            .into();
        assert!(available.available);
        assert_eq!(available.version.as_deref(), Some("1.2.0"));
        assert!(available.error.is_none());

        let uptodate: UpdateCheck = UpdateStatus::UpToDate.into();
        assert!(!uptodate.available);
        assert!(uptodate.version.is_none());
    }

    #[test]
    fn detects_a_zip_artifact() {
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(is_zip(b"PK\x05\x06"));
        // A Mach-O executable must never be mistaken for a bundle archive.
        assert!(!is_zip(&[0xcf, 0xfa, 0xed, 0xfe]));
        assert!(!is_zip(b""));
        assert!(!is_zip(b"PK"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn recognizes_an_app_bundle_layout() {
        use std::path::Path;
        assert_eq!(
            bundle_root(Path::new("/Applications/Foo.app/Contents/MacOS/foo")),
            Some(PathBuf::from("/Applications/Foo.app"))
        );
        // A loose executable, or anything not in the canonical layout, is not a
        // bundle — those keep the plain binary-swap path.
        assert_eq!(bundle_root(Path::new("/usr/local/bin/foo")), None);
        assert_eq!(bundle_root(Path::new("/tmp/Foo.app/foo")), None);
        assert_eq!(
            bundle_root(Path::new("/tmp/Foo.bundle/Contents/MacOS/foo")),
            None
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_a_bare_binary_for_a_bundle() {
        // The 0.4.0 regression in Elyra Sjá: a bare binary dropped into a signed
        // bundle broke its seal and macOS then refused to open the app.
        let dir = temp_dir("elyra-test").unwrap();
        let bin = dir.join("artifact.bin");
        std::fs::write(&bin, [0xcf, 0xfa, 0xed, 0xfe]).unwrap();
        let err = apply_bundle(&dir.join("Foo.app"), &bin).unwrap_err();
        assert!(
            err.to_string().contains("not a .app zip"),
            "unexpected error: {err}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Build a minimal, ad-hoc signed `.app` so the real `codesign` /
    /// `CFBundleIdentifier` checks can be exercised without a Developer ID.
    #[cfg(target_os = "macos")]
    fn fake_app(dir: &std::path::Path, name: &str, identifier: &str) -> PathBuf {
        let app = dir.join(format!("{name}.app"));
        let macos = app.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        // A bundle needs a real Mach-O main executable to be signable.
        std::fs::copy("/bin/echo", macos.join(name)).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>{name}</string>
<key>CFBundleIdentifier</key><string>{identifier}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleVersion</key><string>1.0</string>
</dict></plist>"#
            ),
        )
        .unwrap();
        let ok = std::process::Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-"])
            .arg(&app)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "could not ad-hoc sign the test bundle");
        app
    }

    #[cfg(target_os = "macos")]
    fn zip_app(app: &std::path::Path, out: &std::path::Path) {
        let ok = std::process::Command::new("/usr/bin/ditto")
            .args(["-c", "-k", "--keepParent"])
            .arg(app)
            .arg(out)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok, "ditto failed to archive the test bundle");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn accepts_a_correctly_signed_bundle() {
        // Guards against the failure mode that only rejection tests can't catch:
        // a malformed codesign invocation (e.g. a flag it doesn't know) makes every
        // update look unsigned, and auto-update silently stops working for everyone.
        let dir = temp_dir("elyra-test").unwrap();
        let app = fake_app(&dir, "Good", "com.example.good");
        assert_eq!(verify_signature(&app), Ok(()));

        // And it must still survive the ditto round-trip the updater performs.
        let zip = dir.join("good.zip");
        zip_app(&app, &zip);
        let work = temp_dir("elyra-test-x").unwrap();
        std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&zip)
            .arg(&work)
            .status()
            .unwrap();
        assert_eq!(verify_signature(&find_app(&work).unwrap()), Ok(()));

        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&work).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_an_update_for_a_different_application() {
        let dir = temp_dir("elyra-test").unwrap();
        // What is installed, and what the archive contains, are different apps.
        let installed = fake_app(&dir, "Installed", "com.example.installed");
        let other_dir = dir.join("other");
        std::fs::create_dir_all(&other_dir).unwrap();
        let intruder = fake_app(&other_dir, "Installed", "com.example.intruder");
        let zip = dir.join("update.zip");
        zip_app(&intruder, &zip);

        let err = apply_bundle(&installed, &zip).unwrap_err();
        assert!(
            err.to_string().contains("different application"),
            "unexpected error: {err}"
        );
        // The installed bundle must be untouched after a refusal.
        assert!(installed.join("Contents/Info.plist").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn refuses_an_update_whose_signature_is_broken() {
        let dir = temp_dir("elyra-test").unwrap();
        let installed = fake_app(&dir, "App", "com.example.app");
        let staging = dir.join("staging");
        std::fs::create_dir_all(&staging).unwrap();
        let tampered = fake_app(&staging, "App", "com.example.app");
        // Tamper *after* signing: this is what a corrupted or modified download
        // looks like, and it must never be installed.
        std::fs::write(tampered.join("Contents/Resources.txt"), b"injected").unwrap();
        let zip = dir.join("update.zip");
        zip_app(&tampered, &zip);

        let err = apply_bundle(&installed, &zip).unwrap_err();
        assert!(
            err.to_string().contains("code signature does not verify"),
            "unexpected error: {err}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A one-shot HTTP server that serves `body` once, for the download tests.
    fn serve_once(body: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut scratch = [0u8; 1024];
                let _ = stream.read(&mut scratch); // consume the request line/headers
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{port}/app.bin")
    }

    #[test]
    fn plain_http_is_refused_for_manifest_and_artifact() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();

        assert!(matches!(
            updater.check("http://releases.example.com/latest.json", "macos-aarch64"),
            Err(Error::Insecure(_))
        ));
        let info = UpdateInfo {
            version: "1.1.0".into(),
            notes: None,
            url: "http://releases.example.com/app.bin".into(),
            signature: "sig".into(),
        };
        assert!(matches!(
            updater.download_verified(&info),
            Err(Error::Insecure(_))
        ));
    }

    #[test]
    fn insecure_can_be_opted_into_and_loopback_is_always_allowed() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();
        // Loopback is a dev server, not a network attacker.
        assert!(updater
            .check_url("http://127.0.0.1:8080/latest.json")
            .is_ok());
        assert!(updater
            .check_url("http://localhost:8080/latest.json")
            .is_ok());
        assert!(updater.check_url("https://example.com/x.json").is_ok());
        assert!(updater.check_url("http://example.com/x.json").is_err());

        let lax = Updater::new(&pk, "1.0.0").unwrap().allow_insecure(true);
        assert!(lax.check_url("http://example.com/x.json").is_ok());
    }

    #[test]
    fn streams_a_verified_artifact_to_disk_and_reports_progress() {
        let (signing, pk) = keypair();
        // 300 KiB so the 64 KiB read loop runs several times.
        let artifact: Vec<u8> = (0..300 * 1024).map(|i| (i % 251) as u8).collect();
        let signature =
            base64::engine::general_purpose::STANDARD.encode(signing.sign(&artifact).to_bytes());

        let updater = Updater::new(&pk, "1.0.0").unwrap();
        let info = UpdateInfo {
            version: "1.1.0".into(),
            notes: None,
            url: serve_once(artifact.clone()),
            signature,
        };

        let mut progress = Vec::new();
        let staged = updater
            .download_verified_with_progress(&info, |got, total| progress.push((got, total)))
            .expect("download must succeed");

        assert_eq!(std::fs::read(&staged).unwrap(), artifact);
        assert!(progress.len() > 3, "progress must be reported per chunk");
        assert_eq!(progress.first().unwrap().0, 0);
        assert_eq!(progress.last().unwrap().0, artifact.len() as u64);
        assert_eq!(progress[0].1, Some(artifact.len() as u64));
        let _ = std::fs::remove_file(staged);
    }

    #[test]
    fn a_tampered_artifact_leaves_nothing_staged() {
        let (signing, pk) = keypair();
        let artifact = b"the real update".to_vec();
        let signature =
            base64::engine::general_purpose::STANDARD.encode(signing.sign(&artifact).to_bytes());

        let updater = Updater::new(&pk, "1.0.0").unwrap();
        // A version string unique to this test, so a parallel test's staged file
        // can't be mistaken for ours.
        let info = UpdateInfo {
            version: "9.9.9-tamper".into(),
            notes: None,
            url: serve_once(b"a malicious payload".to_vec()),
            signature,
        };

        let before = staged_files_in_temp();
        assert!(matches!(
            updater.download_verified(&info),
            Err(Error::Signature)
        ));
        // The partially downloaded file must be cleaned up, not left behind.
        assert_eq!(staged_files_in_temp(), before);
    }

    #[test]
    fn an_oversized_artifact_is_refused() {
        let (_, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap().max_artifact_bytes(64);
        let info = UpdateInfo {
            version: "1.1.0".into(),
            notes: None,
            url: serve_once(vec![0u8; 4096]),
            signature: "sig".into(),
        };
        assert!(matches!(
            updater.download_verified(&info),
            Err(Error::TooLarge { limit: 64 })
        ));
    }

    /// Staged artifacts left in the temp dir (used to assert cleanup).
    fn staged_files_in_temp() -> usize {
        std::fs::read_dir(std::env::temp_dir())
            .map(|dir| {
                dir.filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name()
                            .to_string_lossy()
                            .starts_with("elyra-update-9.9.9-tamper-")
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    #[test]
    fn signature_roundtrip() {
        let (signing, pk) = keypair();
        let updater = Updater::new(&pk, "1.0.0").unwrap();

        let artifact = b"the new binary bytes";
        let sig =
            base64::engine::general_purpose::STANDARD.encode(signing.sign(artifact).to_bytes());

        // Correct signature verifies; tampered data does not.
        assert!(updater.verify(artifact, &sig).is_ok());
        assert!(updater.verify(b"tampered bytes", &sig).is_err());
    }
}
