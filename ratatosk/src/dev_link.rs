//! `rata mcp dev`'s way into the running debug build (RFC 0026): the app's dev
//! endpoint, reached as the framework opens it (`framework/src/dev.rs`):
//!
//! - **Unix:** the socket `dev.sock` in the app's folder, beside its
//!   `instance.token` (`~/Library/Application Support/<slug>/` on macOS,
//!   `$XDG_CONFIG_HOME/<slug>/` on Linux).
//! - **Windows:** loopback, on eight ports derived from `elyra-dev/<slug>`.
//!
//! The handshake is the framework's: each side proves it holds the token
//! (HMAC-SHA256 over both nonces) without sending it. Then one JSON request
//! per line, one answer per line.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use serde_json::{json, Value};
use sha2::Sha256;

/// The greeting a dev endpoint answers (`DEV_HELLO` in the framework).
const HELLO: &str = "ELYRA-DEV/1";

/// How long an answer may take: an action settles in the page for up to two
/// seconds, and the app gives a job ten.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A connection to a running debug build.
pub struct Link {
    reader: BufReader<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
    next: u64,
}

/// What can't be reached, said so an agent knows what to do.
pub fn not_running(names: &[String]) -> String {
    format!(
        "no running debug build of {} to look at: start it with `rata dev` (or `cargo run`), \
         and keep it open. A release build has no dev endpoint.",
        names
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(" or ")
    )
}

impl Link {
    /// Reach the running debug build of the app named one of `names` (its
    /// `AboutInfo` name first): `None` when none answers.
    pub fn connect(names: &[String]) -> Option<Link> {
        names.iter().find_map(|name| {
            let dir = app_config_dir(name)?;
            let token = std::fs::read_to_string(dir.join("instance.token")).ok()?;
            let token = token.trim();
            #[cfg(unix)]
            {
                let stream = std::os::unix::net::UnixStream::connect(dir.join("dev.sock")).ok()?;
                stream.set_read_timeout(Some(TIMEOUT)).ok()?;
                let writer = stream.try_clone().ok()?;
                Link::greet(Box::new(stream), Box::new(writer), token.as_bytes())
            }
            #[cfg(not(unix))]
            {
                ports_in("elyra-dev", name).into_iter().find_map(|port| {
                    let stream =
                        std::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).ok()?;
                    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
                    let writer = stream.try_clone().ok()?;
                    Link::greet(Box::new(stream), Box::new(writer), token.as_bytes())
                })
            }
        })
    }

    /// The client's side of the handshake on a fresh connection.
    pub fn greet(
        read: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
        key: &[u8],
    ) -> Option<Link> {
        let mut reader = BufReader::new(read);
        let client = nonce();
        writer
            .write_all(format!("{HELLO} {client}\n").as_bytes())
            .ok()?;
        writer.flush().ok()?;
        let line = read_line(&mut reader)?;
        let (server, proof) = line.split_once(' ')?;
        // Whatever answers must prove it holds the token before it's sent ours.
        if !is_nonce(server) || !verify(key, &[HELLO, "server", &client, server], proof) {
            return None;
        }
        let answer = sign(key, &[HELLO, "client", &client, server]);
        writer.write_all(format!("{answer}\n").as_bytes()).ok()?;
        writer.flush().ok()?;
        (read_line(&mut reader)?.as_str() == "OK").then_some(Link {
            reader,
            writer,
            next: 1,
        })
    }

    /// Ask `op` of `window` (the focused one when `None`), with `args`.
    pub fn ask(&mut self, op: &str, window: Option<&str>, args: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        let request = json!({ "id": id, "op": op, "window": window, "args": args });
        self.writer
            .write_all(format!("{request}\n").as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| format!("the app went away: {e}"))?;
        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(|e| format!("the app didn't answer: {e}"))?;
        if line.is_empty() {
            return Err("the app closed the connection".into());
        }
        let answer: Value =
            serde_json::from_str(&line).map_err(|e| format!("the app answered `{line}`: {e}"))?;
        match answer.get("error").and_then(Value::as_str) {
            Some(error) => Err(error.to_owned()),
            None => Ok(answer.get("ok").cloned().unwrap_or(Value::Null)),
        }
    }
}

/// One handshake line, without its newline: at most 512 bytes.
fn read_line(reader: &mut impl BufRead) -> Option<String> {
    let mut line = String::new();
    reader.take(512).read_line(&mut line).ok()?;
    line.ends_with('\n').then(|| line.trim_end().to_owned())
}

fn mac(key: &[u8], parts: &[&str]) -> Hmac<Sha256> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(parts.join("|").as_bytes());
    mac
}

fn sign(key: &[u8], parts: &[&str]) -> String {
    mac(key, parts)
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verify(key: &[u8], parts: &[&str], proof: &str) -> bool {
    if !proof.len().is_multiple_of(2) || !proof.is_ascii() {
        return false;
    }
    let Some(bytes) = (0..proof.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&proof[i..i + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()
    else {
        return false;
    };
    mac(key, parts).verify_slice(&bytes).is_ok()
}

fn is_nonce(s: &str) -> bool {
    (32..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 256 random bits as hex, as the framework makes them.
fn nonce() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    (0..4u64)
        .map(|round| {
            let mut hasher = RandomState::new().build_hasher();
            hasher.write_u64(round);
            hasher.write_u64(u64::from(std::process::id()));
            hasher.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or_default(),
            );
            format!("{:016x}", hasher.finish())
        })
        .collect()
}

/// The app's name as `slug` writes it: the framework's folders and ports.
fn slug(app: &str) -> String {
    let slug: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "elyra-app".into()
    } else {
        slug.into()
    }
}

/// `<config dir>/<slug>`: the app's folder, where the framework keeps its
/// token (and, on Unix, the dev socket).
pub fn app_config_dir(app: &str) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join(slug(app)))
}

/// The eight candidate ports for `namespace`/`app` on Windows, as the
/// framework derives them (FNV-1a, spread 2039 apart from 49152).
#[cfg_attr(unix, allow(dead_code))]
fn ports_in(namespace: &str, app: &str) -> [u16; 8] {
    let mut seed: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("{namespace}/{}", slug(app)).bytes() {
        seed ^= u64::from(b);
        seed = seed.wrapping_mul(0x0100_0000_01b3);
    }
    std::array::from_fn(|i| 49152 + ((seed + i as u64 * 2039) % 16384) as u16)
}

/// The names the app may be known by, most likely first: its `AboutInfo`
/// name (what the framework names its folder after), a window title, then
/// `[bundle].name` and the crate.
pub fn app_names(src: &Path, bundle_name: &str, app_crate: &str) -> Vec<String> {
    let mut files = Vec::new();
    crate::make_resource::rust_files(src, &mut files);
    let mut names = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for call in ["AboutInfo::new(", ".title("] {
            for (at, _) in text.match_indices(call) {
                let rest = text[at + call.len()..].trim_start();
                if let Some((literal, _)) = rest.strip_prefix('"').and_then(|r| r.split_once('"')) {
                    names.push(literal.to_owned());
                }
            }
        }
    }
    names.push(bundle_name.to_owned());
    names.push(app_crate.to_owned());
    let mut seen = std::collections::HashSet::new();
    names.retain(|n| !n.is_empty() && seen.insert(slug(n)));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server's side of the handshake, as the framework's `welcome` runs it.
    #[cfg(unix)]
    fn welcome(stream: &mut std::os::unix::net::UnixStream, key: &[u8], hello: &str) -> bool {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let Some(line) = read_line(&mut reader) else {
            return false;
        };
        let Some(client) = line.strip_prefix(hello).and_then(|r| r.strip_prefix(' ')) else {
            return false;
        };
        let server = nonce();
        let proof = sign(key, &[hello, "server", client, &server]);
        if stream
            .write_all(format!("{server} {proof}\n").as_bytes())
            .is_err()
        {
            return false;
        }
        let Some(answer) = read_line(&mut reader) else {
            return false;
        };
        if !verify(key, &[hello, "client", client, &server], &answer) {
            return false;
        }
        stream.write_all(b"OK\n").is_ok()
    }

    #[cfg(unix)]
    fn pair() -> (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ) {
        std::os::unix::net::UnixStream::pair().unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn it_greets_a_dev_endpoint_and_asks_it() {
        let (client, mut server) = pair();
        let app = std::thread::spawn(move || {
            assert!(welcome(&mut server, b"the-token-0123456789", HELLO));
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["op"], "snapshot");
            assert_eq!(request["window"], "main");
            let answer = json!({ "id": request["id"], "ok": "route: /" });
            server.write_all(format!("{answer}\n").as_bytes()).unwrap();
            line.clear();
            reader.read_line(&mut line).unwrap();
            let request: Value = serde_json::from_str(&line).unwrap();
            let answer = json!({ "id": request["id"], "error": "no element e9" });
            server.write_all(format!("{answer}\n").as_bytes()).unwrap();
        });
        let writer = client.try_clone().unwrap();
        let mut link = Link::greet(Box::new(client), Box::new(writer), b"the-token-0123456789")
            .expect("greeted");
        assert_eq!(
            link.ask("snapshot", Some("main"), json!({})).unwrap(),
            "route: /"
        );
        assert_eq!(
            link.ask("act", None, json!({ "ref": "e9" })).unwrap_err(),
            "no element e9"
        );
        app.join().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn it_doesnt_talk_to_something_without_the_token_or_the_greeting() {
        // The wrong token: the server can't prove itself, so the client stops.
        let (client, mut server) = pair();
        let app = std::thread::spawn(move || welcome(&mut server, b"someone-else's-token", HELLO));
        let writer = client.try_clone().unwrap();
        assert!(Link::greet(Box::new(client), Box::new(writer), b"the-token-0123456789").is_none());
        let _ = app.join();
        // The MCP endpoint (another greeting) doesn't take it either.
        let (client, mut server) = pair();
        let app = std::thread::spawn(move || {
            welcome(&mut server, b"the-token-0123456789", "ELYRA-MCP/1")
        });
        let writer = client.try_clone().unwrap();
        assert!(Link::greet(Box::new(client), Box::new(writer), b"the-token-0123456789").is_none());
        assert!(!app.join().unwrap());
    }

    /// Something holding the socket without the token learns nothing: the
    /// client hangs up before it sends its own proof.
    #[cfg(unix)]
    #[test]
    fn an_impostor_never_gets_the_clients_proof() {
        let (client, mut server) = pair();
        // A client that wrongly answers would wait for `OK` forever: fail, not hang.
        for side in [&client, &server] {
            side.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        }
        let impostor = std::thread::spawn(move || {
            let mut reader = BufReader::new(server.try_clone().unwrap());
            let hello = read_line(&mut reader).unwrap();
            let client_nonce = hello.strip_prefix("ELYRA-DEV/1 ").unwrap().to_owned();
            let mine = nonce();
            let fake = sign(
                b"not-the-token-at-all",
                &[HELLO, "server", &client_nonce, &mine],
            );
            server
                .write_all(format!("{mine} {fake}\n").as_bytes())
                .unwrap();
            // What the client sends next, if anything, before it hangs up.
            let mut rest = String::new();
            let _ = reader.read_to_string(&mut rest);
            rest
        });
        let writer = client.try_clone().unwrap();
        assert!(Link::greet(Box::new(client), Box::new(writer), b"the-token-0123456789").is_none());
        assert_eq!(
            impostor.join().unwrap(),
            "",
            "the client sent nothing after the impostor's line"
        );
    }

    #[test]
    fn the_proofs_are_the_frameworks() {
        // HMAC-SHA256 over the parts joined by `|`, in lowercase hex.
        let proof = sign(b"key", &["ELYRA-DEV/1", "server", "a", "b"]);
        assert_eq!(proof.len(), 64);
        assert!(verify(b"key", &["ELYRA-DEV/1", "server", "a", "b"], &proof));
        assert!(!verify(
            b"key",
            &["ELYRA-DEV/1", "client", "a", "b"],
            &proof
        ));
        assert!(!verify(b"key", &["x"], "not hex"));
        assert!(is_nonce(&nonce()));
        assert_ne!(nonce(), nonce());
    }

    #[test]
    fn ports_and_folders_follow_the_frameworks_slug() {
        assert_eq!(slug("Elyra Example"), "elyra-example");
        assert_eq!(slug("!!"), "elyra-app");
        let ports = ports_in("elyra-dev", "Elyra Example");
        assert!(ports.iter().all(|p| *p >= 49152));
        assert_ne!(ports, ports_in("elyra-mcp", "Elyra Example"));
        assert!(app_config_dir("Elyra Example")
            .unwrap()
            .ends_with("elyra-example"));
    }

    #[test]
    fn the_apps_names_come_from_its_source_first() {
        let dir = std::env::temp_dir().join(format!("rata-dev-names-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            r#"fn main() { App::new().title("CRM").about(AboutInfo::new("Acme CRM", "1.0")).run(); }"#,
        )
        .unwrap();
        let names = app_names(&dir.join("src"), "acme", "acme-crm");
        assert_eq!(names, ["Acme CRM", "CRM", "acme"]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
