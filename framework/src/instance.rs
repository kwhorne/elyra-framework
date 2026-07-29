//! Single-instance support.
//!
//! The first instance becomes **primary** and listens on a private rendezvous
//! endpoint; later launches connect, hand over their payload (e.g. a deep-link
//! URL), and exit.
//!
//! ## Why not a bare loopback port
//! The original implementation used a TCP port derived from the app name, with a
//! handshake string (`ELYRA-SI/<AppName>`) that anyone could reconstruct — so any
//! local process, including another user's, could inject deep-link payloads that
//! the app forwards to the frontend as `elyra:deep-link`.
//!
//! Now:
//! * **Unix** — an `AF_UNIX` socket in the user's runtime dir, created with mode
//!   `0600`, so the OS itself keeps other users out.
//! * **Windows** — still loopback TCP (std has no named pipes), but the handshake
//!   requires a **random per-install token** stored in the user's app-data
//!   directory. A process that can't read that file can't be mistaken for a
//!   second launch.
//!
//! Payloads are length-limited and single-line, and the caller validates the URL
//! before doing anything with it.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Upper bound on a forwarded payload (a URL, not a document).
const MAX_PAYLOAD: usize = 8 * 1024;
const IO_TIMEOUT: Duration = Duration::from_millis(750);

/// A filesystem-safe slug for `app`.
fn slug(app: &str) -> String {
    let s: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = s.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "elyra-app".to_string()
    } else {
        trimmed
    }
}

/// Where the per-install secret lives (readable only by this user).
fn token_path(app: &str) -> Option<PathBuf> {
    crate::winstate::app_dir(app).map(|d| d.join("instance.token"))
}

/// Load the per-install token, creating it on first use. `None` when we have no
/// writable app dir (then the handshake falls back to the app id only).
fn token(app: &str) -> Option<String> {
    let path = token_path(app)?;
    let read = |p: &PathBuf| {
        std::fs::read_to_string(p)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| t.len() >= 16)
    };
    if let Some(existing) = read(&path) {
        return Some(existing);
    }

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    // Write the token to a private temp file first, then *link* it into place.
    // `hard_link` fails if the target exists, which makes "create or discover"
    // atomic and — crucially — means a concurrent reader never observes a
    // created-but-empty file (which used to yield two different handshakes).
    let fresh = crate::security::random_hex_token();
    // The temp name must be unique *per attempt*, not per process: two threads
    // sharing one name could interleave writes, so the file that got linked into
    // place held a different token than the one the winner returned.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = path.with_extension(format!(
        "tmp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    {
        let mut file = opts.open(&tmp).ok()?;
        file.write_all(fresh.as_bytes()).ok()?;
        file.flush().ok()?;
    }
    let linked = std::fs::hard_link(&tmp, &path).is_ok();
    let _ = std::fs::remove_file(&tmp);
    if linked {
        return Some(fresh);
    }
    // Someone else won the race; their token is already complete on disk (it was
    // linked only after being fully written). Retry briefly in case a competing
    // attempt is between its write and its link.
    for _ in 0..20 {
        if let Some(existing) = read(&path) {
            return Some(existing);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

/// The handshake line: app id + the per-install secret.
fn handshake(app: &str) -> String {
    match token(app) {
        Some(secret) => format!("ELYRA-SI/{app}/{secret}"),
        None => format!("ELYRA-SI/{app}"),
    }
}

// ---------------------------------------------------------------------------
// Unix: an AF_UNIX socket with 0600 permissions.
// ---------------------------------------------------------------------------

#[cfg(unix)]
pub(crate) use unix_impl::{bind_primary, notify_primary, serve};

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    /// `$XDG_RUNTIME_DIR/elyra-<slug>.sock`, falling back to the temp dir with the
    /// uid in the name so two users can't collide.
    fn socket_path(app: &str) -> PathBuf {
        let name = format!("elyra-{}-{}.sock", slug(app), uid());
        match std::env::var_os("XDG_RUNTIME_DIR") {
            Some(dir) => PathBuf::from(dir).join(name),
            None => std::env::temp_dir().join(name),
        }
    }

    fn uid() -> u32 {
        // SAFETY: getuid() is always safe; it reads the process's own identity.
        unsafe { libc_getuid() }
    }

    // Avoid a `libc` dependency for one call.
    extern "C" {
        #[link_name = "getuid"]
        fn libc_getuid() -> u32;
    }

    pub(crate) fn bind_primary(app: &str) -> Option<UnixListener> {
        let path = socket_path(app);
        match UnixListener::bind(&path) {
            Ok(listener) => {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                Some(listener)
            }
            Err(_) => {
                // A leftover socket from a crashed run: if nobody answers, replace it.
                if UnixStream::connect(&path).is_err() {
                    let _ = std::fs::remove_file(&path);
                    let listener = UnixListener::bind(&path).ok()?;
                    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
                    return Some(listener);
                }
                None
            }
        }
    }

    pub(crate) fn notify_primary(app: &str, payload: &str) -> bool {
        let Ok(stream) = UnixStream::connect(socket_path(app)) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        super::exchange(stream, app, payload)
    }

    pub(crate) fn serve(
        listener: UnixListener,
        app: String,
        on_payload: impl Fn(String) + Send + 'static,
    ) {
        std::thread::spawn(move || {
            let expected = handshake(&app);
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if let Some(payload) = super::accept(stream, &expected) {
                    on_payload(payload);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Windows: loopback TCP, gated by the per-install token.
// ---------------------------------------------------------------------------

#[cfg(not(unix))]
pub(crate) use tcp_impl::{bind_primary, notify_primary, serve};

#[cfg(not(unix))]
mod tcp_impl {
    use super::*;
    use std::hash::{Hash, Hasher};
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    fn port_for(app: &str) -> u16 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        slug(app).hash(&mut h);
        "elyra-single-instance".hash(&mut h);
        49152 + (h.finish() % 16384) as u16
    }

    pub(crate) fn bind_primary(app: &str) -> Option<TcpListener> {
        TcpListener::bind((Ipv4Addr::LOCALHOST, port_for(app))).ok()
    }

    pub(crate) fn notify_primary(app: &str, payload: &str) -> bool {
        let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port_for(app))) else {
            return false;
        };
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
        super::exchange(stream, app, payload)
    }

    pub(crate) fn serve(
        listener: TcpListener,
        app: String,
        on_payload: impl Fn(String) + Send + 'static,
    ) {
        std::thread::spawn(move || {
            let expected = handshake(&app);
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if let Some(payload) = super::accept(stream, &expected) {
                    on_payload(payload);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Shared protocol: "<handshake>\n<payload>\n" -> "<handshake>\n"
// ---------------------------------------------------------------------------

/// Client side: send the handshake + payload, expect the handshake echoed back.
fn exchange<S>(mut stream: S, app: &str, payload: &str) -> bool
where
    S: std::io::Read + std::io::Write,
{
    let expected = handshake(app);
    let payload = sanitize(payload);
    if stream
        .write_all(format!("{expected}\n{payload}\n").as_bytes())
        .is_err()
    {
        return false;
    }
    let _ = stream.flush();
    let mut ack = String::new();
    let mut reader = BufReader::new(&mut stream).take(MAX_PAYLOAD as u64);
    if reader.read_line(&mut ack).is_err() {
        return false;
    }
    // A stranger on the endpoint can't produce the expected ack.
    ack.trim() == expected
}

/// Server side: validate the handshake, read the payload, acknowledge.
fn accept<S>(mut stream: S, expected: &str) -> Option<String>
where
    S: std::io::Read + std::io::Write,
{
    let mut reader = BufReader::new(&mut stream).take((MAX_PAYLOAD * 2) as u64);
    let mut hello = String::new();
    if reader.read_line(&mut hello).is_err() {
        return None;
    }
    // Also rejects an HTTP request from a browser tab: the request line can never
    // match the handshake.
    if hello.trim() != expected {
        return None;
    }
    let mut payload = String::new();
    let _ = reader.read_line(&mut payload);
    let _ = stream.write_all(format!("{expected}\n").as_bytes());
    let _ = stream.flush();
    Some(payload.trim().to_string())
}

/// Keep a payload to one line and a sane length.
fn sanitize(payload: &str) -> String {
    let single_line: String = payload.replace(['\n', '\r'], " ");
    single_line.chars().take(MAX_PAYLOAD).collect()
}

/// Whether `payload` is a deep link for `scheme` — parsed, not just prefixed, so
/// a forwarded string can't smuggle something else past the check.
pub(crate) fn is_deep_link(payload: &str, scheme: &str) -> bool {
    let prefix = format!("{scheme}://");
    if !payload.starts_with(&prefix) {
        return false;
    }
    let rest = &payload[prefix.len()..];
    // Reject control characters, whitespace, and quotes that could confuse a
    // frontend that interpolates the URL.
    !rest.is_empty()
        && rest.len() <= MAX_PAYLOAD
        && !rest
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '"' || c == '\'' || c == '<')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_id(tag: &str) -> String {
        format!("elyra-si-test-{}-{tag}", std::process::id())
    }

    #[test]
    fn primary_receives_secondary_payload() {
        let app = app_id("basic");
        let listener = bind_primary(&app).expect("bind primary");
        let (tx, rx) = std::sync::mpsc::channel();
        serve(listener, app.clone(), move |p| {
            let _ = tx.send(p);
        });
        assert!(notify_primary(&app, "myapp://open/42"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "myapp://open/42"
        );
    }

    #[test]
    fn no_primary_means_nothing_to_notify() {
        assert!(!notify_primary(&app_id("absent"), ""));
    }

    #[test]
    fn a_wrong_handshake_is_rejected() {
        // Regression: the handshake used to be derivable from the app name alone,
        // so any local process could inject a deep link.
        let expected = "ELYRA-SI/app/secret-token";
        let mut wire: Vec<u8> = Vec::new();
        wire.extend_from_slice(b"ELYRA-SI/app\nmyapp://evil\n");
        let mut cursor = std::io::Cursor::new(wire);
        assert!(accept(&mut cursor, expected).is_none());

        // An HTTP request from a browser tab is refused too.
        let mut http = std::io::Cursor::new(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec());
        assert!(accept(&mut http, expected).is_none());
    }

    #[test]
    fn the_right_handshake_is_accepted() {
        let expected = "ELYRA-SI/app/secret-token";
        let wire = format!("{expected}\nmyapp://open\n").into_bytes();
        let mut cursor = std::io::Cursor::new(wire);
        assert_eq!(
            accept(&mut cursor, expected).as_deref(),
            Some("myapp://open")
        );
    }

    #[test]
    fn tokens_are_persistent_per_app_and_differ_between_apps() {
        let a = app_id("tok-a");
        let b = app_id("tok-b");
        let first = token(&a);
        if first.is_none() {
            return; // no writable config dir in this environment
        }
        assert_eq!(token(&a), first, "the token must be stable across calls");
        assert_ne!(token(&b), first, "different apps get different tokens");
        assert!(handshake(&a).starts_with(&format!("ELYRA-SI/{a}/")));
    }

    #[test]
    fn concurrent_first_starts_agree_on_one_token() {
        // Regression: creating the token with create-then-write let a concurrent
        // reader see an empty file, so the two sides derived different handshakes
        // and a legitimate second launch was rejected.
        let app = app_id("race");
        let mut handles = Vec::new();
        for _ in 0..8 {
            let app = app.clone();
            handles.push(std::thread::spawn(move || token(&app)));
        }
        let tokens: Vec<Option<String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        if tokens[0].is_none() {
            return; // no writable config dir here
        }
        assert!(
            tokens.iter().all(|t| t.is_some()),
            "every caller gets a token"
        );
        assert!(
            tokens.windows(2).all(|w| w[0] == w[1]),
            "all callers must agree: {tokens:?}"
        );
    }

    #[test]
    fn payloads_are_sanitized() {
        assert_eq!(sanitize("a\nb\rc"), "a b c");
        assert_eq!(sanitize(&"x".repeat(MAX_PAYLOAD * 2)).len(), MAX_PAYLOAD);
    }

    #[test]
    fn deep_links_are_validated_not_just_prefixed() {
        assert!(is_deep_link("myapp://open/42", "myapp"));
        assert!(is_deep_link("myapp://x?y=1&z=2", "myapp"));
        assert!(!is_deep_link("myapp://", "myapp"));
        assert!(!is_deep_link("other://open", "myapp"));
        assert!(!is_deep_link("myapp://open me", "myapp"));
        assert!(!is_deep_link("myapp://a\"><script>", "myapp"));
        assert!(!is_deep_link("", "myapp"));
    }
}
