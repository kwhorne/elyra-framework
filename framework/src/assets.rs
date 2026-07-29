//! Serving the frontend from memory.
//!
//! Assets are resolved through an [`AssetResolver`] closure so the framework
//! never hard-codes an embed path — the app crate owns its own
//! `#[derive(RustEmbed)]` and hands us [`asset_resolver::<Assets>()`].
//!
//! Bytes are returned as a [`Cow`], so an embedded asset is **borrowed** from the
//! binary's static data instead of copied on every request, and each asset
//! carries an `etag` (its content hash) so repeat loads can be answered with
//! `304 Not Modified`. See [`crate::shell`] for the caching/range handling.
//!
//! When no asset matches `index.html` (e.g. before you've run the frontend
//! build), the shell serves [`FALLBACK_HTML`]: a dependency-free page that
//! exercises the IPC bridge, so `cargo run` works without npm.

use std::borrow::Cow;
use std::sync::Arc;

/// A single resolved asset.
pub struct Asset {
    /// The file's bytes — borrowed from the binary for embedded assets.
    pub bytes: Cow<'static, [u8]>,
    pub mime: String,
    /// A strong validator for `If-None-Match` (the content hash), without quotes.
    pub etag: Option<String>,
}

impl Asset {
    /// An asset from owned bytes, with no validator.
    pub fn new(bytes: Vec<u8>, mime: impl Into<String>) -> Self {
        Self {
            bytes: Cow::Owned(bytes),
            mime: mime.into(),
            etag: None,
        }
    }

    /// Attach an `ETag` value (the content hash).
    pub fn with_etag(mut self, etag: impl Into<String>) -> Self {
        self.etag = Some(etag.into());
        self
    }
}

/// Resolves a request path (e.g. `"index.html"`, `"assets/app.js"`) to bytes.
pub type AssetResolver = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

/// Build an [`AssetResolver`] backed by a `#[derive(RustEmbed)]` type.
///
/// The embedded bytes are borrowed (no per-request copy) and rust-embed's
/// SHA-256 metadata becomes the `ETag`.
pub fn asset_resolver<A: rust_embed::RustEmbed>() -> AssetResolver {
    Arc::new(|path: &str| {
        A::get(path).map(|file| Asset {
            mime: mime_for(path).to_string(),
            etag: Some(hex16(&file.metadata.sha256_hash())),
            bytes: file.data,
        })
    })
}

/// The first 16 bytes of a hash, hex-encoded — plenty for an `ETag`.
fn hex16(hash: &[u8]) -> String {
    hash.iter().take(16).fold(String::new(), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Minimal extension -> MIME mapping for the assets a Vite build produces.
pub fn mime_for(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("wav") => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// Whether a path looks content-hashed (`app-B1x9Kd2f.js`), i.e. safe to cache
/// forever. Vite emits `name-<hash>.ext`; a non-hashed file gets `no-cache`
/// instead so a new build is always picked up.
pub fn is_fingerprinted(path: &str) -> bool {
    let stem = match path.rsplit_once('.') {
        Some((stem, _)) => stem,
        None => return false,
    };
    let Some((_, suffix)) = stem.rsplit_once('-') else {
        return false;
    };
    suffix.len() >= 8
        && suffix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
        && suffix.chars().any(|c| c.is_ascii_digit())
}

/// A self-contained demo page (no build step, no npm) that talks to the
/// `elyra://localhost/__cmd/*` bridge with a tiny inline MessagePack codec.
pub const FALLBACK_HTML: &str = include_str!("fallback.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprinted_paths_are_detected() {
        assert!(is_fingerprinted("assets/app-B1x9Kd2f.js"));
        assert!(is_fingerprinted("assets/index-4f2a91bc.css"));
        assert!(!is_fingerprinted("index.html"));
        assert!(!is_fingerprinted("assets/logo.svg"));
        assert!(!is_fingerprinted("assets/my-icon.svg")); // suffix too short
        assert!(!is_fingerprinted("noextension"));
    }

    #[test]
    fn hex16_is_32_chars() {
        let hash = [0xabu8; 32];
        assert_eq!(hex16(&hash).len(), 32);
        assert!(hex16(&hash).starts_with("abab"));
    }
}
