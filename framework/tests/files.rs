//! `ChosenFile` (RFC 0009 step 2) through a command, as the frontend calls
//! it: a granted file decodes, a path nobody chose doesn't.

use std::collections::HashMap;

use elyra::files::{ChosenFile, FileChoice};
use elyra::testing::TestApp;
use elyra::{command, commands, App, Ctx};

#[command]
async fn file_size(_ctx: Ctx, file: ChosenFile) -> elyra::Result<u64> {
    file.size()
        .ok_or_else(|| elyra::Error::Command(format!("{} isn't there", file.name())))
}

#[command]
async fn chosen_kind(_ctx: Ctx, file: ChosenFile) -> elyra::Result<FileChoice> {
    Ok(file.kind())
}

fn app() -> TestApp {
    TestApp::new(App::new().commands(commands![file_size, chosen_kind]))
}

fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("elyra-files-{}-{name}", std::process::id()));
    std::fs::write(&path, contents).unwrap();
    path
}

#[tokio::test]
async fn a_chosen_file_reaches_the_command() {
    let path = temp_file("a.txt", "hello");
    let app = app();
    let file = ChosenFile::from_path(&path);
    assert_eq!(app.invoke_ok::<u64>("file_size", (&file,)).await, 5);
    // As the frontend sends it: a map.
    let as_map: HashMap<&str, String> = HashMap::from([
        (
            "id",
            serde_json::to_value(&file).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        ("path", path.display().to_string()),
        ("name", "a.txt".into()),
    ]);
    assert_eq!(app.invoke_ok::<u64>("file_size", (as_map,)).await, 5);
    assert_eq!(
        app.invoke_ok::<FileChoice>("chosen_kind", (ChosenFile::for_saving("/tmp/out.pdf"),))
            .await,
        FileChoice::Save
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn a_path_nobody_chose_is_refused() {
    let secret = temp_file("secret.txt", "do not send");
    let app = app();
    let forged: HashMap<&str, String> = HashMap::from([
        ("id", "0123456789abcdef0123456789abcdef".into()),
        ("path", secret.display().to_string()),
        ("name", "secret.txt".into()),
    ]);
    let err = app.invoke_err("file_size", (forged,)).await;
    assert!(err.contains("not a file the user chose"), "{err}");
    let bare = app
        .invoke_err("file_size", (secret.display().to_string(),))
        .await;
    assert!(bare.contains("not a file the user chose"), "{bare}");
    let _ = std::fs::remove_file(secret);
}

#[tokio::test]
async fn the_frontend_cant_swap_the_path() {
    let chosen = temp_file("chosen.txt", "ok");
    let secret = temp_file("swap-secret.txt", "a much longer secret");
    let app = app();
    let file = ChosenFile::from_path(&chosen);
    let swapped: HashMap<&str, String> = HashMap::from([
        (
            "id",
            serde_json::to_value(&file).unwrap()["id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ),
        ("path", secret.display().to_string()),
    ]);
    // The grant's file, not the one named.
    assert_eq!(app.invoke_ok::<u64>("file_size", (swapped,)).await, 2);
    let _ = std::fs::remove_file(chosen);
    let _ = std::fs::remove_file(secret);
}
