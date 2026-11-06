//! The auto-update routes that drive the runtime's toast (feature `updater`).
//! The flow itself — the check, the schedule, installing, relaunching, and
//! the `elyra:update` phases — is [`UpdaterRuntime`]'s.
//!
//! Installing and restarting are privileged — they download, verify, replace
//! the app and relaunch it — so they sit behind their own opt-in capability
//! (`UpdaterInstall`). Everything here assumes [`guard`](super::guard) already
//! allowed the call.

use std::sync::Arc;

use super::protocol::{msgpack_err, msgpack_ok, Body};
use super::{ipc_handle, Runner};
use crate::updater::UpdaterRuntime;

/// Start the update schedule (a no-op unless the updater is configured with
/// `auto_check`), and let a relaunch quit the app as the tray's Quit does.
pub(super) fn spawn_startup_update_check(runner: &Arc<Runner>) {
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return;
    };
    if let Some(windows) = runner.ctx.try_get::<crate::window::Windows>() {
        rt.set_quit(move || {
            if !windows.quit() {
                std::process::exit(0);
            }
        });
    }
    rt.start(&ipc_handle(runner));
}

/// `GET /__update/check` — report whether a newer release exists.
pub(super) async fn serve_update_check(runner: &Runner) -> Body {
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return msgpack_ok(&crate::updater::UpdateCheck {
            available: false,
            version: None,
            notes: None,
            error: Some("updater not configured".into()),
            ready: false,
            restart: None,
        });
    };
    msgpack_ok(&rt.check_for_frontend().await)
}

/// `POST /__update/install` — download + verify + put in place in the
/// background, streaming progress over `elyra:update`. Returns immediately.
pub(super) fn serve_update_install(runner: &Arc<Runner>) -> Body {
    if let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() {
        tokio::spawn(async move {
            let _ = rt.install().await;
        });
    }
    msgpack_ok(&true)
}

/// `POST /__update/restart` — start the installed version and quit.
pub(super) fn serve_update_restart(runner: &Arc<Runner>) -> Body {
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return msgpack_err("updater not configured".into());
    };
    match rt.relaunch() {
        Ok(()) => msgpack_ok(&true),
        Err(e) => msgpack_err(e.to_string()),
    }
}
