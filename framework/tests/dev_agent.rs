//! A debug build's pages carry the coding agent's script (RFC 0026); a
//! release's don't (`tests/inspector_release.rs`).
#![cfg(debug_assertions)]

use elyra::testing::TestShell;
use elyra::App;

#[test]
fn a_debug_page_has_the_agent_script() {
    let shell = TestShell::new(App::new().prepare());
    assert!(shell
        .page_scripts()
        .iter()
        .any(|s| s.contains("window.__elyraAgent = {")));
}
