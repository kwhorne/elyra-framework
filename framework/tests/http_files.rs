//! Files over `Http` (RFC 0009 step 1) against real sockets: multipart
//! uploads streamed from disk, downloads streamed to it, progress, and the
//! timeout that counts the time without progress.
#![cfg(feature = "http")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use elyra::http::{Http, HttpError, HttpFake, Progress};
use serde::Serialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// What the server received.
#[derive(Debug, Clone)]
struct Seen {
    line: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// The multipart parts: (headers, content).
    fn parts(&self) -> Vec<(String, Vec<u8>)> {
        let content_type = self.header("content-type").unwrap_or_default();
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .expect("a multipart boundary");
        let delimiter = format!("--{boundary}");
        split(&self.body, delimiter.as_bytes())
            .into_iter()
            .skip(1)
            .filter(|p| !p.starts_with(b"--"))
            .map(|p| {
                let p = p.strip_prefix(b"\r\n").unwrap_or(p);
                let end = find(p, b"\r\n\r\n").expect("part headers");
                let content = &p[end + 4..];
                let content = content.strip_suffix(b"\r\n").unwrap_or(content);
                (
                    String::from_utf8_lossy(&p[..end]).into_owned(),
                    content.to_vec(),
                )
            })
            .collect()
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn split<'a>(mut haystack: &'a [u8], needle: &[u8]) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    while let Some(at) = find(haystack, needle) {
        out.push(&haystack[..at]);
        haystack = &haystack[at + needle.len()..];
    }
    out.push(haystack);
    out
}

/// How the server answers.
#[derive(Clone)]
enum Reply {
    /// These bytes, at once.
    Raw(Vec<u8>),
    /// A head, then each chunk after its pause; the connection then closes.
    Slow(String, Vec<(Duration, Vec<u8>)>),
    /// Never answer (and never read the body).
    Hang,
}

/// A server on 127.0.0.1 that answers the n-th request with `script[n]` (the
/// last one from then on).
async fn server(script: Vec<Reply>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        let mut n = 0;
        while let Ok((stream, _)) = listener.accept().await {
            let reply = script[n.min(script.len() - 1)].clone();
            n += 1;
            let record = record.clone();
            tokio::spawn(async move { serve(stream, reply, record).await });
        }
    });
    (base, seen)
}

async fn serve(mut stream: TcpStream, reply: Reply, record: Arc<Mutex<Vec<Seen>>>) {
    if let Reply::Hang = reply {
        tokio::time::sleep(Duration::from_secs(60)).await;
        return;
    }
    let mut buf = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    let head_end = loop {
        let Ok(read) = stream.read(&mut chunk).await else {
            return;
        };
        if read == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..read]);
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let line = lines.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect();
    let header = |name: &str| {
        headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    };
    let mut body = buf[head_end + 4..].to_vec();
    if let Some(length) = header("content-length").and_then(|v| v.parse::<usize>().ok()) {
        while body.len() < length {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => body.extend_from_slice(&chunk[..read]),
            }
        }
    } else if header("transfer-encoding").is_some_and(|v| v.contains("chunked")) {
        while find(&body, b"0\r\n\r\n").is_none() {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(read) => body.extend_from_slice(&chunk[..read]),
            }
        }
        body = dechunk(&body);
    }
    record.lock().unwrap().push(Seen {
        line,
        headers,
        body,
    });
    match reply {
        Reply::Raw(bytes) => {
            let _ = stream.write_all(&bytes).await;
        }
        Reply::Slow(head, chunks) => {
            let _ = stream.write_all(head.as_bytes()).await;
            for (pause, bytes) in chunks {
                tokio::time::sleep(pause).await;
                if stream.write_all(&bytes).await.is_err() {
                    return;
                }
                let _ = stream.flush().await;
            }
        }
        Reply::Hang => unreachable!(),
    }
    let _ = stream.shutdown().await;
}

fn dechunk(mut body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(end) = find(body, b"\r\n") {
        let size = usize::from_str_radix(&String::from_utf8_lossy(&body[..end]), 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        out.extend_from_slice(&body[end + 2..end + 2 + size]);
        body = &body[end + 2 + size + 2..];
    }
    out
}

fn reply(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body);
    Reply::Raw(bytes)
}

fn json_reply(status: &str, body: serde_json::Value) -> Reply {
    reply(
        status,
        &[("Content-Type", "application/json")],
        body.to_string().as_bytes(),
    )
}

/// A fresh folder for a test's files.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("elyra-http-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn recorded() -> (
    Arc<Mutex<Vec<Progress>>>,
    impl Fn(Progress) + Send + Sync + 'static,
) {
    let progress = Arc::new(Mutex::new(Vec::new()));
    let into = progress.clone();
    (progress, move |p| into.lock().unwrap().push(p))
}

#[derive(Serialize)]
struct Expense {
    amount: f64,
    paid: bool,
    note: Option<String>,
    tags: Vec<&'static str>,
}

#[tokio::test]
async fn a_file_is_sent_as_multipart_with_fields_and_progress() {
    let dir = folder("upload");
    let receipt = dir.join("receipt.pdf");
    let contents: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&receipt, &contents).unwrap();
    let (base, seen) = server(vec![json_reply("201 Created", json!({ "id": 1 }))]).await;
    let (progress, report) = recorded();

    let res = Http::new()
        .post(format!("{base}/api/expenses"))
        .attach("receipt", &receipt)
        .attach_bytes("thumb", vec![1u8, 2, 3], "thumb.png", "image/png")
        .field("currency", "NOK")
        .fields(&Expense {
            amount: 129.5,
            paid: true,
            note: None,
            tags: vec!["travel", "taxi"],
        })
        .on_progress(report)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    assert_eq!(res.saved_to(), None);

    let seen = seen.lock().unwrap()[0].clone();
    assert_eq!(seen.line, "POST /api/expenses HTTP/1.1");
    assert!(seen
        .header("content-type")
        .unwrap()
        .starts_with("multipart/form-data; boundary="));
    // A known length, which PHP and nginx prefer to chunks.
    assert!(
        seen.header("content-length").is_some(),
        "{:?}",
        seen.headers
    );
    let parts = seen.parts();
    let part = |name: &str| {
        parts
            .iter()
            .find(|(h, _)| h.contains(&format!("name=\"{name}\"")))
            .unwrap_or_else(|| panic!("no part {name}: {parts:?}"))
            .clone()
    };
    let (headers, body) = part("receipt");
    assert!(headers.contains("filename=\"receipt.pdf\""), "{headers}");
    assert!(
        headers.contains("Content-Type: application/pdf"),
        "{headers}"
    );
    assert_eq!(body, contents);
    let (headers, body) = part("thumb");
    assert!(headers.contains("filename=\"thumb.png\"") && headers.contains("image/png"));
    assert_eq!(body, [1, 2, 3]);
    assert_eq!(part("currency").1, b"NOK");
    assert_eq!(part("amount").1, b"129.5");
    assert_eq!(part("paid").1, b"1");
    let tags: Vec<_> = parts
        .iter()
        .filter(|(h, _)| h.contains("name=\"tags[]\""))
        .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
        .collect();
    assert_eq!(tags, ["travel", "taxi"]);
    assert!(!parts.iter().any(|(h, _)| h.contains("name=\"note\"")));

    let progress = progress.lock().unwrap();
    let total = contents.len() as u64 + 3;
    assert!(progress.len() > 2, "reported as it went: {progress:?}");
    assert!(progress.iter().all(|p| p.total == Some(total)));
    assert_eq!(progress.last().unwrap().done, total);
    assert!(progress.windows(2).all(|w| w[0].done <= w[1].done));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_missing_file_is_a_file_error_and_nothing_is_sent() {
    let (base, seen) = server(vec![json_reply("201 Created", json!({}))]).await;
    let err = Http::new()
        .post(format!("{base}/api/expenses"))
        .attach("receipt", "/no/such/folder/receipt.pdf")
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HttpError::File(m) if m.starts_with("receipt.pdf:")),
        "{err:?}"
    );
    // The folder isn't in the message: it can name the user.
    assert!(!err.to_string().contains("/no/such"), "{err}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_file_that_changes_while_it_is_sent_is_refused() {
    let dir = folder("changes");
    let file = dir.join("growing.log");
    std::fs::write(&file, vec![b'a'; 500_000]).unwrap();
    let (base, _seen) = server(vec![json_reply("201 Created", json!({}))]).await;
    let append_to = file.clone();
    let err = Http::new()
        .post(format!("{base}/api/logs"))
        .attach("log", &file)
        .on_progress(move |p| {
            // Another program writes to it as it's being sent.
            if p.done < 100_000 {
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&append_to)
                    .unwrap();
                f.write_all(b"more").unwrap();
            }
        })
        .send()
        .await
        .unwrap_err();
    assert_eq!(err, HttpError::FileChanged("growing.log".into()));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_retry_sends_the_whole_file_again() {
    let dir = folder("retry");
    let file = dir.join("scan.jpg");
    std::fs::write(&file, vec![7u8; 100_000]).unwrap();
    let (base, seen) = server(vec![
        json_reply("503 Service Unavailable", json!({})),
        json_reply("201 Created", json!({ "id": 2 })),
    ])
    .await;
    let res = Http::new()
        .post(format!("{base}/api/scans"))
        .attach("scan", &file)
        .header("Idempotency-Key", "k1")
        .retry(1, Duration::ZERO)
        .retry_any_method()
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), 201);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    for s in seen.iter() {
        let (headers, body) = s.parts().remove(0);
        assert!(headers.contains("image/jpeg"), "{headers}");
        assert_eq!(body.len(), 100_000);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_json_body_and_parts_cant_be_mixed() {
    let err = Http::new()
        .post("http://127.0.0.1:9/x")
        .json(&json!({ "a": 1 }))
        .field("b", "2")
        .send()
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::Encode(m) if m.contains("not both")));
}

#[tokio::test]
async fn a_download_is_streamed_to_the_chosen_file() {
    let dir = folder("download-to");
    let target = dir.join("invoice.pdf");
    std::fs::write(&target, "the old one").unwrap();
    let pdf: Vec<u8> = (0..200_000u32).map(|i| (i % 97) as u8).collect();
    let (base, seen) = server(vec![reply(
        "200 OK",
        &[("Content-Type", "application/pdf")],
        &pdf,
    )])
    .await;
    let (progress, report) = recorded();
    let res = Http::new()
        .get(format!("{base}/api/invoices/7/pdf"))
        .on_progress(report)
        .download_to(&target)
        .await
        .unwrap();
    assert!(res.ok());
    assert_eq!(res.saved_to(), Some(target.as_path()));
    assert!(res.bytes().is_empty(), "not kept in memory");
    assert_eq!(std::fs::read(&target).unwrap(), pdf);
    assert_eq!(names(&dir), ["invoice.pdf"], "no .part left behind");
    let progress = progress.lock().unwrap();
    assert_eq!(
        progress.last(),
        Some(&Progress {
            done: pdf.len() as u64,
            total: Some(pdf.len() as u64)
        })
    );
    assert_eq!(
        seen.lock().unwrap()[0].header("accept"),
        Some("application/json")
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_download_into_a_folder_takes_the_servers_name_cleaned_and_numbered() {
    let dir = folder("download-into");
    std::fs::write(dir.join("Faktura 7.pdf"), "already here").unwrap();
    let (base, _seen) = server(vec![
        reply(
            "200 OK",
            &[(
                "Content-Disposition",
                "attachment; filename=\"x.pdf\"; filename*=UTF-8''Faktura%207.pdf",
            )],
            b"new",
        ),
        reply(
            "200 OK",
            &[(
                "Content-Disposition",
                "attachment; filename=\"../../.ssh/authorized_keys\"",
            )],
            b"key",
        ),
        reply("200 OK", &[], b"by url"),
    ])
    .await;
    let http = Http::new();
    let first = http
        .get(format!("{base}/api/invoices/7/pdf"))
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(
        first.saved_to(),
        Some(dir.join("Faktura 7 (1).pdf").as_path())
    );
    assert_eq!(
        std::fs::read(dir.join("Faktura 7.pdf")).unwrap(),
        b"already here"
    );
    let second = http
        .get(format!("{base}/api/keys"))
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(
        second.saved_to(),
        Some(dir.join("authorized_keys").as_path())
    );
    let third = http
        .get(format!("{base}/exports/report%20q3.csv?token=abc"))
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(third.saved_to(), Some(dir.join("report q3.csv").as_path()));
    assert_eq!(
        names(&dir),
        [
            "Faktura 7 (1).pdf",
            "Faktura 7.pdf",
            "authorized_keys",
            "report q3.csv"
        ]
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_answer_that_isnt_a_success_isnt_saved() {
    let dir = folder("not-found");
    let (base, _seen) = server(vec![json_reply(
        "404 Not Found",
        json!({ "message": "No such invoice." }),
    )])
    .await;
    let res = Http::new()
        .get(format!("{base}/api/invoices/9/pdf"))
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(res.saved_to(), None);
    assert_eq!(
        res.json::<serde_json::Value>().unwrap()["message"],
        "No such invoice."
    );
    assert!(names(&dir).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_download_cut_short_leaves_nothing() {
    let dir = folder("cut");
    let target = dir.join("big.zip");
    std::fs::write(&target, "the old one").unwrap();
    // Says 100 000 bytes, sends 10, and closes.
    let (base, _seen) = server(vec![Reply::Slow(
        "HTTP/1.1 200 OK\r\nContent-Length: 100000\r\nConnection: close\r\n\r\n".into(),
        vec![(Duration::ZERO, vec![b'z'; 10])],
    )])
    .await;
    let err = Http::new()
        .get(format!("{base}/big.zip"))
        .download_to(&target)
        .await
        .unwrap_err();
    assert!(matches!(err, HttpError::Unreachable(_)), "{err:?}");
    assert_eq!(std::fs::read(&target).unwrap(), b"the old one", "untouched");
    assert_eq!(names(&dir), ["big.zip"], "no .part left behind");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_slow_download_that_keeps_going_isnt_timed_out() {
    let dir = folder("slow");
    // Eight chunks 100 ms apart: longer than the timeout in all, never
    // quiet for as long.
    let chunks = (0..8)
        .map(|_| (Duration::from_millis(100), vec![b's'; 1000]))
        .collect();
    let (base, _seen) = server(vec![Reply::Slow(
        "HTTP/1.1 200 OK\r\nContent-Length: 8000\r\nConnection: close\r\n\r\n".into(),
        chunks,
    )])
    .await;
    let res = Http::new()
        .get(format!("{base}/slow.bin"))
        .timeout(Duration::from_millis(400))
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(
        std::fs::metadata(res.saved_to().unwrap()).unwrap().len(),
        8000
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn an_upload_with_no_answer_times_out_after_the_quiet_time() {
    let dir = folder("quiet");
    let file = dir.join("a.txt");
    std::fs::write(&file, "small").unwrap();
    let (base, _seen) = server(vec![Reply::Hang]).await;
    let started = std::time::Instant::now();
    let err = Http::new()
        .post(format!("{base}/api/a"))
        .attach("a", &file)
        .timeout(Duration::from_millis(300))
        .send()
        .await
        .unwrap_err();
    assert!(
        matches!(&err, HttpError::Unreachable(m) if m.contains("no progress")),
        "{err:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_fake_records_parts_and_answers_downloads() {
    let dir = folder("fake");
    let file = dir.join("contract.pdf");
    std::fs::write(&file, "%PDF-1.7").unwrap();
    let fake = HttpFake::new()
        .post(
            "https://crm.test/api/customers/7/attachments",
            201,
            json!({ "id": 1 }),
        )
        .get_bytes(
            "https://crm.test/api/invoices/7/pdf",
            200,
            b"%PDF".to_vec(),
            "invoice-7.pdf",
        );
    let http = Http::fake(fake.clone());
    let (progress, report) = recorded();
    http.post("https://crm.test/api/customers/7/attachments")
        .attach("file", &file)
        .field("note", "Signed copy")
        .on_progress(report)
        .send()
        .await
        .unwrap();
    let sent = fake.assert_sent("POST", "https://crm.test/api/customers/7/attachments");
    let part = sent.part("file").unwrap();
    assert_eq!(part.file_name.as_deref(), Some("contract.pdf"));
    assert_eq!(part.content_type.as_deref(), Some("application/pdf"));
    assert_eq!(part.bytes, b"%PDF-1.7");
    assert_eq!(sent.part("note").unwrap().text(), "Signed copy");
    assert_eq!(
        progress.lock().unwrap().last(),
        Some(&Progress {
            done: 8,
            total: Some(8)
        })
    );

    let res = http
        .get("https://crm.test/api/invoices/7/pdf")
        .download_into(&dir)
        .await
        .unwrap();
    assert_eq!(res.saved_to(), Some(dir.join("invoice-7.pdf").as_path()));
    assert_eq!(std::fs::read(dir.join("invoice-7.pdf")).unwrap(), b"%PDF");
    let _ = std::fs::remove_dir_all(dir);
}
