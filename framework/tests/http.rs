//! `Http` (RFC 0005 step 1) against real sockets: a tiny HTTP/1.1 server that
//! answers from a script and records what it was sent.
#![cfg(feature = "http")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use elyra::http::{Http, HttpError, HttpFake, Method};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// A request the server received: the request line and the headers.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// A server on 127.0.0.1 that answers the n-th request with `script[n]` (the
/// last one from then on) — raw HTTP, so a test controls every byte.
async fn server(script: Vec<String>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        let mut n = 0;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read the head, then the body by Content-Length.
            let head_end = loop {
                let Ok(read) = stream.read(&mut chunk).await else {
                    break None;
                };
                if read == 0 {
                    break None;
                }
                buf.extend_from_slice(&chunk[..read]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    break Some(i);
                }
            };
            let Some(head_end) = head_end else { continue };
            let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
            let mut lines = head.split("\r\n");
            let line = lines.next().unwrap_or_default().to_owned();
            let headers: Vec<(String, String)> = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                .collect();
            let length: usize = headers
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, v)| v.parse().ok())
                .unwrap_or(0);
            let mut body = buf[head_end + 4..].to_vec();
            while body.len() < length {
                let Ok(read) = stream.read(&mut chunk).await else {
                    break;
                };
                if read == 0 {
                    break;
                }
                body.extend_from_slice(&chunk[..read]);
            }
            record.lock().unwrap().push(Seen {
                line,
                headers,
                body: String::from_utf8_lossy(&body).to_string(),
            });
            let reply = script[n.min(script.len() - 1)].clone();
            n += 1;
            if reply == "HANG" {
                tokio::time::sleep(Duration::from_secs(30)).await;
                continue;
            }
            let _ = stream.write_all(reply.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
    });
    (base, seen)
}

fn reply(status: &str, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

#[derive(Deserialize, Debug, PartialEq)]
struct Rates {
    nok: f64,
}

#[tokio::test]
async fn a_get_asks_for_json_and_decodes_it() {
    let (base, seen) = server(vec![reply("200 OK", "", r#"{"nok":10.5}"#)]).await;
    let res = Http::new()
        .get(format!("{base}/rates"))
        .query(&json!({ "base": "EUR", "page": 2, "skip": null }))
        .send()
        .await
        .unwrap();
    assert!(res.ok());
    assert_eq!(res.json::<Rates>().unwrap(), Rates { nok: 10.5 });
    let seen = seen.lock().unwrap()[0].clone();
    assert!(
        seen.line == "GET /rates?base=EUR&page=2 HTTP/1.1"
            || seen.line == "GET /rates?page=2&base=EUR HTTP/1.1",
        "{}",
        seen.line
    );
    assert_eq!(seen.header("accept"), Some("application/json"));
}

#[tokio::test]
async fn a_status_that_isnt_a_success_is_still_an_answer() {
    let (base, _) = server(vec![reply("404 Not Found", "", r#"{"message":"Gone"}"#)]).await;
    let res = Http::new().get(format!("{base}/x")).send().await.unwrap();
    assert_eq!(res.status(), 404);
    assert!(!res.ok());
    assert_eq!(res.json::<Value>().unwrap()["message"], "Gone");
    assert!(matches!(res.json::<Rates>(), Err(HttpError::Decode(_))));
}

#[tokio::test]
async fn a_json_body_and_a_token_are_sent() {
    let (base, seen) = server(vec![reply("201 Created", "", "{}")]).await;
    Http::new()
        .post(format!("{base}/orders"))
        .token("secret-token")
        .json(&json!({ "qty": 2 }))
        .send()
        .await
        .unwrap();
    let seen = seen.lock().unwrap()[0].clone();
    assert_eq!(seen.header("authorization"), Some("Bearer secret-token"));
    assert_eq!(seen.header("content-type"), Some("application/json"));
    assert_eq!(seen.body, r#"{"qty":2}"#);
}

#[tokio::test]
async fn a_redirect_to_another_host_drops_the_token() {
    let (other, other_seen) = server(vec![reply("200 OK", "", "{}")]).await;
    let (base, seen) = server(vec![reply(
        "302 Found",
        &format!("Location: {other}/landed\r\n"),
        "",
    )])
    .await;
    let res = Http::new()
        .get(format!("{base}/start"))
        .token("secret-token")
        .send()
        .await
        .unwrap();
    assert!(res.ok());
    assert_eq!(
        seen.lock().unwrap()[0].header("authorization"),
        Some("Bearer secret-token")
    );
    let landed = other_seen.lock().unwrap()[0].clone();
    assert_eq!(landed.line, "GET /landed HTTP/1.1");
    assert_eq!(
        landed.header("authorization"),
        None,
        "not carried to another host"
    );
}

#[tokio::test]
async fn a_redirect_away_from_https_is_refused() {
    let (base, _) = server(vec![reply(
        "302 Found",
        "Location: http://example.com/phish\r\n",
        "",
    )])
    .await;
    let err = Http::new()
        .get(format!("{base}/start"))
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HttpError::Unreachable(m) if m.contains("isn't HTTPS")),
        "{err}"
    );
}

#[tokio::test]
async fn plain_http_is_refused_before_anything_is_sent() {
    let err = Http::new()
        .get("http://example.com/x")
        .send()
        .await
        .unwrap_err();
    assert_eq!(err, HttpError::Insecure("http://example.com/x".into()));
    assert!(matches!(
        Http::new().get("not a url").send().await,
        Err(HttpError::InvalidUrl(_))
    ));
}

#[tokio::test]
async fn retries_are_for_5xx_and_idempotent_methods() {
    let script = vec![
        reply("503 Service Unavailable", "", "{}"),
        reply("503 Service Unavailable", "", "{}"),
        reply("200 OK", "", "{}"),
    ];
    let (base, seen) = server(script.clone()).await;
    let res = Http::new()
        .get(format!("{base}/x"))
        .retry(3, Duration::from_millis(1))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(seen.lock().unwrap().len(), 3);

    // A POST isn't repeated…
    let (base, seen) = server(script.clone()).await;
    let res = Http::new()
        .post(format!("{base}/x"))
        .retry(3, Duration::from_millis(1))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 503);
    assert_eq!(seen.lock().unwrap().len(), 1);
    // …unless asked.
    let (base, seen) = server(script).await;
    let res = Http::new()
        .post(format!("{base}/x"))
        .retry(3, Duration::from_millis(1))
        .retry_any_method()
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(seen.lock().unwrap().len(), 3);

    // A 4xx is an answer, not a reason to try again.
    let (base, seen) = server(vec![reply("422 Unprocessable Content", "", "{}")]).await;
    Http::new()
        .get(format!("{base}/x"))
        .retry(3, Duration::from_millis(1))
        .send()
        .await
        .unwrap();
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn no_answer_is_unreachable_within_the_timeout() {
    // Nothing listens here.
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let err = Http::new()
        .get(format!("http://127.0.0.1:{closed}/x"))
        .retry(2, Duration::from_millis(1))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::Unreachable(_)), "{err}");

    let (base, _) = server(vec!["HANG".into()]).await;
    let started = Instant::now();
    let err = Http::new()
        .get(format!("{base}/slow"))
        .timeout(Duration::from_millis(200))
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::Unreachable(_)), "{err}");
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn an_oversized_body_is_refused() {
    let (base, _) = server(vec![
        "HTTP/1.1 200 OK\r\nContent-Length: 999999999999\r\nConnection: close\r\n\r\n{}".into(),
    ])
    .await;
    let err = Http::new()
        .get(format!("{base}/big"))
        .send()
        .await
        .unwrap_err();
    assert_eq!(err, HttpError::TooLarge);
}

/// A command that calls out, as an app's would.
#[command]
async fn rates(ctx: Ctx) -> elyra::Result<f64> {
    let res = ctx
        .get::<Http>()
        .get("https://api.example.com/rates")
        .query(&[("base", "EUR")])
        .token("t0ken")
        .send()
        .await?;
    Ok(res.json::<Rates>()?.nok)
}

#[tokio::test]
async fn the_fake_answers_a_commands_calls_and_records_them() {
    let fake = HttpFake::new().get("https://api.example.com/rates", 200, json!({ "nok": 11.0 }));
    let app = TestApp::new(
        App::new()
            .commands(commands![rates])
            .swap(Http::fake(fake.clone())),
    );
    assert_eq!(app.invoke_ok::<f64>("rates", ()).await, 11.0);
    let sent = fake.assert_sent("GET", "https://api.example.com/rates");
    assert_eq!(sent.url, "https://api.example.com/rates?base=EUR");
    assert_eq!(sent.header("authorization"), Some("Bearer t0ken"));
    fake.assert_not_sent("POST", "https://api.example.com/rates");

    // Later routes win; `*` matches the rest; no route is a clear error.
    let fake = HttpFake::new()
        .get("https://api.example.com/*", 500, json!({}))
        .get("https://api.example.com/a", 200, json!({ "a": 1 }))
        .unreachable(Method::Get, "https://api.example.com/down");
    let http = Http::fake(fake);
    assert_eq!(
        http.get("https://api.example.com/a")
            .send()
            .await
            .unwrap()
            .status(),
        200
    );
    assert_eq!(
        http.get("https://api.example.com/b")
            .send()
            .await
            .unwrap()
            .status(),
        500
    );
    assert_eq!(
        http.get("https://api.example.com/ab")
            .send()
            .await
            .unwrap()
            .status(),
        500,
        "`/a` is exact: `/ab` falls to `*`"
    );
    assert!(matches!(
        http.get("https://api.example.com/down").send().await,
        Err(HttpError::Unreachable(_))
    ));
    let none = http
        .get("https://other.example.com/")
        .send()
        .await
        .unwrap_err();
    assert!(none
        .to_string()
        .contains("no answer for GET https://other.example.com/"));
    // The HTTPS rule holds for the fake too.
    assert!(matches!(
        http.get("http://api.example.com/a").send().await,
        Err(HttpError::Insecure(_))
    ));
}
