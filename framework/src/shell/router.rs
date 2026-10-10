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

use crate::Error;

use super::assets::serve_asset;
use super::guard;
use super::guard::with_cors;
use super::protocol::{is_validation_bag, msgpack_err, msgpack_ok, panic_detail, Body};
use super::{facades, Runner, ABOUT_PATH, CMD_PREFIX, EVENTS_PATH, LIVE_PREFIX, LIVE_STOP_PATH};

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

    // Translation strings and the locale choice: token-gated like every native
    // route, but no capability — they carry nothing a page can abuse.
    if path == "/__i18n" || path.starts_with("/__i18n/") {
        let op = path
            .trim_start_matches("/__i18n")
            .trim_start_matches('/')
            .to_owned();
        return with_cors(
            &runner.policy,
            facades::serve_i18n(runner, &op, request.into_body()),
        );
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

    #[cfg(feature = "backend")]
    if let Some(op) = path.strip_prefix("/__auth/") {
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            serve_auth(runner, &op, request.into_body()).await,
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

    if path.starts_with(LIVE_PREFIX) || path == LIVE_STOP_PATH {
        let client = request
            .headers()
            .get("x-elyra-client-id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.is_empty())
            .unwrap_or(crate::event::DEFAULT_CLIENT)
            .to_owned();
        let body = request.into_body();
        return with_cors(
            &runner.policy,
            serve_live(runner, &path, &client, body).await,
        );
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
            // Tell the frontend *what kind* of failure this is, so a validation
            // bag can be turned into field errors without sniffing the string.
            command_error(&err)
        }
    }
}

/// `/__live/<command>` subscribes; `/__live-stop` (body: the id) ends one.
#[cfg(feature = "database")]
async fn serve_live(runner: &Runner, path: &str, client: &str, body: Vec<u8>) -> Body {
    let Some(live) = runner.ctx.try_get::<crate::live::LiveRegistry>() else {
        return command_error(&Error::Command("live queries aren't set up".into()));
    };
    if path == LIVE_STOP_PATH {
        let stopped = rmp_serde::from_slice::<String>(&body)
            .map(|id| live.unsubscribe(client, &id))
            .unwrap_or(false);
        return msgpack_ok(&stopped);
    }
    let name = path.strip_prefix(LIVE_PREFIX).unwrap_or_default();
    match live.subscribe(client, name, &body).await {
        Ok(subscribed) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/msgpack")
            .header("x-elyra-status", "ok")
            .body(Cow::Owned(crate::live::subscribed_body(&subscribed)))
            .unwrap(),
        Err(err) => {
            crate::warn!(target: "elyra::live", "subscribing to {name} failed: {err}");
            command_error(&err)
        }
    }
}

#[cfg(not(feature = "database"))]
async fn serve_live(_runner: &Runner, _path: &str, _client: &str, _body: Vec<u8>) -> Body {
    command_error(&Error::Command(
        "live queries need elyra's `database` feature".into(),
    ))
}

/// `POST /__auth/<op>` — sign in (`{ email, password }`), sign out, or the
/// state: an `AuthState` either way. A failure answers like a command's, so a
/// `422` reaches the frontend as a `ValidationError`. The token never does.
#[cfg(feature = "backend")]
async fn serve_auth(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    #[derive(serde::Deserialize)]
    struct SignIn {
        email: String,
        password: String,
    }
    let Some(auth) = runner.ctx.try_get::<crate::auth::Auth>() else {
        return command_error(&Error::Command(
            "this app has no backend: App::backend(Backend::new(url))".into(),
        ));
    };
    let outcome = match op {
        "sign-in" => match rmp_serde::from_slice::<SignIn>(&body) {
            Ok(a) => auth
                .sign_in(&a.email, &a.password)
                .await
                .map(|_| auth.state())
                .map_err(Error::from),
            Err(e) => Err(Error::decode(e)),
        },
        "sign-out" => auth
            .sign_out()
            .await
            .map(|()| {
                let mut state = auth.state();
                state.reason = Some("signed-out".into());
                state
            })
            .map_err(Error::from),
        "state" => {
            // Signed in from an earlier run: read the user once.
            if auth.is_signed_in() && auth.state().user.is_none() {
                let _ = auth.user::<serde_json::Value>().await;
            }
            Ok(auth.state())
        }
        other => Err(Error::Command(format!("unknown auth op: {other}"))),
    };
    match outcome {
        Ok(state) => msgpack_ok(&state),
        Err(e) => command_error(&e),
    }
}

/// A command's error as a response: the message, and whether it's a
/// validation bag, so the frontend can show it per field.
fn command_error(err: &Error) -> Body {
    let message = err.to_string();
    let kind = match err.kind() {
        Some(kind) => kind,
        None if is_validation_bag(&message) => "validation",
        None => "command",
    };
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-elyra-status", "error")
        .header("x-elyra-error-kind", kind)
        .body(Cow::Owned(message.into_bytes()))
        .unwrap()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_of(err: &Error) -> String {
        command_error(err)
            .headers()
            .get("x-elyra-error-kind")
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn an_errors_kind_reaches_the_frontend() {
        assert_eq!(
            kind_of(&Error::with_kind("offline", "no connection")),
            "offline"
        );
        assert_eq!(
            kind_of(&Error::Command(r#"{"email":["Taken."]}"#.into())),
            "validation"
        );
        assert_eq!(kind_of(&Error::Command("nope".into())), "command");
        let body = command_error(&Error::with_kind("offline", "no connection"));
        assert_eq!(
            &body.body()[..],
            b"no connection",
            "the message stays verbatim"
        );
    }
}
