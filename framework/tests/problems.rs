//! The app's configuration checked at once (RFC 0025): every problem found
//! before anything starts, listed together by `App::problems()`, returned
//! by `run()` and panicked with by `prepare()`.

use elyra::{command, commands, App, Ctx, Error};

#[command(middleware = "audit")]
async fn audited(_ctx: Ctx) {}

#[command(middleware = "missing")]
async fn also_wired_wrong(_ctx: Ctx) {}

#[command]
async fn fine(_ctx: Ctx) {}

#[cfg(feature = "backend")]
const BASE: &str = "https://api.example.test";

/// The key of the updater's own tests (ed25519, seed `[7; 32]`).
#[cfg(feature = "updater")]
const PUBLIC_KEY: &str = "6kpsY+KcUgq+9VB7Ey7F+ZVHdq6+vnuSQh7qaRRG0iw=";

fn messages(app: &App) -> Vec<String> {
    app.problems()
        .iter()
        .map(|p| p.message().to_owned())
        .collect()
}

#[test]
fn an_app_set_up_right_has_none() {
    assert_eq!(App::new().problems(), vec![]);
}

/// Windows wouldn't show the notifications of an app it can't name (RFC 0030).
#[test]
fn an_identifier_windows_wont_take() {
    let found = messages(&App::new().identifier("com.example.my notes"));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("App::identifier(\"com.example.my notes\"): up to 128"),
        "{found:?}"
    );
    assert_eq!(
        App::new().identifier("com.example.notes").problems(),
        vec![]
    );
}

#[test]
fn a_middleware_name_that_doesnt_resolve() {
    let found = messages(&App::new().commands(commands![audited]));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("invalid middleware wiring:"),
        "{found:?}"
    );
    assert!(found[0].contains("`audit`"), "{found:?}");
}

#[test]
fn every_command_wired_wrong_not_just_the_first() {
    let found = messages(&App::new().commands(commands![audited, fine, also_wired_wrong]));
    assert_eq!(found.len(), 2, "{found:?}");
    // In the commands' order by name.
    assert!(found[0].contains("`also_wired_wrong`") && found[0].contains("`missing`"));
    assert!(found[1].contains("`audited`") && found[1].contains("`audit`"));
}

#[cfg(feature = "tray")]
#[test]
fn staying_in_the_tray_without_one() {
    let found = messages(&App::new().stay_in_tray());
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("App::stay_in_tray() needs a tray"),
        "{found:?}"
    );

    let tray = elyra::tray::TrayConfig::new().quit("Quit");
    assert_eq!(App::new().tray(tray).stay_in_tray().problems(), vec![]);
}

#[cfg(feature = "backend")]
#[test]
fn what_arrives_through_broadcasting_without_it() {
    use elyra::backend::Backend;
    let found = messages(
        &App::new().backend(
            Backend::new(BASE)
                .notifications()
                .presence(["customers.*"])
                .listen(["task.*"]),
        ),
    );
    assert_eq!(found.len(), 3, "{found:?}");
    assert!(found[0].starts_with("Backend::notifications() needs broadcasting"));
    assert!(found[1].starts_with("Backend::presence(..) needs broadcasting"));
    assert!(found[2].starts_with("Backend::listen(..) needs broadcasting"));

    // With broadcasting, none.
    let backend = Backend::new(BASE)
        .broadcasting(elyra::broadcasting::Broadcasting::pusher("key"))
        .notifications()
        .presence(["customers.*"])
        .listen(["task.*"]);
    assert_eq!(App::new().backend(backend).problems(), vec![]);
}

#[cfg(feature = "updater")]
#[test]
fn an_updater_that_couldnt_verify_an_update() {
    use elyra::updater::UpdaterConfig;
    // No key for a fixed manifest: it used to be a log line, and no updates.
    let found = messages(&App::new().updater(UpdaterConfig::new(
        "",
        "https://example.test/latest.json",
        "1.0.0",
    )));
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("the updater can't verify an update:"),
        "{found:?}"
    );

    let cfg = UpdaterConfig::new(PUBLIC_KEY, "https://example.test/latest.json", "1.0.0");
    assert_eq!(App::new().updater(cfg).problems(), vec![]);
}

#[cfg(all(feature = "updater", feature = "backend"))]
#[test]
fn an_update_from_the_backend_without_one() {
    use elyra::updater::UpdaterConfig;
    let cfg = UpdaterConfig::from_backend("/api/desktop/update", "1.0.0").public_key(PUBLIC_KEY);
    let found = messages(&App::new().updater(cfg.clone()));
    assert_eq!(
        found,
        vec!["UpdaterConfig::from_backend(..) needs a backend: add App::backend(..)"]
    );
    let app = App::new()
        .backend(elyra::backend::Backend::new(BASE))
        .updater(cfg);
    assert_eq!(app.problems(), vec![]);
}

/// Every problem the configuration has, in one go: not one per run.
#[cfg(all(feature = "tray", feature = "backend", feature = "updater"))]
mod all_at_once {
    use super::*;
    use elyra::backend::Backend;
    use elyra::updater::UpdaterConfig;

    fn broken() -> App {
        App::new()
            .stay_in_tray()
            .backend(Backend::new(BASE).notifications().listen(["task.*"]))
            .updater(UpdaterConfig::new(
                "",
                "https://example.test/latest.json",
                "1.0.0",
            ))
            .commands(commands![audited])
    }

    #[test]
    fn listed_together_in_the_order_found() {
        let found = messages(&broken());
        assert_eq!(found.len(), 5, "{found:?}");
        let starts = [
            "App::stay_in_tray()",
            "Backend::notifications()",
            "Backend::listen(..)",
            "the updater can't verify",
            "invalid middleware wiring",
        ];
        for (message, start) in found.iter().zip(starts) {
            assert!(
                message.starts_with(start),
                "{message:?} should start {start:?}"
            );
        }
    }

    #[test]
    fn run_returns_them_before_anything_starts() {
        let Err(Error::Config(problems)) = broken().run() else {
            panic!("run() must refuse a broken configuration");
        };
        assert_eq!(problems.list().len(), 5);
        let shown = problems.to_string();
        assert!(
            shown.starts_with("the app's configuration has 5 problems:\n  - App::stay_in_tray()"),
            "{shown}"
        );
        assert_eq!(shown.matches("\n  - ").count(), 5, "{shown}");
        // `fn main() -> elyra::Result<()>` prints `Debug`: the same list,
        // not one escaped line inside `Config(..)`.
        assert_eq!(format!("{problems:?}"), shown);
        let error = Error::Config(problems);
        assert_eq!(error.to_string(), shown);
        assert_eq!(format!("{error:?}"), shown);
        // The other variants print as derived.
        assert_eq!(format!("{:?}", Error::Io("disk".into())), "Io(\"disk\")");
        assert_eq!(
            format!("{:?}", Error::with_kind("offline", "no answer")),
            "Kind { kind: \"offline\", message: \"no answer\" }"
        );
    }

    #[test]
    fn prepare_panics_with_all_of_them() {
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = broken().prepare();
        }))
        .expect_err("prepare() must refuse a broken configuration");
        let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(
            message.starts_with("the app's configuration has 5 problems:"),
            "{message}"
        );
        for part in [
            "needs a tray",
            "Backend::listen",
            "invalid middleware wiring",
        ] {
            assert!(message.contains(part), "{part} missing from {message}");
        }
    }
}

#[test]
fn one_problem_is_said_as_one() {
    let Err(Error::Config(problems)) = App::new().commands(commands![audited]).run() else {
        panic!("run() must refuse a broken configuration");
    };
    assert!(
        problems
            .to_string()
            .starts_with("the app's configuration has a problem: invalid middleware wiring:"),
        "{problems}"
    );
}
