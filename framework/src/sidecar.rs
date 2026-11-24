//! Sidecar child processes (behind the `sidecar` feature).
//!
//! Spawn and manage helper processes: their `stdout`/`stderr` lines and exit are
//! streamed to the frontend on the `elyra:sidecar` event channel, and you can
//! write to `stdin` or kill them by id. Bound in the container as [`Sidecar`];
//! `@elyra/runtime` wraps it as `sidecar` + `onSidecar`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use parking_lot::Mutex;

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
    /// Each running child's process id, to end it at exit (RFC 0053).
    pids: HashMap<u32, u32>,
}

/// Spawns and tracks sidecar processes.
pub struct Sidecar {
    bus: EventBus,
    inner: Arc<Mutex<Inner>>,
}

impl Sidecar {
    pub(crate) fn new(bus: EventBus) -> Self {
        let inner = Arc::new(Mutex::new(Inner {
            next: 1,
            senders: HashMap::new(),
            pids: HashMap::new(),
        }));
        // A sidecar ends with the app (RFC 0053): the process ends without
        // dropping anything, so the children still running are ended here,
        // at once, not through their tasks.
        let running = Arc::downgrade(&inner);
        crate::exit::on_exit(move || {
            if let Some(inner) = running.upgrade() {
                for pid in std::mem::take(&mut inner.lock().pids).into_values() {
                    end(pid);
                }
            }
        });
        Self { bus, inner }
    }

    /// Spawn `program` with `args`. Returns an id used for `write` / `kill` and
    /// carried on every `elyra:sidecar` event.
    pub fn spawn(&self, program: &str, args: &[String]) -> Result<u32, String> {
        let mut child = Command::new(program)
            .args(args)
            // Dropped with its task, it's killed rather than left behind.
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| e.to_string())?;

        let id = {
            let mut inner = self.inner.lock();
            let id = inner.next;
            inner.next += 1;
            id
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
        {
            let mut inner = self.inner.lock();
            inner.senders.insert(id, tx);
            if let Some(pid) = child.id() {
                inner.pids.insert(id, pid);
            }
        }

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
                        let mut inner = inner.lock();
                        inner.senders.remove(&id);
                        inner.pids.remove(&id);
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
            .senders
            .get(&id)
            .map(|tx| tx.send(cmd).is_ok())
            .unwrap_or(false)
    }
}

/// End the process `pid` at once: what the exit hook does to a sidecar still
/// running.
fn end(pid: u32) {
    #[cfg(unix)]
    // SAFETY: `kill` takes any pid and signal; a pid that's gone is an error
    // it returns, not undefined behaviour.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(windows)]
    // SAFETY: a handle opened here, used once and closed.
    unsafe {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
        if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
            let _ = TerminateProcess(handle, 1);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::end;

    #[test]
    fn a_child_still_running_is_ended() {
        #[cfg(unix)]
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        #[cfg(windows)]
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        end(child.id());
        let status = child.wait().unwrap();
        assert!(!status.success());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
