//! The dispatch table: one request in, one response out.
//!
//! [`route`] is the whole IPC surface in reading order — the gate, then the
//! native routes in the order they were added, then the asset fallback. The
//! handlers themselves live next door ([`facades`](super::facades),
//! [`update`](super::update), [`assets`](super::assets)); what stays here is the
//! path → handler mapping, plus the two routes that need the shared state
//! directly: the event long-poll and command dispatch.

use std::borrow::Cow;
use std::sync::Arc;

use wry::http::{header, Request, Response, StatusCode};

use super::assets::serve_asset;
use super::guard;
use super::guard::with_cors;
use super::protocol::{is_validation_bag, msgpack_err, msgpack_ok, panic_detail, Body};
use super::{facades, Runner, ABOUT_PATH, CMD_PREFIX, EVENTS_PATH};

pub(super) async fn route(runner: &Arc<Runner>, request: Request<Vec<u8>>) -> Body {
    // CORS preflight (only reachable from the cross-origin dev server).
    if request.method() == wry::http::Method::OPTIONS {
        return guard::preflight(&runner.policy);
    }

    let path = request.uri().path().to_owned();

    // Token, capability, rate limit and body limits — see `guard`.
    if guard::is_native_route(&path) {
        if let Some(denied) = guard::check(&runner.policy, &runner.registry, &request, &path) {
            return with_cors(&runner.policy, denied);
        }
    }

    if path == EVENTS_PATH {
        // One queue per webview: without a client id every window would race for
        // the same batch (see `crate::event`).
        let client = request
            .headers()
            .get("x-elyra-client-id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.is_empty())
            .map(str::to_owned);
        return with_cors(&runner.policy, serve_events(runner, client).await);
    }

    if path == ABOUT_PATH {
        return with_cors(&runner.policy, serve_about(runner));
    }

    if let Some(op) = path.strip_prefix("/__window/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_window(runner, &op, request.into_body()),
        );
    }

    if let Some(op) = path.strip_prefix("/__store/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_store(runner, &op, request.into_body()),
        );
    }

    if let Some(op) = path.strip_prefix("/__cache/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_cache(runner, &op, request.into_body()),
        );
    }

    if let Some(op) = path.strip_prefix("/__storage/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_storage(runner, &op, request.into_body()),
        );
    }

    if let Some(op) = path.strip_prefix("/__queue/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_queue(runner, &op, request.into_body()),
        );
    }

    if let Some(op) = path.strip_prefix("/__deeplink/") {
        if op == "initial" {
            let url = runner
                .deep_link
                .as_deref()
                .and_then(crate::deeplink::url_in_args);
            return with_cors(&runner.policy, msgpack_ok(&url));
        }
        return with_cors(
            &runner.policy,
            msgpack_err(format!("unknown deeplink op: {op}")),
        );
    }

    #[cfg(feature = "autostart")]
    if let Some(op) = path.strip_prefix("/__autostart/") {
        let op = op.to_owned();
        return with_cors(&runner.policy, facades::serve_autostart(runner, &op));
    }

    #[cfg(feature = "sidecar")]
    if let Some(op) = path.strip_prefix("/__sidecar/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_sidecar(runner, &op, request.into_body()),
        );
    }

    #[cfg(feature = "updater")]
    if path == "/__update/check" {
        return with_cors(
            &runner.policy,
            super::update::serve_update_check(runner).await,
        );
    }
    #[cfg(feature = "updater")]
    if path == "/__update/install" {
        return with_cors(&runner.policy, super::update::serve_update_install(runner));
    }

    #[cfg(feature = "system")]
    if let Some(op) = path.strip_prefix("/__sys/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_system(&runner.policy, &op, request.into_body()).await,
        );
    }

    if path == "/__cancel" {
        if let Ok(id) = rmp_serde::from_slice::<String>(&request.into_body()) {
            if let Some(handle) = runner.cancellations.lock().remove(&id) {
                handle.abort();
            }
        }
        return with_cors(&runner.policy, msgpack_ok(&true));
    }

    if let Some(name) = path.strip_prefix(CMD_PREFIX) {
        let name = name.to_owned();
        let request_id = request
            .headers()
            .get("x-elyra-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        return with_cors(
            &runner.policy,
            serve_command(runner, &name, request_id, request.into_body()).await,
        );
    }

    serve_asset(runner, &path, &request)
}

/// Long-poll: block until the next event batch is ready, then respond. The
/// frontend reconnects immediately, giving a continuous binary stream.
///
/// `client` is the calling webview's id — each one gets its own queue, so an
/// emit reaches every window instead of whichever polled first.
async fn serve_events(runner: &Runner, client: Option<String>) -> Body {
    let batch = match &client {
        Some(id) => runner.bus.next_batch_for(id).await,
        None => runner.bus.next_batch().await,
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(batch))
        .unwrap()
}

async fn serve_command(
    runner: &Runner,
    name: &str,
    request_id: Option<String>,
    body: Vec<u8>,
) -> Body {
    // Always run the command on its own task, even when it isn't cancellable:
    // dispatching inline meant a panicking command (or a missing container
    // binding, which panics by design) dropped the responder without a reply, so
    // the frontend's `await invoke(..)` never settled — no error, no timeout.
    let registry = runner.registry.clone();
    let ctx = runner.ctx.clone();
    let owned_name = name.to_owned();
    let started = std::time::Instant::now();
    let body_len = body.len();
    let task = tokio::spawn(async move { registry.dispatch(ctx, &owned_name, &body).await });

    if let Some(id) = &request_id {
        runner
            .cancellations
            .lock()
            .insert(id.clone(), task.abort_handle());
    }
    let joined = task.await;
    if let Some(id) = &request_id {
        runner.cancellations.lock().remove(id);
    }

    let result = match joined {
        Ok(result) => result,
        Err(e) if e.is_cancelled() => {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .header("x-elyra-status", "error")
                .header("x-elyra-error-kind", "cancelled")
                .body(Cow::Borrowed(b"command cancelled".as_slice()))
                .unwrap();
        }
        Err(e) => {
            // A panic inside the command. Report it as a normal error response so
            // the caller gets a rejection instead of hanging forever.
            let detail = panic_detail(e);
            crate::error!(target: "elyra::command", "`{name}` panicked: {detail}");
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .header("x-elyra-status", "error")
                .header("x-elyra-error-kind", "panic")
                .body(Cow::Owned(
                    format!("command `{name}` panicked: {detail}").into_bytes(),
                ))
                .unwrap();
        }
    };
    match result {
        Ok(bytes) => {
            crate::debug!(
                target: "elyra::command",
                "{name} ok in {:?} ({body_len} B in, {} B out)",
                started.elapsed(),
                bytes.len()
            );
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/msgpack")
                .header("x-elyra-status", "ok")
                .body(Cow::Owned(bytes))
                .unwrap()
        }
        Err(err) => {
            crate::warn!(
                target: "elyra::command",
                "{name} failed in {:?}: {err}",
                started.elapsed()
            );
            let message = err.to_string();
            // Tell the frontend *what kind* of failure this is, so a validation
            // bag can be turned into field errors without sniffing the string.
            let kind = if is_validation_bag(&message) {
                "validation"
            } else {
                "command"
            };
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .header("x-elyra-status", "error")
                .header("x-elyra-error-kind", kind)
                .body(Cow::Owned(message.into_bytes()))
                .unwrap()
        }
    }
}

/// Serve the app's About metadata as MessagePack (named map -> object).
fn serve_about(runner: &Runner) -> Body {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(runner.about.to_msgpack()))
        .unwrap()
}
