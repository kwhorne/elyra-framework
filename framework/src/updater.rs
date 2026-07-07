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
use serde::Deserialize;

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
}

type Result<T> = std::result::Result<T, Error>;

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
        })
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
        let body = http_get(manifest_url)?
            .into_string()
            .map_err(|e| Error::Http(e.to_string()))?;
        self.evaluate(&body, target)
    }

    /// Download the update artifact, verify its signature, and stage it to a
    /// temp file. Never returns an unverified artifact.
    pub fn download_verified(&self, info: &UpdateInfo) -> Result<PathBuf> {
        use std::io::Read;
        let mut reader = http_get(&info.url)?.into_reader();
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|e| Error::Http(e.to_string()))?;

        self.verify(&bytes, &info.signature)?;

        let path = std::env::temp_dir().join(format!("elyra-update-{}.bin", info.version));
        std::fs::write(&path, &bytes).map_err(|e| Error::Io(e.to_string()))?;
        Ok(path)
    }
}

fn b64(input: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(input.trim())
        .map_err(|_| Error::Base64)
}

fn http_get(url: &str) -> Result<ureq::Response> {
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
