//! `rata bundle` — package the release binary into a macOS `.app`.
//!
//! Produces `target/release/bundle/<Name>.app` with an `Info.plist` and the
//! embedded binary, then ad-hoc code-signs it (`codesign -s -`) so it launches
//! locally without a Developer ID. Real Developer ID signing + notarization is
//! left to CI with your certificate.

use std::path::Path;
use std::process::Command;

use crate::config::Config;

pub fn bundle(cfg: &Config) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Err("`rata bundle` currently supports macOS only".into());
    }

    // Ensure a release binary exists.
    println!("bundle: cargo build --release ({})", cfg.app_crate);
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", &cfg.app_crate])
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if !status.success() {
        return Err(format!("cargo build exited with {status}"));
    }

    let bin_src = cfg.root.join("target/release").join(&cfg.app_crate);
    if !bin_src.is_file() {
        return Err(format!("release binary not found at {}", bin_src.display()));
    }

    let app_dir = cfg
        .root
        .join("target/release/bundle")
        .join(format!("{}.app", cfg.bundle_name));
    let contents = app_dir.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");

    // Fresh bundle each time.
    let _ = std::fs::remove_dir_all(&app_dir);
    for dir in [&macos, &resources] {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }

    std::fs::copy(&bin_src, macos.join(&cfg.app_crate)).map_err(|e| format!("copy binary: {e}"))?;
    std::fs::write(contents.join("Info.plist"), info_plist(cfg))
        .map_err(|e| format!("write Info.plist: {e}"))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????")
        .map_err(|e| format!("write PkgInfo: {e}"))?;

    ad_hoc_sign(&app_dir);

    println!("bundle: created {}", app_dir.display());
    Ok(())
}

fn info_plist(cfg: &Config) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>{name}</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleIdentifier</key><string>{id}</string>
    <key>CFBundleVersion</key><string>{version}</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleExecutable</key><string>{bin}</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
"#,
        name = cfg.bundle_name,
        id = cfg.bundle_identifier,
        version = cfg.bundle_version,
        bin = cfg.app_crate,
    )
}

/// Ad-hoc sign so Gatekeeper lets it run locally. Best-effort.
fn ad_hoc_sign(app_dir: &Path) {
    match Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app_dir)
        .status()
    {
        Ok(status) if status.success() => println!("bundle: ad-hoc signed"),
        Ok(status) => eprintln!("bundle: codesign exited with {status} (unsigned)"),
        Err(e) => eprintln!("bundle: codesign unavailable ({e}) — left unsigned"),
    }
}
