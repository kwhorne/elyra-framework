//! The wire shapes every IPC response takes: the MessagePack success/error
//! encodings and the headers that carry status out-of-band.
//!
//! The frontend never parses a status code to decide what happened — it reads
//! `x-elyra-status` and, on failure, `x-elyra-error-kind` (`forbidden`,
//! `bad-request`, `validation`, `cancelled`, `panic`, `command`). Keeping those
//! constructors in one place is what keeps the two sides in step.

use std::borrow::Cow;

use wry::http::{header, Response, StatusCode};

/// Every response the protocol handler produces.
pub(super) type Body = Response<Cow<'static, [u8]>>;

/// An error response with an explicit status and `x-elyra-error-kind`.
pub(super) fn error_response(
    status: StatusCode,
    kind: &'static str,
    message: impl std::fmt::Display,
) -> Body {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-elyra-status", "error")
        .header("x-elyra-error-kind", kind)
        .body(Cow::Owned(message.to_string().into_bytes()))
        .unwrap()
}

/// Encode a serializable value as a MessagePack (named-map) response.
pub(super) fn msgpack_ok<T: serde::Serialize>(value: &T) -> Body {
    let bytes = rmp_serde::to_vec_named(value).unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(bytes))
        .unwrap()
}

/// A plain-text error response (mirrors the command error shape).
pub(super) fn msgpack_err(message: String) -> Body {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-elyra-status", "error")
        .body(Cow::Owned(message.into_bytes()))
        .unwrap()
}

/// Whether an error message is a Laravel-style validation bag: a JSON object
/// mapping field names to arrays of messages.
pub(super) fn is_validation_bag(message: &str) -> bool {
    let trimmed = message.trim_start();
    if !trimmed.starts_with('{') {
        return false;
    }
    serde_json::from_str::<std::collections::BTreeMap<String, Vec<String>>>(trimmed)
        .map(|bag| !bag.is_empty())
        .unwrap_or(false)
}

/// The panic message carried by a `JoinError`, when it can be recovered.
pub(super) fn panic_detail(err: tokio::task::JoinError) -> String {
    if !err.is_panic() {
        return err.to_string();
    }
    // `into_panic` needs ownership; clone the payload out of a caught panic by
    // downcasting the boxed message shapes `panic!` produces.
    match err.try_into_panic() {
        Ok(payload) => {
            if let Some(s) = payload.downcast_ref::<&'static str>() {
                (*s).to_string()
            } else if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "unknown panic payload".to_string()
            }
        }
        Err(e) => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panicking_command_surfaces_its_message() {
        // The response path must never silently drop the responder: before this,
        // a panic left `await invoke(..)` pending forever.
        let joined = tokio::spawn(async { panic!("boom in a command") }).await;
        let err = joined.expect_err("the task must have panicked");
        assert!(err.is_panic());
        assert_eq!(panic_detail(err), "boom in a command");
    }

    #[tokio::test]
    async fn panic_detail_handles_formatted_messages() {
        let value = 42;
        let joined = tokio::spawn(async move { panic!("bad value: {value}") }).await;
        let err = joined.expect_err("the task must have panicked");
        assert_eq!(panic_detail(err), "bad value: 42");
    }

    #[test]
    fn validation_bags_are_detected_for_the_error_kind_header() {
        assert!(is_validation_bag(
            r#"{"email":["The email must be valid."]}"#
        ));
        assert!(is_validation_bag(
            r#"{"age":["must be at least 18"],"email":["required"]}"#
        ));
        // Not a bag: plain messages, empty objects, other JSON shapes.
        assert!(!is_validation_bag("nope"));
        assert!(!is_validation_bag("{}"));
        assert!(!is_validation_bag(r#"{"email":"not-an-array"}"#));
        assert!(!is_validation_bag(r#"["a","b"]"#));
    }
}
