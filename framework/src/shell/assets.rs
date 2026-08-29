//! Serving the embedded frontend: content negotiation, conditional requests and
//! byte ranges over the assets baked into the binary.
//!
//! Assets used to be copied out of the binary on every request and served with
//! only a content type: no validator (so a reload re-transferred and re-parsed
//! the whole bundle) and no `Range` (so `<video>`/`<audio>` couldn't seek).

use std::borrow::Cow;

use wry::http::{header, Request, Response, StatusCode};

use crate::assets::FALLBACK_HTML;

use super::guard::with_csp;
use super::protocol::Body;
use super::Runner;

/// Serve an embedded asset, with conditional-request and range support.
pub(super) fn serve_asset(runner: &Runner, path: &str, request: &Request<Vec<u8>>) -> Body {
    let rel = match path.trim_start_matches('/') {
        "" => "index.html",
        other => other,
    };

    if let Some(resolver) = &runner.assets {
        if let Some(asset) = resolver(rel) {
            return serve_bytes(runner, rel, asset, request);
        }
    }

    // No embedded frontend yet — serve the dependency-free demo page.
    if rel == "index.html" {
        let asset = crate::assets::Asset {
            bytes: Cow::Borrowed(FALLBACK_HTML.as_bytes()),
            mime: "text/html; charset=utf-8".into(),
            etag: None,
        };
        return serve_bytes(runner, rel, asset, request);
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Cow::Borrowed(b"not found".as_slice()))
        .unwrap()
}

/// Respond with an asset's bytes: `304` when the client's validator still
/// matches, `206` for a range request, `200` otherwise.
fn serve_bytes(
    runner: &Runner,
    rel: &str,
    asset: crate::assets::Asset,
    request: &Request<Vec<u8>>,
) -> Body {
    let is_html = asset.mime.starts_with("text/html");
    let header_str = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    };

    // Content-hashed filenames never change; anything else must be revalidated
    // so a fresh build is picked up immediately.
    let cache_control = if is_html {
        "no-cache"
    } else if crate::assets::is_fingerprinted(rel) {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let quoted_etag = asset.etag.as_ref().map(|e| format!("\"{e}\""));

    // Conditional request: nothing to send if the validator still matches.
    if let (Some(etag), Some(inm)) = (&quoted_etag, header_str("if-none-match")) {
        if inm.split(',').any(|candidate| candidate.trim() == etag) {
            let resp = Response::builder()
                .status(StatusCode::NOT_MODIFIED)
                .header(header::ETAG, etag.as_str())
                .header(header::CACHE_CONTROL, cache_control)
                .body(Cow::Borrowed(b"".as_slice()))
                .unwrap();
            return resp;
        }
    }

    let total = asset.bytes.len();
    let base = |status: StatusCode| {
        let mut builder = Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, asset.mime.clone())
            .header(header::CACHE_CONTROL, cache_control)
            .header(header::ACCEPT_RANGES, "bytes");
        if let Some(etag) = &quoted_etag {
            builder = builder.header(header::ETAG, etag.as_str());
        }
        builder
    };

    // Range request (media seeking). Only a single `bytes=a-b` range is honoured.
    if let Some(range) = header_str("range") {
        match parse_byte_range(&range, total) {
            Some((start, end)) => {
                let slice: Vec<u8> = asset.bytes[start..=end].to_vec();
                let resp = base(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{total}"),
                    )
                    .body(Cow::Owned(slice))
                    .unwrap();
                return with_csp(resp, &runner.csp, is_html);
            }
            None if range.starts_with("bytes=") => {
                return Response::builder()
                    .status(StatusCode::RANGE_NOT_SATISFIABLE)
                    .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                    .body(Cow::Borrowed(b"".as_slice()))
                    .unwrap();
            }
            None => {}
        }
    }

    // The common path: hand over the (borrowed) embedded bytes as-is.
    let resp = base(StatusCode::OK).body(asset.bytes).unwrap();
    with_csp(resp, &runner.csp, is_html)
}

/// Parse a single-range `Range: bytes=start-end` header against `total`.
/// Returns an inclusive, in-bounds `(start, end)`.
fn parse_byte_range(value: &str, total: usize) -> Option<(usize, usize)> {
    if total == 0 {
        return None;
    }
    let spec = value.strip_prefix("bytes=")?.trim();
    if spec.contains(',') {
        return None; // multipart ranges: not worth it for a local protocol
    }
    let (start, end) = spec.split_once('-')?;
    let last = total - 1;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            // `bytes=-500` — the final 500 bytes.
            let n: usize = suffix.parse().ok()?;
            if n == 0 {
                return None;
            }
            (total.saturating_sub(n), last)
        }
        (from, "") => (from.parse().ok()?, last),
        (from, to) => (from.parse().ok()?, to.parse::<usize>().ok()?.min(last)),
    };
    if start > end || start > last {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_ranges_are_parsed_and_clamped() {
        assert_eq!(parse_byte_range("bytes=0-99", 1000), Some((0, 99)));
        assert_eq!(parse_byte_range("bytes=990-", 1000), Some((990, 999)));
        assert_eq!(parse_byte_range("bytes=-100", 1000), Some((900, 999)));
        // An end past the last byte is clamped, not rejected.
        assert_eq!(parse_byte_range("bytes=900-5000", 1000), Some((900, 999)));
        // Unsatisfiable / unsupported forms.
        assert_eq!(parse_byte_range("bytes=1000-1200", 1000), None);
        assert_eq!(parse_byte_range("bytes=50-10", 1000), None);
        assert_eq!(parse_byte_range("bytes=0-10,20-30", 1000), None);
        assert_eq!(parse_byte_range("items=0-10", 1000), None);
        assert_eq!(parse_byte_range("bytes=0-10", 0), None);
    }
}
