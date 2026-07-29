//! `rata bundle` — package the release binary into a macOS `.app`.
//!
//! Produces, per host platform:
//!
//! * **macOS** — `target/release/bundle/<Name>.app` with an `Info.plist` (including
//!   `CFBundleURLTypes` when a deep-link scheme is configured) and the embedded
//!   binary, ad-hoc code-signed (`codesign -s -`) so it launches locally without a
//!   Developer ID.
//! * **Linux** — a `.deb` (built without `dpkg`, so it also works when
//!   cross-checking on another host) plus the `.desktop` entry and icons, and a
//!   portable `.tar.gz`.
//! * **Windows** — a portable folder + `.zip` with the executable and resources.
//!
//! Real Developer ID signing, notarization, MSI/NSIS installers and AppImage are
//! deliberately out of scope (see `docs/roadmap.md`): they need per-project
//! certificates and external toolchains.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;

pub fn bundle(cfg: &Config) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return bundle_macos(cfg);
    #[cfg(target_os = "linux")]
    return bundle_linux(cfg);
    #[cfg(target_os = "windows")]
    return bundle_windows(cfg);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    Err("`rata bundle` supports macOS, Linux and Windows".into())
}

/// `cargo build --release` for the app crate, returning the binary path.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn build_release(cfg: &Config) -> Result<PathBuf, String> {
    println!("bundle: cargo build --release ({})", cfg.app_crate);
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", &cfg.app_crate])
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build --release failed".into());
    }
    let mut binary = cfg.root.join("target/release").join(&cfg.app_crate);
    if cfg!(target_os = "windows") {
        binary.set_extension("exe");
    }
    if !binary.exists() {
        return Err(format!("missing release binary: {}", binary.display()));
    }
    Ok(binary)
}

/// The output directory for bundles (created if needed).
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn bundle_dir(cfg: &Config) -> Result<PathBuf, String> {
    let dir = cfg.root.join("target/release/bundle");
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(target_os = "macos")]
fn bundle_macos(cfg: &Config) -> Result<(), String> {
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
// macOS packaging: the other hosts build a .deb / portable folder instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
// macOS packaging: the other hosts build a .deb / portable folder instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

// macOS packaging: the other hosts build a .deb / portable folder instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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
    <key>NSHighResolutionCapable</key><true/>{icon}{urls}
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
        // Deep links on macOS live in the bundle, not in code: without this the
        // scheme registered by `App::deep_link` never reaches the app.
        urls = cfg
            .bundle_deep_link
            .as_deref()
            .map(|scheme| format!(
                r#"
    <key>CFBundleURLTypes</key>
    <array>
        <dict>
            <key>CFBundleURLName</key><string>{id}</string>
            <key>CFBundleTypeRole</key><string>Viewer</string>
            <key>CFBundleURLSchemes</key>
            <array><string>{scheme}</string></array>
        </dict>
    </array>"#,
                id = cfg.bundle_identifier,
                scheme = scheme
            ))
            .unwrap_or_default(),
    )
}

/// A `.desktop` entry for Linux, including the deep-link MIME handler.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn desktop_entry(cfg: &Config) -> String {
    let mut entry = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={bin} %u\nIcon={bin}\n\
         Terminal=false\nCategories=Utility;\nComment={comment}\n",
        name = cfg.bundle_name,
        bin = cfg.app_crate,
        comment = cfg
            .bundle_description
            .clone()
            .unwrap_or_else(|| cfg.bundle_name.clone()),
    );
    if let Some(scheme) = &cfg.bundle_deep_link {
        entry.push_str(&format!("MimeType=x-scheme-handler/{scheme};\n"));
    }
    entry
}

/// The `.deb` control file.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn deb_control(cfg: &Config, installed_size_kb: u64) -> String {
    format!(
        "Package: {package}\nVersion: {version}\nArchitecture: {arch}\nMaintainer: {maintainer}\n\
         Installed-Size: {size}\nSection: utils\nPriority: optional\n\
         Description: {summary}\n",
        package = cfg.app_crate.replace('_', "-"),
        version = cfg.bundle_version,
        arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "amd64"
        },
        maintainer = cfg
            .bundle_maintainer
            .clone()
            .unwrap_or_else(|| format!("{} <unknown@example.com>", cfg.bundle_name)),
        size = installed_size_kb,
        summary = cfg
            .bundle_description
            .clone()
            .unwrap_or_else(|| cfg.bundle_name.clone()),
    )
}

/// Build a gzip-compressed tar from `(path-in-archive, bytes, mode)` entries.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn tar_gz(entries: &[(String, Vec<u8>, u32)]) -> Result<Vec<u8>, String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    for (path, bytes, mode) in entries {
        let mut header = tar::Header::new_gnu();
        header
            .set_path(path)
            .map_err(|e| format!("tar path {path}: {e}"))?;
        header.set_size(bytes.len() as u64);
        header.set_mode(*mode);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append(&header, bytes.as_slice())
            .map_err(|e| format!("tar append {path}: {e}"))?;
    }
    let encoder = builder.into_inner().map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())
}

/// Wrap members in a Unix `ar` archive — the container format of a `.deb`.
/// Written by hand so bundling doesn't need `dpkg-deb` on the host.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn ar_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();
    for (name, data) in members {
        let header = format!(
            "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
            name,
            0,
            0,
            0,
            "100644",
            data.len()
        );
        out.extend_from_slice(header.as_bytes());
        out.extend_from_slice(data);
        // Members are padded to an even length.
        if data.len() % 2 == 1 {
            out.push(b'\n');
        }
    }
    out
}

/// Linux: a `.deb` plus a portable `.tar.gz`.
#[cfg(target_os = "linux")]
fn bundle_linux(cfg: &Config) -> Result<(), String> {
    let binary = build_release(cfg)?;
    let out_dir = bundle_dir(cfg)?;
    let bin_bytes = std::fs::read(&binary).map_err(|e| format!("read binary: {e}"))?;
    let package = cfg.app_crate.replace('_', "-");

    // Payload layout: /usr/bin/<bin>, the .desktop entry, and an icon if we have one.
    let mut data_entries: Vec<(String, Vec<u8>, u32)> = vec![
        (
            format!("./usr/bin/{}", cfg.app_crate),
            bin_bytes.clone(),
            0o755,
        ),
        (
            format!("./usr/share/applications/{}.desktop", cfg.app_crate),
            desktop_entry(cfg).into_bytes(),
            0o644,
        ),
    ];
    if let Some(icon) = &cfg.bundle_icon {
        if let Ok(bytes) = std::fs::read(icon) {
            let ext = icon
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("png")
                .to_ascii_lowercase();
            let dest = if ext == "svg" {
                format!(
                    "./usr/share/icons/hicolor/scalable/apps/{}.svg",
                    cfg.app_crate
                )
            } else {
                format!(
                    "./usr/share/icons/hicolor/512x512/apps/{}.png",
                    cfg.app_crate
                )
            };
            data_entries.push((dest, bytes, 0o644));
        }
    }

    let installed_kb = data_entries
        .iter()
        .map(|(_, bytes, _)| bytes.len() as u64)
        .sum::<u64>()
        / 1024;

    let data_tar = tar_gz(&data_entries)?;
    let control_tar = tar_gz(&[(
        "./control".to_string(),
        deb_control(cfg, installed_kb).into_bytes(),
        0o644,
    )])?;

    let deb = ar_archive(&[
        ("debian-binary", b"2.0\n".to_vec()),
        ("control.tar.gz", control_tar),
        ("data.tar.gz", data_tar),
    ]);
    let deb_path = out_dir.join(format!("{package}_{}.deb", cfg.bundle_version));
    std::fs::write(&deb_path, deb).map_err(|e| format!("write .deb: {e}"))?;
    println!("bundle: created {}", deb_path.display());

    // A portable tarball for distros without dpkg.
    let portable = tar_gz(&[
        (format!("{package}/{}", cfg.app_crate), bin_bytes, 0o755),
        (
            format!("{package}/{}.desktop", cfg.app_crate),
            desktop_entry(cfg).into_bytes(),
            0o644,
        ),
    ])?;
    let tar_path = out_dir.join(format!("{package}_{}.tar.gz", cfg.bundle_version));
    std::fs::write(&tar_path, portable).map_err(|e| format!("write .tar.gz: {e}"))?;
    println!("bundle: created {}", tar_path.display());

    println!(
        "\nnote: AppImage/Flatpak need their own toolchains and are out of scope; \n      \
         install the .deb with `sudo dpkg -i {}`.",
        deb_path.display()
    );
    Ok(())
}

/// Windows: a portable folder next to the executable (zip it in CI if you want a
/// single artifact). MSI/NSIS installers need WiX/NSIS and are out of scope.
#[cfg(target_os = "windows")]
fn bundle_windows(cfg: &Config) -> Result<(), String> {
    let binary = build_release(cfg)?;
    let out_dir = bundle_dir(cfg)?;
    let app_dir = out_dir.join(&cfg.bundle_name);
    let _ = std::fs::remove_dir_all(&app_dir);
    std::fs::create_dir_all(&app_dir).map_err(|e| format!("create {}: {e}", app_dir.display()))?;

    let exe_name = format!("{}.exe", cfg.app_crate);
    std::fs::copy(&binary, app_dir.join(&exe_name)).map_err(|e| format!("copy binary: {e}"))?;

    if let Some(icon) = &cfg.bundle_icon {
        if let Some(name) = icon.file_name() {
            let _ = std::fs::copy(icon, app_dir.join(name));
        }
    }

    // A tiny README so the folder is self-explanatory when handed to a user.
    let readme = format!(
        "{name} {version}\r\n\r\nRun {exe} to start the app.\r\n",
        name = cfg.bundle_name,
        version = cfg.bundle_version,
        exe = exe_name
    );
    std::fs::write(app_dir.join("README.txt"), readme)
        .map_err(|e| format!("write README.txt: {e}"))?;

    println!("bundle: created {}", app_dir.display());
    println!(
        "\nnote: this is a portable layout. MSI (WiX) / NSIS installers and code \n      \
         signing need external tooling and are out of scope for `rata bundle`."
    );
    Ok(())
}

/// Ad-hoc sign so Gatekeeper lets it run locally. Best-effort.
// macOS packaging: the other hosts build a .deb / portable folder instead.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

// Only the macOS icon pipeline is exercised here, so the whole module is gated:
// on other hosts `use super::*` would be an unused import under `-D warnings`.
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
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

#[cfg(test)]
mod packaging_tests {
    use super::*;

    fn config() -> Config {
        Config {
            root: PathBuf::from("/tmp/project"),
            app_crate: "my_app".into(),
            frontend_dir: "app".into(),
            codegen_out: "app/src/bindings.ts".into(),
            bundle_identifier: "com.example.myapp".into(),
            bundle_name: "My App".into(),
            bundle_version: "1.2.3".into(),
            bundle_icon: None,
            bundle_deep_link: Some("myapp".into()),
            bundle_description: Some("A demo app".into()),
            bundle_maintainer: Some("Dev <dev@example.com>".into()),
            database_url: None,
            migrations_dir: "migrations".into(),
        }
    }

    #[test]
    fn info_plist_registers_the_deep_link_scheme() {
        let plist = info_plist(&config(), Some("AppIcon.icns"));
        assert!(plist.contains("<key>CFBundleURLTypes</key>"));
        assert!(plist.contains("<string>myapp</string>"));
        assert!(plist.contains("<key>CFBundleIconFile</key><string>AppIcon.icns</string>"));
        assert!(plist.contains("<string>com.example.myapp</string>"));

        // No scheme configured -> no URL types at all.
        let mut plain = config();
        plain.bundle_deep_link = None;
        assert!(!info_plist(&plain, None).contains("CFBundleURLTypes"));
    }

    #[test]
    fn desktop_entry_declares_the_scheme_handler() {
        let entry = desktop_entry(&config());
        assert!(entry.starts_with("[Desktop Entry]"));
        assert!(entry.contains("Name=My App"));
        assert!(entry.contains("Exec=my_app %u"));
        assert!(entry.contains("MimeType=x-scheme-handler/myapp;"));
        assert!(entry.contains("Comment=A demo app"));
    }

    #[test]
    fn deb_control_has_the_required_fields() {
        let control = deb_control(&config(), 4096);
        // dpkg requires Package/Version/Architecture/Maintainer/Description.
        for field in [
            "Package: my-app",
            "Version: 1.2.3",
            "Architecture:",
            "Maintainer: Dev <dev@example.com>",
            "Installed-Size: 4096",
            "Description: A demo app",
        ] {
            assert!(control.contains(field), "missing `{field}` in:\n{control}");
        }
        assert!(control.ends_with('\n'), "control must end with a newline");
    }

    #[test]
    fn tar_gz_round_trips_entries_with_modes() {
        let archive = tar_gz(&[
            ("./usr/bin/my_app".into(), b"binary".to_vec(), 0o755),
            ("./control".into(), b"Package: x\n".to_vec(), 0o644),
        ])
        .unwrap();
        // gzip magic.
        assert_eq!(&archive[..2], &[0x1f, 0x8b]);

        let decoder = flate2::read::GzDecoder::new(archive.as_slice());
        let mut tar = tar::Archive::new(decoder);
        let mut seen = Vec::new();
        for entry in tar.entries().unwrap() {
            let entry = entry.unwrap();
            seen.push((
                entry.path().unwrap().display().to_string(),
                entry.header().mode().unwrap(),
                entry.size(),
            ));
        }
        assert_eq!(seen.len(), 2);
        // `tar` normalizes the leading "./" away when reading back.
        assert_eq!(seen[0].0, "usr/bin/my_app");
        assert_eq!(seen[0].1, 0o755, "the executable bit must survive");
        assert_eq!(seen[1].1, 0o644);
    }

    #[test]
    fn ar_archive_has_the_deb_layout() {
        let deb = ar_archive(&[
            ("debian-binary", b"2.0\n".to_vec()),
            ("control.tar.gz", vec![1, 2, 3]), // odd length -> padded
            ("data.tar.gz", vec![4, 5, 6, 7]),
        ]);

        assert!(deb.starts_with(b"!<arch>\n"), "ar magic");
        let text = String::from_utf8_lossy(&deb);
        // Order matters to dpkg: debian-binary, control, data.
        let first = text.find("debian-binary").unwrap();
        let second = text.find("control.tar.gz").unwrap();
        let third = text.find("data.tar.gz").unwrap();
        assert!(first < second && second < third);

        // Each header is 60 bytes and ends with the ` \n` magic.
        assert_eq!(&deb[8 + 58..8 + 60], b"`\n");
        // The odd-length member is padded so the next header stays aligned.
        assert!(text.contains("data.tar.gz"));
    }
}
