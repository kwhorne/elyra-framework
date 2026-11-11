//! The microphone (RFC 0032) through the shell's `/__mic/*` routes, with a
//! fake input: a recording belongs to the window that started it, stops to
//! a `ChosenFile` holding a 16 kHz mono WAV, tells its window the level,
//! ends by itself at its longest, and becomes text through the container's
//! `Ai` (an `AiFake` here).
#![cfg(feature = "microphone")]

use std::time::Duration;

use elyra::microphone::{Microphone, CHANNEL, SAMPLE_RATE};
use elyra::testing::TestShell;
use elyra::window::WindowConfig;
use elyra::{App, EventBus};
use serde_json::{json, Value};

/// One second of a stereo tone at 48 kHz, half as loud on the right.
fn tone() -> Vec<f32> {
    (0..48_000)
        .flat_map(|i| {
            let s = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.8;
            [s, s * 0.5]
        })
        .collect()
}

fn folder() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("elyra-mic-test-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An app that listens, with windows `main` and `other`.
fn shell_with(mic: Microphone) -> (TestShell, EventBus) {
    let prepared = App::new()
        .microphone("Dictate notes.")
        .swap(mic.folder(folder()))
        .prepare();
    let bus = prepared.bus.clone();
    bus.register_client("main-1");
    let shell = TestShell::new(prepared)
        .window(WindowConfig::new("main"))
        .window(WindowConfig::new("other"));
    (shell, bus)
}

fn shell() -> (TestShell, EventBus) {
    shell_with(Microphone::fake(48_000, 2, tone()))
}

/// `/__mic/<op>` from window `window` as client `client`: the answer as
/// JSON, or the error's message.
async fn mic(
    shell: &TestShell,
    window: &str,
    client: &str,
    op: &str,
    body: Vec<u8>,
) -> Result<Value, String> {
    let request = wry::http::Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__mic/{op}"))
        .header("x-elyra-token", shell.token())
        .header("x-elyra-client-id", client)
        .body(body)
        .unwrap();
    let response = shell.handle_in(window, request).await;
    if response
        .headers()
        .get("x-elyra-status")
        .is_some_and(|v| v == "error")
    {
        return Err(String::from_utf8_lossy(response.body()).into_owned());
    }
    Ok(rmp_serde::from_slice(response.body()).unwrap())
}

fn msgpack(value: Value) -> Vec<u8> {
    rmp_serde::to_vec_named(&value).unwrap()
}

/// `main-1`'s microphone events, until one is of type `until`.
async fn events_until(bus: &EventBus, until: &str) -> Vec<Value> {
    let mut seen = Vec::new();
    for _ in 0..100 {
        let batch = tokio::time::timeout(Duration::from_secs(5), bus.next_batch_for("main-1"))
            .await
            .expect("an event in time");
        let batch: Vec<(String, rmpv::Value)> = rmp_serde::from_slice(&batch).unwrap();
        for (channel, value) in batch {
            if channel != CHANNEL {
                continue;
            }
            let mut buf = Vec::new();
            rmpv::encode::write_value(&mut buf, &value).unwrap();
            let event: Value = rmp_serde::from_slice(&buf).unwrap();
            let done = event["type"] == until;
            seen.push(event);
            if done {
                return seen;
            }
        }
    }
    panic!("never saw {until}: {seen:?}");
}

#[tokio::test]
async fn a_recording_stops_to_a_16_khz_mono_wav() {
    let (shell, bus) = shell();
    assert_eq!(
        mic(&shell, "main", "main-1", "start", vec![]).await,
        Ok(json!(true))
    );
    let events = events_until(&bus, "level").await;
    let level = events.last().unwrap()["level"].as_f64().unwrap();
    assert!((0.7..=0.81).contains(&level), "the tone's peak: {level}");

    let file = mic(&shell, "main", "main-1", "stop", vec![]).await.unwrap();
    assert_eq!(file["kind"], "open");
    let name = file["name"].as_str().unwrap();
    assert!(
        name.starts_with("recording-") && name.ends_with(".wav"),
        "{name}"
    );
    let bytes = std::fs::read(file["path"].as_str().unwrap()).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
    assert_eq!(
        u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
        SAMPLE_RATE
    );
    // One second at 16 kHz, two bytes a sample.
    assert_eq!(bytes.len(), 44 + 32_000);

    // Stopped is stopped.
    let again = mic(&shell, "main", "main-1", "stop", vec![]).await;
    assert!(again.unwrap_err().contains("isn't recording"));
}

#[tokio::test]
async fn one_recording_at_a_time_and_only_its_window_stops_it() {
    let (shell, _bus) = shell();
    mic(&shell, "main", "main-1", "start", vec![])
        .await
        .unwrap();
    let twice = mic(&shell, "other", "other-1", "start", vec![]).await;
    assert!(twice.unwrap_err().contains("already recording"));
    let theirs = mic(&shell, "other", "other-1", "stop", vec![]).await;
    assert!(theirs.unwrap_err().contains("another window"));
    let theirs = mic(&shell, "other", "other-1", "cancel", vec![]).await;
    assert!(theirs.unwrap_err().contains("another window"));

    assert_eq!(
        mic(&shell, "main", "main-1", "cancel", vec![]).await,
        Ok(json!(true))
    );
    // Free again, for any window.
    assert_eq!(
        mic(&shell, "other", "other-1", "start", vec![]).await,
        Ok(json!(true))
    );
}

#[tokio::test]
async fn closing_the_window_drops_its_recording() {
    let (shell, _bus) = shell();
    mic(&shell, "main", "main-1", "start", vec![])
        .await
        .unwrap();
    shell.close("other");
    let still = mic(&shell, "main", "main-1", "stop", vec![]).await;
    assert!(still.is_ok(), "another window closing changes nothing");

    mic(&shell, "main", "main-1", "start", vec![])
        .await
        .unwrap();
    shell.close("main");
    // A window that opens next may record.
    assert_eq!(
        mic(&shell, "other", "other-1", "start", vec![]).await,
        Ok(json!(true))
    );
}

#[tokio::test]
async fn it_stops_by_itself_at_its_longest() {
    let (shell, bus) = shell_with(Microphone::fake(48_000, 2, tone()).max_seconds(1));
    // A call may ask for longer; the microphone's cap wins.
    let started = mic(
        &shell,
        "main",
        "main-1",
        "start",
        msgpack(json!({ "max_seconds": 60 })),
    );
    assert_eq!(started.await, Ok(json!(true)));
    let events = events_until(&bus, "stopped").await;
    let file = &events.last().unwrap()["file"];
    assert!(file["name"].as_str().unwrap().ends_with(".wav"));
    assert!(std::path::Path::new(file["path"].as_str().unwrap()).exists());
    let after = mic(&shell, "main", "main-1", "stop", vec![]).await;
    assert!(after.unwrap_err().contains("isn't recording"));
}

#[tokio::test]
async fn a_microphone_that_cant_be_opened_says_why() {
    let (shell, _bus) = shell_with(Microphone::fake_unavailable(
        "the user didn't allow the microphone",
    ));
    let err = mic(&shell, "main", "main-1", "start", vec![])
        .await
        .unwrap_err();
    assert!(err.contains("didn't allow"), "{err}");
}

#[tokio::test]
async fn without_app_microphone_the_routes_are_closed() {
    let shell = TestShell::new(
        App::new()
            .swap(Microphone::fake(16_000, 1, vec![]))
            .prepare(),
    )
    .window(WindowConfig::new("main"));
    let err = mic(&shell, "main", "main-1", "start", vec![])
        .await
        .unwrap_err();
    assert!(err.contains("Microphone"), "{err}");
}

#[test]
fn the_configuration_is_checked_before_anything_starts() {
    let problems =
        |app: App| -> Vec<String> { app.problems().iter().map(ToString::to_string).collect() };
    assert!(problems(App::new().microphone("Dictate notes.")).is_empty());
    let empty = problems(App::new().microphone("  "));
    assert!(empty.iter().any(|p| p.contains("say why")), "{empty:?}");
    let alone = problems(App::new().microphone_max_seconds(30));
    assert!(
        alone.iter().any(|p| p.contains("nothing records")),
        "{alone:?}"
    );
}

/// What `rata bundle` asks: why the app listens, for `Info.plist`. One
/// test, as the variable is the process's.
#[test]
fn it_tells_rata_bundle_why_it_listens() {
    let out = std::env::temp_dir().join(format!("elyra-mic-inspect-{}.json", std::process::id()));
    let ask = |app: App| {
        std::env::set_var("ELYRA_BUNDLE_INSPECT", &out);
        let result = app.run();
        std::env::remove_var("ELYRA_BUNDLE_INSPECT");
        result.expect("inspect mode must succeed");
        let text = std::fs::read_to_string(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        serde_json::from_str::<Value>(&text).unwrap()
    };
    assert_eq!(
        ask(App::new().microphone("Dictate notes."))["microphone"],
        "Dictate notes."
    );
    assert_eq!(ask(App::new())["microphone"], Value::Null);
}

#[cfg(feature = "ai")]
mod transcribing {
    use super::*;
    use elyra::ai::{Ai, AiFake};

    #[tokio::test]
    async fn a_recording_becomes_text_through_the_containers_ai() {
        let fake = AiFake::new().transcript("Send a reminder on these.");
        let prepared = App::new()
            .microphone("Dictate notes.")
            .swap(Microphone::fake(48_000, 2, tone()).folder(folder()))
            .swap(Ai::fake(fake.clone()))
            .prepare();
        let shell = TestShell::new(prepared).window(WindowConfig::new("main"));
        mic(&shell, "main", "main-1", "start", vec![])
            .await
            .unwrap();
        let file = mic(&shell, "main", "main-1", "stop", vec![]).await.unwrap();

        let text = mic(
            &shell,
            "main",
            "main-1",
            "transcribe",
            msgpack(json!({ "file": file, "language": "no" })),
        )
        .await;
        assert_eq!(text, Ok(json!("Send a reminder on these.")));
        let sent = fake.transcriptions();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].filename, file["name"].as_str().unwrap());
        assert_eq!(sent[0].bytes, 44 + 32_000);
        assert_eq!(sent[0].language.as_deref(), Some("no"));
    }

    #[tokio::test]
    async fn only_a_file_the_user_chose_or_recorded_is_sent() {
        let fake = AiFake::new();
        let prepared = App::new()
            .microphone("Dictate notes.")
            .swap(Microphone::fake(16_000, 1, vec![]))
            .swap(Ai::fake(fake.clone()))
            .prepare();
        let shell = TestShell::new(prepared).window(WindowConfig::new("main"));
        let forged = json!({ "id": "0123456789abcdef0123456789abcdef", "path": "/etc/passwd" });
        let err = mic(
            &shell,
            "main",
            "main-1",
            "transcribe",
            msgpack(json!({ "file": forged })),
        )
        .await
        .unwrap_err();
        assert!(err.contains("not a file the user chose"), "{err}");
        // A path chosen to save to may be written, not read.
        let save = elyra::files::ChosenFile::for_saving(std::env::temp_dir().join("out.wav"));
        let err = mic(
            &shell,
            "main",
            "main-1",
            "transcribe",
            msgpack(json!({ "file": save })),
        )
        .await
        .unwrap_err();
        assert!(err.contains("wasn't chosen to be read"), "{err}");
        assert!(fake.transcriptions().is_empty());
    }

    #[tokio::test]
    async fn with_nothing_to_transcribe_it_says_what_to_add() {
        let prepared = App::new()
            .microphone("Dictate notes.")
            .swap(Microphone::fake(16_000, 1, vec![0.1; 1_600]).folder(folder()))
            .prepare();
        let shell = TestShell::new(prepared).window(WindowConfig::new("main"));
        mic(&shell, "main", "main-1", "start", vec![])
            .await
            .unwrap();
        let file = mic(&shell, "main", "main-1", "stop", vec![]).await.unwrap();
        let err = mic(
            &shell,
            "main",
            "main-1",
            "transcribe",
            msgpack(json!({ "file": file })),
        )
        .await
        .unwrap_err();
        assert!(err.contains("Backend::transcription()"), "{err}");
    }
}
