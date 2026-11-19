//! `rata check`'s view of the app (RFC 0036): `ELYRA_CHECK` writes each
//! command with its ability, whether it's a read, and who it's granted to,
//! the grants and whether a command uses each, the startup problems and
//! the bindings, then exits without a window.

use elyra::mcp::Mcp;
use elyra::{command, commands, App, Ctx};

#[command(live, can = "notes.view")]
async fn notes_index(_ctx: Ctx) -> Vec<String> {
    Vec::new()
}

#[command(can = "notes.delete")]
async fn notes_destroy(_ctx: Ctx, id: i64) -> i64 {
    id
}

#[command]
async fn notes_purge(_ctx: Ctx) {}

#[command(can = "reports.export")]
async fn reports_export(_ctx: Ctx) -> String {
    String::new()
}

#[test]
fn the_check_mode_writes_the_app_s_shape_and_exits() {
    let out = std::env::temp_dir().join(format!("elyra-check-shape-{}.json", std::process::id()));
    // Only this test sets it, in this test binary.
    std::env::set_var("ELYRA_CHECK", &out);
    App::new()
        .allow_abilities(["notes.view", "notse.delete"])
        .mcp(
            Mcp::new()
                .allow_abilities(["reports.*"])
                .confirm("reports.export"),
        )
        .commands(commands![
            notes_index,
            notes_destroy,
            notes_purge,
            reports_export
        ])
        .run()
        .expect("the check mode returns");
    std::env::remove_var("ELYRA_CHECK");
    let shape: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let _ = std::fs::remove_file(&out);

    let command = |name: &str| {
        shape["commands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .cloned()
            .unwrap_or_else(|| panic!("no {name} in {shape}"))
    };
    assert_eq!(
        command("notes_index"),
        serde_json::json!({ "name": "notes_index", "ability": "notes.view", "live": true,
                            "frontend": true, "assistant": null, "mcp": null })
    );
    let destroy = command("notes_destroy");
    assert_eq!(destroy["frontend"], false, "notse.delete is a typo");
    assert_eq!(
        command("notes_purge")["frontend"],
        true,
        "no ability: open to the page"
    );
    assert_eq!(command("notes_purge")["live"], false);
    let export = command("reports_export");
    assert_eq!(export["frontend"], false);
    assert_eq!(export["mcp"], serde_json::json!({ "confirm": true }));

    let grants = shape["grants"].as_array().unwrap();
    assert_eq!(grants[0]["pattern"], "notes.view");
    assert_eq!(grants[0]["used"], true);
    assert_eq!(grants[1]["used"], false);
    assert_eq!(shape["mcp"], true);
    assert!(shape["problems"].as_array().unwrap().is_empty());
    let ts = shape["bindings"]["ts"].as_str().expect("the bindings");
    assert!(ts.contains("notes_destroy"));
}
