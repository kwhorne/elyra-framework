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
    // The inspector's call log (RFC 0014): a command, a live query opened, an
    // assistant turn asked, each with its outcome, time and sizes.
    #[cfg(debug_assertions)]
    if let Some(command) = logged(request.uri().path()) {
        let window = request
            .headers()
            .get("x-elyra-client-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let sent = request.body().len();
        let started = std::time::Instant::now();
        let response = route_inner(runner, request).await;
        if let Some(calls) = runner.ctx.try_get::<crate::inspector::Calls>() {
            calls.record(&window, call_row(runner, command, &response, started, sent));
        }
        return response;
    }
    route_inner(runner, request).await
}

/// What the call log names a path: `customers_update`, `live:customers_show`,
/// `assistant`. Other routes aren't logged (the inspector's own neither).
#[cfg(debug_assertions)]
fn logged(path: &str) -> Option<String> {
    if let Some(name) = path.strip_prefix(CMD_PREFIX) {
        return Some(name.to_owned());
    }
    if let Some(name) = path.strip_prefix(LIVE_PREFIX) {
        return Some(format!("live:{name}"));
    }
    (path == "/__assistant/prompt").then(|| "assistant".to_owned())
}

#[cfg(debug_assertions)]
fn call_row(
    runner: &Runner,
    command: String,
    response: &Body,
    started: std::time::Instant,
    sent: usize,
) -> crate::inspector::CallRow {
    let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok());
    // Every refusal and failure the shell answers says so in this header.
    let failed = header("x-elyra-status") == Some("error");
    let outcome = match (failed, header("x-elyra-error-kind")) {
        (false, _) => "ok".to_owned(),
        (true, Some(kind)) => kind.to_owned(),
        (true, None) => "error".to_owned(),
    };
    let name = command.strip_prefix("live:").unwrap_or(&command);
    let ability = matches!(outcome.as_str(), "denied" | "forbidden")
        .then(|| runner.registry.ability_of(name))
        .flatten()
        .map(str::to_owned);
    crate::inspector::CallRow {
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        command,
        outcome,
        ms: started.elapsed().as_millis() as u64,
        sent,
        received: response.body().len(),
        ability,
    }
}

async fn route_inner(runner: &Arc<Runner>, request: Request<Vec<u8>>) -> Body {
    // CORS preflight (only reachable from the cross-origin dev server).
    if request.method() == wry::http::Method::OPTIONS {
        return guard::preflight(&runner.policy);
    }

    let path = request.uri().path().to_owned();

    // Token, capability, rate limit and body limits — see `guard`.
    if guard::is_native_route(&path) {
        // What the signed-in user may do, when the backend says (RFC 0010).
        #[cfg(feature = "backend")]
        let abilities = runner.ctx.try_get::<crate::abilities::Abilities>();
        #[cfg(feature = "backend")]
        let user = abilities
            .as_deref()
            .map(|a| a as &dyn crate::security::UserAbilities);
        #[cfg(not(feature = "backend"))]
        let user: Option<&dyn crate::security::UserAbilities> = None;
        if let Some(denied) = guard::check(&runner.policy, &runner.registry, user, &request, &path)
        {
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

    // The developer's inspector (RFC 0014): not compiled into a release, so
    // there these paths are unknown like any other.
    #[cfg(debug_assertions)]
    if let Some(op) = path.strip_prefix("/__inspector/") {
        let op = op.to_owned();
        let window = request
            .headers()
            .get("x-elyra-client-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        return with_cors(
            &runner.policy,
            super::inspector::serve(runner, &op, &window, request.body()),
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

    // Without a backend there's no one to ask what the user may do: the app's
    // own grants are all there is (RFC 0010), so `can` hides nothing.
    #[cfg(not(feature = "backend"))]
    if matches!(
        path.as_str(),
        "/__auth/abilities" | "/__auth/abilities-refresh"
    ) {
        #[derive(serde::Serialize)]
        struct State {
            loaded: bool,
            abilities: [&'static str; 1],
            #[serde(skip_serializing_if = "Vec::is_empty")]
            masked: Vec<String>,
        }
        return with_cors(
            &runner.policy,
            msgpack_ok(&State {
                loaded: true,
                abilities: ["*"],
                masked: runner.policy.mask().list(),
            }),
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

    // The app's assistant (RFC 0011): one conversation per window.
    #[cfg(feature = "ai")]
    if let Some(op) = path.strip_prefix("/__assistant/") {
        let client = request
            .headers()
            .get("x-elyra-client-id")
            .and_then(|v| v.to_str().ok())
            .filter(|id| !id.is_empty())
            .unwrap_or(crate::event::DEFAULT_CLIENT)
            .to_owned();
        let op = op.to_owned();
        return with_cors(
            &runner.policy,
            serve_assistant(runner, &op, &client, request.into_body()),
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
            facades::serve_system(
                &runner.policy,
                runner.ctx.try_get::<crate::notifier::Notifier>(),
                &op,
                request.into_body(),
            )
            .await,
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
/// `/__assistant/<op>` for window `client`: `prompt` (the user's text, or
/// `{ text, route }` with the window's route for its note, RFC 0013; the
/// answer arrives on `elyra:assistant`), `selected` (`{ kind, ids, route }`:
/// what the window has selected, for the next turn's note), `confirm`
/// (`{ id, accept }`), `stop`,
/// `reset`, `tools` (the names the user is offered), `source` (where the
/// model is reached) and `key` (keep the user's own key, or forget it with
/// `null`).
#[cfg(feature = "ai")]
fn serve_assistant(runner: &Runner, op: &str, client: &str, body: Vec<u8>) -> Body {
    #[derive(serde::Deserialize)]
    struct Confirm {
        id: String,
        accept: bool,
    }
    #[derive(serde::Deserialize)]
    struct Selected {
        kind: String,
        ids: Vec<serde_json::Value>,
        route: Option<String>,
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Prompt {
        Text(String),
        Turn { text: String, route: Option<String> },
    }
    let Some(assistants) = runner.ctx.try_get::<crate::assistant::Assistants>() else {
        return command_error(&Error::Command(
            "this app has no assistant: App::assistant(Assistant::new()..)".into(),
        ));
    };
    match op {
        "prompt" => match rmp_serde::from_slice::<Prompt>(&body) {
            Ok(prompt) => match match prompt {
                Prompt::Text(text) => assistants.start(runner.bus.clone(), client, text, None),
                Prompt::Turn { text, route } => {
                    assistants.start(runner.bus.clone(), client, text, route)
                }
            } {
                Ok(()) => msgpack_ok(&true),
                Err(busy) => command_error(&Error::Command(busy)),
            },
            Err(e) => command_error(&Error::decode(e)),
        },
        "selected" => match rmp_serde::from_slice::<Selected>(&body) {
            Ok(s) => match assistants.select(client, s.route, &s.kind, s.ids) {
                Ok(()) => msgpack_ok(&true),
                Err(e) => command_error(&Error::Command(e)),
            },
            Err(e) => command_error(&Error::decode(e)),
        },
        "confirm" => match rmp_serde::from_slice::<Confirm>(&body) {
            Ok(c) => msgpack_ok(&assistants.answer(client, &c.id, c.accept)),
            Err(e) => command_error(&Error::decode(e)),
        },
        "stop" => {
            let stopped = assistants.stop(client);
            if stopped {
                let _ = runner.bus.emit_to(
                    client,
                    crate::assistant::CHANNEL,
                    &crate::assistant::AssistantEvent::Stopped,
                );
            }
            msgpack_ok(&stopped)
        }
        "reset" => {
            assistants.reset(client);
            msgpack_ok(&true)
        }
        "tools" => msgpack_ok(&assistants.tools()),
        "source" => msgpack_ok(&assistants.source()),
        // Write-only: the key is never sent back.
        "key" => match rmp_serde::from_slice::<Option<String>>(&body) {
            Ok(key) => match assistants.set_key(key.as_deref()) {
                Ok(kept) => msgpack_ok(&kept),
                Err(e) => command_error(&Error::Command(e)),
            },
            Err(e) => command_error(&Error::decode(e)),
        },
        _ => msgpack_err(format!("unknown assistant op: {op}")),
    }
}

#[cfg(feature = "backend")]
async fn serve_auth(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    #[derive(serde::Deserialize)]
    struct SignIn {
        email: String,
        password: String,
    }
    // What the signed-in user may do (RFC 0010): answered by an app without a
    // backend too, whose own grants are all there is.
    if matches!(op, "abilities" | "abilities-refresh") {
        return serve_abilities(runner, op).await;
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
        // The broadcasting connection's state (RFC 0006), for the runtime's
        // `broadcasting` store; `offline` when the app doesn't listen.
        "broadcasting" => {
            let state = runner
                .ctx
                .try_get::<crate::broadcasting::Broadcaster>()
                .map_or(crate::broadcasting::State::Offline, |b| b.state());
            return msgpack_ok(&serde_json::json!({ "state": state }));
        }
        // Sign out though writes wait: they're dropped (RFC 0007).
        "sign-out-discarding" => auth
            .sign_out_discarding()
            .await
            .map(|()| {
                let mut state = auth.state();
                state.reason = Some("signed-out".into());
                state
            })
            .map_err(Error::from),
        // Laravel's notifications (RFC 0008): the list, and reading it.
        "notifications"
        | "notifications-refresh"
        | "notifications-more"
        | "notifications-read"
        | "notifications-read-all" => return serve_notifications(runner, op, &body).await,
        // Files (RFC 0009): uploads and downloads while they run.
        "transfers" | "transfer-cancel" => return serve_transfers(runner, op, &body),
        // Working offline (RFC 0007): the connection, and the outbox.
        #[cfg(all(feature = "backend", feature = "database"))]
        "connection" | "outbox" | "outbox-retry" | "outbox-discard" | "outbox-keep-mine"
        | "outbox-take-theirs" => return serve_offline(runner, op, &body).await,
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

/// The user's abilities, for the runtime's `can` and `abilities` stores:
/// read once, then followed on `elyra:abilities`. The page may read them and
/// ask for them again, nothing else. An app without `Backend::abilities()`
/// answers `*`: there, the app's own grants are all there is, and the guard
/// enforces them.
#[cfg(feature = "backend")]
async fn serve_abilities(runner: &Runner, op: &str) -> Body {
    use crate::abilities::{Abilities, AbilitiesState};
    let Some(abilities) = runner.ctx.try_get::<Abilities>() else {
        return msgpack_ok(&AbilitiesState {
            loaded: true,
            abilities: vec!["*".into()],
            masked: runner.policy.mask().list(),
        });
    };
    if op == "abilities-refresh" {
        if let Err(e) = abilities.refresh().await {
            return command_error(&Error::from(e));
        }
    }
    msgpack_ok(&abilities.state())
}

/// The transfers, for the runtime's `transfers` store: read once, then
/// followed on `elyra:transfers`. The page may list them and cancel one,
/// nothing else.
#[cfg(feature = "backend")]
fn serve_transfers(runner: &Runner, op: &str, body: &[u8]) -> Body {
    use crate::transfers::Transfers;
    #[derive(serde::Deserialize)]
    struct One {
        id: String,
    }
    let Some(transfers) = runner.ctx.try_get::<Transfers>() else {
        return msgpack_ok(&Vec::<crate::transfers::Transfer>::new());
    };
    if op == "transfer-cancel" {
        match rmp_serde::from_slice::<One>(body) {
            Ok(one) => {
                transfers.cancel(&one.id);
            }
            Err(e) => return command_error(&Error::decode(e)),
        }
    }
    msgpack_ok(&transfers.list())
}

/// The notifications, for the runtime's `notifications` store: read once
/// (from the server, the first time), then followed on `elyra:notifications`.
/// The page may read the list and mark it read, nothing else. Without
/// `.notifications()`, there are none.
#[cfg(feature = "backend")]
async fn serve_notifications(runner: &Runner, op: &str, body: &[u8]) -> Body {
    use crate::notifications::{Notifications, NotificationsState};
    #[derive(serde::Deserialize)]
    struct One {
        id: String,
    }
    let Some(notifications) = runner.ctx.try_get::<Notifications>() else {
        return msgpack_ok(&NotificationsState::default());
    };
    let done = match op {
        "notifications" if notifications.loaded() => Ok(()),
        "notifications" | "notifications-refresh" => notifications.refresh().await.map(|_| ()),
        "notifications-more" => notifications.more().await.map(|_| ()),
        "notifications-read-all" => notifications.mark_all_read().await,
        _ => match rmp_serde::from_slice::<One>(body) {
            Ok(one) => notifications.mark_read(&one.id).await,
            Err(e) => return command_error(&Error::decode(e)),
        },
    };
    // Not signed in yet, or offline with nothing kept: the list is empty, not
    // an error, when the page only asked to see it.
    match done {
        Err(e) if op != "notifications" => command_error(&Error::from(e)),
        _ => msgpack_ok(&notifications.state()),
    }
}

/// The connection's state and the outbox, for the runtime's `connection` and
/// `outbox` stores. The page lists, retries and discards; it can't add a write
/// or change one's method or path — a retry may change the body only, as
/// saving the form again would.
#[cfg(all(feature = "backend", feature = "database"))]
async fn serve_offline(runner: &Runner, op: &str, body: &[u8]) -> Body {
    use crate::offline::{Connection, ConnectionState, Outbox};
    #[derive(serde::Deserialize)]
    struct Item {
        id: i64,
        #[serde(default)]
        body: Option<serde_json::Value>,
    }
    if op == "connection" {
        let state = runner
            .ctx
            .try_get::<crate::backend::Backend>()
            .and_then(|b| b.connection())
            .unwrap_or(ConnectionState {
                state: Connection::Online,
                since: None,
                pending: 0,
                failed: 0,
            });
        return msgpack_ok(&state);
    }
    let Some(outbox) = runner.ctx.try_get::<Outbox>() else {
        if op == "outbox" {
            return msgpack_ok(&Vec::<crate::offline::OutboxItem>::new());
        }
        return command_error(&Error::Command(
            "this app doesn't work offline: Backend::offline()".into(),
        ));
    };
    if op != "outbox" {
        let item = match rmp_serde::from_slice::<Item>(body) {
            Ok(item) => item,
            Err(e) => return command_error(&Error::decode(e)),
        };
        let done = match op {
            "outbox-retry" => outbox.retry(item.id, item.body).await,
            "outbox-discard" => outbox.discard(item.id).await,
            "outbox-keep-mine" => outbox.keep_mine(item.id).await,
            _ => outbox.take_theirs(item.id).await,
        };
        if let Err(e) = done {
            return command_error(&Error::from(e));
        }
    }
    msgpack_ok(&outbox.items().await)
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
