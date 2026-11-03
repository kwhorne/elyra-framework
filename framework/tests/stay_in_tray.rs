//! An app that stays in the tray (RFC 0020): it starts with the icon only,
//! is a single instance, and doesn't start without a tray.
#![cfg(feature = "tray")]

use elyra::tray::TrayConfig;
use elyra::window::WindowConfig;
use elyra::App;

fn tray() -> TrayConfig {
    TrayConfig::new()
        .tooltip("Timer")
        .item("open", "Open Timer")
        .quit("Quit")
}

#[test]
fn it_starts_with_the_icon_only() {
    let app = App::new()
        .window(WindowConfig::new("log"))
        .tray(tray().popover(WindowConfig::new("quick")))
        .stay_in_tray()
        .prepare();
    assert!(app.stay_in_tray);
    // The main window and the other startup windows: made, hidden.
    assert_eq!(app.windows.len(), 2);
    assert!(app.windows.iter().all(|w| !w.visible));
    // The popover is hidden as ever (RFC 0019).
    let popover = app.tray.as_ref().and_then(|t| t.popover.as_ref()).unwrap();
    assert!(!popover.visible);
}

#[test]
fn it_is_a_single_instance() {
    let app = App::new().tray(tray()).stay_in_tray().prepare();
    assert!(app.single_instance);
}

#[test]
fn an_ordinary_app_is_unchanged() {
    let app = App::new()
        .window(WindowConfig::new("log"))
        .tray(tray())
        .prepare();
    assert!(!app.stay_in_tray);
    assert!(!app.single_instance);
    assert!(app.windows.iter().all(|w| w.visible));
}

#[test]
#[should_panic(expected = "stay_in_tray() needs a tray")]
fn it_does_not_start_without_a_tray() {
    let _ = App::new().stay_in_tray().prepare();
}

/// What `rata bundle` asks (`ELYRA_BUNDLE_INSPECT`): the app says whether it
/// stays in the tray, and exits without a window. One test, as the variable
/// is the process's.
#[test]
fn it_tells_rata_bundle_whether_it_stays_in_the_tray() {
    let out = std::env::temp_dir().join(format!("elyra-bundle-test-{}.json", std::process::id()));
    let ask = |app: App| {
        std::env::set_var("ELYRA_BUNDLE_INSPECT", &out);
        let result = app.run();
        std::env::remove_var("ELYRA_BUNDLE_INSPECT");
        result.expect("inspect mode must succeed");
        let text = std::fs::read_to_string(&out).unwrap();
        let _ = std::fs::remove_file(&out);
        serde_json::from_str::<serde_json::Value>(&text).unwrap()
    };
    assert_eq!(
        ask(App::new().tray(tray()).stay_in_tray())["stay_in_tray"],
        true
    );
    assert_eq!(ask(App::new().tray(tray()))["stay_in_tray"], false);
}
