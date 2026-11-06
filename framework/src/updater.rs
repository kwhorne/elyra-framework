//! Auto-updater (behind the `updater` feature).
//!
//! Two ways to know a release is genuine, and either or both may be used:
//!
//! - **An ed25519 signature**, as Tauri's: releases are published as an
//!   artifact plus a signature, listed in a JSON manifest, and the app ships
//!   the matching public key. A compromised update server still can't push a
//!   malicious binary.
//! - **A Developer ID** (macOS, RFC 0024): the artifact is a zip of the whole
//!   signed, notarized `.app`, and the bundle inside must be signed by the
//!   app's team over its bundle id, unaltered — the check Gatekeeper makes.
//!
//! And two places to ask: a fixed manifest URL ([`UpdaterConfig::new`]), or
//! the signed-in user's own server ([`UpdaterConfig::from_backend`], RFC 0024),
//! as that user, with the artifact downloaded from that server alone.
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
//! `evaluate` (manifest parse + semver), `verify` (ed25519) and the bundle
//! decisions ([`bundle`]) are pure and unit-tested. `check` /
//! `download_verified` do real HTTP; [`bundle::NativeSystem`] runs Apple's
//! `ditto` and `codesign`.

pub mod bundle;
#[cfg(feature = "backend")]
pub mod feed;
mod runtime;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use semver::Version;
use serde::{Deserialize, Serialize};

pub use bundle::DeveloperId;
pub use runtime::{Target, UpdaterRuntime};

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
    /// The updater's configuration can't verify anything, or names an
    /// impossible Developer ID.
    #[error("invalid updater configuration: {0}")]
    Config(String),
    /// No one is signed in to a server to ask (RFC 0024).
    #[error("not signed in, so there is no server to ask")]
    SignedOut,
    /// The manifest points the download at another origin than the active
    /// server's; the token is never sent there.
    #[error("the update is on another server ({0}); the sign-in is only sent to its own")]
    ForeignOrigin(String),
    /// The download's SHA-256 isn't the manifest's.
    #[error("the download doesn't match its SHA-256 checksum")]
    Digest,
    /// The download isn't the size the manifest said.
    #[error("the download is {actual} bytes, not the {expected} the server said")]
    SizeMismatch { expected: u64, actual: u64 },
    /// The bundle's code signature isn't what it must be.
    #[error("the update was refused: {0}")]
    CodeSignature(String),
    /// The bundle in the archive is no newer than the running app.
    #[error("the update is version {found}, which isn't newer than {current}")]
    NotNewer { found: String, current: String },
    /// The running app can't be replaced where it is.
    #[error("can't install the update: {0}")]
    NotInstallable(String),
    /// An install is already running.
    #[error("an update is already being installed")]
    Busy,
    /// Relaunch asked for before an update was put in place.
    #[error("no update has been installed yet")]
    NothingInstalled,
}

type Result<T> = std::result::Result<T, Error>;

/// Default cap on a downloaded artifact (1 GiB).
pub const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;

/// Details of an available update.
#[derive(Debug, Clone, Default)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
    pub url: String,
    /// Base64 ed25519 signature of the artifact at `url`; empty when the
    /// manifest carries none (a backend manifest trusted by its Developer ID).
    pub signature: String,
    /// The artifact's SHA-256 in hex, when the manifest says (always, from a
    /// backend).
    pub sha256: Option<String>,
    /// The artifact's size in bytes, when the manifest says.
    pub size: Option<u64>,
    /// When it was released, as the server wrote it.
    pub released_at: Option<String>,
}

/// Result of an update check.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    UpToDate,
    Available(UpdateInfo),
}

/// Serializable summary sent to the frontend by the `/__update/check` endpoint
/// (signature + URL stay server-side).
#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The update is installed and waits for a restart (RFC 0024).
    pub ready: bool,
    /// `"ask"`: installing doesn't restart, a separate Restart does (RFC
    /// 0024); `"now"`: installing restarts the app.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
}

impl From<UpdateStatus> for UpdateCheck {
    fn from(status: UpdateStatus) -> Self {
        match status {
            UpdateStatus::UpToDate => UpdateCheck {
                available: false,
                version: None,
                notes: None,
                error: None,
                ready: false,
                restart: None,
            },
            UpdateStatus::Available(info) => UpdateCheck {
                available: true,
                version: Some(info.version),
                notes: info.notes,
                error: None,
                ready: false,
                restart: None,
            },
        }
    }
}

/// Configuration for the framework's built-in update flow (`App::updater`).
#[derive(Clone)]
pub struct UpdaterConfig {
    /// Base64 ed25519 public key the app was built with; empty for none (a
    /// backend manifest trusted by its Developer ID alone).
    pub public_key: String,
    /// URL of the JSON manifest listing the latest release per platform;
    /// empty for [`from_backend`](Self::from_backend).
    pub manifest_url: String,
    /// The running app's version (typically `env!("CARGO_PKG_VERSION")`).
    pub current_version: String,
    /// Check for updates silently (default: false — opt in with
    /// [`auto_check`](UpdaterConfig::auto_check)).
    pub auto_check: bool,
    /// Allow plain `http://` for the manifest/artifact. **Off by default**: the
    /// ed25519 signature protects integrity, but plain HTTP still lets a network
    /// attacker hide that an update exists at all.
    pub allow_insecure: bool,
    /// Refuse an artifact larger than this (default 1 GiB), so a hostile or
    /// broken server can't make the app fill the disk.
    pub max_artifact_bytes: u64,
    /// The backend route the manifest is asked of (RFC 0024), set by
    /// [`from_backend`](Self::from_backend).
    pub backend_path: Option<String>,
    /// The Developer ID a macOS bundle must be signed with (RFC 0024).
    pub developer_id: Option<DeveloperId>,
    /// Install without restarting; a separate Restart relaunches (RFC 0024).
    /// On for [`from_backend`](Self::from_backend), off for [`new`](Self::new).
    pub relaunch_when_asked: bool,
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
            backend_path: None,
            developer_id: None,
            relaunch_when_asked: false,
        }
    }

    /// Ask the signed-in user's server (RFC 0024): `GET {server}{path}
    /// ?platform=…&version=…` with their token, through the app's
    /// [`Backend`](crate::backend::Backend). Signed out, nothing is asked. The
    /// release is downloaded from that server's origin only, with the same
    /// token, and verified by its SHA-256 and by
    /// [`developer_id`](Self::developer_id) and/or
    /// [`public_key`](Self::public_key) — at least one is required. Installing
    /// doesn't restart: a separate Restart does.
    #[cfg(feature = "backend")]
    pub fn from_backend(path: impl Into<String>, current_version: impl Into<String>) -> Self {
        Self {
            backend_path: Some(path.into()),
            relaunch_when_asked: true,
            ..Self::new("", "", current_version)
        }
    }

    /// Require (also) an ed25519 signature, with this base64 public key.
    pub fn public_key(mut self, public_key: impl Into<String>) -> Self {
        self.public_key = public_key.into();
        self
    }

    /// On macOS, require the update's bundle to be signed with a Developer ID
    /// certificate issued to `team_id` (`52AT44QFCM`), over `bundle_id`
    /// (`works.felagi.desktop`), strictly valid and unaltered.
    pub fn developer_id(
        mut self,
        team_id: impl Into<String>,
        bundle_id: impl Into<String>,
    ) -> Self {
        self.developer_id = Some(DeveloperId::new(team_id, bundle_id));
        self
    }

    /// Install without restarting, and relaunch only when asked (the toast's
    /// Restart, `restartToUpdate()`), so the app isn't restarted mid-sentence.
    pub fn relaunch_when_asked(mut self, yes: bool) -> Self {
        self.relaunch_when_asked = yes;
        self
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

    /// Toggle the silent check (default: false). With a manifest URL, the
    /// shell checks on launch; from the backend, shortly after launch or
    /// sign-in and every 24 hours after. A newer release shows the toast.
    pub fn auto_check(mut self, yes: bool) -> Self {
        self.auto_check = yes;
        self
    }

    /// Whether the manifest comes from the backend.
    pub fn is_from_backend(&self) -> bool {
        self.backend_path.is_some()
    }

    /// Build the [`Updater`] this config describes, refusing a configuration
    /// that couldn't verify an update.
    pub fn build(&self) -> Result<Updater> {
        if let Some(id) = &self.developer_id {
            id.check().map_err(Error::Config)?;
        }
        let updater = if !self.public_key.trim().is_empty() {
            Updater::new(&self.public_key, &self.current_version)?
        } else if self.is_from_backend() {
            if self.developer_id.is_none() {
                return Err(Error::Config(
                    "an update from the backend needs .developer_id(..) or .public_key(..): \
                     nothing would verify it"
                        .into(),
                ));
            }
            Updater::for_version(&self.current_version)?
        } else {
            // A fixed manifest URL is trusted by its ed25519 key, as before.
            return Err(Error::PublicKey);
        };
        Ok(updater
            .allow_insecure(self.allow_insecure)
            .max_artifact_bytes(self.max_artifact_bytes))
    }
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
    /// Hex; checked as well as the signature when given.
    #[serde(default)]
    sha256: Option<String>,
}

/// Checks for and verifies updates against a bundled public key.
#[derive(Clone)]
pub struct Updater {
    public_key: Option<VerifyingKey>,
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
        let mut updater = Self::for_version(current_version)?;
        updater.public_key = Some(public_key);
        Ok(updater)
    }

    /// An updater without an ed25519 key, for releases trusted by their code
    /// signature (RFC 0024): [`verify`](Self::verify) refuses everything.
    pub fn for_version(current_version: &str) -> Result<Self> {
        let current = Version::parse(current_version).map_err(|e| Error::Version(e.to_string()))?;
        Ok(Self {
            public_key: None,
            current,
            allow_insecure: false,
            max_artifact_bytes: DEFAULT_MAX_ARTIFACT_BYTES,
        })
    }

    /// Whether an ed25519 key is configured.
    pub fn has_public_key(&self) -> bool {
        self.public_key.is_some()
    }

    /// The running version.
    pub fn current_version(&self) -> &Version {
        &self.current
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
            sha256: release.sha256.clone(),
            ..UpdateInfo::default()
        }))
    }

    /// Verify an ed25519 signature (base64) over `data` with the bundled key.
    pub fn verify(&self, data: &[u8], signature_b64: &str) -> Result<()> {
        let key = self.public_key.as_ref().ok_or(Error::PublicKey)?;
        let bytes = b64(signature_b64)?;
        let arr: [u8; 64] = bytes.as_slice().try_into().map_err(|_| Error::Signature)?;
        let signature = Signature::from_bytes(&arr);
        key.verify_strict(data, &signature)
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
    /// file is only kept if its ed25519 signature verifies (and its SHA-256,
    /// when the manifest gave one); on any failure it is removed before
    /// returning.
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
        let verified =
            self.verify_file(&staged, &info.signature)
                .and_then(|()| match &info.sha256 {
                    Some(sha) => verify_sha256(&staged, sha),
                    None => Ok(()),
                });
        if let Err(e) = verified {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
        Ok(staged)
    }

    /// Verify a staged file's signature.
    pub fn verify_file(&self, path: &Path, signature_b64: &str) -> Result<()> {
        let bytes = std::fs::read(path).map_err(|e| Error::Io(e.to_string()))?;
        self.verify(&bytes, signature_b64)
    }

    /// Install a verified, staged update and relaunch. Never returns on success
    /// — the process exits and the new version starts once it has; returns
    /// `Err` only if the swap or relaunch fails.
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
    ///
    /// The built-in flow ([`UpdaterRuntime`]) installs and relaunches as two
    /// steps instead.
    pub fn apply_and_relaunch(staged: &Path) -> Result<()> {
        let exe = std::env::current_exe().map_err(|e| Error::Io(e.to_string()))?;
        let target = runtime::target_for(
            &exe,
            cfg!(target_os = "macos"),
            false,
            true,
            &bundle::NativeSystem,
        )?;
        let work = temp_dir("elyra-update")?;
        let applied = runtime::apply(&bundle::NativeSystem, &target, staged, &work, None, None);
        let _ = std::fs::remove_dir_all(&work);
        applied?;
        bundle::relaunch_after_exit(target.path()).map_err(Error::Io)?;
        std::process::exit(0);
    }
}

/// Compare a file's SHA-256 with `expected` (hex, either case).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    use sha2::Digest;
    use std::io::Read;
    let mut file = std::fs::File::open(path).map_err(|e| Error::Io(e.to_string()))?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| Error::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(Error::Digest)
    }
}

/// A fresh, private temp directory (0700 on Unix) with an unpredictable name.
pub(crate) fn temp_dir(prefix: &str) -> Result<PathBuf> {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{prefix}-{}-{nanos}-{n}", std::process::id()));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&dir).map_err(|e| Error::Io(e.to_string()))?;
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
        use bundle::is_zip;
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(is_zip(b"PK\x05\x06"));
        // A Mach-O executable must never be mistaken for a bundle archive.
        assert!(!is_zip(&[0xcf, 0xfa, 0xed, 0xfe]));
        assert!(!is_zip(b""));
        assert!(!is_zip(b"PK"));
    }

    #[test]
    fn recognizes_an_app_bundle_layout() {
        use bundle::bundle_root;
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

    /// The real check, as for a bundle trusted by its ed25519 key alone.
    #[cfg(target_os = "macos")]
    fn intact(app: &Path) -> std::result::Result<(), String> {
        use bundle::BundleSystem;
        bundle::judge_intact(&bundle::NativeSystem.code_signature(app, None))
    }

    #[cfg(target_os = "macos")]
    fn find_app(dir: &Path) -> PathBuf {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .find(|p| p.extension().is_some_and(|x| x == "app"))
            .unwrap()
    }

    /// Install `zip` over `installed` as `apply_and_relaunch` would, without
    /// relaunching.
    #[cfg(target_os = "macos")]
    fn install_legacy(installed: &Path, zip: &Path) -> Result<()> {
        let work = temp_dir("elyra-test-work").unwrap();
        let trust = bundle::BundleTrust {
            developer_id: None,
            current: None,
        };
        let out = bundle::install_bundle(&bundle::NativeSystem, zip, installed, &work, &trust);
        let _ = std::fs::remove_dir_all(&work);
        out
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
        assert_eq!(intact(&app), Ok(()));

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
        assert_eq!(intact(&find_app(&work)), Ok(()));

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

        let err = install_legacy(&installed, &zip).unwrap_err();
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

        let err = install_legacy(&installed, &zip).unwrap_err();
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
            ..UpdateInfo::default()
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
            ..UpdateInfo::default()
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
            ..UpdateInfo::default()
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
            ..UpdateInfo::default()
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

    #[test]
    fn a_backend_config_needs_something_to_verify_with() {
        let cfg = UpdaterConfig::new("", "", "1.0.0");
        assert!(matches!(cfg.build(), Err(Error::PublicKey)));
        #[cfg(feature = "backend")]
        {
            let cfg = UpdaterConfig::from_backend("/api/desktop/update", "1.0.0");
            assert!(cfg.relaunch_when_asked, "two steps from the backend");
            assert!(matches!(cfg.build(), Err(Error::Config(_))));
            let built = cfg
                .clone()
                .developer_id("52AT44QFCM", "works.felagi.desktop")
                .build()
                .unwrap();
            assert!(!built.has_public_key());
            let (_, pk) = keypair();
            assert!(cfg.clone().public_key(pk).build().unwrap().has_public_key());
            assert!(matches!(
                cfg.developer_id("52AT\" or true", "x").build(),
                Err(Error::Config(_))
            ));
        }
        // A key-less updater refuses every signature.
        let keyless = Updater::for_version("1.0.0").unwrap();
        assert!(keyless.verify(b"bytes", "sig").is_err());
    }

    #[test]
    fn a_sha256_is_compared_case_blind() {
        let dir = temp_dir("elyra-test-sha").unwrap();
        let file = dir.join("a.bin");
        std::fs::write(&file, b"abc").unwrap();
        let sha = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_sha256(&file, sha).is_ok());
        assert!(verify_sha256(&file, &sha.to_uppercase()).is_ok());
        assert!(matches!(
            verify_sha256(&file, &"0".repeat(64)),
            Err(Error::Digest)
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
