//! Single-instance support.
//!
//! Uses a loopback TCP rendezvous on a port derived from the app id: the first
//! instance binds it (primary), later instances connect and hand over their
//! payload (e.g. a deep-link URL) then exit. A short magic-string handshake
//! guards against an unrelated process happening to hold the port.
//!
//! Portable (no platform code, no extra crates); loopback-only, so nothing is
//! exposed off the machine.

use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::Duration;

fn port_for(app: &str) -> u16 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    app.hash(&mut h);
    "elyra-single-instance".hash(&mut h);
    // Dynamic/private range 49152..=65535.
    49152 + (h.finish() % 16384) as u16
}

fn magic(app: &str) -> String {
    format!("ELYRA-SI/{app}")
}

/// Try to hand our `payload` to an already-running primary. Returns `true` if a
/// primary acknowledged it — in which case this process should exit.
pub(crate) fn notify_primary(app: &str, payload: &str) -> bool {
    let Ok(mut stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port_for(app))) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
    let line = format!("{}\n{}\n", magic(app), payload.replace(['\n', '\r'], " "));
    if stream.write_all(line.as_bytes()).is_err() {
        return false;
    }
    let _ = stream.flush();
    let mut ack = String::new();
    if BufReader::new(&stream).read_line(&mut ack).is_err() {
        return false;
    }
    // A stranger on the port won't produce the expected ack.
    ack.trim() == magic(app)
}

/// Become the primary by binding the rendezvous port. `None` means someone else
/// holds it (a race we lost, or an unrelated process) — run without enforcement.
pub(crate) fn bind_primary(app: &str) -> Option<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port_for(app))).ok()
}

/// Serve the accept loop on a background thread, invoking `on_payload` for each
/// valid second launch.
pub(crate) fn serve(
    listener: TcpListener,
    app: String,
    on_payload: impl Fn(String) + Send + 'static,
) {
    std::thread::spawn(move || {
        let expected = magic(&app);
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_millis(750)));
            let mut writer = &stream;
            let mut reader = BufReader::new(&stream);
            let mut hello = String::new();
            if reader.read_line(&mut hello).is_err() || hello.trim() != expected {
                continue;
            }
            let mut payload = String::new();
            let _ = reader.read_line(&mut payload);
            let _ = writeln!(writer, "{expected}");
            let _ = writer.flush();
            on_payload(payload.trim().to_string());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_is_stable_and_in_range() {
        let p = port_for("MyApp");
        assert_eq!(p, port_for("MyApp"));
        assert!((49152..=65535).contains(&p));
        assert_ne!(port_for("MyApp"), port_for("OtherApp"));
    }

    #[test]
    fn primary_receives_secondary_payload() {
        // Unique per process so parallel/repeat CI runs on one machine can't
        // collide on the derived rendezvous port.
        let app = &format!("elyra-si-test-{}", std::process::id());
        let listener = bind_primary(app).expect("bind primary");
        let (tx, rx) = std::sync::mpsc::channel();
        serve(listener, app.to_string(), move |p| {
            let _ = tx.send(p);
        });
        assert!(notify_primary(app, "myapp://open/42"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "myapp://open/42"
        );
        // No primary for a different id → nothing to notify.
        assert!(!notify_primary("elyra-si-absent", ""));
    }
}
