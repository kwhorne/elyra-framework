//! elyracode/elyra-backend's desktop routes against a real Laravel app
//! (scripts/laravel-backend.sh): `Route::elyraBroadcasting()`, the Reverb
//! settings `Backend::broadcasting_from(..)` connects with (RFC 0021), and
//! `Route::elyraUpdates()`, the releases `php artisan elyra:release` put on
//! the disk, as `UpdaterConfig::from_backend(..)` checks and downloads them
//! (RFC 0024). Skipped unless `ELYRA_LARAVEL_URL` says where it is; the
//! broadcasting test needs its Reverb running too.
#![cfg(all(feature = "backend", feature = "database"))]

use std::time::Duration;

use elyra::auth::Auth;
use elyra::backend::Backend;
use elyra::broadcasting::{Broadcaster, State};
use elyra::http::Http;
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};
use serde_json::{json, Value};

fn laravel() -> Option<String> {
    let url = std::env::var("ELYRA_LARAVEL_URL").ok();
    if url.is_none() {
        eprintln!("skipping: set ELYRA_LARAVEL_URL (see scripts/laravel-backend.sh)");
    }
    url
}

/// Ada's token, as the web has one.
async fn web_token(http: &Http, url: &str) -> String {
    http.post(format!("{url}/api/sanctum/token"))
        .json(&json!({ "email": "ada@example.com", "password": "secret", "device_name": "web" }))
        .send()
        .await
        .unwrap()
        .text()
        .trim()
        .to_owned()
}

/// How many customers there are, live.
#[command(live)]
async fn customers_total(ctx: Ctx) -> elyra::Result<i64> {
    let page: Value = ctx
        .get::<Backend>()
        .get("/api/customers")
        .query(&json!({ "per_page": 1 }))
        .json()
        .await?;
    Ok(page["total"].as_i64().unwrap_or(-1))
}

#[tokio::test]
async fn the_settings_are_reverbs_and_need_a_signed_in_user() {
    let Some(url) = laravel() else {
        return;
    };
    let http = Http::new();
    let refused = http
        .get(format!("{url}/api/desktop/broadcasting"))
        .header("Accept", "application/json")
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 401);

    let token = web_token(&http, &url).await;
    let settings: Value = http
        .get(format!("{url}/api/desktop/broadcasting"))
        .token(&token)
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();
    // As `php artisan install:broadcasting --reverb` wrote the .env:
    // REVERB_HOST=localhost, REVERB_SCHEME=http.
    assert!(
        settings["key"].as_str().is_some_and(|k| !k.is_empty()),
        "{settings}"
    );
    assert_eq!(settings["host"], "localhost", "{settings}");
    assert_eq!(settings["scheme"], "ws", "{settings}");
    assert!(settings["port"].as_u64().is_some(), "{settings}");
    // The key and port CI's other tests are given (`<key>@<host>:<port>`).
    if let Ok(pusher) = std::env::var("ELYRA_PUSHER") {
        let (key, address) = pusher.split_once('@').expect("<key>@<host>:<port>");
        let port: u64 = address
            .rsplit_once(':')
            .expect("<host>:<port>")
            .1
            .parse()
            .unwrap();
        assert_eq!(settings["key"], key);
        assert_eq!(settings["port"], port);
    }
}

#[tokio::test]
async fn broadcasting_from_the_server_connects_without_a_key_in_the_app() {
    let Some(url) = laravel() else {
        return;
    };
    // No key, host or port here: all of it from Route::elyraBroadcasting().
    let app = TestApp::new(
        App::new()
            .backend(
                Backend::new(&url)
                    .broadcasting_from("/api/desktop/broadcasting")
                    .follow("customers", "/api/customers"),
            )
            .live_batch_window(Duration::from_millis(5))
            .commands(commands![customers_total]),
    );
    app.get::<Auth>()
        .sign_in("ada@example.com", "secret")
        .await
        .expect("signed in");
    let broadcaster = app.get::<Broadcaster>();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while broadcaster.state() != State::Connected {
        assert!(
            tokio::time::Instant::now() < deadline,
            "never connected: {:?}",
            broadcaster.state()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let mut total = app.live::<i64>("customers_total", ()).await;
    let before = *total.value();

    // On the web: Laravel broadcasts it, and the open list follows.
    let http = Http::new();
    let token = web_token(&http, &url).await;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let created: Value = http
        .post(format!("{url}/api/customers"))
        .token(&token)
        .json(&json!({ "name": format!("From {stamp}"), "email": format!("from{stamp}@example.com") }))
        .send()
        .await
        .unwrap()
        .json()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), total.next())
        .await
        .expect("the list followed the web");
    assert_eq!(*total.value(), before + 1);

    http.delete(format!("{url}/api/customers/{}", created["id"]))
        .token(&token)
        .send()
        .await
        .unwrap();
    app.get::<Auth>().sign_out().await.unwrap();
}

#[cfg(feature = "updater")]
mod updates {
    use super::*;
    use elyra::updater::{UpdateStatus, Updater, UpdaterConfig, UpdaterRuntime};
    use sha2::Digest;

    /// The release the harness offers, 2.0.0 for every platform.
    fn artifact() -> Vec<u8> {
        let mut bytes = b"PK\x03\x04".to_vec();
        bytes.extend((0..4096).map(|i| (i % 251) as u8));
        bytes
    }

    fn app(url: &str, version: &str) -> TestApp {
        TestApp::new(
            App::new().backend(Backend::new(url)).updater(
                UpdaterConfig::from_backend("/api/desktop/update", version)
                    .developer_id("52AT44QFCM", "works.felagi.desktop"),
            ),
        )
    }

    #[tokio::test]
    async fn a_newer_release_is_offered_and_downloaded_from_this_server() {
        let Some(url) = laravel() else {
            return;
        };
        let app = app(&url, "1.4.0");
        app.get::<Auth>()
            .sign_in("ada@example.com", "secret")
            .await
            .expect("signed in");
        let updater = app.get::<UpdaterRuntime>();

        let UpdateStatus::Available(info) = updater.check().await.expect("checked") else {
            panic!("expected 2.0.0 to be offered");
        };
        let bytes = artifact();
        let sha: String = sha2::Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(info.version, "2.0.0");
        assert_eq!(info.notes.as_deref(), Some("- Faster search"));
        assert_eq!(info.sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(info.size, Some(bytes.len() as u64));
        let file = format!("App-2.0.0-{}.zip", Updater::current_target());
        assert_eq!(info.url, format!("{url}/api/desktop/download/{file}"));

        // Downloaded with Ada's token, and its size and SHA-256 checked.
        let path = updater
            .download(&info, |_, _| {})
            .await
            .expect("downloaded");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
        app.get::<Auth>().sign_out().await.unwrap();
    }

    #[tokio::test]
    async fn the_newest_version_is_up_to_date() {
        let Some(url) = laravel() else {
            return;
        };
        let app = app(&url, "2.0.0");
        app.get::<Auth>()
            .sign_in("ada@example.com", "secret")
            .await
            .expect("signed in");
        assert!(matches!(
            app.get::<UpdaterRuntime>().check().await.expect("checked"),
            UpdateStatus::UpToDate
        ));
        app.get::<Auth>().sign_out().await.unwrap();
    }

    #[tokio::test]
    async fn the_routes_refuse_what_they_should() {
        let Some(url) = laravel() else {
            return;
        };
        let http = Http::new();
        let file = format!("App-2.0.0-{}.zip", Updater::current_target());
        // Signed out: neither the offer nor the file.
        for path in [
            "/api/desktop/update?platform=linux-x86_64&version=1.0.0".to_string(),
            format!("/api/desktop/download/{file}"),
        ] {
            let res = http
                .get(format!("{url}{path}"))
                .header("Accept", "application/json")
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 401, "{path}");
        }
        let token = web_token(&http, &url).await;
        // Laravel's own 422 for what the desktop wouldn't send.
        let res = http
            .get(format!(
                "{url}/api/desktop/update?platform=amiga-m68k&version=1.0.0"
            ))
            .header("Accept", "application/json")
            .token(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 422);
        // Only a file that's a release.
        for path in [
            "/api/desktop/download/releases.json",
            "/api/desktop/download/nope.zip",
        ] {
            let res = http
                .get(format!("{url}{path}"))
                .header("Accept", "application/json")
                .token(&token)
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 404, "{path}");
        }
    }
}
