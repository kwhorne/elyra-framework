//! The private endpoint a running app serves MCP on (RFC 0003), so that
//! `myapp --mcp` — the stdio shim an MCP client launches — reaches the app
//! that's already open: the same database, live queries and events, and the
//! windows update as the agent works.
//!
//! It's built like single-instance's ([`crate::instance`]), separate from it,
//! and only there when the app calls `App::mcp`:
//!
//! - **Unix:** an `AF_UNIX` socket with mode `0600`, beside single-instance's.
//! - **Windows:** loopback TCP, on ports derived from the app's name.
//!
//! Either way a connection is served only after a challenge-response on the
//! per-install token. Each side proves it holds the token without sending it,
//! so neither a stranger that connects nor one that *listens* (holding the
//! socket path or a port first) learns it — and the shim never hands an
//! impostor the agent's calls.

use std::sync::Arc;
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
    ReadHalf, WriteHalf,
};

use super::McpServer;

/// The first line a client sends, before its nonce.
const HELLO: &str = "ELYRA-MCP/1";

/// How long either side waits for the other's next handshake line.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

/// A handshake line is a nonce and a proof — nothing longer.
const MAX_LINE: u64 = 512;

/// The Unix socket's extension, beside single-instance's `.sock`.
#[cfg(unix)]
const SOCKET_EXT: &str = "mcp.sock";

/// The Windows port namespace, apart from single-instance's.
#[cfg(not(unix))]
const PORTS: &str = "elyra-mcp";

/// A connection to the endpoint: a socket, or a loopback stream.
pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}

#[cfg(unix)]
type Listener = std::os::unix::net::UnixListener;
#[cfg(not(unix))]
type Listener = std::net::TcpListener;

/// The endpoint, bound and holding the token it checks clients against.
pub(crate) struct Endpoint {
    listener: Listener,
    key: Arc<[u8]>,
}

/// Bind `app`'s endpoint. `None` when there's no per-install token to check
/// clients against (no writable app dir), or another instance holds it.
pub(crate) fn bind(app: &str) -> Option<Endpoint> {
    let key = crate::instance::token(app)?;
    let listener = bind_listener(app)?;
    listener.set_nonblocking(true).ok()?;
    Some(Endpoint {
        listener,
        key: key.into_bytes().into(),
    })
}

#[cfg(unix)]
fn bind_listener(app: &str) -> Option<Listener> {
    use std::os::unix::fs::PermissionsExt;
    let path = crate::instance::socket_path(app, SOCKET_EXT);
    let bound = Listener::bind(&path).ok().or_else(|| {
        // A leftover socket from a crashed run: if nobody answers, replace it.
        if std::os::unix::net::UnixStream::connect(&path).is_ok() {
            return None;
        }
        let _ = std::fs::remove_file(&path);
        Listener::bind(&path).ok()
    })?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).ok()?;
    Some(bound)
}

#[cfg(not(unix))]
fn bind_listener(app: &str) -> Option<Listener> {
    use crate::instance::loopback;
    loopback::bind_first(&loopback::ports_in(PORTS, app))
}

impl Endpoint {
    /// Serve `server` to every client that passes the handshake — each on its
    /// own connection — for as long as the runtime runs.
    pub(crate) async fn serve(self, server: McpServer) {
        #[cfg(unix)]
        let listener = tokio::net::UnixListener::from_std(self.listener);
        #[cfg(not(unix))]
        let listener = tokio::net::TcpListener::from_std(self.listener);
        let listener = match listener {
            Ok(listener) => listener,
            Err(e) => {
                crate::warn!(target: "elyra::mcp", "the MCP endpoint can't listen: {e}");
                return;
            }
        };
        loop {
            let stream = match listener.accept().await {
                Ok((stream, _)) => stream,
                Err(e) => {
                    // Out of file descriptors, say: back off rather than spin.
                    crate::debug!(target: "elyra::mcp", "accept failed: {e}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };
            tokio::spawn(serve_one(stream, server.clone(), self.key.clone()));
        }
    }
}

/// One connection: the handshake, then MCP until it closes.
async fn serve_one<S: Stream>(stream: S, server: McpServer, key: Arc<[u8]>) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let welcomed = tokio::time::timeout(HANDSHAKE_TIMEOUT, welcome(&mut reader, &mut writer, &key))
        .await
        .unwrap_or(false);
    if !welcomed {
        crate::debug!(target: "elyra::mcp", "refused a connection: bad handshake");
        return;
    }
    crate::info!(target: "elyra::mcp", "an MCP client connected");
    if let Err(e) = server.serve(reader, writer).await {
        crate::debug!(target: "elyra::mcp", "an MCP connection ended: {e}");
    }
}

/// A shim's side of a connection that passed the handshake.
pub(crate) struct Link {
    reader: BufReader<ReadHalf<Box<dyn Stream>>>,
    writer: WriteHalf<Box<dyn Stream>>,
}

/// Reach `app`'s running instance. `None` when it isn't running, has no MCP
/// endpoint, or whatever answers can't prove it holds the token.
pub(crate) async fn connect(app: &str) -> Option<Link> {
    let key = crate::instance::token(app)?;
    #[cfg(unix)]
    {
        let path = crate::instance::socket_path(app, SOCKET_EXT);
        let stream = tokio::net::UnixStream::connect(path).await.ok()?;
        greet(Box::new(stream), key.as_bytes()).await
    }
    #[cfg(not(unix))]
    {
        connect_loopback(
            &crate::instance::loopback::ports_in(PORTS, app),
            key.as_bytes(),
        )
        .await
    }
}

/// Try each candidate port: one nobody holds is refused at once, one a
/// stranger holds fails the handshake — either way, on to the next.
#[cfg(any(test, not(unix)))]
async fn connect_loopback(ports: &[u16], key: &[u8]) -> Option<Link> {
    for port in ports {
        let Ok(stream) =
            tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, *port)).await
        else {
            continue;
        };
        if let Some(link) = greet(Box::new(stream), key).await {
            return Some(link);
        }
    }
    None
}

/// The client handshake on `stream`, then the link.
async fn greet(stream: Box<dyn Stream>, key: &[u8]) -> Option<Link> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    tokio::time::timeout(HANDSHAKE_TIMEOUT, hello(&mut reader, &mut writer, key))
        .await
        .unwrap_or(false)
        .then_some(Link { reader, writer })
}

/// Pipe MCP lines between the client (`input` / `output`, the shim's stdio)
/// and the app, until the app closes the connection. When the client closes
/// first, the app is told, and its answers to what's in flight still arrive.
pub(crate) async fn pipe<I, O>(link: Link, mut input: I, mut output: O) -> std::io::Result<()>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin + Send + 'static,
{
    let Link {
        mut reader,
        mut writer,
    } = link;
    let mut back = tokio::spawn(async move {
        tokio::io::copy_buf(&mut reader, &mut output).await?;
        output.flush().await
    });
    let forward = async {
        tokio::io::copy(&mut input, &mut writer).await?;
        writer.shutdown().await
    };
    let joined = |r: Result<std::io::Result<()>, tokio::task::JoinError>| {
        r.map_err(std::io::Error::other)?
    };
    tokio::select! {
        // The app went away: so does the shim, and the client starts afresh.
        done = &mut back => joined(done),
        sent = forward => {
            sent?;
            joined(back.await)
        }
    }
}

// ---------------------------------------------------------------------------
// The handshake:
//
//   client → `ELYRA-MCP/1 <client nonce>`
//   server → `<server nonce> <proof("server")>`
//   client → `<proof("client")>`
//   server → `OK`
//
// where proof(role) = HMAC-SHA256(token, "ELYRA-MCP/1|<role>|<client>|<server>").
// Fresh nonces on both sides: a recorded exchange can't be replayed, and the
// server proves itself before the client answers.
// ---------------------------------------------------------------------------

/// The server's side. `true` once the client has proven it holds `key`.
async fn welcome<R, W>(reader: &mut R, writer: &mut W, key: &[u8]) -> bool
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let Some(line) = read_line(reader).await else {
        return false;
    };
    // Also turns away an HTTP request from a browser tab.
    let Some(client) = line
        .strip_prefix(HELLO)
        .and_then(|rest| rest.strip_prefix(' '))
        .filter(|n| is_nonce(n))
    else {
        return false;
    };
    let server = crate::security::random_token();
    let reply = format!("{server} {}\n", hex(&proof(key, "server", client, &server)));
    if writer.write_all(reply.as_bytes()).await.is_err() || writer.flush().await.is_err() {
        return false;
    }
    let Some(answer) = read_line(reader).await else {
        return false;
    };
    if !verify(key, "client", client, &server, &answer) {
        return false;
    }
    writer.write_all(b"OK\n").await.is_ok() && writer.flush().await.is_ok()
}

/// The client's side. `true` once the server has proven it holds `key` and
/// accepted the client's proof.
async fn hello<R, W>(reader: &mut R, writer: &mut W, key: &[u8]) -> bool
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let client = crate::security::random_token();
    let greeting = format!("{HELLO} {client}\n");
    if writer.write_all(greeting.as_bytes()).await.is_err() || writer.flush().await.is_err() {
        return false;
    }
    let Some(line) = read_line(reader).await else {
        return false;
    };
    let Some((server, server_proof)) = line.split_once(' ') else {
        return false;
    };
    // An impostor can't get past this, so it never sees the client's proof.
    if !is_nonce(server) || !verify(key, "server", &client, server, server_proof) {
        return false;
    }
    let answer = format!("{}\n", hex(&proof(key, "client", &client, server)));
    if writer.write_all(answer.as_bytes()).await.is_err() || writer.flush().await.is_err() {
        return false;
    }
    read_line(reader).await.as_deref() == Some("OK")
}

fn mac(key: &[u8], role: &str, client: &str, server: &str) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(format!("{HELLO}|{role}|{client}|{server}").as_bytes());
    mac
}

fn proof(key: &[u8], role: &str, client: &str, server: &str) -> Vec<u8> {
    mac(key, role, client, server)
        .finalize()
        .into_bytes()
        .to_vec()
}

/// Check a proof in constant time.
fn verify(key: &[u8], role: &str, client: &str, server: &str, proof_hex: &str) -> bool {
    unhex(proof_hex)
        .is_some_and(|bytes| mac(key, role, client, server).verify_slice(&bytes).is_ok())
}

/// One handshake line, without its newline — `None` on EOF, an error, or a
/// line too long to be one.
async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Option<String> {
    let mut line = String::new();
    reader
        .take(MAX_LINE)
        .read_line(&mut line)
        .await
        .ok()
        .filter(|_| line.ends_with('\n'))?;
    Some(line.trim_end().to_owned())
}

fn is_nonce(s: &str) -> bool {
    (32..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(super) fn unhex(s: &str) -> Option<Vec<u8>> {
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
    use crate::command::CommandRegistry;
    use crate::container::{Container, Ctx};
    use crate::mcp::Mcp;
    use serde_json::{json, Value};

    const KEY: &[u8] = b"the-per-install-token-0123456789";

    /// Run both sides of the handshake over an in-memory pipe.
    async fn handshake(client_key: &[u8], server_key: &[u8]) -> (bool, bool) {
        let (a, b) = tokio::io::duplex(4096);
        let (cr, mut cw) = tokio::io::split(a);
        let (sr, mut sw) = tokio::io::split(b);
        let (mut cr, mut sr) = (BufReader::new(cr), BufReader::new(sr));
        // Each side hangs up when it's done, as a real one would.
        tokio::join!(
            async move { hello(&mut cr, &mut cw, client_key).await },
            async move { welcome(&mut sr, &mut sw, server_key).await }
        )
    }

    #[tokio::test]
    async fn the_handshake_passes_with_the_same_token() {
        assert_eq!(handshake(KEY, KEY).await, (true, true));
    }

    #[tokio::test]
    async fn the_handshake_fails_with_a_different_token() {
        let (client, server) = handshake(b"someone-else's-token", KEY).await;
        assert!(!client && !server);
    }

    #[tokio::test]
    async fn an_impostor_listening_learns_nothing() {
        // Something holding the socket path or port first, without the token.
        let (a, b) = tokio::io::duplex(4096);
        let (cr, mut cw) = tokio::io::split(a);
        let mut cr = BufReader::new(cr);
        let impostor = tokio::spawn(async move {
            let (sr, mut sw) = tokio::io::split(b);
            let mut sr = BufReader::new(sr);
            let greeting = read_line(&mut sr).await.unwrap_or_default();
            let fake = format!("{} {}\n", "ab".repeat(32), "cd".repeat(32));
            let _ = sw.write_all(fake.as_bytes()).await;
            // Whatever the client sends next, if anything.
            let mut rest = String::new();
            let _ = tokio::time::timeout(Duration::from_millis(200), sr.read_line(&mut rest)).await;
            (greeting, rest)
        });
        assert!(!hello(&mut cr, &mut cw, KEY).await, "the client refuses it");
        drop((cr, cw));
        let (greeting, rest) = impostor.await.unwrap();
        assert!(greeting.starts_with(HELLO), "{greeting}");
        assert!(!greeting.contains(std::str::from_utf8(KEY).unwrap()));
        assert_eq!(rest, "", "no proof for the impostor to replay");
    }

    #[tokio::test]
    async fn a_stranger_is_turned_away() {
        for opening in ["POST / HTTP/1.1\r\n", "ELYRA-MCP/1 not-a-nonce\n", "\n"] {
            let (a, b) = tokio::io::duplex(4096);
            let (sr, mut sw) = tokio::io::split(b);
            let mut sr = BufReader::new(sr);
            let (_cr, mut cw) = tokio::io::split(a);
            cw.write_all(opening.as_bytes()).await.unwrap();
            assert!(!welcome(&mut sr, &mut sw, KEY).await, "{opening:?}");
        }
        // A replayed client proof doesn't fit a fresh server nonce.
        let (a, b) = tokio::io::duplex(4096);
        let (sr, mut sw) = tokio::io::split(b);
        let mut sr = BufReader::new(sr);
        let (_cr, mut cw) = tokio::io::split(a);
        let client = "12".repeat(32);
        let stale = hex(&proof(KEY, "client", &client, &"34".repeat(32)));
        cw.write_all(format!("{HELLO} {client}\n{stale}\n").as_bytes())
            .await
            .unwrap();
        assert!(!welcome(&mut sr, &mut sw, KEY).await);
    }

    fn server() -> McpServer {
        McpServer::new(
            Ctx::new(Arc::new(Container::new())),
            Arc::new(CommandRegistry::new()),
            Mcp::new(),
            "Endpoint Test",
            "1.2.3",
        )
        .unwrap()
    }

    fn discover(id: u64) -> String {
        format!(
            "{}\n",
            json!({ "jsonrpc": "2.0", "id": id, "method": "server/discover", "params": {} })
        )
    }

    /// The shim, end to end: bind the real endpoint, serve it, and pipe a
    /// client's lines through `connect` + `pipe`.
    #[tokio::test]
    async fn a_shim_reaches_the_running_app() {
        let app = format!("elyra-mcp-endpoint-test-{}", std::process::id());
        let Some(endpoint) = bind(&app) else {
            eprintln!("skipping: no writable app dir for the token here");
            return;
        };
        tokio::spawn(endpoint.serve(server()));
        let link = connect(&app).await.expect("the shim connects");

        let (mut client, shim_stdio) = tokio::io::duplex(1 << 16);
        let (shim_in, shim_out) = tokio::io::split(shim_stdio);
        let piping = tokio::spawn(pipe(link, shim_in, shim_out));

        client.write_all(discover(1).as_bytes()).await.unwrap();
        client.write_all(discover(2).as_bytes()).await.unwrap();
        let mut lines = BufReader::new(client).lines();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let line = lines.next_line().await.unwrap().expect("a reply");
            let reply: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                reply.pointer("/result/_meta/io.modelcontextprotocol~1serverInfo/name"),
                Some(&json!("Endpoint Test")),
                "{reply}"
            );
            ids.push(reply["id"].as_u64().unwrap());
        }
        ids.sort_unstable();
        assert_eq!(ids, [1, 2]);

        // The client closes stdin: the shim tells the app, which hangs up.
        drop(lines);
        tokio::time::timeout(Duration::from_secs(5), piping)
            .await
            .expect("the shim finishes")
            .unwrap()
            .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_socket_is_private_and_held_once() {
        use std::os::unix::fs::PermissionsExt;
        let app = format!("elyra-mcp-endpoint-perm-{}", std::process::id());
        let Some(endpoint) = bind(&app) else {
            return;
        };
        let path = crate::instance::socket_path(&app, SOCKET_EXT);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        tokio::spawn(endpoint.serve(server()));
        assert!(
            bind(&app).is_none(),
            "a second instance doesn't take it over"
        );
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_shim_skips_a_stranger_on_a_port() {
        // A stranger holds the first candidate and answers nonsense; the app
        // is on a later one. (Windows' scheme, on real sockets everywhere.)
        let stranger = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ports = [
            stranger.local_addr().unwrap().port(),
            app.local_addr().unwrap().port(),
        ];
        tokio::spawn(async move {
            while let Ok((mut s, _)) = stranger.accept().await {
                let _ = s.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            }
        });
        tokio::spawn(async move {
            while let Ok((s, _)) = app.accept().await {
                tokio::spawn(serve_one(s, server(), KEY.into()));
            }
        });

        let link = connect_loopback(&ports, KEY)
            .await
            .expect("reaches the app");
        let Link {
            mut reader,
            mut writer,
        } = link;
        writer.write_all(discover(7).as_bytes()).await.unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.contains("\"id\":7"), "{line}");

        assert!(
            connect_loopback(&ports, b"wrong-token").await.is_none(),
            "and refuses without the token"
        );
    }
}
