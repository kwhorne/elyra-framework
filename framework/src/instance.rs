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
//! The handshake is a challenge-response on that token ([`crate::proof`]):
//! neither side sends it. It used to travel in the clear, so a process that
//! took a loopback port before the app learned it from the next launch — and
//! could then inject deep links itself.
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
pub(crate) fn slug(app: &str) -> String {
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
pub(crate) fn token(app: &str) -> Option<String> {
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

/// The protocol's first word.
const HELLO: &str = "ELYRA-SI/2";

/// What the two sides prove they hold: the app id and the per-install secret
/// (the app id alone when there's no writable app dir for one).
fn key(app: &str) -> Vec<u8> {
    match token(app) {
        Some(secret) => format!("{app}/{secret}").into_bytes(),
        None => app.as_bytes().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Unix: an AF_UNIX socket with 0600 permissions.
// ---------------------------------------------------------------------------

/// `$XDG_RUNTIME_DIR/elyra-<slug>-<uid>.<ext>`, falling back to the temp dir —
/// the uid in the name so two users can't collide.
#[cfg(unix)]
pub(crate) fn socket_path(app: &str, ext: &str) -> PathBuf {
    let name = format!("elyra-{}-{}.{ext}", slug(app), uid());
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join(name),
        None => std::env::temp_dir().join(name),
    }
}

#[cfg(unix)]
fn uid() -> u32 {
    // SAFETY: getuid() is always safe; it reads the process's own identity.
    unsafe { libc_getuid() }
}

// Avoid a `libc` dependency for one call.
#[cfg(unix)]
extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

#[cfg(unix)]
pub(crate) use unix_impl::{bind_primary, notify_primary, serve};

#[cfg(unix)]
mod unix_impl {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::{UnixListener, UnixStream};

    fn socket_path(app: &str) -> PathBuf {
        super::socket_path(app, "sock")
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
            let key = key(&app);
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if let Some(payload) = super::accept(stream, &app, &key) {
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
    use super::loopback;
    use super::*;
    use std::net::TcpListener;

    pub(crate) fn bind_primary(app: &str) -> Option<TcpListener> {
        loopback::bind_first(&loopback::ports_for(app))
    }

    pub(crate) fn notify_primary(app: &str, payload: &str) -> bool {
        loopback::notify_any(&loopback::ports_for(app), app, payload)
    }

    pub(crate) fn serve(
        listener: TcpListener,
        app: String,
        on_payload: impl Fn(String) + Send + 'static,
    ) {
        std::thread::spawn(move || {
            let key = key(&app);
            for stream in listener.incoming().flatten() {
                let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
                let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
                if let Some(payload) = super::accept(stream, &app, &key) {
                    on_payload(payload);
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Loopback ports (Windows), compiled everywhere so the logic is tested on every
// platform with real sockets.
// ---------------------------------------------------------------------------

#[cfg(any(test, not(unix)))]
pub(crate) mod loopback {
    use std::net::{Ipv4Addr, TcpListener, TcpStream};

    /// How many ports an app may end up on.
    pub(super) const CANDIDATES: usize = 8;

    /// The distance between candidates. Windows reserves *blocks* of ports in
    /// the dynamic range (Hyper-V, WSL and Docker take `excludedportrange`
    /// chunks of ~100), and nothing can bind inside one; spreading the
    /// candidates this far apart means one reserved block can't cover them all.
    const SPREAD: u64 = 2039;

    /// FNV-1a: a hash that is the same in every build. `DefaultHasher` makes no
    /// such promise across Rust versions, so an app update built with a newer
    /// toolchain could pick different ports from the instance already running.
    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h
    }

    /// The candidate ports for `app`, in the order both sides try them.
    pub(super) fn ports_for(app: &str) -> [u16; CANDIDATES] {
        ports_in("elyra-single-instance", app)
    }

    /// The candidate ports for `app`'s endpoint named `namespace` — each
    /// endpoint its own set, so they don't take each other's ports.
    pub(crate) fn ports_in(namespace: &str, app: &str) -> [u16; CANDIDATES] {
        let seed = fnv1a(format!("{namespace}/{}", super::slug(app)).as_bytes());
        std::array::from_fn(|i| 49152 + ((seed + i as u64 * SPREAD) % 16384) as u16)
    }

    /// Become the primary on the first candidate that can be bound.
    pub(crate) fn bind_first(ports: &[u16]) -> Option<TcpListener> {
        ports
            .iter()
            .find_map(|port| TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).ok())
    }

    /// Reach the primary on whichever candidate it holds. A port nobody holds is
    /// refused at once; a port some other program holds fails the handshake —
    /// either way, on to the next.
    pub(super) fn notify_any(ports: &[u16], app: &str, payload: &str) -> bool {
        ports.iter().any(|port| {
            let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, *port)) else {
                return false;
            };
            let _ = stream.set_read_timeout(Some(super::IO_TIMEOUT));
            let _ = stream.set_write_timeout(Some(super::IO_TIMEOUT));
            super::exchange(stream, app, payload)
        })
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::{Read, Write};

        #[test]
        fn candidates_are_stable_distinct_and_in_the_dynamic_range() {
            let ports = ports_for("Example App");
            // Pinned: a change here moves every installed app to new ports, so
            // an update would stop finding the instance that's already running.
            assert_eq!(
                ports,
                [64400, 50055, 52094, 54133, 56172, 58211, 60250, 62289]
            );
            assert_eq!(ports.len(), CANDIDATES);
            let mut unique = ports.to_vec();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), CANDIDATES, "{ports:?}");
            assert!(ports.iter().all(|p| *p >= 49152), "{ports:?}");
            assert_ne!(ports_for("Another App"), ports);
        }

        #[test]
        fn the_hash_is_the_same_in_every_build() {
            // FNV-1a test vectors: if these move, so do the ports.
            assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
            assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
            assert_eq!(fnv1a(b"foobar"), 0x8594_4171_f739_67e8);
        }

        #[test]
        fn a_taken_candidate_is_skipped_by_both_sides() {
            let app = format!("elyra-loopback-test-{}", std::process::id());
            let ports = ports_for(&app);
            // Something else — here, a stranger that answers with nonsense —
            // already holds the first candidate, as a reserved range or another
            // program would.
            let Ok(stranger) = TcpListener::bind((Ipv4Addr::LOCALHOST, ports[0])) else {
                eprintln!("skipping: candidate port {} is unavailable here", ports[0]);
                return;
            };
            std::thread::spawn(move || {
                for mut s in stranger.incoming().flatten() {
                    let mut buf = [0u8; 256];
                    let _ = s.read(&mut buf);
                    let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n");
                }
            });

            let primary = bind_first(&ports).expect("a later candidate binds");
            let bound = primary.local_addr().unwrap().port();
            assert_ne!(bound, ports[0], "must not reuse the taken port");
            assert!(ports.contains(&bound));

            let (tx, rx) = std::sync::mpsc::channel();
            super::super::serve_test(primary, app.clone(), move |p| {
                let _ = tx.send(p);
            });
            assert!(
                notify_any(&ports, &app, "myapp://open/7"),
                "reaches the primary past the stranger"
            );
            assert_eq!(
                rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(),
                "myapp://open/7"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Shared protocol, a challenge-response on the key (see `crate::proof`):
//
//   client → `ELYRA-SI/2 <client nonce>`
//   server → `<server nonce> <proof("server")>`
//   client → `<proof("client", payload)> <payload>`
//   server → `OK`
//
// The server proves itself first, so the client never hands its payload — a
// URL that may be private — to an impostor; the client's proof covers the
// payload, so it arrives as it was sent.
// ---------------------------------------------------------------------------

/// One line, without its newline — `None` on EOF, an error, or a line too
/// long to be one.
fn read_line<R: BufRead>(reader: &mut R) -> Option<String> {
    let mut line = String::new();
    reader
        .take((MAX_PAYLOAD * 2) as u64)
        .read_line(&mut line)
        .ok()
        .filter(|_| line.ends_with('\n'))?;
    Some(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Client side: prove ourselves to a primary that proved itself, and hand it
/// `payload`. `true` once it acknowledged.
fn exchange<S>(stream: S, app: &str, payload: &str) -> bool
where
    S: std::io::Read + std::io::Write,
{
    exchange_with(stream, app, &key(app), payload)
}

fn exchange_with<S>(stream: S, app: &str, key: &[u8], payload: &str) -> bool
where
    S: std::io::Read + std::io::Write,
{
    let payload = sanitize(payload);
    let client = crate::proof::nonce();
    let mut reader = BufReader::new(stream);
    if reader
        .get_mut()
        .write_all(format!("{HELLO} {client}\n").as_bytes())
        .is_err()
    {
        return false;
    }
    let _ = reader.get_mut().flush();
    let Some(line) = read_line(&mut reader) else {
        return false;
    };
    let Some((server, proof)) = line.split_once(' ') else {
        return false;
    };
    // A stranger on the endpoint can't produce this, so it never sees the
    // payload.
    if !crate::proof::is_nonce(server)
        || !crate::proof::verify(key, &[HELLO, app, "server", &client, server], proof)
    {
        return false;
    }
    let ours = crate::proof::sign(key, &[HELLO, app, "client", &client, server, &payload]);
    if reader
        .get_mut()
        .write_all(format!("{ours} {payload}\n").as_bytes())
        .is_err()
    {
        return false;
    }
    let _ = reader.get_mut().flush();
    read_line(&mut reader).as_deref() == Some("OK")
}

/// Server side: prove ourselves, check the client's proof, take its payload.
fn accept<S>(stream: S, app: &str, key: &[u8]) -> Option<String>
where
    S: std::io::Read + std::io::Write,
{
    let mut reader = BufReader::new(stream);
    // Also rejects an HTTP request from a browser tab: its request line isn't
    // a hello.
    let hello = read_line(&mut reader)?;
    let client = hello
        .strip_prefix(HELLO)
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|n| crate::proof::is_nonce(n))?
        .to_owned();
    let server = crate::proof::nonce();
    let ours = crate::proof::sign(key, &[HELLO, app, "server", &client, &server]);
    reader
        .get_mut()
        .write_all(format!("{server} {ours}\n").as_bytes())
        .ok()?;
    let _ = reader.get_mut().flush();
    let line = read_line(&mut reader)?;
    let (proof, payload) = line.split_once(' ').unwrap_or((line.as_str(), ""));
    if !crate::proof::verify(
        key,
        &[HELLO, app, "client", &client, &server, payload],
        proof,
    ) {
        return None;
    }
    let _ = reader.get_mut().write_all(b"OK\n");
    let _ = reader.get_mut().flush();
    Some(payload.to_owned())
}

/// Serve a loopback listener with the shared protocol — the Windows `serve`,
/// available to tests on every platform.
#[cfg(test)]
fn serve_test(
    listener: std::net::TcpListener,
    app: String,
    on_payload: impl Fn(String) + Send + 'static,
) {
    std::thread::spawn(move || {
        let key = key(&app);
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
            if let Some(payload) = accept(stream, &app, &key) {
                on_payload(payload);
            }
        }
    });
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

    /// Run `server` on one loopback connection and `client` on the other end.
    fn pair<T: Send + 'static>(
        server: impl FnOnce(std::net::TcpStream) -> T + Send + 'static,
        client: impl FnOnce(std::net::TcpStream) -> bool,
    ) -> (bool, T) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
            server(stream)
        });
        let stream = std::net::TcpStream::connect(addr).unwrap();
        let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
        let ok = client(stream);
        (ok, handle.join().unwrap())
    }

    #[test]
    fn the_right_key_hands_over_the_payload() {
        let (ok, got) = pair(
            |s| accept(s, "app", b"app/secret"),
            |s| exchange_with(s, "app", b"app/secret", "myapp://open/7"),
        );
        assert!(ok);
        assert_eq!(got.as_deref(), Some("myapp://open/7"));
        // An empty payload (a plain second launch) too.
        let (ok, got) = pair(
            |s| accept(s, "app", b"k"),
            |s| exchange_with(s, "app", b"k", ""),
        );
        assert!(ok && got.as_deref() == Some(""));
    }

    #[test]
    fn a_wrong_key_or_another_app_is_rejected() {
        let (ok, got) = pair(
            |s| accept(s, "app", b"app/secret"),
            |s| exchange_with(s, "app", b"app/guessed", "myapp://evil"),
        );
        assert!(!ok && got.is_none());
        let (ok, got) = pair(
            |s| accept(s, "app", b"k"),
            |s| exchange_with(s, "other", b"k", "myapp://evil"),
        );
        assert!(!ok && got.is_none());
    }

    #[test]
    fn strangers_are_turned_away_before_anything_is_said() {
        // The old protocol (the token in the clear), and a browser tab.
        for opening in [
            &b"ELYRA-SI/app/secret\nmyapp://evil\n"[..],
            b"POST / HTTP/1.1\r\nHost: x\r\n\r\n",
        ] {
            let mut wire = std::io::Cursor::new(opening.to_vec());
            assert!(accept(&mut wire, "app", b"k").is_none());
            assert_eq!(
                wire.get_ref().len(),
                opening.len(),
                "nothing written back to a stranger"
            );
        }
    }

    #[test]
    fn an_impostor_learns_neither_the_token_nor_the_payload() {
        // Something holding the endpoint first, without the token.
        let (ok, seen) = pair(
            |s| {
                let mut reader = BufReader::new(s);
                let hello = read_line(&mut reader).unwrap_or_default();
                let fake = format!("{} {}\n", "ab".repeat(32), "cd".repeat(32));
                let _ = reader.get_mut().write_all(fake.as_bytes());
                let mut rest = String::new();
                let _ = reader.read_line(&mut rest);
                (hello, rest)
            },
            |s| exchange_with(s, "app", b"app/secret", "myapp://private/42"),
        );
        assert!(!ok, "the client refuses it");
        let (hello, rest) = seen;
        assert!(
            hello.starts_with(HELLO) && !hello.contains("secret"),
            "{hello}"
        );
        assert_eq!(rest, "", "no proof and no payload for the impostor");
    }

    #[test]
    fn a_payload_altered_on_the_way_is_rejected() {
        let (_, got) = pair(
            |s| accept(s, "app", b"k"),
            |s| {
                // A client that proves itself for one payload and sends another.
                let mut reader = BufReader::new(s);
                let client = crate::proof::nonce();
                let _ = reader
                    .get_mut()
                    .write_all(format!("{HELLO} {client}\n").as_bytes());
                let line = read_line(&mut reader).unwrap();
                let (server, _) = line.split_once(' ').unwrap();
                let proof = crate::proof::sign(
                    b"k",
                    &[HELLO, "app", "client", &client, server, "myapp://a"],
                );
                let _ = reader
                    .get_mut()
                    .write_all(format!("{proof} myapp://b\n").as_bytes());
                read_line(&mut reader).as_deref() == Some("OK")
            },
        );
        assert!(got.is_none());
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
        assert!(key(&a).starts_with(format!("{a}/").as_bytes()));
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
