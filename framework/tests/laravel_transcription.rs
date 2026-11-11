//! RFC 0032 against a real Laravel app (scripts/laravel-backend.sh) with this
//! repository's elyracode/elyra-backend: a recording becomes text through
//! `Route::elyraTranscription()`, which adds the server's OpenAI key and caps
//! what may be sent. OpenAI is a local stand-in at
//! `ELYRA_TRANSCRIPTION_UPSTREAM`, where the app's .env points
//! `ELYRA_TRANSCRIPTION_UPSTREAM`. Skipped unless `ELYRA_LARAVEL_URL` and
//! `ELYRA_TRANSCRIPTION_UPSTREAM` say where they are; CI starts them.
#![cfg(all(feature = "ai", feature = "backend", feature = "microphone"))]

use std::sync::Arc;

use elyra::ai::{Ai, Error as AiError, Provider};
use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::files::ChosenFile;
use elyra::http::Http;
use elyra::testing::TestApp;
use elyra::App;
use parking_lot::Mutex;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request as the stand-in for OpenAI saw it.
struct Seen {
    authorization: Option<String>,
    body: Vec<u8>,
}

impl Seen {
    /// A multipart field's value, as sent.
    fn field(&self, name: &str) -> Option<String> {
        let body = String::from_utf8_lossy(&self.body);
        let marker = format!("name=\"{name}\"");
        let at = body.find(&marker)?;
        let rest = &body[at..];
        let start = rest.find("\r\n\r\n")? + 4;
        let end = rest[start..].find("\r\n")?;
        Some(rest[start..start + end].to_owned())
    }

    fn has(&self, bytes: &[u8]) -> bool {
        self.body.windows(bytes.len()).any(|w| w == bytes)
    }
}

/// OpenAI's transcription endpoint, at `address`: it answers each request
/// with the text it was told to.
async fn openai(address: &str, text: &'static str) -> Arc<Mutex<Vec<Seen>>> {
    let listener = TcpListener::bind(address).await.expect("the upstream port");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let record = record.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let end = loop {
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break i + 4;
                    }
                    let mut chunk = [0u8; 8192];
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&buf[..end]).to_string();
                let header = |name: &str| {
                    head.lines()
                        .skip(1)
                        .filter_map(|l| l.split_once(':'))
                        .find(|(k, _)| k.trim().eq_ignore_ascii_case(name))
                        .map(|(_, v)| v.trim().to_owned())
                };
                let length: usize = header("content-length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                while buf.len() < end + length {
                    let mut chunk = [0u8; 8192];
                    match socket.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                    }
                }
                record.lock().push(Seen {
                    authorization: header("authorization"),
                    body: buf[end..end + length].to_vec(),
                });
                let answer = json!({ "text": text }).to_string();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    seen
}

/// A short recording, as the microphone writes them.
fn recording(seconds: f32) -> ChosenFile {
    let samples = vec![0.25f32; (16_000.0 * seconds) as usize];
    let path = std::env::temp_dir().join(format!(
        "elyra-transcription-{}-{seconds}.wav",
        std::process::id()
    ));
    std::fs::write(&path, elyra::microphone::wav(&samples, 16_000, 1)).unwrap();
    ChosenFile::from_path(path)
}

/// A Sanctum token for `email`, as the desktop gets one.
async fn token(url: &str, email: &str) -> String {
    let response = Http::new()
        .post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": email, "password": "secret", "device_name": "tests" }))
        .send()
        .await
        .unwrap();
    response.text().trim_matches('"').to_owned()
}

/// The proxy, as `Backend::transcription()` reaches it, with `token`.
fn through_laravel(url: &str, token: String) -> Ai {
    Ai::builder()
        .base_url(Provider::OpenAI, format!("{url}/api/elyra/transcription"))
        .bearer(Provider::OpenAI, move || Some(token.clone()))
        .retries(0)
        .build()
}

fn status(err: AiError) -> u16 {
    match err {
        AiError::Api { status, .. } => status,
        other => panic!("not an API error: {other:?}"),
    }
}

/// One test, in order: the steps share the user's per-minute allowance.
#[tokio::test]
async fn a_recording_becomes_text_through_laravel() {
    let (Ok(url), Ok(upstream)) = (
        std::env::var("ELYRA_LARAVEL_URL"),
        std::env::var("ELYRA_TRANSCRIPTION_UPSTREAM"),
    ) else {
        eprintln!("skipped: ELYRA_LARAVEL_URL / ELYRA_TRANSCRIPTION_UPSTREAM not set");
        return;
    };
    let seen = openai(&upstream, "Send a reminder on these.").await;

    let app = TestApp::new(
        App::new()
            .backend(Backend::new(&url).transcription())
            .token_store(Arc::new(MemoryTokens::new())),
    );
    let file = recording(0.5);

    // Signed out, nothing is sent.
    let signed_out = elyra::microphone::transcribe(app.ctx(), &file, None).await;
    assert!(signed_out.is_err());
    assert!(seen.lock().is_empty());

    app.get::<Auth>()
        .sign_in("bob@example.com", "secret")
        .await
        .unwrap();
    let text = elyra::microphone::transcribe(app.ctx(), &file, Some("no")).await;
    assert_eq!(text.as_deref(), Ok("Send a reminder on these."));
    {
        let seen = seen.lock();
        assert_eq!(seen.len(), 1);
        let sent = &seen[0];
        // The server's key went to OpenAI, not the user's token.
        assert_eq!(
            sent.authorization.as_deref(),
            Some("Bearer sk-openai-server-test")
        );
        assert_eq!(sent.field("model").as_deref(), Some("whisper-1"));
        assert_eq!(sent.field("language").as_deref(), Some("no"));
        assert!(sent.has(b"RIFF"), "the WAV itself");
        assert!(sent.has(file.name().as_bytes()), "under its own name");
    }

    // What isn't allowed is refused before OpenAI hears of it.
    let ai = through_laravel(&url, token(&url, "bob@example.com").await);
    let other_model = ai
        .transcribe(std::fs::read(file.path()).unwrap(), "x.wav")
        .model("gpt-5")
        .generate()
        .await
        .unwrap_err();
    assert_eq!(status(other_model), 422);
    // Over the server's 64 KB (two seconds is 64 KB and a header).
    let large = recording(2.5);
    let too_large = ai
        .transcribe(std::fs::read(large.path()).unwrap(), "long.wav")
        .generate()
        .await
        .unwrap_err();
    assert_eq!(status(too_large), 413);
    assert_eq!(seen.lock().len(), 1);

    // Four a minute: one more, then no.
    ai.transcribe(std::fs::read(file.path()).unwrap(), "x.wav")
        .generate()
        .await
        .unwrap();
    let busy = ai
        .transcribe(std::fs::read(file.path()).unwrap(), "x.wav")
        .generate()
        .await
        .unwrap_err();
    assert_eq!(status(busy), 429);
    assert_eq!(seen.lock().len(), 2);
}
