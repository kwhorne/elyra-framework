//! A Laravel backend (RFC 0005): the API the desktop app shares with its web
//! half, called as the signed-in user, with Laravel's answers mapped to
//! Elyra's errors. Behind the `backend` feature.
//!
//! ```ignore
//! App::new().backend(Backend::new("https://crm.example.com"))
//!
//! #[command(live, can = "customers.view")]
//! async fn customers_index(ctx: Ctx, query: CustomerQuery) -> elyra::Result<Page<Customer>> {
//!     Ok(ctx.get::<Backend>().get("/api/customers").query(&query).json().await?)
//! }
//! ```
//!
//! A `422` becomes the same [`ValidationErrors`] a local command returns, so
//! a form shows the server's messages per field. A `401` forgets the token.
//! No answer at all is an `offline` error, which the UI can tell apart.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

use crate::http::{Http, HttpError, Method, Request, Response};
use crate::validation::ValidationErrors;

/// The Laravel API, as the signed-in user. Cheap to clone; clones share the
/// token.
#[derive(Clone)]
pub struct Backend {
    http: Http,
    base: String,
    token: Arc<RwLock<Option<String>>>,
    routes: Routes,
    /// Told when the server says the token is no longer good (a `401`).
    signed_out: Arc<RwLock<Option<SignedOut>>>,
    written: Arc<RwLock<Option<Written>>>,
}

/// What to do when the server says the token is no longer good.
type SignedOut = Arc<dyn Fn() + Send + Sync>;

/// What to do after a write changed a resource: re-run the live queries that
/// read it.
type Written = Arc<dyn Fn(&str) + Send + Sync>;

/// The routes `Auth` uses, relative to the base URL.
#[derive(Clone, Debug)]
pub(crate) struct Routes {
    pub token: String,
    pub revoke: String,
    pub user: String,
}

impl Backend {
    /// The backend at `base_url` (`https://crm.example.com`), with Laravel's
    /// documented routes for app tokens.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            http: Http::new(),
            base: base_url.into().trim_end_matches('/').to_owned(),
            token: Arc::new(RwLock::new(None)),
            // In `routes/api.php`, where Laravel prefixes them with `/api`.
            routes: Routes {
                token: "/api/sanctum/token".into(),
                revoke: "/api/sanctum/token".into(),
                user: "/api/user".into(),
            },
            signed_out: Arc::new(RwLock::new(None)),
            written: Arc::new(RwLock::new(None)),
        }
    }

    /// Where a token is issued (`POST`); `/api/sanctum/token` by default.
    pub fn token_route(mut self, path: impl Into<String>) -> Self {
        self.routes.token = path.into();
        self
    }

    /// Where the current token is revoked (`DELETE`); `/api/sanctum/token`
    /// by default.
    pub fn revoke_route(mut self, path: impl Into<String>) -> Self {
        self.routes.revoke = path.into();
        self
    }

    /// Where the signed-in user is read (`GET`); `/api/user` by default.
    pub fn user_route(mut self, path: impl Into<String>) -> Self {
        self.routes.user = path.into();
        self
    }

    pub fn base_url(&self) -> &str {
        &self.base
    }

    pub(crate) fn routes(&self) -> &Routes {
        &self.routes
    }

    /// This backend, answered by `fake` — for a test that uses a `Backend`
    /// without an app. In an app, `App::swap(Http::fake(..))` does it.
    pub fn with_fake_for_tests(self, fake: &crate::http::HttpFake) -> Self {
        self.with_http(Http::fake(fake.clone()))
    }

    /// Use the app's `Http` (so a test's `HttpFake` answers).
    pub(crate) fn with_http(mut self, http: Http) -> Self {
        self.http = http;
        self
    }

    /// The token requests carry. `Auth` sets it when the user signs in;
    /// a test may set one directly.
    pub fn use_token(&self, token: Option<String>) {
        *self.token.write() = token;
    }

    /// Whether a token is held.
    pub fn has_token(&self) -> bool {
        self.token.read().is_some()
    }

    /// Run `f` when the server says the token is no longer good.
    pub(crate) fn on_signed_out(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.signed_out.write() = Some(Arc::new(f));
    }

    /// Run `f` with a resource's key after a write to it succeeds.
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    pub(crate) fn on_write(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        *self.written.write() = Some(Arc::new(f));
    }

    pub fn get(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Get, path)
    }

    pub fn post(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Post, path)
    }

    pub fn put(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Put, path)
    }

    pub fn patch(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Patch, path)
    }

    pub fn delete(&self, path: impl Into<String>) -> BackendRequest {
        self.request(Method::Delete, path)
    }

    /// A request to `path` (`/api/customers`) on the backend.
    pub fn request(&self, method: Method, path: impl Into<String>) -> BackendRequest {
        let path = path.into();
        let mut request = self.http.request(method, format!("{}{}", self.base, path));
        if let Some(token) = self.token.read().as_deref() {
            request = request.token(token);
        }
        BackendRequest {
            backend: self.clone(),
            method,
            path,
            request,
        }
    }
}

/// A request to the backend; [`send`](Self::send) or [`json`](Self::json)
/// sends it.
pub struct BackendRequest {
    backend: Backend,
    method: Method,
    path: String,
    request: Request,
}

impl BackendRequest {
    pub fn query(mut self, query: &impl Serialize) -> Self {
        self.request = self.request.query(query);
        self
    }

    /// A JSON body.
    pub fn body(mut self, body: &impl Serialize) -> Self {
        self.request = self.request.json(body);
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.request = self.request.header(name, value);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request = self.request.timeout(timeout);
        self
    }

    pub fn retry(mut self, times: u32, sleep: Duration) -> Self {
        self.request = self.request.retry(times, sleep);
        self
    }

    /// Send it: a success is the [`Response`], anything else a
    /// [`BackendError`].
    pub async fn send(self) -> Result<Response, BackendError> {
        if !self.path.starts_with('/') {
            return Err(BackendError::Http(HttpError::InvalidUrl(self.path)));
        }
        // Live queries: a read depends on its resource, and a live command's
        // re-run may not write (it would set itself off again).
        let key = resource_key(&self.path);
        let write = self.method != Method::Get;
        #[cfg(feature = "database")]
        if write {
            elyra_db::live::check_write(&key)
                .map_err(|e| BackendError::LiveWrite(e.to_string()))?;
        } else {
            elyra_db::live::depends_on(&key);
        }
        let response = self.request.send().await.map_err(|e| match e {
            HttpError::Unreachable(why) => BackendError::Unreachable(why),
            other => BackendError::Http(other),
        })?;
        let outcome = answer(response);
        if write && outcome.is_ok() {
            let written = self.backend.written.read().clone();
            if let Some(written) = written {
                written(&key);
            }
        }
        if matches!(
            outcome,
            Err(BackendError::Unauthenticated | BackendError::Expired)
        ) {
            // Only a token that was there can have expired: a `401` before
            // signing in isn't a sign-out.
            let had_token = self.backend.has_token();
            self.backend.use_token(None);
            let signed_out = self.backend.signed_out.read().clone();
            if let (true, Some(signed_out)) = (had_token, signed_out) {
                signed_out();
            }
        }
        outcome
    }

    /// Send it and decode a record: an API Resource's answer is wrapped in
    /// `{ "data": … }` (Laravel's default), a model returned as-is isn't —
    /// either works.
    pub async fn resource<T: DeserializeOwned>(self) -> Result<T, BackendError> {
        let value: Value = self.json().await?;
        let value = match value {
            Value::Object(mut map) if map.contains_key("data") => {
                map.remove("data").unwrap_or(Value::Null)
            }
            other => other,
        };
        serde_json::from_value(value)
            .map_err(|e| BackendError::Http(HttpError::Decode(e.to_string())))
    }

    /// Send it and decode a success as `T` (`204 No Content` as `null`).
    pub async fn json<T: DeserializeOwned>(self) -> Result<T, BackendError> {
        let response = self.send().await?;
        if response.bytes().is_empty() {
            return serde_json::from_value(Value::Null)
                .map_err(|e| BackendError::Http(HttpError::Decode(e.to_string())));
        }
        response.json().map_err(BackendError::Http)
    }
}

/// What a Laravel answer that isn't a success means.
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// `422`: Laravel's validation bag, per field.
    #[error("{0}")]
    Validation(ValidationErrors),
    /// `401`: the token is gone (revoked, expired) — the user is signed out.
    #[error("signed out: sign in again")]
    Unauthenticated,
    /// `419`: a session-only check; for a token, the same as signed out.
    #[error("the session expired: sign in again")]
    Expired,
    /// `403`, with Laravel's message.
    #[error("{0}")]
    Forbidden(String),
    /// `404`.
    #[error("not found")]
    NotFound,
    /// `429`, and how many seconds to wait when the server said.
    #[error("too many requests{}", retry_after.map(|s| format!("; try again in {s} s")).unwrap_or_default())]
    TooManyRequests { retry_after: Option<u64> },
    /// `5xx`, without the error page's HTML.
    #[error("the server failed ({0})")]
    Server(u16),
    /// No answer: offline, DNS, TLS, or the timeout.
    #[error("can't reach the server: {0}")]
    Unreachable(String),
    /// Another status, with Laravel's message when it sent one.
    #[error("the server answered {status}: {message}")]
    Other { status: u16, message: String },
    /// The request couldn't be made, or the answer isn't the type asked for.
    #[error("{0}")]
    Http(HttpError),
    /// A live command's re-run tried to write — which would set it off again.
    #[error("{0}")]
    LiveWrite(String),
    /// Signed in, but there's no keychain to keep the token in (the token was
    /// revoked again).
    #[error("this system has no keychain to keep the sign-in in: {0}")]
    Keychain(String),
}

impl From<BackendError> for crate::Error {
    fn from(e: BackendError) -> Self {
        let message = e.to_string();
        match e {
            BackendError::Validation(bag) => bag.into(),
            BackendError::Unauthenticated | BackendError::Expired => {
                crate::Error::with_kind("unauthenticated", message)
            }
            // Not `forbidden`: that kind is the IPC guard's (a missing token or
            // capability). This is the server's policy saying no.
            BackendError::Forbidden(_) => crate::Error::with_kind("denied", message),
            BackendError::NotFound => crate::Error::with_kind("not-found", message),
            BackendError::TooManyRequests { .. } => {
                crate::Error::with_kind("too-many-requests", message)
            }
            BackendError::Server(_) => crate::Error::with_kind("server", message),
            BackendError::Unreachable(_) => crate::Error::with_kind("offline", message),
            BackendError::Other { .. }
            | BackendError::Http(_)
            | BackendError::Keychain(_)
            | BackendError::LiveWrite(_) => crate::Error::Command(message),
        }
    }
}

/// The live-query key a request to `path` reads or writes: its resource, the
/// path without the query and without trailing ids — `/api/customers/12?x=1`
/// and `/api/customers` are both `backend:/api/customers`. That's what
/// Laravel's `apiResource` routes look like; anything else can use
/// `ctx.depends_on` / `ctx.invalidate`.
pub fn resource_key(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    while segments.last().is_some_and(|s| is_id(s)) {
        segments.pop();
    }
    format!("backend:/{}", segments.join("/"))
}

/// An id in a path: a number, a UUID, or a ULID.
fn is_id(segment: &str) -> bool {
    let number = segment.chars().all(|c| c.is_ascii_digit());
    let uuid = segment.len() == 36
        && segment.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });
    let ulid = segment.len() == 26 && segment.chars().all(|c| c.is_ascii_alphanumeric());
    number || uuid || ulid
}

/// Map a Laravel answer: a success stays the response.
fn answer(response: Response) -> Result<Response, BackendError> {
    let status = response.status();
    if response.ok() {
        return Ok(response);
    }
    let body: Option<Value> = response.json().ok();
    let message = body
        .as_ref()
        .and_then(|b| b["message"].as_str())
        .map(str::to_owned);
    Err(match status {
        422 => match body.as_ref().and_then(|b| b["errors"].as_object()) {
            Some(errors) => {
                let mut bag = ValidationErrors::new();
                for (field, messages) in errors {
                    for m in messages.as_array().into_iter().flatten() {
                        if let Some(m) = m.as_str() {
                            bag.add(field, m);
                        }
                    }
                }
                if bag.is_empty() {
                    bag.add(
                        "_",
                        message.unwrap_or_else(|| "The given data was invalid.".into()),
                    );
                }
                BackendError::Validation(bag)
            }
            None => BackendError::Other {
                status,
                message: message.unwrap_or_else(|| "The given data was invalid.".into()),
            },
        },
        401 => BackendError::Unauthenticated,
        403 => BackendError::Forbidden(
            message.unwrap_or_else(|| "This action is unauthorized.".into()),
        ),
        404 => BackendError::NotFound,
        419 => BackendError::Expired,
        429 => BackendError::TooManyRequests {
            retry_after: response
                .header("retry-after")
                .and_then(|s| s.trim().parse().ok()),
        },
        500..=599 => BackendError::Server(status),
        _ => BackendError::Other {
            status,
            message: message.unwrap_or_default(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_resource_is_its_path_without_ids_or_query() {
        assert_eq!(resource_key("/api/customers"), "backend:/api/customers");
        assert_eq!(resource_key("/api/customers/12"), "backend:/api/customers");
        assert_eq!(
            resource_key("/api/customers?page=2&search=ada"),
            "backend:/api/customers"
        );
        assert_eq!(
            resource_key("/api/customers/12/notes"),
            "backend:/api/customers/12/notes"
        );
        assert_eq!(
            resource_key("/api/customers/12/notes/5"),
            "backend:/api/customers/12/notes"
        );
        assert_eq!(
            resource_key("/api/orders/9b2f5c1e-3d4a-4e5f-8a9b-0c1d2e3f4a5b"),
            "backend:/api/orders"
        );
        assert_eq!(
            resource_key("/api/orders/01JA2B3C4D5E6F7G8H9J0K1M2N"),
            "backend:/api/orders"
        );
        assert_eq!(
            resource_key("/api/customers/export"),
            "backend:/api/customers/export"
        );
        assert_eq!(resource_key("/api/user"), "backend:/api/user");
    }
}
