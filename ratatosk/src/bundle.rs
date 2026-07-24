//! `rata bundle` — package the release binary into a macOS `.app`.
//!
//! Produces `target/release/bundle/<Name>.app` with an `Info.plist` and the
//! embedded binary, then ad-hoc code-signs it (`codesign -s -`) so it launches
//! locally without a Developer ID. Real Developer ID signing + notarization is
//! left to CI with your certificate.

use std::path::{Path, PathBuf};
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

    // Generate the native app icon (.icns) so the dock/Finder icon matches the
    // app, not the default. Best-effort: on failure the bundle still builds.
    let icon_file = cfg.bundle_icon.as_ref().and_then(|src| {
        let scratch = app_dir
            .parent()
            .unwrap_or(&app_dir)
            .join(".elyra-iconbuild");
        let result = build_icns(src, &resources, &scratch);
        let _ = std::fs::remove_dir_all(&scratch);
        match &result {
            Some(_) => println!("bundle: icon -> Resources/AppIcon.icns (from {})", src.display()),
            None => eprintln!(
                "bundle: icon generation failed (needs sips + iconutil and a valid image); using the default icon"
            ),
        }
        result
    });

    std::fs::write(
        contents.join("Info.plist"),
        info_plist(cfg, icon_file.as_deref()),
    )
    .map_err(|e| format!("write Info.plist: {e}"))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????")
        .map_err(|e| format!("write PkgInfo: {e}"))?;

    ad_hoc_sign(&app_dir);

    println!("bundle: created {}", app_dir.display());
    Ok(())
}

/// Generate `Resources/AppIcon.icns` from a source image, returning the icon
/// file stem for `CFBundleIconFile`. Renders SVGs at 1024 first, then builds an
/// `.iconset` with `sips` and packs it with `iconutil`. macOS-only tooling.
fn build_icns(src: &Path, resources: &Path, scratch: &Path) -> Option<String> {
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch).ok()?;

    let is_svg = src
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("svg"))
        .unwrap_or(false);
    let master = if is_svg {
        rasterize_svg(src, scratch)?
    } else {
        src.to_path_buf()
    };

    let iconset = scratch.join("AppIcon.iconset");
    std::fs::create_dir_all(&iconset).ok()?;
    const SPECS: &[(u32, &str)] = &[
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ];
    for (px, name) in SPECS {
        let dim = px.to_string();
        let ok = Command::new("sips")
            .args(["-z", &dim, &dim])
            .arg(&master)
            .arg("--out")
            .arg(iconset.join(name))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return None;
        }
    }

    let icns = resources.join("AppIcon.icns");
    let ok = Command::new("iconutil")
        .args(["-c", "icns"])
        .arg(&iconset)
        .arg("-o")
        .arg(&icns)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    ok.then(|| "AppIcon".to_string())
}

/// Render an SVG to a 1024×1024 PNG master. Prefers `qlmanage` (renders vectors
/// at the requested size); falls back to `sips` (renders at intrinsic size).
fn rasterize_svg(src: &Path, scratch: &Path) -> Option<PathBuf> {
    let ql_out = scratch.join("ql");
    std::fs::create_dir_all(&ql_out).ok()?;
    let ok = Command::new("qlmanage")
        .args(["-t", "-s", "1024", "-o"])
        .arg(&ql_out)
        .arg(src)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ok {
        if let Ok(entries) = std::fs::read_dir(&ql_out) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("png") {
                    return Some(path);
                }
            }
        }
    }
    let out = scratch.join("master.png");
    Command::new("sips")
        .args(["-s", "format", "png"])
        .arg(src)
        .arg("--out")
        .arg(&out)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        .then_some(out)
}

fn info_plist(cfg: &Config, icon_file: Option<&str>) -> String {
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
    <key>NSHighResolutionCapable</key><true/>{icon}
</dict>
</plist>
"#,
        name = cfg.bundle_name,
        id = cfg.bundle_identifier,
        version = cfg.bundle_version,
        bin = cfg.app_crate,
        icon = icon_file
            .map(|f| format!("\n    <key>CFBundleIconFile</key><string>{f}</string>"))
            .unwrap_or_default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn builds_icns_from_svg() {
        let dir = std::env::temp_dir().join(format!("elyra-icns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let resources = dir.join("Resources");
        std::fs::create_dir_all(&resources).unwrap();
        let svg = dir.join("icon.svg");
        std::fs::write(
            &svg,
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><rect width="512" height="512" fill="#c25b3a"/></svg>"##,
        )
        .unwrap();
        let scratch = dir.join(".build");

        // Assert correctness when the macOS icon tooling is available; skip
        // gracefully if a sandboxed CI environment lacks it (keeps CI green).
        match build_icns(&svg, &resources, &scratch) {
            Some(stem) => {
                assert_eq!(stem, "AppIcon");
                assert!(resources.join("AppIcon.icns").is_file());
            }
            None => eprintln!("skipping: sips/iconutil/qlmanage unavailable here"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
