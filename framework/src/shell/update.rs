//! The auto-update routes and the `elyra:update` phase stream that drives the
//! runtime's toast (feature `updater`).
//!
//! Installing is a privileged operation — it downloads, verifies a signature,
//! replaces the binary and relaunches — so it sits behind its own opt-in
//! capability (`UpdaterInstall`) and its own rate limit. Everything here assumes
//! [`guard`](super::guard) already allowed the call.

use std::sync::Arc;

use super::protocol::{msgpack_ok, Body};
use super::{ipc_handle, Runner};

/// A phase update on the `elyra:update` channel, consumed by the runtime toast.
#[derive(serde::Serialize)]
struct UpdatePhase {
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl UpdatePhase {
    fn available(version: String, notes: Option<String>) -> Self {
        Self {
            phase: "available",
            version: Some(version),
            notes,
            progress: None,
            message: None,
        }
    }
    fn downloading(progress: u8) -> Self {
        Self {
            phase: "downloading",
            version: None,
            notes: None,
            progress: Some(progress),
            message: None,
        }
    }
    fn simple(phase: &'static str) -> Self {
        Self {
            phase,
            version: None,
            notes: None,
            progress: None,
            message: None,
        }
    }
    fn error(message: String) -> Self {
        Self {
            phase: "error",
            version: None,
            notes: None,
            progress: None,
            message: Some(message),
        }
    }
}

/// Spawn the silent startup update check (no-op if the updater isn't configured
/// or auto-check is disabled).
pub(super) fn spawn_startup_update_check(runner: &Arc<Runner>) {
    let Some(rt) = runner.ctx.try_get::<crate::updater::UpdaterRuntime>() else {
        return;
    };
    if !rt.auto_check {
        return;
    }
    let runner = Arc::clone(runner);
    let handle = ipc_handle(&runner);
    handle.spawn(async move {
        update_check_and_emit(&runner).await;
    });
}

/// Run a check and, if an update is available, emit an `available` phase.
async fn update_check_and_emit(runner: &Arc<Runner>) {
    use crate::updater::{UpdateStatus, UpdaterRuntime};
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return;
    };
    let rt2 = rt.clone();
    let result =
        tokio::task::spawn_blocking(move || rt2.updater.check(&rt2.manifest_url, &rt2.target))
            .await;
    if let Ok(Ok(UpdateStatus::Available(info))) = result {
        let _ = runner.bus.emit(
            "elyra:update",
            &UpdatePhase::available(info.version, info.notes),
        );
    }
}

/// `GET /__update/check` — report whether a newer release exists.
pub(super) async fn serve_update_check(runner: &Runner) -> Body {
    use crate::updater::{UpdateCheck, UpdaterRuntime};
    let err = |message: String| UpdateCheck {
        available: false,
        version: None,
        notes: None,
        error: Some(message),
    };
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return msgpack_ok(&err("updater not configured".into()));
    };
    let rt2 = rt.clone();
    let check = match tokio::task::spawn_blocking(move || {
        rt2.updater.check(&rt2.manifest_url, &rt2.target)
    })
    .await
    {
        Ok(Ok(status)) => UpdateCheck::from(status),
        Ok(Err(e)) => err(e.to_string()),
        Err(e) => err(e.to_string()),
    };
    msgpack_ok(&check)
}

/// `POST /__update/install` — download + verify + apply in the background,
/// streaming progress over `elyra:update`. Returns immediately.
pub(super) fn serve_update_install(runner: &Arc<Runner>) -> Body {
    let runner = Arc::clone(runner);
    tokio::spawn(async move { run_update_install(runner).await });
    msgpack_ok(&true)
}

async fn run_update_install(runner: Arc<Runner>) {
    use crate::updater::{UpdateStatus, Updater, UpdaterRuntime};
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return;
    };
    let bus = runner.bus.clone();

    // Re-check to obtain the signed artifact URL + signature.
    let rt_check = rt.clone();
    let status = match tokio::task::spawn_blocking(move || {
        rt_check
            .updater
            .check(&rt_check.manifest_url, &rt_check.target)
    })
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return emit_err(&bus, e.to_string()),
        Err(e) => return emit_err(&bus, e.to_string()),
    };
    let info = match status {
        UpdateStatus::Available(info) => info,
        UpdateStatus::UpToDate => {
            let _ = bus.emit("elyra:update", &UpdatePhase::simple("up-to-date"));
            return;
        }
    };

    let bus_dl = bus.clone();
    let rt_dl = rt.clone();
    let staged = tokio::task::spawn_blocking(move || {
        rt_dl
            .updater
            .download_verified_with_progress(&info, |got, total| {
                let pct = match total {
                    Some(t) if t > 0 => ((got.saturating_mul(100)) / t) as u8,
                    _ => 0,
                };
                let _ = bus_dl.emit("elyra:update", &UpdatePhase::downloading(pct));
            })
    })
    .await;

    let staged = match staged {
        Ok(Ok(path)) => path,
        Ok(Err(e)) => return emit_err(&bus, e.to_string()),
        Err(e) => return emit_err(&bus, e.to_string()),
    };

    let _ = bus.emit("elyra:update", &UpdatePhase::simple("ready"));
    // Let the frontend paint "Restarting…" before we re-exec.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if let Err(e) = Updater::apply_and_relaunch(&staged) {
        emit_err(&bus, e.to_string());
    }
}

fn emit_err(bus: &crate::event::EventBus, message: String) {
    let _ = bus.emit("elyra:update", &UpdatePhase::error(message));
}
