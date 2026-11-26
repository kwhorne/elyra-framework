//! The app's menu at runtime (RFC 0055): items greyed out, checked and
//! labelled from the page, through `/__menu/*`, and what a read-only window
//! and a revoked capability may do.

use elyra::security::Capability;
use elyra::testing::http::Request;
use elyra::testing::TestShell;
use elyra::{App, AppMenu, Item, Menu, MenuItemState, Submenu, SystemItem, WindowConfig};

fn menu() -> Menu {
    Menu::new().submenu(
        Submenu::new("Document")
            .entry(
                Item::new("doc.save", "Save")
                    .accelerator("CmdOrCtrl+S")
                    .enabled(false),
            )
            .check("doc.track", "Track changes", false)
            .system(SystemItem::Copy)
            .submenu(Submenu::new("Zoom").item("zoom.in", "Zoom in")),
    )
}

fn post(shell: &TestShell, op: &str, body: Vec<u8>) -> Request<Vec<u8>> {
    Request::builder()
        .method("POST")
        .uri(format!("elyra://localhost/__menu/{op}"))
        .header("x-elyra-token", shell.token())
        .body(body)
        .unwrap()
}

#[derive(serde::Serialize)]
struct Set<'a> {
    id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    checked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<&'a str>,
}

fn set(id: &str, enabled: Option<bool>, checked: Option<bool>, label: Option<&str>) -> Vec<u8> {
    rmp_serde::to_vec_named(&Set {
        id,
        enabled,
        checked,
        label,
    })
    .unwrap()
}

#[tokio::test]
async fn the_page_greys_out_checks_and_relabels_an_item() {
    let prepared = App::new().menu(menu()).prepare();
    let app_menu = prepared.ctx.get::<AppMenu>();
    let shell = TestShell::new(prepared);
    let res = shell
        .handle(post(
            &shell,
            "set",
            set("doc.save", Some(true), None, Some("Save Draft")),
        ))
        .await;
    assert_eq!(res.status(), 200);
    assert!(rmp_serde::from_slice::<bool>(res.body()).unwrap());
    let res = shell
        .handle(post(
            &shell,
            "set",
            set("doc.track", None, Some(true), None),
        ))
        .await;
    assert!(rmp_serde::from_slice::<bool>(res.body()).unwrap());
    // An item the menu doesn't have: `false`.
    let res = shell
        .handle(post(&shell, "set", set("nope", Some(true), None, None)))
        .await;
    assert!(!rmp_serde::from_slice::<bool>(res.body()).unwrap());

    let res = shell
        .handle(post(
            &shell,
            "state",
            rmp_serde::to_vec(&"doc.save").unwrap(),
        ))
        .await;
    let save: Option<MenuItemState> = rmp_serde::from_slice(res.body()).unwrap();
    assert_eq!(
        save,
        Some(MenuItemState {
            enabled: true,
            checked: None,
            label: "Save Draft".into()
        })
    );
    // Rust sees the same state.
    assert_eq!(app_menu.state("doc.track").unwrap().checked, Some(true));
    assert!(app_menu.state("zoom.in").is_some());
}

#[tokio::test]
async fn a_read_only_window_reads_the_menu_and_changes_nothing() {
    let prepared = App::new().menu(menu()).prepare();
    let app_menu = prepared.ctx.get::<AppMenu>();
    let shell = TestShell::new(prepared).window(WindowConfig::new("quick").read_only());
    let res = shell
        .handle_in(
            "quick",
            post(&shell, "set", set("doc.save", Some(true), None, None)),
        )
        .await;
    assert_eq!(
        res.status(),
        403,
        "{:?}",
        String::from_utf8_lossy(res.body())
    );
    assert!(!app_menu.state("doc.save").unwrap().enabled);
    let res = shell
        .handle_in(
            "quick",
            post(&shell, "state", rmp_serde::to_vec(&"doc.save").unwrap()),
        )
        .await;
    assert_eq!(res.status(), 200);
}

#[tokio::test]
async fn without_the_capability_the_page_leaves_the_menu_alone() {
    let prepared = App::new()
        .menu(menu())
        .deny_frontend(Capability::Menu)
        .prepare();
    let app_menu = prepared.ctx.get::<AppMenu>();
    let shell = TestShell::new(prepared);
    let res = shell
        .handle(post(&shell, "set", set("doc.save", Some(true), None, None)))
        .await;
    assert_eq!(res.status(), 403);
    assert!(!app_menu.state("doc.save").unwrap().enabled);
}

#[test]
fn a_menu_that_would_mislead_stops_the_app() {
    let twice = Menu::new().submenu(Submenu::new("A").item("x", "One").check("x", "Two", false));
    let problems = App::new().menu(twice).problems();
    assert!(
        problems
            .iter()
            .any(|p| p.to_string().contains("`x` is used twice")),
        "{problems:?}"
    );
}
