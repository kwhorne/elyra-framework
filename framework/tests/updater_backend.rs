//! Updates from the backend (RFC 0024): `UpdaterConfig::from_backend(..)`
//! asks the active server, as the signed-in user, and downloads from that
//! server's origin alone. An `HttpFake` answers both servers; a fake bundle
//! system stands in for `ditto` and `codesign`.
#![cfg(all(feature = "backend", feature = "updater"))]

#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use elyra::auth::{Auth, MemoryTokens};
use elyra::backend::Backend;
use elyra::http::{Http, HttpFake};
use elyra::testing::TestApp;
#[cfg(target_os = "macos")]
use elyra::updater::bundle::{BundleInfo, BundleSystem, CodeSignature, DeveloperId};
use elyra::updater::{Error, UpdateStatus, Updater, UpdaterConfig, UpdaterRuntime};
use elyra::App;
use serde_json::{json, Value};
use sha2::Digest;

const ACME: &str = "https://acme.felagi.test";
const BETA: &str = "https://beta.felagi.test";
const ROUTE: &str = "/api/desktop/update";
const TEAM: &str = "52AT44QFCM";
const BUNDLE: &str = "works.felagi.desktop";

fn at(server: &str, path: &str) -> String {
    format!("{server}{path}")
}

fn sha(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The zip a release would be (only its magic matters to the fake system).
fn artifact() -> Vec<u8> {
    let mut bytes = b"PK\x03\x04".to_vec();
    bytes.extend((0..4096).map(|i| (i % 251) as u8));
    bytes
}

/// What Félagi's `/api/desktop/update` answers for a newer release.
fn release(server: &str, url: &str, bytes: &[u8]) -> Value {
    json!({
        "version": "2.0.0",
        "notes": format!("- from {server}"),
        "released_at": "2026-11-06",
        "url": url,
        "size": bytes.len(),
        "sha256": sha(bytes),
    })
}

fn config() -> UpdaterConfig {
    UpdaterConfig::from_backend(ROUTE, "1.0.0").developer_id(TEAM, BUNDLE)
}

fn app(fake: &HttpFake) -> TestApp {
    let app = TestApp::new(
        App::new()
            .backend(Backend::chosen_at_sign_in().multiple_servers())
            .token_store(Arc::new(MemoryTokens::new()))
            .updater(config())
            .swap(Http::fake(fake.clone())),
    );
    app.listen();
    app
}

async fn sign_in(app: &TestApp, server: &str, token: &str) {
    app.get::<Auth>()
        .sign_in_with_token(server, token, json!({ "id": 7 }))
        .await
        .unwrap();
}

fn updater(app: &TestApp) -> Arc<UpdaterRuntime> {
    app.get::<UpdaterRuntime>()
}

fn update_requests(fake: &HttpFake, server: &str) -> Vec<elyra::http::Sent> {
    fake.sent_to("GET", &at(server, ROUTE))
}

#[tokio::test]
async fn the_manifest_is_asked_of_the_active_server_with_the_users_token() {
    let bytes = artifact();
    let fake = HttpFake::new().get(
        at(ACME, ROUTE),
        200,
        release(ACME, &at(ACME, "/desktop/Felagi-2.0.0.zip"), &bytes),
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;

    let UpdateStatus::Available(info) = updater(&app).check().await.unwrap() else {
        panic!("expected an update");
    };
    assert_eq!(info.version, "2.0.0");
    assert_eq!(info.url, at(ACME, "/desktop/Felagi-2.0.0.zip"));
    assert_eq!(info.sha256.as_deref(), Some(sha(&bytes).as_str()));
    assert_eq!(info.size, Some(bytes.len() as u64));
    assert_eq!(info.released_at.as_deref(), Some("2026-11-06"));

    let sent = update_requests(&fake, ACME);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].header("Authorization"), Some("Bearer acme-token"));
    let url = reqwest_url(&sent[0].url);
    let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    assert!(query.contains(&("platform".into(), Updater::current_target())));
    assert!(query.contains(&("version".into(), "1.0.0".into())));
}

fn reqwest_url(url: &str) -> reqwest::Url {
    reqwest::Url::parse(url).unwrap()
}

#[tokio::test]
async fn no_content_is_up_to_date() {
    let fake = HttpFake::new().get(at(ACME, ROUTE), 204, Value::Null);
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    assert!(matches!(
        updater(&app).check().await.unwrap(),
        UpdateStatus::UpToDate
    ));
    let check = updater(&app).check_for_frontend().await;
    assert!(!check.available);
    assert!(check.error.is_none());
}

#[tokio::test]
async fn a_release_no_newer_than_this_one_is_up_to_date() {
    let fake = HttpFake::new().get(
        at(ACME, ROUTE),
        200,
        json!({ "data": { "version": "1.0.0", "url": "/x.zip", "sha256": "0".repeat(64) } }),
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    assert!(matches!(
        updater(&app).check().await.unwrap(),
        UpdateStatus::UpToDate
    ));
}

#[tokio::test]
async fn signed_out_nothing_is_asked() {
    let fake = HttpFake::new().get(at(ACME, ROUTE), 204, Value::Null);
    let app = app(&fake);
    assert!(!updater(&app).can_check());
    assert!(matches!(updater(&app).check().await, Err(Error::SignedOut)));
    assert!(fake.sent().is_empty(), "nothing may be sent signed out");
    let check = updater(&app).check_for_frontend().await;
    assert!(!check.available);
    assert!(check.error.unwrap().contains("not signed in"));
    // The schedule's check says nothing at all.
    updater(&app).announce(true).await;
    app.assert_not_emitted("elyra:update").await;
}

#[tokio::test]
async fn a_download_on_another_origin_is_refused_and_the_token_stays_home() {
    let bytes = artifact();
    for foreign in [
        "https://cdn.evil.test/Felagi-2.0.0.zip",
        "//cdn.evil.test/Felagi-2.0.0.zip",
        "http://acme.felagi.test/Felagi-2.0.0.zip",
        "https://acme.felagi.test:8443/Felagi-2.0.0.zip",
    ] {
        let fake = HttpFake::new()
            .get(at(ACME, ROUTE), 200, release(ACME, foreign, &bytes))
            .get_bytes(foreign, 200, bytes.clone(), "Felagi.zip");
        let app = app(&fake);
        sign_in(&app, ACME, "acme-token").await;
        let err = updater(&app).check().await.unwrap_err();
        assert!(matches!(err, Error::ForeignOrigin(_)), "{foreign}: {err}");
        assert_eq!(
            fake.sent().len(),
            1,
            "{foreign}: only the manifest was asked"
        );
    }
}

#[tokio::test]
async fn the_artifact_is_downloaded_with_the_token_and_verified() {
    let bytes = artifact();
    let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
    let fake = HttpFake::new()
        .get(
            at(ACME, ROUTE),
            200,
            release(ACME, "/desktop/Felagi-2.0.0.zip", &bytes),
        )
        .get_bytes(&url, 200, bytes.clone(), "Felagi-2.0.0.zip");
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let UpdateStatus::Available(info) = updater(&app).check().await.unwrap() else {
        panic!("expected an update");
    };
    let progress = Arc::new(Mutex::new(Vec::new()));
    let seen = progress.clone();
    let staged = updater(&app)
        .download(&info, move |got, total| {
            seen.lock().unwrap().push((got, total))
        })
        .await
        .unwrap();
    assert_eq!(std::fs::read(&staged).unwrap(), bytes);
    assert_eq!(
        progress.lock().unwrap().last(),
        Some(&(bytes.len() as u64, Some(bytes.len() as u64)))
    );
    let sent = fake.assert_sent("GET", &url);
    assert_eq!(sent.header("Authorization"), Some("Bearer acme-token"));
    let _ = std::fs::remove_dir_all(staged.parent().unwrap());
}

#[tokio::test]
async fn a_sha_mismatch_is_refused_and_nothing_is_left() {
    // Bytes of this test's own, to look for afterwards.
    let mut bytes = artifact();
    bytes.extend_from_slice(b"a_sha_mismatch_is_refused_and_nothing_is_left");
    let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
    let mut answer = release(ACME, &url, &bytes);
    answer["sha256"] = json!(sha(b"something else"));
    let fake = HttpFake::new().get(at(ACME, ROUTE), 200, answer).get_bytes(
        &url,
        200,
        bytes.clone(),
        "Felagi-2.0.0.zip",
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let UpdateStatus::Available(info) = updater(&app).check().await.unwrap() else {
        panic!("expected an update");
    };
    assert!(matches!(
        updater(&app).download(&info, |_, _| {}).await,
        Err(Error::Digest)
    ));
    assert!(fake.sent_to("GET", &url).len() == 1, "it was downloaded");
    assert!(
        !staged_anywhere(&bytes),
        "the unverified download is removed"
    );
}

#[tokio::test]
async fn a_download_of_another_size_than_the_server_said_is_refused() {
    let bytes = artifact();
    let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
    let mut answer = release(ACME, &url, &bytes);
    answer["size"] = json!(bytes.len() + 1);
    let fake = HttpFake::new().get(at(ACME, ROUTE), 200, answer).get_bytes(
        &url,
        200,
        bytes.clone(),
        "Felagi-2.0.0.zip",
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let UpdateStatus::Available(info) = updater(&app).check().await.unwrap() else {
        panic!("expected an update");
    };
    assert!(matches!(
        updater(&app).download(&info, |_, _| {}).await,
        Err(Error::SizeMismatch { .. })
    ));
}

#[tokio::test]
async fn after_a_switch_the_new_active_server_is_asked_with_its_token() {
    let bytes = artifact();
    let fake = HttpFake::new().get(at(ACME, ROUTE), 204, Value::Null).get(
        at(BETA, ROUTE),
        200,
        release(BETA, "/desktop/Felagi-2.0.0.zip", &bytes),
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    sign_in(&app, BETA, "beta-token").await; // added, and active

    let UpdateStatus::Available(info) = updater(&app).check().await.unwrap() else {
        panic!("expected Beta's update");
    };
    assert_eq!(info.url, at(BETA, "/desktop/Felagi-2.0.0.zip"));
    assert_eq!(
        update_requests(&fake, BETA)[0].header("Authorization"),
        Some("Bearer beta-token")
    );
    assert!(update_requests(&fake, ACME).is_empty());

    app.get::<Auth>().switch_to(ACME).await.unwrap();
    assert!(matches!(
        updater(&app).check().await.unwrap(),
        UpdateStatus::UpToDate
    ));
    assert_eq!(
        update_requests(&fake, ACME)[0].header("Authorization"),
        Some("Bearer acme-token")
    );
}

#[tokio::test]
async fn the_schedule_offers_a_release_once_and_withdraws_it_after_a_sign_out() {
    let bytes = artifact();
    let fake = HttpFake::new().get(
        at(ACME, ROUTE),
        200,
        release(ACME, "/desktop/Felagi-2.0.0.zip", &bytes),
    );
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let _ = app.drain_events().await;

    updater(&app).announce(true).await;
    let phases: Vec<Phase> = app.events_on("elyra:update").await;
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].phase, "available");
    assert_eq!(phases[0].version.as_deref(), Some("2.0.0"));
    assert_eq!(phases[0].restart.as_deref(), Some("ask"));

    // A sign-in elsewhere wakes it, but the same version isn't offered twice…
    updater(&app).announce(false).await;
    app.assert_not_emitted("elyra:update").await;
    // …and once signed out it's withdrawn, without asking anyone.
    app.get::<Auth>().sign_out().await.ok();
    let asked = update_requests(&fake, ACME).len();
    updater(&app).announce(false).await;
    let phases: Vec<Phase> = app.events_on("elyra:update").await;
    assert_eq!(phases.last().map(|p| p.phase.as_str()), Some("up-to-date"));
    assert_eq!(update_requests(&fake, ACME).len(), asked);
}

#[tokio::test]
async fn installing_from_a_build_is_refused_before_anything_is_downloaded() {
    let bytes = artifact();
    let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
    let fake = HttpFake::new()
        .get(at(ACME, ROUTE), 200, release(ACME, &url, &bytes))
        .get_bytes(&url, 200, bytes, "Felagi.zip");
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let _ = app.drain_events().await;

    // The test binary runs from target/debug/deps: a build.
    let err = updater(&app).install().await.unwrap_err();
    assert!(matches!(err, Error::NotInstallable(_)), "{err}");
    assert!(fake.sent_to("GET", &url).is_empty());
    let phases: Vec<Phase> = app.events_on("elyra:update").await;
    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].phase, "error");
    assert!(phases[0]
        .message
        .as_deref()
        .unwrap()
        .contains("started from a build"));
    assert!(matches!(
        updater(&app).relaunch(),
        Err(Error::NothingInstalled)
    ));
}

#[derive(serde::Deserialize, Debug)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
struct Phase {
    phase: String,
    version: Option<String>,
    message: Option<String>,
    restart: Option<String>,
    progress: Option<u8>,
}

/// Whether a file with exactly `bytes` is in any of the update's staging
/// directories.
fn staged_anywhere(bytes: &[u8]) -> bool {
    let Ok(dir) = std::fs::read_dir(std::env::temp_dir()) else {
        return false;
    };
    dir.flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("elyra-update-"))
        .filter_map(|e| std::fs::read_dir(e.path()).ok())
        .flat_map(|files| files.flatten())
        .any(|f| std::fs::read(f.path()).is_ok_and(|b| b == bytes))
}

/// `ditto` and `codesign`, faked: the archive expands to a bundle of the
/// given identifier and version, signed as the test says.
#[cfg(target_os = "macos")]
struct FakeMac {
    signature: CodeSignature,
    relaunched: Mutex<Option<PathBuf>>,
}

#[cfg(target_os = "macos")]
fn write_app(dir: &Path, identifier: &str, version: &str) -> PathBuf {
    let app = dir.join("Félagi.app");
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::write(
        app.join("Contents/Info.plist"),
        format!("{identifier}\n{version}"),
    )
    .unwrap();
    app
}

#[cfg(target_os = "macos")]
impl BundleSystem for FakeMac {
    fn expand(&self, _zip: &Path, into: &Path) -> Result<(), String> {
        write_app(into, BUNDLE, "2.0.0");
        Ok(())
    }
    fn code_signature(&self, _app: &Path, expected: Option<&DeveloperId>) -> CodeSignature {
        assert_eq!(expected, Some(&DeveloperId::new(TEAM, BUNDLE)));
        self.signature.clone()
    }
    fn bundle_info(&self, app: &Path) -> BundleInfo {
        let text = std::fs::read_to_string(app.join("Contents/Info.plist")).unwrap_or_default();
        let mut lines = text.lines().map(str::to_owned);
        BundleInfo {
            identifier: lines.next(),
            version: lines.next(),
        }
    }
    fn clear_quarantine(&self, _app: &Path) -> Result<(), String> {
        Ok(())
    }
    fn can_write(&self, _dir: &Path) -> bool {
        true
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::rename(from, to)
    }
    fn remove(&self, path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }
    fn relaunch(&self, app: &Path) -> Result<(), String> {
        *self.relaunched.lock().unwrap() = Some(app.to_owned());
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn signed(team: &str, identifier: &str) -> CodeSignature {
    CodeSignature {
        signed: true,
        invalid: None,
        identifier: Some(identifier.into()),
        team_id: Some(team.into()),
        developer_id: team == TEAM && identifier == BUNDLE,
    }
}

/// An "installed" Félagi in a temp folder, and a runtime that thinks it runs
/// from it.
#[cfg(target_os = "macos")]
fn installed(app: &TestApp, system: Arc<FakeMac>) -> (PathBuf, Arc<UpdaterRuntime>) {
    let root = std::env::temp_dir().join(format!(
        "elyra-test-installed-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let bundle = write_app(&root.join("Applications"), BUNDLE, "1.0.0");
    let rt = UpdaterRuntime::new(&config(), Some(app.events().clone()))
        .unwrap()
        .with_backend(&config(), (*app.get::<Backend>()).clone())
        .with_system(system)
        .with_executable(bundle.join("Contents/MacOS/felagi"));
    (root, Arc::new(rt))
}

#[cfg(target_os = "macos")]
fn version_of(bundle: &Path) -> String {
    std::fs::read_to_string(bundle.join("Contents/Info.plist"))
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_owned()
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn install_puts_the_verified_bundle_in_place_and_restart_is_a_second_step() {
    let bytes = artifact();
    let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
    let fake = HttpFake::new()
        .get(at(ACME, ROUTE), 200, release(ACME, &url, &bytes))
        .get_bytes(&url, 200, bytes, "Felagi.zip");
    let app = app(&fake);
    sign_in(&app, ACME, "acme-token").await;
    let _ = app.drain_events().await;

    let system = Arc::new(FakeMac {
        signature: signed(TEAM, BUNDLE),
        relaunched: Mutex::new(None),
    });
    let (root, rt) = installed(&app, system.clone());
    let quit = Arc::new(AtomicBool::new(false));
    let quitting = quit.clone();
    rt.set_quit(move || quitting.store(true, Ordering::SeqCst));

    rt.install().await.unwrap();
    let bundle = root.join("Applications/Félagi.app");
    assert_eq!(version_of(&bundle), "2.0.0");
    let phases: Vec<Phase> = app.events_on("elyra:update").await;
    assert_eq!(
        phases.first().map(|p| p.phase.as_str()),
        Some("downloading")
    );
    assert_eq!(phases.last().unwrap().phase, "ready");
    assert_eq!(phases.last().unwrap().restart.as_deref(), Some("ask"));
    assert!(phases.iter().any(|p| p.progress == Some(100)));
    // Installed, and still running: nothing relaunched yet.
    assert!(system.relaunched.lock().unwrap().is_none());
    assert!(!quit.load(Ordering::SeqCst));
    let check = rt.check_for_frontend().await;
    assert!(check.ready);

    rt.relaunch().unwrap();
    assert_eq!(
        system.relaunched.lock().unwrap().as_deref(),
        Some(bundle.as_path())
    );
    assert!(quit.load(Ordering::SeqCst));
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn a_bundle_signed_by_another_team_or_for_another_app_is_not_installed() {
    for signature in [
        signed("7G383N3VY7", BUNDLE),
        signed(TEAM, "dev.eterm.eterm"),
        CodeSignature::default(),
    ] {
        let bytes = artifact();
        let url = at(ACME, "/desktop/Felagi-2.0.0.zip");
        let fake = HttpFake::new()
            .get(at(ACME, ROUTE), 200, release(ACME, &url, &bytes))
            .get_bytes(&url, 200, bytes, "Felagi.zip");
        let app = app(&fake);
        sign_in(&app, ACME, "acme-token").await;
        let system = Arc::new(FakeMac {
            signature,
            relaunched: Mutex::new(None),
        });
        let (root, rt) = installed(&app, system);
        let err = rt.install().await.unwrap_err();
        assert!(matches!(err, Error::CodeSignature(_)), "{err}");
        assert_eq!(version_of(&root.join("Applications/Félagi.app")), "1.0.0");
        assert!(matches!(rt.relaunch(), Err(Error::NothingInstalled)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
