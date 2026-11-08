//! `rata bundle` — package the release binary for the host platform.
//!
//! Produces, per host platform:
//!
//! * **macOS** — `target/release/bundle/<Name>.app` with an `Info.plist` (including
//!   `CFBundleURLTypes` when a deep-link scheme is configured) and the embedded
//!   binary, ad-hoc code-signed (`codesign -s -`) so it launches locally without a
//!   Developer ID.
//! * **Linux** — a `.deb` (built without `dpkg`, so it also works when
//!   cross-checking on another host) plus the `.desktop` entry and icons, a
//!   portable `.tar.gz`, and an AppImage when `appimagetool` is there.
//! * **Windows** — the `.exe` with its icon and version information linked in
//!   ([`crate::windows_res`]), a portable folder, and an installer when NSIS
//!   is there ([`crate::nsis`]).
//!
//! Signing (a Developer ID and notarization, Authenticode) stays out (see
//! `docs/roadmap.md`): it needs each app's own certificates.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::Config;
#[cfg(any(target_os = "linux", target_os = "windows"))]
use crate::icon;

pub fn bundle(cfg: &Config) -> Result<(), String> {
    bundle_for_host(cfg)?;
    // The frontend is embedded in the binary: one scan covers both.
    let mut binary = cfg.root.join("target").join("release").join(&cfg.app_crate);
    if cfg!(target_os = "windows") {
        binary.set_extension("exe");
    }
    if let Ok(bytes) = std::fs::read(&binary) {
        for key in api_keys(&bytes) {
            eprintln!(
                "bundle: warning: the app contains what looks like an API key ({key}). \
                 Anyone with the app can read it: keep provider keys on the server \
                 (Backend::assistant()) or in the user's keychain."
            );
        }
    }
    Ok(())
}

fn bundle_for_host(cfg: &Config) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return bundle_macos(cfg);
    #[cfg(target_os = "linux")]
    return bundle_linux(cfg);
    #[cfg(target_os = "windows")]
    return bundle_windows(cfg);
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    Err("`rata bundle` supports macOS, Linux and Windows".into())
}

/// What look like provider API keys in `bytes` — Anthropic's `sk-ant-…`,
/// OpenAI's `sk-proj-…` and `sk-…` — each shown by its start and length only,
/// so the warning doesn't print the key it warns about.
fn api_keys(bytes: &[u8]) -> Vec<String> {
    let key_char = |b: u8| b.is_ascii_alphanumeric() || b == b'-' || b == b'_';
    let mut found = Vec::new();
    let mut i = 0;
    while let Some(at) = bytes[i..].windows(3).position(|w| w == b"sk-") {
        let start = i + at;
        let end = start + bytes[start..].iter().take_while(|b| key_char(**b)).count();
        let token = &bytes[start..end];
        let body = token.len() - 3;
        // A key is long and mostly random; a word like `sk-ant-…` in a
        // message, or a short placeholder, isn't. A binary packs its strings
        // together, so a provider's own prefix counts wherever it starts; a
        // bare `sk-` only where no word runs into it (`task-…`, `risk-…`).
        let prefixed = token.starts_with(b"sk-ant-") || token.starts_with(b"sk-proj-");
        let preceded = start > 0 && key_char(bytes[start - 1]);
        let digits = token.iter().filter(|b| b.is_ascii_digit()).count();
        if (prefixed || !preceded) && body >= 32 && digits >= 4 {
            let shown = String::from_utf8_lossy(&token[..token.len().min(12)]);
            found.push(format!("{shown}…, {} characters", token.len()));
        }
        i = end.max(start + 3);
    }
    found
}

/// `cargo build --release` for the app crate, returning the binary path.
/// With `link`, a file for the linker (the Windows resources), the binary
/// is linked with it: `cargo rustc`, whose extra arguments reach the
/// binary alone, so nothing else is rebuilt.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn build_release(cfg: &Config, link: Option<&Path>) -> Result<PathBuf, String> {
    println!("bundle: cargo build --release ({})", cfg.app_crate);
    let mut cargo = Command::new("cargo");
    match link {
        Some(file) => cargo
            .args(["rustc", "--release", "-p", &cfg.app_crate])
            .args(["--bin", &cfg.app_crate, "--", "-C"])
            .arg(format!("link-arg={}", file.display())),
        None => cargo.args(["build", "--release", "-p", &cfg.app_crate]),
    };
    let status = cargo
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("cargo build: {e}"))?;
    if !status.success() {
        return Err("cargo build --release failed".into());
    }
    let mut binary = cfg.root.join("target").join("release").join(&cfg.app_crate);
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
    let dir = cfg.root.join("target").join("release").join("bundle");
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

    let bin_src = cfg.root.join("target").join("release").join(&cfg.app_crate);
    if !bin_src.is_file() {
        return Err(format!("release binary not found at {}", bin_src.display()));
    }

    let app_dir = cfg
        .root
        .join("target")
        .join("release")
        .join("bundle")
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

    // An app that stays in the tray (RFC 0020) is an accessory from launch:
    // set in code alone, its Dock icon shows for a moment first.
    let stay_in_tray = inspected_stay_in_tray(&inspect(cfg, &bin_src));
    if stay_in_tray {
        println!("bundle: stays in the tray -> LSUIElement (no Dock icon at launch)");
    }
    std::fs::write(
        contents.join("Info.plist"),
        info_plist(cfg, icon_file.as_deref(), stay_in_tray),
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
fn info_plist(cfg: &Config, icon_file: Option<&str>, stay_in_tray: bool) -> String {
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
    <key>NSHighResolutionCapable</key><true/>{icon}{urls}{ui_element}
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
        ui_element = if stay_in_tray {
            "\n    <key>LSUIElement</key><true/>"
        } else {
            ""
        },
    )
}

/// What the release binary says of how it runs, when asked
/// (`ELYRA_BUNDLE_INSPECT`): the Rust builder is the one place it's said —
/// whether it stays in the tray (RFC 0020), and who it is to Windows (RFC
/// 0030). An app that doesn't answer in time (built on an Elyra before 0.33,
/// it would open its window instead) is stopped, and its answer is empty.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn inspect(cfg: &Config, binary: &Path) -> String {
    let out = std::env::temp_dir().join(format!("elyra-bundle-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let Ok(mut child) = Command::new(binary)
        .env("ELYRA_BUNDLE_INSPECT", &out)
        .current_dir(&cfg.root)
        .spawn()
    else {
        return String::new();
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("bundle: the app didn't say how it runs; bundling it as an ordinary app");
                break;
            }
        }
    }
    let text = std::fs::read_to_string(&out).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    text
}

/// `{"stay_in_tray": true}`, from [`inspect`]; anything else is no.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn inspected_stay_in_tray(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v["stay_in_tray"].as_bool())
        .unwrap_or(false)
}

/// `{"app_id": "com.example.notes"}`, from [`inspect`]: the AppUserModelID
/// the app registers its notifications under (RFC 0030). An app on an
/// Elyra before 0.44 says nothing, and registers none.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn inspected_app_id(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|v| v["app_id"].as_str().map(str::to_owned))
        .filter(|id| !id.is_empty())
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
         Installed-Size: {size}\nDepends: {depends}\nSection: utils\nPriority: optional\n\
         Description: {summary}\n",
        // What wry's WebKitGTK webview needs on the machine it's installed on.
        depends = "libwebkit2gtk-4.1-0, libgtk-3-0",
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
        // A path ending in `/` is a directory.
        if path.ends_with('/') {
            header.set_entry_type(tar::EntryType::Directory);
        }
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

/// `entries` after the directories they're in, each once, parents first,
/// as `dpkg-deb` writes a package's files: dpkg makes no directory a
/// package doesn't list.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn with_dirs(entries: Vec<(String, Vec<u8>, u32)>) -> Vec<(String, Vec<u8>, u32)> {
    let mut dirs = std::collections::BTreeSet::new();
    for (path, _, _) in &entries {
        let mut at = 0;
        while let Some(slash) = path[at..].find('/') {
            at += slash + 1;
            dirs.insert(path[..at].to_string());
        }
    }
    dirs.into_iter()
        .map(|dir| (dir, Vec::new(), 0o755))
        .chain(entries)
        .collect()
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

/// The icon at `sizes` as PNGs, or none (said, not failed) when there's no
/// source or it can't be read.
#[cfg(target_os = "linux")]
fn icon_pngs(cfg: &Config, sizes: &[u32]) -> Vec<(u32, Vec<u8>)> {
    let Some(src) = &cfg.bundle_icon else {
        return Vec::new();
    };
    let pngs = icon::render(src, sizes).and_then(|images| {
        images
            .iter()
            .map(|image| Ok((image.size(), image.png()?)))
            .collect::<Result<Vec<_>, String>>()
    });
    pngs.unwrap_or_else(|e| {
        eprintln!("bundle: icon: {e}; bundling without one");
        Vec::new()
    })
}

/// Linux: a `.deb`, a portable `.tar.gz`, and an AppImage.
#[cfg(target_os = "linux")]
fn bundle_linux(cfg: &Config) -> Result<(), String> {
    let binary = build_release(cfg, None)?;
    let out_dir = bundle_dir(cfg)?;
    let bin_bytes = std::fs::read(&binary).map_err(|e| format!("read binary: {e}"))?;
    let package = cfg.app_crate.replace('_', "-");
    let pngs = icon_pngs(cfg, &[256, 512]);

    // Payload layout: /usr/bin/<bin>, the .desktop entry, and the icons.
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
    for (size, png) in &pngs {
        data_entries.push((
            format!(
                "./usr/share/icons/hicolor/{size}x{size}/apps/{}.png",
                cfg.app_crate
            ),
            png.clone(),
            0o644,
        ));
    }
    if let Some(svg) = cfg
        .bundle_icon
        .as_ref()
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("svg")))
    {
        if let Ok(bytes) = std::fs::read(svg) {
            data_entries.push((
                format!(
                    "./usr/share/icons/hicolor/scalable/apps/{}.svg",
                    cfg.app_crate
                ),
                bytes,
                0o644,
            ));
        }
    }

    let installed_kb = data_entries
        .iter()
        .map(|(_, bytes, _)| bytes.len() as u64)
        .sum::<u64>()
        / 1024;

    let data_tar = tar_gz(&with_dirs(data_entries))?;
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
        (
            format!("{package}/{}", cfg.app_crate),
            bin_bytes.clone(),
            0o755,
        ),
        (
            format!("{package}/{}.desktop", cfg.app_crate),
            desktop_entry(cfg).into_bytes(),
            0o644,
        ),
    ])?;
    let tar_path = out_dir.join(format!("{package}_{}.tar.gz", cfg.bundle_version));
    std::fs::write(&tar_path, portable).map_err(|e| format!("write .tar.gz: {e}"))?;
    println!("bundle: created {}", tar_path.display());

    let icon = pngs
        .iter()
        .find(|(size, _)| *size == 256)
        .map(|(_, png)| png.clone());
    appimage(cfg, &out_dir, &bin_bytes, icon)
}

/// The AppImage: an AppDir (the binary, `AppRun`, the `.desktop` entry and
/// the icon), made into one file by `appimagetool` when it's there. The app
/// uses the system's WebKitGTK, as the `.deb` does, rather than carrying
/// its own.
#[cfg(target_os = "linux")]
fn appimage(
    cfg: &Config,
    out_dir: &Path,
    bin_bytes: &[u8],
    icon: Option<Vec<u8>>,
) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let package = cfg.app_crate.replace('_', "-");
    let bin = &cfg.app_crate;
    let app_dir = out_dir.join(format!("{package}.AppDir"));
    let _ = std::fs::remove_dir_all(&app_dir);
    // appimagetool wants an icon; without the app's own, a plain one.
    let icon = match icon {
        Some(png) => png,
        None => icon::plain(256)?,
    };
    let desktop = desktop_entry(cfg).into_bytes();
    let run = app_run(bin);
    let files: Vec<(String, &[u8], u32)> = vec![
        (format!("usr/bin/{bin}"), bin_bytes, 0o755),
        ("AppRun".into(), run.as_bytes(), 0o755),
        (format!("{bin}.desktop"), &desktop, 0o644),
        (
            format!("usr/share/applications/{bin}.desktop"),
            &desktop,
            0o644,
        ),
        (format!("{bin}.png"), &icon, 0o644),
        (".DirIcon".into(), &icon, 0o644),
        (
            format!("usr/share/icons/hicolor/256x256/apps/{bin}.png"),
            &icon,
            0o644,
        ),
    ];
    for (path, bytes, mode) in files {
        let path = app_dir.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }

    let Some(tool) = appimagetool() else {
        println!(
            "\nnote: no appimagetool here, so no AppImage; the AppDir is at {}.\n      \
             Get appimagetool (https://github.com/AppImage/appimagetool/releases), put it \n      \
             on the PATH or in $APPIMAGETOOL, and run `rata bundle` again.",
            app_dir.display()
        );
        return Ok(());
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    let out = out_dir.join(format!("{package}_{}_{arch}.AppImage", cfg.bundle_version));
    let _ = std::fs::remove_file(&out);
    println!("bundle: appimagetool ({})", tool.display());
    let status = Command::new(&tool)
        .arg(&app_dir)
        .arg(&out)
        .env("ARCH", arch)
        .status()
        .map_err(|e| format!("{}: {e}", tool.display()))?;
    if !status.success() || !out.is_file() {
        return Err(format!("appimagetool exited with {status}"));
    }
    println!("bundle: created {}", out.display());
    Ok(())
}

/// `AppRun`: what an AppImage runs, the app inside it.
// Linux packaging: exercised by unit tests on every platform.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn app_run(bin: &str) -> String {
    format!(
        "#!/bin/sh\n\
         # Written by `rata bundle`: starts the app inside the AppImage.\n\
         HERE=\"$(dirname \"$(readlink -f \"$0\")\")\"\n\
         exec \"$HERE/usr/bin/{bin}\" \"$@\"\n"
    )
}

/// `appimagetool`: `$APPIMAGETOOL`, or on the `PATH`.
#[cfg(target_os = "linux")]
fn appimagetool() -> Option<PathBuf> {
    if let Some(tool) = std::env::var_os("APPIMAGETOOL") {
        return Some(PathBuf::from(tool));
    }
    [
        "appimagetool",
        "appimagetool-x86_64.AppImage",
        "appimagetool-aarch64.AppImage",
    ]
    .into_iter()
    .find(|name| {
        Command::new(name)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
    })
    .map(PathBuf::from)
}

/// Windows: the `.exe` with its icon and version information linked in, a
/// portable folder, and an installer when NSIS is there.
#[cfg(target_os = "windows")]
fn bundle_windows(cfg: &Config) -> Result<(), String> {
    use crate::nsis;
    use crate::windows_res::{self, VersionInfo};

    let out_dir = bundle_dir(cfg)?;
    let work = out_dir.join(".elyra");
    std::fs::create_dir_all(&work).map_err(|e| format!("create {}: {e}", work.display()))?;
    let exe_name = format!("{}.exe", cfg.app_crate);

    // The icon at Windows' sizes: linked into the `.exe` (a 256 px PNG,
    // as Windows reads it), and an `.ico` of bitmaps for the installer.
    let rendered = cfg.bundle_icon.as_ref().and_then(|src| {
        icon::render(src, icon::WINDOWS_SIZES)
            .map_err(|e| eprintln!("bundle: icon: {e}; using the default icon"))
            .ok()
    });
    let mut linked = Vec::new();
    let mut ico = None;
    if let Some(images) = &rendered {
        linked = icon::windows_images(images, 256)?;
        let path = work.join("app.ico");
        std::fs::write(&path, icon::ico(&icon::windows_images(images, u32::MAX)?))
            .map_err(|e| format!("write {}: {e}", path.display()))?;
        ico = Some(path);
    }
    let res = windows_res::res(
        &linked,
        &VersionInfo {
            name: &cfg.bundle_name,
            version: &cfg.bundle_version,
            publisher: cfg.bundle_publisher.as_deref(),
            exe: &exe_name,
        },
    );
    let res_path = work.join(format!("{}.res", cfg.app_crate));
    std::fs::write(&res_path, res).map_err(|e| format!("write {}: {e}", res_path.display()))?;
    // Microsoft's linker takes a `.res` as it is; the GNU toolchain's doesn't.
    let msvc = cfg!(target_env = "msvc");
    if !msvc {
        eprintln!(
            "bundle: the .exe's icon and version are linked by Microsoft's linker (the msvc \
             toolchain); this build uses another, so the .exe has neither"
        );
    }
    let binary = build_release(cfg, msvc.then_some(res_path.as_path()))?;
    if msvc {
        println!(
            "bundle: icon and version -> {exe_name}{}",
            if ico.is_some() {
                ""
            } else {
                " (version only: no icon)"
            }
        );
    }

    // Who the app is to Windows: its uninstaller removes that (RFC 0030).
    let app_id = inspected_app_id(&inspect(cfg, &binary));

    let app_dir = out_dir.join(nsis::file_name(&cfg.bundle_name));
    let _ = std::fs::remove_dir_all(&app_dir);
    std::fs::create_dir_all(&app_dir).map_err(|e| format!("create {}: {e}", app_dir.display()))?;
    std::fs::copy(&binary, app_dir.join(&exe_name)).map_err(|e| format!("copy binary: {e}"))?;
    if let Some(ico) = &ico {
        let _ = std::fs::copy(ico, app_dir.join(format!("{}.ico", cfg.app_crate)));
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

    let Some(makensis) = nsis::makensis() else {
        println!(
            "\nnote: no NSIS (makensis) here, so no installer. Install it \n      \
             (`winget install NSIS.NSIS` or `choco install nsis`) and run `rata bundle` again."
        );
        return Ok(());
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    };
    let out = out_dir.join(format!(
        "{}_{}_{arch}-setup.exe",
        nsis::file_name(&cfg.bundle_name),
        cfg.bundle_version
    ));
    let size_kb = std::fs::metadata(&binary)
        .map(|m| m.len() / 1024)
        .unwrap_or(0);
    let script = nsis::script(&nsis::Installer {
        name: &cfg.bundle_name,
        identifier: &cfg.bundle_identifier,
        version: &cfg.bundle_version,
        publisher: cfg.bundle_publisher.as_deref(),
        exe: &exe_name,
        exe_path: &binary,
        icon: ico.as_deref(),
        deep_link: cfg.bundle_deep_link.as_deref(),
        app_id: app_id.as_deref(),
        out: &out,
        size_kb,
    });
    let nsi = work.join("installer.nsi");
    std::fs::write(&nsi, script).map_err(|e| format!("write {}: {e}", nsi.display()))?;
    let _ = std::fs::remove_file(&out);
    println!("bundle: makensis ({})", makensis.display());
    let status = Command::new(&makensis)
        .args(["/V2", "/INPUTCHARSET", "UTF8"])
        .arg(&nsi)
        .status()
        .map_err(|e| format!("{}: {e}", makensis.display()))?;
    if !status.success() || !out.is_file() {
        return Err(format!(
            "makensis exited with {status} (the script: {})",
            nsi.display()
        ));
    }
    println!("bundle: created {}", out.display());
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

    #[test]
    fn api_keys_are_found_and_not_printed() {
        let anthropic = format!(
            "sk-ant-api03-{}",
            "aB3dE5gH7jK9mN1pQ3sT5vX7zA9cE1gI3kM5oQ7sU9wY1aC3eG5iK7mO9qS1uW3yA5cE7g"
        );
        let openai = format!(
            "sk-proj-{}",
            "Zx9Cv8Bn7Mm6Ll5Kk4Jj3Hh2Gg1Ff0DdSsAaQqWwEeRrTtYy"
        );
        let blob = format!("\0\0header{anthropic}\0text sk-ant-… the key\0\x01{openai}\"tail");
        let found = api_keys(blob.as_bytes());
        assert_eq!(
            found,
            [
                format!("sk-ant-api03…, {} characters", anthropic.len()),
                format!("sk-proj-Zx9C…, {} characters", openai.len()),
            ]
        );
        assert!(
            !found.iter().any(|f| f.contains("aB3dE5")),
            "the key itself isn't shown"
        );
        // Packed against another string, as in a binary: still a key.
        let packed = format!("ANTHROPIC_API_KEY{anthropic}\0OPENAI{openai}");
        assert_eq!(api_keys(packed.as_bytes()).len(), 2);
    }

    #[test]
    fn words_and_placeholders_are_not_keys() {
        for text in [
            "sk-ant-mine",
            // Digits, but far too short for a key.
            "sk-ant-a1b2c3d4e5",
            "placeholder=\"sk-ant-…\"",
            "task-abcdefghijklmnopqrstuvwxyzabcdefghijkl1234",
            "sk-abcdefghijklmnopqrstuvwxyzabcdefghijklmnop",
            "risk-assessment-and-mitigation-strategies-for-production",
        ] {
            assert!(api_keys(text.as_bytes()).is_empty(), "{text}");
        }
    }

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
            bundle_publisher: Some("Example AS".into()),
            database_url: None,
            migrations_dir: "migrations".into(),
        }
    }

    #[test]
    fn info_plist_registers_the_deep_link_scheme() {
        let plist = info_plist(&config(), Some("AppIcon.icns"), false);
        assert!(plist.contains("<key>CFBundleURLTypes</key>"));
        assert!(plist.contains("<string>myapp</string>"));
        assert!(plist.contains("<key>CFBundleIconFile</key><string>AppIcon.icns</string>"));
        assert!(plist.contains("<string>com.example.myapp</string>"));

        // No scheme configured -> no URL types at all.
        let mut plain = config();
        plain.bundle_deep_link = None;
        assert!(!info_plist(&plain, None, false).contains("CFBundleURLTypes"));
    }

    #[test]
    fn an_app_that_stays_in_the_tray_is_an_accessory_from_launch() {
        let plist = info_plist(&config(), None, true);
        assert!(plist.contains("<key>LSUIElement</key><true/>\n</dict>"));
        assert!(!info_plist(&config(), None, false).contains("LSUIElement"));
    }

    #[test]
    fn the_app_says_whether_it_stays_in_the_tray() {
        assert!(inspected_stay_in_tray(r#"{"stay_in_tray":true}"#));
        assert!(!inspected_stay_in_tray(r#"{"stay_in_tray":false}"#));
        // No answer (an older Elyra), or a garbled one: an ordinary app.
        assert!(!inspected_stay_in_tray(""));
        assert!(!inspected_stay_in_tray("{"));
        assert!(!inspected_stay_in_tray(r#"{"stay_in_tray":"yes"}"#));
    }

    #[test]
    fn the_app_says_who_it_is_to_windows() {
        assert_eq!(
            inspected_app_id(r#"{"stay_in_tray":false,"app_id":"com.example.notes"}"#).as_deref(),
            Some("com.example.notes")
        );
        // An older Elyra registers none: nothing to remove.
        assert_eq!(inspected_app_id(r#"{"stay_in_tray":false}"#), None);
        assert_eq!(inspected_app_id(r#"{"app_id":""}"#), None);
        assert_eq!(inspected_app_id(""), None);
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
    fn app_run_starts_the_binary_inside_the_appimage() {
        let script = app_run("my_app");
        assert!(script.starts_with("#!/bin/sh\n"));
        assert!(script.contains("readlink -f \"$0\""));
        assert!(script.ends_with("exec \"$HERE/usr/bin/my_app\" \"$@\"\n"));
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
            "Depends: libwebkit2gtk-4.1-0, libgtk-3-0",
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
    fn a_package_lists_its_directories_parents_first() {
        let entries = with_dirs(vec![
            ("./usr/bin/my_app".into(), b"bin".to_vec(), 0o755),
            (
                "./usr/share/applications/my_app.desktop".into(),
                vec![],
                0o644,
            ),
        ]);
        let paths: Vec<_> = entries.iter().map(|e| e.0.as_str()).collect();
        assert_eq!(
            paths,
            [
                "./",
                "./usr/",
                "./usr/bin/",
                "./usr/share/",
                "./usr/share/applications/",
                "./usr/bin/my_app",
                "./usr/share/applications/my_app.desktop",
            ]
        );
        let archive = tar_gz(&entries).unwrap();
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive.as_slice()));
        let kinds: Vec<_> = tar
            .entries()
            .unwrap()
            .map(|e| e.unwrap().header().entry_type().is_dir())
            .collect();
        assert_eq!(kinds, [true, true, true, true, true, false, false]);
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
