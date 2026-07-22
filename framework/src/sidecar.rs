//! Sidecar child processes (behind the `sidecar` feature).
//!
//! Spawn and manage helper processes: their `stdout`/`stderr` lines and exit are
//! streamed to the frontend on the `elyra:sidecar` event channel, and you can
//! write to `stdin` or kill them by id. Bound in the container as [`Sidecar`];
//! `@elyra/runtime` wraps it as `sidecar` + `onSidecar`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::event::EventBus;

/// An event emitted on `elyra:sidecar`.
#[derive(Serialize)]
struct SidecarEvent<'a> {
    id: u32,
    /// `"data"` for output lines, `"exit"` when the process ends.
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<i32>,
}

enum Cmd {
    Write(Vec<u8>),
    Kill,
}

/// Stream a pipe's lines onto the `elyra:sidecar` channel (one line per event).
fn spawn_reader<R>(id: u32, stream: &'static str, pipe: Option<R>, bus: EventBus)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    if let Some(pipe) = pipe {
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = bus.emit(
                    "elyra:sidecar",
                    &SidecarEvent {
                        id,
                        kind: "data",
                        stream: Some(stream),
                        line: Some(line),
                        code: None,
                    },
                );
            }
        });
    }
}

struct Inner {
    next: u32,
    senders: HashMap<u32, mpsc::UnboundedSender<Cmd>>,
}

/// Spawns and tracks sidecar processes.
pub struct Sidecar {
    bus: EventBus,
    inner: Arc<Mutex<Inner>>,
}

impl Sidecar {
    pub(crate) fn new(bus: EventBus) -> Self {
        Self {
            bus,
            inner: Arc::new(Mutex::new(Inner {
                next: 1,
                senders: HashMap::new(),
            })),
        }
    }

    /// Spawn `program` with `args`. Returns an id used for `write` / `kill` and
    /// carried on every `elyra:sidecar` event.
    pub fn spawn(&self, program: &str, args: &[String]) -> Result<u32, String> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let id = {
            let mut inner = self.inner.lock().unwrap();
            let id = inner.next;
            inner.next += 1;
            id
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        self.inner.lock().unwrap().senders.insert(id, tx);

        // Independent reader tasks for stdout/stderr — one line = one event.
        spawn_reader(id, "stdout", child.stdout.take(), self.bus.clone());
        spawn_reader(id, "stderr", child.stderr.take(), self.bus.clone());

        // The owning task: handle write/kill commands and await exit.
        let bus = self.bus.clone();
        let inner = self.inner.clone();
        let mut stdin = child.stdin.take();
        tokio::spawn(async move {
            // `open` gates the command arm: once every sender is dropped, `recv`
            // resolves to `None` immediately, so we disable the arm (instead of
            // busy-looping) and keep waiting only on the child to exit.
            let mut open = true;
            loop {
                tokio::select! {
                    cmd = rx.recv(), if open => match cmd {
                        Some(Cmd::Write(data)) => {
                            if let Some(si) = stdin.as_mut() {
                                let _ = si.write_all(&data).await;
                                let _ = si.flush().await;
                            }
                        }
                        Some(Cmd::Kill) => { let _ = child.start_kill(); }
                        None => open = false,
                    },
                    status = child.wait() => {
                        let code = status.ok().and_then(|s| s.code());
                        let _ = bus.emit(
                            "elyra:sidecar",
                            &SidecarEvent { id, kind: "exit", stream: None, line: None, code },
                        );
                        inner.lock().unwrap().senders.remove(&id);
                        break;
                    }
                }
            }
        });

        Ok(id)
    }

    /// Write bytes to a running sidecar's stdin. Returns `false` if unknown.
    pub fn write(&self, id: u32, data: Vec<u8>) -> bool {
        self.send(id, Cmd::Write(data))
    }

    /// Ask a sidecar to terminate. Returns `false` if unknown.
    pub fn kill(&self, id: u32) -> bool {
        self.send(id, Cmd::Kill)
    }

    fn send(&self, id: u32, cmd: Cmd) -> bool {
        self.inner
            .lock()
            .unwrap()
            .senders
            .get(&id)
            .map(|tx| tx.send(cmd).is_ok())
            .unwrap_or(false)
    }
}
