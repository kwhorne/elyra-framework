//! Exit hooks run when a signal ends the process (RFC 0053): this test
//! binary starts itself as a child that registers a hook and waits, ends it
//! the way the OS or a terminal would, and checks what the hook left.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

const CHILD: &str = "ELYRA_EXIT_HOOKS_CHILD";

/// In the child: a guard, a hook that writes `marker`, and a wait until
/// something ends the process.
fn child(marker: PathBuf) -> ! {
    let _guard = elyra::exit::guard();
    elyra::exit::on_exit(move || {
        std::fs::write(&marker, "closed").unwrap();
    });
    println!("ready");
    std::thread::sleep(Duration::from_secs(30));
    // Not ended: a failure the parent sees as a missing marker.
    std::process::exit(3);
}

/// Start this test as the child, and wait until it's ready.
fn spawn(marker: &std::path::Path, test: &str) -> std::process::Child {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, marker)
        .stdout(Stdio::piped());
    // Its own process group on Windows, so Ctrl-Break reaches it alone.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = command.spawn().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert!(
        lines.any(|l| l.map(|l| l.contains("ready")).unwrap_or(false)),
        "the child never got ready"
    );
    child
}

fn marker(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("elyra-exit-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

#[cfg(unix)]
#[test]
fn sigterm_runs_the_hooks_then_exits_as_the_signal_would() {
    if let Some(marker) = std::env::var_os(CHILD) {
        child(marker.into());
    }
    let marker = marker("sigterm");
    let mut process = spawn(
        &marker,
        "sigterm_runs_the_hooks_then_exits_as_the_signal_would",
    );
    // SAFETY: a signal to the child this test started.
    unsafe {
        libc::kill(process.id() as libc::pid_t, libc::SIGTERM);
    }
    let status = process.wait().unwrap();
    assert_eq!(status.code(), Some(128 + libc::SIGTERM), "{status:?}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "closed");
    let _ = std::fs::remove_file(&marker);
}

#[cfg(windows)]
#[test]
fn ctrl_break_runs_the_hooks_then_exits() {
    if let Some(marker) = std::env::var_os(CHILD) {
        child(marker.into());
    }
    let marker = marker("ctrl-break");
    let mut process = spawn(&marker, "ctrl_break_runs_the_hooks_then_exits");
    // SAFETY: an event to the process group of the child this test started.
    unsafe {
        windows::Win32::System::Console::GenerateConsoleCtrlEvent(
            windows::Win32::System::Console::CTRL_BREAK_EVENT,
            process.id(),
        )
        .unwrap();
    }
    let status = process.wait().unwrap();
    assert_eq!(status.code(), Some(0xC000_013Au32 as i32), "{status:?}");
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "closed");
    let _ = std::fs::remove_file(&marker);
}

#[test]
fn a_guard_runs_the_hooks_when_main_returns() {
    // In this process: the hooks are the process's, and this is the one test
    // here that runs them in it.
    let marker = marker("guard");
    {
        let _guard = elyra::exit::guard();
        let written = marker.clone();
        elyra::exit::on_exit(move || std::fs::write(&written, "closed").unwrap());
        assert!(!marker.exists());
    }
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "closed");
    let _ = std::fs::remove_file(&marker);
}
