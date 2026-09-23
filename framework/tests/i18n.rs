//! i18n through the real wiring: provider, the `/__i18n` routes, the saved
//! locale across a restart, and `elyra:locale` for the frontend.

use elyra::i18n::{I18nProvider, Translator};
use elyra::store::Store;
use elyra::testing::{TestApp, TestShell};
use elyra::{command, commands, App, Ctx};
use wry::http::{Request, StatusCode};

fn translator() -> Translator {
    Translator::new("en")
        .add_json(
            "en",
            r#"{ "greeting": "Hello, :name!", "files": "file|files" }"#,
        )
        .add_json(
            "nb",
            r#"{ "greeting": "Hei, :name!", "files": "fil|filer" }"#,
        )
}

#[command]
async fn greet(ctx: Ctx, name: String) -> String {
    ctx.get::<Translator>().get("greeting", &[("name", &name)])
}

#[command]
async fn switch(ctx: Ctx, locale: String) {
    ctx.get::<Translator>().set_locale(&locale);
}

fn app(store: Store, locale: Option<&str>) -> App {
    let mut provider = I18nProvider::with_translator(translator());
    if let Some(l) = locale {
        provider = provider.locale(l);
    }
    App::new()
        .provider(provider)
        .swap(store)
        .commands(commands![greet, switch])
}

#[tokio::test]
async fn a_command_translates_in_the_current_locale() {
    let app = TestApp::new(app(Store::fake(), Some("en")));
    assert_eq!(
        app.invoke_ok::<String>("greet", ("Ada",)).await,
        "Hello, Ada!"
    );
    app.listen();
    app.invoke_ok::<()>("switch", ("nb",)).await;
    assert_eq!(
        app.invoke_ok::<String>("greet", ("Ada",)).await,
        "Hei, Ada!"
    );
    app.assert_emitted("elyra:locale").await;
}

#[tokio::test]
async fn the_chosen_locale_survives_a_restart() {
    let path = std::env::temp_dir().join(format!("elyra-i18n-store-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    {
        let first = TestApp::new(app(Store::at(&path), None));
        first.invoke_ok::<()>("switch", ("nb",)).await;
        first.get::<Store>().flush();
    }
    let second = TestApp::new(app(Store::at(&path), None));
    assert_eq!(
        second.get::<Translator>().locale(),
        "nb",
        "the saved choice beats the OS language"
    );
    assert_eq!(
        second.invoke_ok::<String>("greet", ("Ada",)).await,
        "Hei, Ada!"
    );
    let _ = std::fs::remove_file(path);
}

fn ipc(shell: &TestShell, path: &str, body: Vec<u8>) -> Request<Vec<u8>> {
    Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost{path}"))
        .header("x-elyra-token", shell.token())
        .body(body)
        .unwrap()
}

#[derive(serde::Deserialize)]
struct Payload {
    locale: String,
    fallback: String,
    messages: std::collections::BTreeMap<String, String>,
}

#[tokio::test]
async fn the_frontend_routes_serve_and_switch_the_catalog() {
    let shell = TestShell::new(app(Store::fake(), Some("en")).prepare());

    let res = shell.handle(ipc(&shell, "/__i18n", Vec::new())).await;
    assert_eq!(res.status(), StatusCode::OK);
    let p: Payload = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!((p.locale.as_str(), p.fallback.as_str()), ("en", "en"));
    assert_eq!(p.messages["greeting"], "Hello, :name!");

    let res = shell
        .handle(ipc(
            &shell,
            "/__i18n/locale",
            rmp_serde::to_vec("nb").unwrap(),
        ))
        .await;
    let p: Payload = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(p.locale, "nb");
    assert_eq!(p.messages["files"], "fil|filer");
}

#[tokio::test]
async fn the_routes_still_need_the_token() {
    let shell = TestShell::new(app(Store::fake(), None).prepare());
    let res = shell
        .handle(
            Request::builder()
                .uri("elyra://localhost/__i18n")
                .body(Vec::new())
                .unwrap(),
        )
        .await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}
