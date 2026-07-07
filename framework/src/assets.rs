//! Serving the frontend from memory.
//!
//! Assets are resolved through an [`AssetResolver`] closure so the framework
//! never hard-codes an embed path — the app crate owns its own
//! `#[derive(RustEmbed)]` and hands us [`asset_resolver::<Assets>()`].
//!
//! When no asset matches `index.html` (e.g. before you've run the frontend
//! build), the shell serves [`FALLBACK_HTML`]: a dependency-free page that
//! exercises the IPC bridge, so `cargo run` works without npm.

use std::sync::Arc;

/// A single resolved asset.
pub struct Asset {
    pub bytes: Vec<u8>,
    pub mime: String,
}

/// Resolves a request path (e.g. `"index.html"`, `"assets/app.js"`) to bytes.
pub type AssetResolver = Arc<dyn Fn(&str) -> Option<Asset> + Send + Sync>;

/// Build an [`AssetResolver`] backed by a `#[derive(RustEmbed)]` type.
pub fn asset_resolver<A: rust_embed::RustEmbed>() -> AssetResolver {
    Arc::new(|path: &str| {
        A::get(path).map(|file| Asset {
            mime: mime_for(path).to_string(),
            bytes: file.data.into_owned(),
        })
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
        _ => "application/octet-stream",
    }
}

/// A self-contained demo page (no build step, no npm) that talks to the
/// `elyra://localhost/__cmd/*` bridge with a tiny inline MessagePack codec.
pub const FALLBACK_HTML: &str = include_str!("fallback.html");
