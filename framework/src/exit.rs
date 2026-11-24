//! Work done however the app ends (RFC 0053).
//!
//! The event loop ends the process without running destructors, and so does
//! the updater's relaunch, so a `Drop` can't be relied on to end what the app
//! started: an `ssh -N` tunnel, a server it launched, a lock file. An exit
//! hook can. Hooks run once, in the reverse order they were added, on the
//! thread that's ending the process, whichever way it ends:
//!
//! - the event loop's end: the last window closed, Quit, ⌘Q;
//! - a relaunch into an update;
//! - the end of an `--mcp` run;
//! - `SIGINT`, `SIGTERM` or `SIGHUP` (Unix), and Ctrl-C, Ctrl-Break, a
//!   closed console, log-off or shutdown (Windows), each followed by the
//!   exit it would have made;
//! - an [`ExitGuard`] dropped, for a `main` without `App::run`.
//!
//! Nothing runs on `SIGKILL`, a force-quit, a crash or a power loss. A child
//! that must never be left behind watches its own stdin.
//!
//! ```ignore
//! fn main() -> anyhow::Result<()> {
//!     let _exit = elyra::exit::guard();   // hooks on drop, and on Ctrl-C/SIGTERM
//!     elyra::exit::on_exit(tunnel::kill_all);
//!     run_cli()
//! }
//! ```

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, Once};

type Hook = Box<dyn FnOnce() + Send>;

static HOOKS: Mutex<Vec<Hook>> = Mutex::new(Vec::new());

/// Run `hook` once when the process ends, whichever way (see the
/// [module](self)). Keep it quick: signal a child rather than wait for it.
pub fn on_exit(hook: impl FnOnce() + Send + 'static) {
    HOOKS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Box::new(hook));
}

/// Run the hooks now: what every way out calls. Each runs once, last added
/// first; one that panics is logged and the rest still run. A hook added
/// while they run runs too. A later call finds nothing left to run.
pub fn run() {
    loop {
        let hooks = std::mem::take(&mut *HOOKS.lock().unwrap_or_else(|e| e.into_inner()));
        if hooks.is_empty() {
            return;
        }
        for hook in hooks.into_iter().rev() {
            if catch_unwind(AssertUnwindSafe(hook)).is_err() {
                crate::log::log(
                    crate::log::Level::Error,
                    "elyra::exit",
                    "an exit hook panicked; the others still run",
                );
            }
        }
    }
}

/// Runs the hooks when dropped, and ends the process through them on a
/// signal. For a `main` that doesn't call `App::run`, such as a command-line
/// tool built without the window: hold it for the life of `main`.
#[must_use = "the hooks run when the guard is dropped: keep it for the life of `main`"]
pub struct ExitGuard(());

impl Drop for ExitGuard {
    fn drop(&mut self) {
        run();
    }
}

/// An [`ExitGuard`], with the signal handlers installed.
pub fn guard() -> ExitGuard {
    on_signals();
    ExitGuard(())
}

/// Install the handlers that run the hooks before the OS's signals end the
/// process, once: `SIGINT`, `SIGTERM` and `SIGHUP` on Unix; the console's
/// events and the session's end on Windows.
pub(crate) fn on_signals() {
    static INSTALLED: Once = Once::new();
    INSTALLED.call_once(platform::install);
}

#[cfg(unix)]
mod platform {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    use signal_hook::iterator::Signals;

    pub(super) fn install() {
        let mut signals = match Signals::new([SIGINT, SIGTERM, SIGHUP]) {
            Ok(signals) => signals,
            Err(e) => {
                crate::log::log(
                    crate::log::Level::Warn,
                    "elyra::exit",
                    &format!("no signal handlers: {e}"),
                );
                return;
            }
        };
        let _ = std::thread::Builder::new()
            .name("elyra-exit-signals".into())
            .spawn(move || {
                if let Some(signal) = signals.forever().next() {
                    super::run();
                    // The exit the signal would have made.
                    std::process::exit(128 + signal);
                }
            });
    }
}

#[cfg(windows)]
mod platform {
    use windows::core::{w, BOOL};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Console::{
        SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_CLOSE_EVENT, CTRL_C_EVENT, CTRL_LOGOFF_EVENT,
        CTRL_SHUTDOWN_EVENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, MSG, WINDOW_EX_STYLE, WM_ENDSESSION, WNDCLASSW, WS_OVERLAPPED,
    };

    /// `STATUS_CONTROL_C_EXIT`: how a process ended by Ctrl-C exits.
    const CONTROL_C_EXIT: i32 = 0xC000_013Au32 as i32;

    unsafe extern "system" fn console(event: u32) -> BOOL {
        super::run();
        let code = match event {
            e if e == CTRL_C_EVENT || e == CTRL_BREAK_EVENT => CONTROL_C_EXIT,
            e if e == CTRL_CLOSE_EVENT || e == CTRL_LOGOFF_EVENT || e == CTRL_SHUTDOWN_EVENT => 0,
            _ => return BOOL(0),
        };
        std::process::exit(code);
    }

    /// The session ending: log-off, shutdown, a restart for updates. Sent to
    /// top-level windows only (a message-only window gets no broadcast), so
    /// this is a hidden one that's never shown.
    unsafe extern "system" fn session(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_ENDSESSION && wparam.0 != 0 {
            super::run();
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    pub(super) fn install() {
        unsafe {
            if let Err(e) = SetConsoleCtrlHandler(Some(console), true) {
                crate::log::log(
                    crate::log::Level::Warn,
                    "elyra::exit",
                    &format!("no console handler: {e}"),
                );
            }
        }
        let _ = std::thread::Builder::new()
            .name("elyra-exit-session".into())
            .spawn(|| unsafe {
                let Ok(instance) = GetModuleHandleW(None) else {
                    return;
                };
                let class = WNDCLASSW {
                    lpfnWndProc: Some(session),
                    hInstance: instance.into(),
                    lpszClassName: w!("ElyraExitSession"),
                    ..Default::default()
                };
                if RegisterClassW(&class) == 0 {
                    return;
                }
                // Never shown; it's there to hear the session end.
                let Ok(_hwnd) = CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("ElyraExitSession"),
                    w!(""),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                ) else {
                    return;
                };
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            });
    }
}

#[cfg(not(any(unix, windows)))]
mod platform {
    pub(super) fn install() {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    // The hooks are the process's: one test drives them all, so tests running
    // in parallel don't take each other's.
    #[test]
    fn hooks_run_once_last_first_and_survive_a_panic() {
        let order = Arc::new(StdMutex::new(Vec::new()));
        for n in 1..=3 {
            let order = order.clone();
            on_exit(move || order.lock().unwrap().push(n));
        }
        on_exit(|| panic!("a hook that fails"));
        let late = order.clone();
        let during = order.clone();
        on_exit(move || {
            // Added while the hooks run: it runs too.
            on_exit(move || late.lock().unwrap().push(9));
            during.lock().unwrap().push(4);
        });
        run();
        assert_eq!(*order.lock().unwrap(), [4, 3, 2, 1, 9]);
        run();
        assert_eq!(order.lock().unwrap().len(), 5, "once only");

        // A guard runs them when dropped.
        let dropped = order.clone();
        {
            let _guard = ExitGuard(());
            on_exit(move || dropped.lock().unwrap().push(10));
        }
        assert_eq!(order.lock().unwrap().last(), Some(&10));
    }
}
