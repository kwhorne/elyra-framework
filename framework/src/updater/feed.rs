//! Updates from the backend (RFC 0024): the manifest is asked of the active
//! server as the signed-in user, and the release is downloaded from that
//! server with the same token, never from anywhere else.
//!
//! ```text
//! GET {server}{path}?platform=macos-aarch64&version=1.4.0
//! Authorization: Bearer …
//!
//! 200 { "version": "1.5.0", "notes": "…", "released_at": "2026-11-06",
//!       "url": "https://acme.example.com/desktop/Felagi-1.5.0-macos-aarch64.zip",
//!       "size": 31457280, "sha256": "…" }
//! 204 (up to date)
//! ```

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use super::{Error, UpdateInfo, UpdateStatus};
use crate::backend::{Backend, BackendError};
use crate::http::HttpError;

/// How long a download may go without progress.
const IDLE: Duration = Duration::from_secs(60);

/// The server's answer: the newest release for the platform asked about.
/// A `{ "data": … }` wrapper (an API Resource) is fine.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub version: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub released_at: Option<String>,
    /// Where the artifact is: on the server's own origin, absolute (or a
    /// path, resolved against the server).
    pub url: String,
    /// The artifact's size in bytes, when the server says.
    #[serde(default)]
    pub size: Option<u64>,
    /// The artifact's SHA-256, hex. Required.
    pub sha256: String,
    /// A base64 ed25519 signature over the artifact; required when the app
    /// has a public key ([`UpdaterConfig::public_key`](super::UpdaterConfig::public_key)).
    #[serde(default)]
    pub signature: Option<String>,
}

/// Ask the active server whether there's a newer release for `target`.
/// Signed out, nothing is sent: [`Error::SignedOut`].
pub(crate) async fn check(
    backend: &Backend,
    path: &str,
    current: &semver::Version,
    target: &str,
) -> Result<UpdateStatus, Error> {
    let Some(base) = signed_in(backend) else {
        return Err(Error::SignedOut);
    };
    let response = backend
        .get(path)
        .query(&[("platform", target), ("version", &current.to_string())])
        .no_offline()
        .send()
        .await
        .map_err(from_backend)?;
    if response.status() == 204 || response.bytes().is_empty() {
        return Ok(UpdateStatus::UpToDate);
    }
    let value: Value = response
        .json()
        .map_err(|e| Error::Manifest(e.to_string()))?;
    let value = match value {
        Value::Object(mut map) if map.contains_key("data") => {
            map.remove("data").unwrap_or(Value::Null)
        }
        other => other,
    };
    let release: Release =
        serde_json::from_value(value).map_err(|e| Error::Manifest(e.to_string()))?;
    evaluate(&base, release, current)
}

/// Decide what the server's release means for `current`, on `base`. Pure.
pub(crate) fn evaluate(
    base: &str,
    release: Release,
    current: &semver::Version,
) -> Result<UpdateStatus, Error> {
    let version = semver::Version::parse(release.version.trim())
        .map_err(|e| Error::Version(e.to_string()))?;
    if &version <= current {
        return Ok(UpdateStatus::UpToDate);
    }
    let sha256 = release.sha256.trim().to_ascii_lowercase();
    if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::Manifest("`sha256` isn't a SHA-256 in hex".into()));
    }
    let url = on_origin(base, &release.url)?;
    Ok(UpdateStatus::Available(UpdateInfo {
        version: release.version.trim().to_owned(),
        notes: release.notes,
        url,
        signature: release.signature.unwrap_or_default(),
        sha256: Some(sha256),
        size: release.size,
        released_at: release.released_at,
    }))
}

/// Download `url` into `dest` with the signed-in user's token, only if it's
/// on the active server's origin. More than `max` bytes stops it.
pub(crate) async fn download(
    backend: &Backend,
    url: &str,
    dest: &Path,
    max: u64,
    progress: impl Fn(u64, Option<u64>) + Send + Sync + 'static,
) -> Result<(), Error> {
    let (Some(base), Some(token)) = (signed_in(backend), backend.current_token()) else {
        return Err(Error::SignedOut);
    };
    // Checked again here: the active server may have changed since the
    // manifest was read, and the token is the active one's.
    let url = on_origin(&base, url)?;
    if backend.unplugged().is_on() {
        return Err(Error::Http(format!(
            "can't reach the server: {}",
            crate::inspector::Unplugged::REASON
        )));
    }
    let cancel = Arc::new(crate::http::Cancel::default());
    let too_large = Arc::new(AtomicBool::new(false));
    let (stop, flag) = (cancel.clone(), too_large.clone());
    let response = backend
        .http()
        .get(&url)
        .token(&token)
        .timeout(IDLE)
        .on_progress(move |p| {
            if p.done > max || p.total.is_some_and(|t| t > max) {
                flag.store(true, Ordering::SeqCst);
                stop.cancel();
            } else {
                progress(p.done, p.total);
            }
        })
        .cancel_with(cancel)
        .download_to(dest)
        .await;
    match response {
        Ok(res) if res.saved_to().is_some() => Ok(()),
        Ok(res) if res.status() == 401 || res.status() == 419 => Err(Error::SignedOut),
        Ok(res) => Err(Error::Http(format!(
            "the server answered {} for the download",
            res.status()
        ))),
        Err(HttpError::Cancelled) if too_large.load(Ordering::SeqCst) => {
            Err(Error::TooLarge { limit: max })
        }
        Err(e) => Err(Error::Http(e.to_string())),
    }
}

/// The active server, when someone is signed in to it.
fn signed_in(backend: &Backend) -> Option<String> {
    backend.base_url().filter(|_| backend.has_token())
}

/// `url` resolved against `base`, refused unless it's on `base`'s origin
/// (scheme, host and port): the token goes to the server it belongs to, and
/// nowhere else.
pub(crate) fn on_origin(base: &str, url: &str) -> Result<String, Error> {
    let base_url =
        reqwest::Url::parse(base).map_err(|_| Error::Manifest(format!("bad server `{base}`")))?;
    let resolved = base_url
        .join(url.trim())
        .map_err(|_| Error::Manifest(format!("bad url `{url}`")))?;
    if resolved.origin() != base_url.origin() {
        return Err(Error::ForeignOrigin(
            resolved.origin().ascii_serialization(),
        ));
    }
    Ok(resolved.to_string())
}

fn from_backend(e: BackendError) -> Error {
    match e {
        BackendError::NoServer | BackendError::Unauthenticated | BackendError::Expired => {
            Error::SignedOut
        }
        other => Error::Http(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACME: &str = "https://acme.felagi.test";

    fn release(url: &str) -> Release {
        Release {
            version: "2.0.0".into(),
            notes: Some("new".into()),
            released_at: Some("2026-11-06".into()),
            url: url.into(),
            size: Some(10),
            sha256: "AB".repeat(32),
            signature: None,
        }
    }

    #[test]
    fn only_the_servers_own_origin_is_downloaded_from() {
        assert_eq!(
            on_origin(ACME, "https://acme.felagi.test/desktop/F.zip").unwrap(),
            "https://acme.felagi.test/desktop/F.zip"
        );
        assert_eq!(
            on_origin(ACME, "/desktop/F.zip").unwrap(),
            "https://acme.felagi.test/desktop/F.zip"
        );
        for foreign in [
            "https://evil.test/F.zip",
            "//evil.test/F.zip",
            "http://acme.felagi.test/F.zip",
            "https://acme.felagi.test:8443/F.zip",
            "https://acme.felagi.test.evil.test/F.zip",
        ] {
            assert!(
                matches!(on_origin(ACME, foreign), Err(Error::ForeignOrigin(_))),
                "{foreign}"
            );
        }
    }

    #[test]
    fn a_release_is_evaluated_against_the_running_version() {
        let current = semver::Version::new(1, 0, 0);
        match evaluate(ACME, release("/F.zip"), &current).unwrap() {
            UpdateStatus::Available(info) => {
                assert_eq!(info.url, "https://acme.felagi.test/F.zip");
                assert_eq!(info.sha256.as_deref(), Some("ab".repeat(32).as_str()));
                assert_eq!(info.size, Some(10));
            }
            other => panic!("{other:?}"),
        }
        let newer = semver::Version::new(2, 0, 0);
        assert!(matches!(
            evaluate(ACME, release("/F.zip"), &newer).unwrap(),
            UpdateStatus::UpToDate
        ));
        let mut bad = release("/F.zip");
        bad.sha256 = "nope".into();
        assert!(matches!(
            evaluate(ACME, bad, &current),
            Err(Error::Manifest(_))
        ));
    }
}
