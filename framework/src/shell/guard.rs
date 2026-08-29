//! The gate in front of every native route — the file to read if you want to
//! know what a script running in the webview is actually allowed to do.
//!
//! Nothing else in [`shell`](super) decides access. A request for `/__*` reaches
//! a handler only after [`check`] has passed it, and every response leaves
//! through [`with_cors`], so the four mechanisms are enforced in one place:
//!
//! 1. **Token** — the per-run secret from [`crate::security::Policy`], injected
//!    into the webview before any page script runs. A page we never loaded
//!    doesn't have it.
//! 2. **Capability** — the route must map to a capability the app granted the
//!    frontend, and stay inside that capability's rate limit.
//! 3. **Ability** — a command declaring `#[command(can = "…")]` additionally
//!    needs that ability granted. `Capability::Commands` is one grant covering
//!    all of `/__cmd/*`, so without this an XSS reaches every command an app
//!    registers; a declared ability takes that command back out of the blanket.
//! 4. **Structural limits** — oversized or deeply nested bodies are refused
//!    before serde's recursive deserializer ever sees them.
//! 5. **Origin** — CORS headers are emitted only for the dev server's exact
//!    origin, and only under `rata dev`. A production build emits none.
//!
//! [`with_csp`] adds the sixth: the Content-Security-Policy on HTML responses.

use std::borrow::Cow;

use wry::http::{Request, Response, StatusCode};

use crate::command::CommandRegistry;
use crate::security::{Policy, RouteDenied};

use super::protocol::{error_response, Body};
use super::CMD_PREFIX;

/// Whether `path` is a native route (rather than an app asset).
pub(super) fn is_native_route(path: &str) -> bool {
    path.starts_with("/__")
}

/// Run every gate a `/__*` request must pass. `Some(response)` is the refusal to
/// send back; `None` means the request may proceed to its handler.
///
/// Ordered cheapest-first, and by blast radius: authenticate before deciding
/// what the caller may do, and decide that before touching the body at all.
pub(super) fn check(
    policy: &Policy,
    registry: &CommandRegistry,
    request: &Request<Vec<u8>>,
    path: &str,
) -> Option<Body> {
    // Everything under `/__*` is a native capability (commands, filesystem,
    // clipboard, sidecar, updater). Require this run's token, which only scripts
    // in a page *we* loaded can have — see `crate::security`.
    let token = request
        .headers()
        .get("x-elyra-token")
        .and_then(|v| v.to_str().ok());
    if !policy.token_matches(token) {
        return Some(forbidden("missing or invalid x-elyra-token"));
    }

    // The route must be a capability the app granted the frontend, and must
    // stay inside its rate limit (destructive/expensive ops are opt-in).
    if let Err(denied) = policy.allows_route(path) {
        let status = match denied {
            RouteDenied::RateLimited(_) => StatusCode::TOO_MANY_REQUESTS,
            RouteDenied::NotGranted(_) => StatusCode::FORBIDDEN,
        };
        return Some(error_response(status, "forbidden", denied));
    }

    // `Capability::Commands` is a single grant over all of `/__cmd/*`. A command
    // that declares an ability is denied by default until the app names it, so
    // one hostile script can't reach every command the app happens to register.
    if let Some(name) = path.strip_prefix(CMD_PREFIX) {
        if let Some(ability) = registry.ability_of(name) {
            if !policy.grants_ability(ability) {
                return Some(error_response(
                    StatusCode::FORBIDDEN,
                    "forbidden",
                    format!(
                        "command `{name}` requires the `{ability}` ability, which is not \
                         granted to the frontend (grant it with App::allow_ability)"
                    ),
                ));
            }
        }
    }

    // Structural limits before any decoding: oversized bodies are refused,
    // and deep nesting can't be handed to serde's recursive deserializer.
    if let Err(e) = crate::wire::check(request.body(), policy.max_body()) {
        let status = match e {
            crate::wire::WireError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            _ => StatusCode::BAD_REQUEST,
        };
        return Some(error_response(status, "bad-request", e));
    }

    None
}

/// The `204` answer to a CORS preflight (only reachable from the dev server).
pub(super) fn preflight(policy: &Policy) -> Body {
    with_cors(
        policy,
        Response::builder()
            .status(StatusCode::NO_CONTENT)
            .body(Cow::Borrowed(b"".as_slice()))
            .unwrap(),
    )
}

/// Add CORS headers **only** for the dev server's exact origin, and only while
/// `ELYRA_DEV_URL` is set (i.e. under `rata dev`).
///
/// A production build emits no `Access-Control-Allow-*` at all: the app is
/// same-origin under `elyra://localhost`, so it needs none, and a foreign origin
/// must not be able to read an IPC response. `*` here used to hand every origin
/// that could reach the protocol full access to commands, storage, the clipboard
/// and sidecar spawning.
pub(super) fn with_cors(policy: &Policy, mut response: Body) -> Body {
    let Some(origin) = policy.dev_origin() else {
        return response;
    };
    let headers = response.headers_mut();
    if let Ok(value) = origin.parse() {
        headers.insert("access-control-allow-origin", value);
    }
    headers.insert("vary", "origin".parse().unwrap());
    headers.insert(
        "access-control-allow-methods",
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        "access-control-allow-headers",
        "content-type, accept, x-elyra-request-id, x-elyra-token, x-elyra-client-id"
            .parse()
            .unwrap(),
    );
    response
}

/// `403` for a request that didn't present this run's IPC token.
pub(super) fn forbidden(reason: &str) -> Body {
    error_response(StatusCode::FORBIDDEN, "forbidden", reason)
}

/// Attach the Content-Security-Policy to HTML responses.
pub(super) fn with_csp(mut resp: Body, csp: &Option<String>, is_html: bool) -> Body {
    if is_html {
        if let Some(policy) = csp {
            if let Ok(value) = policy.parse() {
                resp.headers_mut().insert("content-security-policy", value);
            }
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Body {
        Response::builder()
            .status(StatusCode::OK)
            .body(Cow::Borrowed(b"x".as_slice()))
            .unwrap()
    }

    fn request(token: Option<&str>) -> Request<Vec<u8>> {
        let mut builder = Request::builder().uri("elyra://localhost/__cmd/greet");
        if let Some(token) = token {
            builder = builder.header("x-elyra-token", token);
        }
        builder.body(Vec::new()).unwrap()
    }

    /// A registered command, standing in for one `#[command]` would generate.
    struct Stub(&'static str, Option<&'static str>);

    impl crate::command::Command for Stub {
        fn name(&self) -> &'static str {
            self.0
        }

        fn ability(&self) -> Option<&'static str> {
            self.1
        }

        fn call<'a>(
            &'a self,
            _ctx: crate::Ctx,
            _args: &'a [u8],
        ) -> crate::command::BoxFuture<'a, crate::Result<Vec<u8>>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn signature(&self, _types: &mut specta::Types) -> crate::command::CommandSig {
            unimplemented!("the guard never asks a command for its signature")
        }
    }

    /// An empty registry — for the paths where no command is involved.
    fn no_commands() -> CommandRegistry {
        CommandRegistry::new()
    }

    fn registry_with(commands: &[Stub]) -> CommandRegistry {
        let mut registry = CommandRegistry::new();
        for Stub(name, ability) in commands {
            registry.register(Box::new(Stub(name, *ability)));
        }
        registry
    }

    #[test]
    fn production_responses_carry_no_cors_headers() {
        // Regression: `access-control-allow-origin: *` used to be added to every
        // response, handing any origin that could reach the protocol full access
        // to commands, storage, the clipboard and sidecar spawning.
        let policy = Policy::test_policy();
        let resp = with_cors(&policy, body());
        assert!(resp.headers().get("access-control-allow-origin").is_none());
        assert!(resp.headers().get("access-control-allow-headers").is_none());
    }

    #[test]
    fn dev_responses_echo_the_exact_dev_origin() {
        let policy = Policy::test_policy_with_dev_origin("http://localhost:5173");
        let resp = with_cors(&policy, body());
        assert_eq!(
            resp.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap()),
            Some("http://localhost:5173")
        );
        assert_eq!(
            resp.headers().get("vary").map(|v| v.to_str().unwrap()),
            Some("origin")
        );
        // The token header must be allowed through, or dev-mode IPC breaks.
        let allowed = resp
            .headers()
            .get("access-control-allow-headers")
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_string();
        assert!(allowed.contains("x-elyra-token"));
        assert!(allowed.contains("x-elyra-client-id"));
    }

    #[test]
    fn the_init_script_hands_the_token_to_the_frontend() {
        let policy = Policy::test_policy();
        let script = policy.init_script();
        assert!(script.contains(policy.token()));
        assert!(script.contains("__ELYRA__"));
    }

    #[test]
    fn only_native_routes_are_gated() {
        assert!(is_native_route("/__cmd/greet"));
        assert!(is_native_route("/__events"));
        assert!(!is_native_route("/index.html"));
        assert!(!is_native_route("/assets/app.js"));
    }

    #[test]
    fn a_request_without_the_token_is_refused() {
        let policy = Policy::test_policy();
        let denied = check(&policy, &no_commands(), &request(None), "/__cmd/greet")
            .expect("a tokenless request must be refused");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            denied.headers().get("x-elyra-error-kind").unwrap(),
            "forbidden"
        );
    }

    #[test]
    fn a_wrong_token_is_refused() {
        let policy = Policy::test_policy();
        let denied = check(
            &policy,
            &no_commands(),
            &request(Some("not-the-token")),
            "/__cmd/greet",
        )
        .expect("a forged token must be refused");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn the_run_token_passes_a_granted_route() {
        let policy = Policy::test_policy();
        let request = request(Some(policy.token()));
        assert!(check(&policy, &no_commands(), &request, "/__cmd/greet").is_none());
    }

    #[test]
    fn an_ungranted_capability_is_refused_even_with_the_token() {
        // `StoreClear` is opt-in: the token alone must not wipe a user's settings.
        let policy = Policy::test_policy();
        let request = request(Some(policy.token()));
        let denied = check(&policy, &no_commands(), &request, "/__store/clear")
            .expect("an ungranted capability must be refused");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn a_command_without_a_declared_ability_needs_no_grant() {
        // The blanket `Capability::Commands` grant is still enough — declaring
        // an ability is opt-in, so existing apps keep working unchanged.
        let policy = Policy::test_policy();
        let registry = registry_with(&[Stub("greet", None)]);
        let request = request(Some(policy.token()));
        assert!(check(&policy, &registry, &request, "/__cmd/greet").is_none());
    }

    #[test]
    fn a_declared_ability_is_denied_until_it_is_granted() {
        // The point of the whole mechanism: `Capability::Commands` alone must not
        // reach a command the app marked as privileged.
        let policy = Policy::test_policy();
        assert!(policy.grants(crate::security::Capability::Commands));
        let registry = registry_with(&[Stub("greet", Some("posts.delete"))]);
        let request = request(Some(policy.token()));
        let denied = check(&policy, &registry, &request, "/__cmd/greet")
            .expect("an ungranted ability must be refused");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            denied.headers().get("x-elyra-error-kind").unwrap(),
            "forbidden"
        );
        let body = String::from_utf8_lossy(denied.body()).to_string();
        assert!(body.contains("posts.delete"), "{body}");
        assert!(body.contains("App::allow_ability"), "{body}");
    }

    #[test]
    fn a_granted_ability_passes() {
        let policy = Policy::test_policy_granting(&["posts.delete"]);
        let registry = registry_with(&[Stub("greet", Some("posts.delete"))]);
        let request = request(Some(policy.token()));
        assert!(check(&policy, &registry, &request, "/__cmd/greet").is_none());
    }

    #[test]
    fn a_grant_for_one_ability_does_not_cover_another() {
        let policy = Policy::test_policy_granting(&["posts.create"]);
        let registry = registry_with(&[Stub("greet", Some("posts.delete"))]);
        let request = request(Some(policy.token()));
        assert!(check(&policy, &registry, &request, "/__cmd/greet").is_some());
    }

    #[test]
    fn a_namespace_grant_covers_its_abilities() {
        let policy = Policy::test_policy_granting(&["posts.*"]);
        let registry = registry_with(&[Stub("greet", Some("posts.delete"))]);
        let request = request(Some(policy.token()));
        assert!(check(&policy, &registry, &request, "/__cmd/greet").is_none());
        // …but not a neighbouring namespace.
        let registry = registry_with(&[Stub("greet", Some("users.delete"))]);
        assert!(check(&policy, &registry, &request, "/__cmd/greet").is_some());
    }

    #[test]
    fn an_unknown_command_is_not_an_authorization_error() {
        // A typo must fall through to dispatch and answer `UnknownCommand`,
        // rather than being reported as a permission problem.
        let policy = Policy::test_policy();
        let registry = registry_with(&[Stub("greet", Some("posts.delete"))]);
        let request = request(Some(policy.token()));
        assert!(check(&policy, &registry, &request, "/__cmd/greeet").is_none());
    }

    #[test]
    fn an_oversized_body_is_refused_before_decoding() {
        let policy = Policy::test_policy();
        let request = Request::builder()
            .uri("elyra://localhost/__cmd/greet")
            .header("x-elyra-token", policy.token())
            .body(vec![0u8; policy.max_body() + 1])
            .unwrap();
        let denied = check(&policy, &no_commands(), &request, "/__cmd/greet")
            .expect("an oversized body must be refused");
        assert_eq!(denied.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            denied.headers().get("x-elyra-error-kind").unwrap(),
            "bad-request"
        );
    }
}
