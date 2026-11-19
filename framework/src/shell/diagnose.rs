//! The diagnostics' routes and menu item (RFC 0033), and the page's errors.

use std::sync::Arc;

use wry::http::StatusCode;

use super::protocol::{error_response, msgpack_ok, Body};
use super::{ipc_handle, Runner, WindowRef};
use crate::diagnostics::{PageError, PageErrors, Seen, Setup};

/// What the diagnostics say of the windows: asked of the event loop, which
/// isn't there under the test harness.
async fn seen(runner: &Runner) -> Option<Seen> {
    runner.ctx.try_get::<crate::window::Windows>()?.seen().await
}

fn off() -> Body {
    error_response(
        StatusCode::NOT_FOUND,
        "off",
        "diagnostics are off: add App::diagnostics()",
    )
}

/// `/__diagnostics/text` and `/__diagnostics/save`.
pub(super) async fn serve(runner: &Runner, op: &str) -> Body {
    let Some(setup) = runner.ctx.try_get::<Setup>() else {
        return off();
    };
    match op {
        "text" => {
            let seen = seen(runner).await;
            msgpack_ok(&crate::diagnostics::text_with(&runner.ctx, &setup, seen).await)
        }
        #[cfg(feature = "system")]
        "save" => {
            let seen = seen(runner).await;
            match crate::diagnostics::save(&runner.ctx, &setup, seen).await {
                Ok(path) => msgpack_ok(&path.map(|p| p.display().to_string())),
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "io", e),
            }
        }
        #[cfg(not(feature = "system"))]
        "save" => error_response(
            StatusCode::NOT_FOUND,
            "off",
            "saving diagnostics needs elyra's `system` feature",
        ),
        other => error_response(
            StatusCode::NOT_FOUND,
            "unknown",
            format!("unknown diagnostics op: {other}"),
        ),
    }
}

/// Help → Save Diagnostics…, or the tray's item: the save dialog, off the
/// event loop.
pub(super) fn save_from_menu(runner: &Arc<Runner>, seen: Seen) {
    let Some(setup) = runner.ctx.try_get::<Setup>() else {
        return;
    };
    #[cfg(feature = "system")]
    {
        let ctx = runner.ctx.clone();
        ipc_handle(runner).spawn(async move {
            if let Err(e) = crate::diagnostics::save(&ctx, &setup, Some(seen)).await {
                crate::error!(target: "elyra::diagnostics", "couldn't save the diagnostics: {e}");
            }
        });
    }
    #[cfg(not(feature = "system"))]
    {
        let _ = (setup, seen, ipc_handle(runner));
    }
}

/// `/__page-error`: an uncaught error or unhandled rejection in the page,
/// kept for the diagnostics and logged with the target `page`, in every app.
pub(super) fn serve_page_error(runner: &Runner, window: Option<&WindowRef>, body: &[u8]) -> Body {
    let Ok(error) = rmp_serde::from_slice::<PageError>(body) else {
        return error_response(StatusCode::BAD_REQUEST, "decode", "not a page error");
    };
    if let Some(errors) = runner.ctx.try_get::<PageErrors>() {
        let label = window.map_or("main", |w| w.label.as_str());
        errors.record(label, error);
    }
    msgpack_ok(&true)
}
