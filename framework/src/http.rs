//! An HTTP client — Laravel's `Http` facade, in Rust (RFC 0005). Behind the
//! `http` feature; `App` binds one, so a command resolves it:
//!
//! ```ignore
//! let res = ctx.get::<Http>()
//!     .get("https://api.example.com/rates")
//!     .query(&[("base", "NOK")])
//!     .timeout(Duration::from_secs(10))
//!     .retry(3, Duration::from_millis(200))
//!     .send()
//!     .await?;
//! let rates: Rates = res.json()?;
//! ```
//!
//! - It asks for JSON (`Accept: application/json`) and sends JSON bodies.
//! - Every request has a timeout (30 s unless set). Retries happen only on a
//!   failed connection or a `5xx`, and only for idempotent methods unless
//!   [`Request::retry_any_method`] says otherwise.
//! - HTTPS only, except to loopback — redirects included. A redirect to
//!   another host doesn't carry `Authorization` or cookies.
//! - A response other than a success is still a [`Response`]: the caller
//!   decides what a `404` means. Only a request that got no answer is an
//!   [`HttpError`].
//! - Logs (`elyra::http`) carry the method, host, path, status and duration:
//!   never the query, the body or a header, which can hold personal data or
//!   a token.
//!
//! Tests swap in a fake that answers from a table and records what was sent:
//! [`HttpFake`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use reqwest::Url;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// How long a request may take unless [`Request::timeout`] says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// The largest response body read, so a misbehaving server can't fill memory.
pub const MAX_BODY: usize = 32 * 1024 * 1024;

/// The most redirects followed.
const MAX_REDIRECTS: usize = 10;

/// A request that got no usable answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HttpError {
    /// No answer: the connection, DNS, TLS, or the timeout.
    #[error("unreachable: {0}")]
    Unreachable(String),
    /// Not HTTPS, and not to loopback.
    #[error("refusing `{0}`: only HTTPS, except to loopback")]
    Insecure(String),
    #[error("invalid URL `{0}`")]
    InvalidUrl(String),
    /// The query or body couldn't be encoded.
    #[error("couldn't encode the request: {0}")]
    Encode(String),
    /// The body isn't the type asked for.
    #[error("couldn't decode the response: {0}")]
    Decode(String),
    #[error("the response is larger than {MAX_BODY} bytes")]
    TooLarge,
}

impl From<HttpError> for crate::Error {
    fn from(e: HttpError) -> Self {
        crate::Error::Command(e.to_string())
    }
}

/// The client. Cheap to clone; clones share their connection pool.
#[derive(Clone)]
pub struct Http {
    client: reqwest::Client,
    fake: Option<HttpFake>,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if !secure(attempt.url()) {
                    let to = attempt.url().to_string();
                    attempt.error(format!("redirected to `{to}`, which isn't HTTPS"))
                } else {
                    // reqwest drops `Authorization` and cookies when the host
                    // or port changes.
                    attempt.follow()
                }
            }))
            .build()
            .expect("an HTTP client with no unusual settings builds");
        Self { client, fake: None }
    }

    /// A client that never touches the network: `fake` answers, and records
    /// what was sent. Bind it with `App::swap(Http::fake(fake.clone()))`.
    pub fn fake(fake: HttpFake) -> Self {
        Self {
            fake: Some(fake),
            ..Self::new()
        }
    }

    pub fn get(&self, url: impl AsRef<str>) -> Request {
        self.request(Method::Get, url)
    }

    pub fn post(&self, url: impl AsRef<str>) -> Request {
        self.request(Method::Post, url)
    }

    pub fn put(&self, url: impl AsRef<str>) -> Request {
        self.request(Method::Put, url)
    }

    pub fn patch(&self, url: impl AsRef<str>) -> Request {
        self.request(Method::Patch, url)
    }

    pub fn delete(&self, url: impl AsRef<str>) -> Request {
        self.request(Method::Delete, url)
    }

    pub fn request(&self, method: Method, url: impl AsRef<str>) -> Request {
        Request {
            http: self.clone(),
            method,
            url: url.as_ref().to_owned(),
            query: Vec::new(),
            headers: Vec::new(),
            body: None,
            timeout: DEFAULT_TIMEOUT,
            retries: 0,
            retry_sleep: Duration::ZERO,
            retry_any: false,
            encode_error: None,
        }
    }
}

/// An HTTP method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Whether repeating it can't do more than doing it once.
    pub fn idempotent(self) -> bool {
        !matches!(self, Method::Post | Method::Patch)
    }

    fn reqwest(self) -> reqwest::Method {
        match self {
            Method::Get => reqwest::Method::GET,
            Method::Post => reqwest::Method::POST,
            Method::Put => reqwest::Method::PUT,
            Method::Patch => reqwest::Method::PATCH,
            Method::Delete => reqwest::Method::DELETE,
        }
    }
}

/// A request being built; [`send`](Request::send) sends it.
pub struct Request {
    http: Http,
    method: Method,
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Option<Vec<u8>>,
    timeout: Duration,
    retries: u32,
    retry_sleep: Duration,
    retry_any: bool,
    encode_error: Option<String>,
}

impl Request {
    /// Query parameters: a struct or map of scalars, or pairs. A `None`
    /// field is left out; a list repeats its key as `key[]`, as Laravel reads it.
    pub fn query(mut self, query: &impl Serialize) -> Self {
        match serde_json::to_value(query) {
            Ok(value) => {
                if let Err(e) = flatten_query(&value, &mut self.query) {
                    self.encode_error.get_or_insert(e);
                }
            }
            Err(e) => {
                self.encode_error.get_or_insert(e.to_string());
            }
        }
        self
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// `Authorization: Bearer <token>`.
    pub fn token(self, token: &str) -> Self {
        self.header("Authorization", format!("Bearer {token}"))
    }

    /// A JSON body.
    pub fn json(mut self, body: &impl Serialize) -> Self {
        match serde_json::to_vec(body) {
            Ok(bytes) => {
                self.body = Some(bytes);
                self.headers
                    .push(("Content-Type".into(), "application/json".into()));
            }
            Err(e) => {
                self.encode_error.get_or_insert(e.to_string());
            }
        }
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Try again up to `times` more, `sleep` apart, after a failed
    /// connection or a `5xx` — for a method that's safe to repeat.
    pub fn retry(mut self, times: u32, sleep: Duration) -> Self {
        self.retries = times;
        self.retry_sleep = sleep;
        self
    }

    /// Retry a `POST` or `PATCH` too — when the server makes it safe (an
    /// idempotency key, say).
    pub fn retry_any_method(mut self) -> Self {
        self.retry_any = true;
        self
    }

    /// Send it, with the retries asked for.
    pub async fn send(self) -> Result<Response, HttpError> {
        if let Some(e) = &self.encode_error {
            return Err(HttpError::Encode(e.clone()));
        }
        let mut url = Url::parse(&self.url).map_err(|_| HttpError::InvalidUrl(self.url.clone()))?;
        if !secure(&url) {
            return Err(HttpError::Insecure(self.url.clone()));
        }
        if !self.query.is_empty() {
            url.query_pairs_mut()
                .extend_pairs(self.query.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        }
        let may_retry = self.retry_any || self.method.idempotent();
        let mut attempt = 0;
        loop {
            let started = Instant::now();
            let outcome = self.once(&url).await;
            let retry = may_retry
                && attempt < self.retries
                && match &outcome {
                    Ok(res) => res.status >= 500,
                    Err(HttpError::Unreachable(_)) => true,
                    Err(_) => false,
                };
            match &outcome {
                Ok(res) => crate::info!(
                    target: "elyra::http",
                    "{} {}{} → {} in {:?}",
                    self.method.as_str(),
                    url.host_str().unwrap_or_default(),
                    url.path(),
                    res.status,
                    started.elapsed()
                ),
                Err(e) => crate::warn!(
                    target: "elyra::http",
                    "{} {}{} failed: {}",
                    self.method.as_str(),
                    url.host_str().unwrap_or_default(),
                    url.path(),
                    // The kind, not the detail: it can echo the URL.
                    match e {
                        HttpError::Unreachable(_) => "unreachable",
                        HttpError::TooLarge => "too large",
                        _ => "error",
                    }
                ),
            }
            if !retry {
                return outcome;
            }
            attempt += 1;
            if !self.retry_sleep.is_zero() {
                tokio::time::sleep(self.retry_sleep).await;
            }
        }
    }

    async fn once(&self, url: &Url) -> Result<Response, HttpError> {
        if let Some(fake) = &self.http.fake {
            return fake.answer(self, url);
        }
        let mut builder = self
            .http
            .client
            .request(self.method.reqwest(), url.clone())
            .timeout(self.timeout)
            .header("Accept", "application/json");
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &self.body {
            builder = builder.body(body.clone());
        }
        let mut res = builder.send().await.map_err(unreachable)?;
        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_owned(), v.to_str().ok()?.to_owned())))
            .collect();
        if res.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            return Err(HttpError::TooLarge);
        }
        let mut body = Vec::new();
        while let Some(chunk) = res.chunk().await.map_err(unreachable)? {
            if body.len() + chunk.len() > MAX_BODY {
                return Err(HttpError::TooLarge);
            }
            body.extend_from_slice(&chunk);
        }
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}

fn unreachable(e: reqwest::Error) -> HttpError {
    // reqwest's message names the URL; keep the cause without it — and the
    // causes under it, which say what actually went wrong.
    let e = e.without_url();
    let mut message = e.to_string();
    let mut source = std::error::Error::source(&e);
    while let Some(cause) = source {
        message.push_str(": ");
        message.push_str(&cause.to_string());
        source = cause.source();
    }
    HttpError::Unreachable(message)
}

/// HTTPS, or anything to loopback.
fn secure(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => match url.host_str() {
            Some("localhost") => true,
            Some(host) => host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()),
            None => false,
        },
        _ => false,
    }
}

fn flatten_query(value: &Value, out: &mut Vec<(String, String)>) -> Result<(), String> {
    let scalar = |v: &Value| -> Option<String> {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(if *b { "1" } else { "0" }.to_owned()),
            _ => None,
        }
    };
    let mut pair = |key: &str, v: &Value| -> Result<(), String> {
        match v {
            Value::Null => Ok(()),
            Value::Array(items) => items.iter().try_for_each(|item| {
                let text = scalar(item).ok_or_else(|| format!("`{key}` holds a non-scalar"))?;
                out.push((format!("{key}[]"), text));
                Ok(())
            }),
            other => {
                let text = scalar(other).ok_or_else(|| format!("`{key}` isn't a scalar"))?;
                out.push((key.to_owned(), text));
                Ok(())
            }
        }
    };
    match value {
        Value::Object(map) => map.iter().try_for_each(|(k, v)| pair(k, v)),
        // Pairs: `[("base", "NOK")]`.
        Value::Array(pairs) => {
            pairs
                .iter()
                .try_for_each(|p| match p.as_array().map(Vec::as_slice) {
                    Some([Value::String(k), v]) => pair(k, v),
                    _ => Err("a query is a struct, a map, or key-value pairs".into()),
                })
        }
        Value::Null => Ok(()),
        _ => Err("a query is a struct, a map, or key-value pairs".into()),
    }
}

/// An answer: any status, the headers, and the body (read whole).
#[derive(Debug, Clone)]
pub struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Response {
    pub fn status(&self) -> u16 {
        self.status
    }

    /// `2xx`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// A header, by name in any case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn bytes(&self) -> &[u8] {
        &self.body
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// The body as `T`.
    pub fn json<T: DeserializeOwned>(&self) -> Result<T, HttpError> {
        serde_json::from_slice(&self.body).map_err(|e| HttpError::Decode(e.to_string()))
    }
}

/// A fake for tests: answers from a table, and records each request. Clones
/// share the table and the record.
///
/// ```ignore
/// let fake = HttpFake::new()
///     .get("https://api.example.com/rates", 200, json!({ "NOK": 1 }))
///     .post("https://api.example.com/orders", 422, json!({ "errors": {} }));
/// let app = TestApp::new(app().swap(Http::fake(fake.clone())));
/// // …
/// fake.assert_sent("POST", "https://api.example.com/orders");
/// ```
#[derive(Clone, Default)]
pub struct HttpFake {
    inner: Arc<FakeInner>,
}

#[derive(Default)]
struct FakeInner {
    routes: Mutex<Vec<(Method, String, FakeAnswer)>>,
    sent: Mutex<Vec<Sent>>,
}

#[derive(Clone)]
enum FakeAnswer {
    Respond(u16, Vec<(String, String)>, Vec<u8>),
    Unreachable,
}

/// A request the fake received.
#[derive(Debug, Clone)]
pub struct Sent {
    pub method: &'static str,
    /// The full URL, query included.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// The JSON body, if there was one.
    pub body: Option<Value>,
}

impl Sent {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl HttpFake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `method url` (the query ignored; a trailing `*` matches any
    /// rest) with `status` and a JSON `body`. Added later, matched first.
    pub fn on(self, method: Method, url: impl Into<String>, status: u16, body: Value) -> Self {
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        self.inner.routes.lock().push((
            method,
            url.into(),
            FakeAnswer::Respond(
                status,
                vec![("content-type".into(), "application/json".into())],
                bytes,
            ),
        ));
        self
    }

    /// Answer with headers too (`Retry-After`, say).
    pub fn on_with_headers(
        self,
        method: Method,
        url: impl Into<String>,
        status: u16,
        headers: &[(&str, &str)],
        body: Value,
    ) -> Self {
        let bytes = serde_json::to_vec(&body).unwrap_or_default();
        let headers = headers
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        self.inner.routes.lock().push((
            method,
            url.into(),
            FakeAnswer::Respond(status, headers, bytes),
        ));
        self
    }

    /// No answer at all, as when offline.
    pub fn unreachable(self, method: Method, url: impl Into<String>) -> Self {
        self.inner
            .routes
            .lock()
            .push((method, url.into(), FakeAnswer::Unreachable));
        self
    }

    pub fn get(self, url: impl Into<String>, status: u16, body: Value) -> Self {
        self.on(Method::Get, url, status, body)
    }

    pub fn post(self, url: impl Into<String>, status: u16, body: Value) -> Self {
        self.on(Method::Post, url, status, body)
    }

    pub fn put(self, url: impl Into<String>, status: u16, body: Value) -> Self {
        self.on(Method::Put, url, status, body)
    }

    pub fn patch(self, url: impl Into<String>, status: u16, body: Value) -> Self {
        self.on(Method::Patch, url, status, body)
    }

    pub fn delete(self, url: impl Into<String>, status: u16, body: Value) -> Self {
        self.on(Method::Delete, url, status, body)
    }

    /// Every request so far, in order.
    pub fn sent(&self) -> Vec<Sent> {
        self.inner.sent.lock().clone()
    }

    /// The requests to `method url` (the query ignored).
    pub fn sent_to(&self, method: &str, url: &str) -> Vec<Sent> {
        self.sent()
            .into_iter()
            .filter(|s| s.method == method && without_query(&s.url) == url)
            .collect()
    }

    /// # Panics
    /// When nothing was sent to `method url`.
    pub fn assert_sent(&self, method: &str, url: &str) -> Sent {
        self.sent_to(method, url)
            .pop()
            .unwrap_or_else(|| panic!("no {method} {url} was sent; sent: {:?}", self.urls()))
    }

    /// # Panics
    /// When something was sent to `method url`.
    pub fn assert_not_sent(&self, method: &str, url: &str) {
        assert!(
            self.sent_to(method, url).is_empty(),
            "{method} {url} was sent"
        );
    }

    fn urls(&self) -> Vec<String> {
        self.sent()
            .iter()
            .map(|s| format!("{} {}", s.method, s.url))
            .collect()
    }

    fn answer(&self, request: &Request, url: &Url) -> Result<Response, HttpError> {
        let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        headers.extend(request.headers.iter().cloned());
        self.inner.sent.lock().push(Sent {
            method: request.method.as_str(),
            url: url.to_string(),
            headers,
            body: request
                .body
                .as_ref()
                .and_then(|b| serde_json::from_slice(b).ok()),
        });
        let bare = without_query(url.as_str());
        let answer = self
            .inner
            .routes
            .lock()
            .iter()
            .rev()
            .find(|(method, pattern, _)| {
                *method == request.method
                    && match pattern.strip_suffix('*') {
                        Some(prefix) => bare.starts_with(prefix),
                        None => bare == pattern.as_str(),
                    }
            })
            .map(|(_, _, answer)| answer.clone());
        match answer {
            Some(FakeAnswer::Respond(status, headers, body)) => Ok(Response {
                status,
                headers,
                body,
            }),
            Some(FakeAnswer::Unreachable) => Err(HttpError::Unreachable(
                "the fake answered with no answer".into(),
            )),
            None => Err(HttpError::Unreachable(format!(
                "HttpFake has no answer for {} {bare}",
                request.method.as_str()
            ))),
        }
    }
}

fn without_query(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_https_except_to_loopback() {
        let ok = |u: &str| secure(&Url::parse(u).unwrap());
        assert!(ok("https://example.com/x"));
        assert!(ok("http://localhost:8000/x"));
        assert!(ok("http://127.0.0.1/x"));
        assert!(ok("http://[::1]:9000/x"));
        assert!(!ok("http://example.com/x"));
        assert!(!ok("http://127.0.0.1.example.com/x"));
        assert!(!ok("ftp://localhost/x"));
    }

    #[test]
    fn queries_flatten_as_laravel_reads_them() {
        let mut out = Vec::new();
        flatten_query(
            &json!({ "search": "ada", "page": 2, "done": true, "none": null, "ids": [1, 2] }),
            &mut out,
        )
        .unwrap();
        out.sort();
        assert_eq!(
            out,
            [
                ("done".into(), "1".into()),
                ("ids[]".into(), "1".into()),
                ("ids[]".into(), "2".into()),
                ("page".into(), "2".into()),
                ("search".into(), "ada".into()),
            ]
        );
        let mut pairs = Vec::new();
        flatten_query(&json!([["base", "NOK"]]), &mut pairs).unwrap();
        assert_eq!(pairs, [("base".into(), "NOK".into())]);
        assert!(flatten_query(&json!({ "nested": { "a": 1 } }), &mut Vec::new()).is_err());
        assert!(flatten_query(&json!("text"), &mut Vec::new()).is_err());
    }

    #[test]
    fn idempotent_methods() {
        assert!(
            Method::Get.idempotent() && Method::Put.idempotent() && Method::Delete.idempotent()
        );
        assert!(!Method::Post.idempotent() && !Method::Patch.idempotent());
    }
}
