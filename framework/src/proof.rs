//! Challenge-response proofs for the app's private local endpoints —
//! single-instance's and the MCP shim's (RFC 0003).
//!
//! Each side proves it holds the per-install token with an HMAC-SHA256 over
//! fresh nonces, without ever sending the token: a process that connects
//! without it gets nowhere, and one that *listens* first (holding the socket
//! path or a loopback port) learns nothing it could use.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

fn mac(key: &[u8], parts: &[&str]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(parts.join("|").as_bytes());
    mac
}

/// The proof over `parts` (joined with `|`), hex.
pub(crate) fn sign(key: &[u8], parts: &[&str]) -> String {
    hex(&mac(key, parts).finalize().into_bytes())
}

/// Whether `proof` (hex) is the one over `parts` — compared in constant time.
pub(crate) fn verify(key: &[u8], parts: &[&str], proof: &str) -> bool {
    unhex(proof).is_some_and(|bytes| mac(key, parts).verify_slice(&bytes).is_ok())
}

/// A fresh nonce: 256 random bits, hex.
pub(crate) fn nonce() -> String {
    crate::security::random_token()
}

/// Whether `s` looks like a nonce — hex, and neither short nor huge.
pub(crate) fn is_nonce(s: &str) -> bool {
    (32..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofs_bind_the_key_and_every_part() {
        let p = sign(b"key", &["a", "b"]);
        assert!(verify(b"key", &["a", "b"], &p));
        assert!(!verify(b"other", &["a", "b"], &p));
        assert!(!verify(b"key", &["a", "c"], &p));
        assert!(!verify(b"key", &["a|b"], &sign(b"key", &["a"])));
        assert!(!verify(b"key", &["a", "b"], "not hex"));
        assert!(is_nonce(&nonce()) && !is_nonce("xyz") && !is_nonce(&"a".repeat(200)));
        assert_eq!(unhex(&hex(&[0, 255, 16])), Some(vec![0, 255, 16]));
    }
}
