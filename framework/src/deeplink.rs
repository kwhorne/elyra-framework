//! Deep-link (custom URL scheme) support.
//!
//! Two halves: **delivery** — the launch URL is read from the command line
//! (`initial`), later URLs arrive on the `elyra:deep-link` channel (macOS via
//! the system open-URL event, other platforms via single-instance forwarding);
//! and **registration** — associating a scheme (`myapp://`) with this
//! executable so the OS routes links to it.
//!
//! On macOS registration lives in the bundle's `Info.plist`
//! (`CFBundleURLTypes`) — a packaging concern; the runtime only handles the
//! incoming URLs. Windows and Linux are registered here at startup.

/// The first `<scheme>://…` argument this process was launched with, if any.
pub(crate) fn url_in_args(scheme: &str) -> Option<String> {
    let prefix = format!("{scheme}://");
    std::env::args().skip(1).find(|a| a.starts_with(&prefix))
}

/// Associate `scheme` with the current executable (idempotent). No-op on macOS,
/// where this belongs in the bundle `Info.plist`.
pub(crate) fn register(scheme: &str, app_name: &str) {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let _ = (scheme, app_name, &exe);

    #[cfg(target_os = "windows")]
    {
        let exe = exe.display().to_string();
        let base = format!("HKCU\\Software\\Classes\\{scheme}");
        let run = |args: &[&str]| {
            let _ = std::process::Command::new("reg").args(args).output();
        };
        run(&["add", &base, "/ve", "/d", &format!("URL:{scheme}"), "/f"]);
        run(&["add", &base, "/v", "URL Protocol", "/d", "", "/f"]);
        run(&[
            "add",
            &format!("{base}\\shell\\open\\command"),
            "/ve",
            "/d",
            &format!("\"{exe}\" \"%1\""),
            "/f",
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let dir = std::path::Path::new(&home).join(".local/share/applications");
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join(format!("{app_name}-elyra.desktop"));
        let desktop = format!(
            "[Desktop Entry]\nType=Application\nName={app_name}\nExec={} %u\nTerminal=false\nMimeType=x-scheme-handler/{scheme};\n",
            exe.display()
        );
        if std::fs::write(&file, desktop).is_ok() {
            let _ = std::process::Command::new("xdg-mime")
                .args([
                    "default",
                    &format!("{app_name}-elyra.desktop"),
                    &format!("x-scheme-handler/{scheme}"),
                ])
                .output();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_matching_scheme_only() {
        // No matching arg in the test harness's argv.
        assert!(super::url_in_args("definitely-not-a-scheme").is_none());
    }
}
