//! The Windows installer `rata bundle` makes with NSIS: a `…-setup.exe` that
//! installs the app for the user who runs it, with no administrator's
//! prompt, into `%LOCALAPPDATA%\Programs\<Name>`, where the app can also
//! replace itself when it updates (`Updater`, a loose executable). It adds a
//! Start menu shortcut, the URL scheme of `[bundle].deep_link`, and an entry
//! in Settings → Apps whose Uninstall removes all of it but the user's data,
//! with what the app wrote of itself: its launch at login, and the name and
//! icon of its notifications.
//!
//! The script is written here, as text tested on every platform;
//! `makensis` turns it into the installer.

use std::path::Path;

/// What goes into the installer.
pub struct Installer<'a> {
    pub name: &'a str,
    pub identifier: &'a str,
    pub version: &'a str,
    pub publisher: Option<&'a str>,
    /// The executable's name as installed (`my_app.exe`).
    pub exe: &'a str,
    /// The built executable, to put in the installer.
    pub exe_path: &'a Path,
    /// The `.ico` for the installer, its uninstaller and their windows.
    pub icon: Option<&'a Path>,
    pub deep_link: Option<&'a str>,
    /// The AppUserModelID the app registers its notifications under, as it
    /// said when asked: the uninstaller removes it.
    pub app_id: Option<&'a str>,
    /// The installer to write.
    pub out: &'a Path,
    /// What's installed, in KB, for Settings → Apps.
    pub size_kb: u64,
}

/// A string as NSIS reads one between double quotes: `$` and `"` escaped,
/// and no line breaks.
fn quoted(text: &str) -> String {
    let escaped: String = text
        .chars()
        .filter(|c| *c != '\r' && *c != '\n')
        .map(|c| match c {
            '$' => "$$".to_string(),
            '"' => "$\\\"".to_string(),
            c => c.to_string(),
        })
        .collect();
    format!("\"{escaped}\"")
}

/// `name` as a file or folder name: what Windows refuses in one replaced.
pub fn file_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .collect();
    let safe = safe.trim().trim_end_matches('.').to_string();
    if safe.is_empty() {
        "App".into()
    } else {
        safe
    }
}

/// The installer's script.
pub fn script(i: &Installer) -> String {
    let folder = file_name(i.name);
    let exe = i.exe;
    let uninstall = format!(
        "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}",
        i.identifier
    );
    let own = format!("Software\\{}", i.identifier);
    let [a, b, c, d] = crate::windows_res::numeric_version(i.version);
    let q = quoted;

    let mut s = String::new();
    let mut line = |text: String| {
        s.push_str(&text);
        s.push('\n');
    };
    line("; Written by `rata bundle`; run with `makensis`. Installs for the user".into());
    line("; who runs it, with no administrator's prompt.".into());
    line("Unicode true".into());
    line("ManifestDPIAware true".into());
    line("RequestExecutionLevel user".into());
    line("SetCompressor /SOLID lzma".into());
    line("!include \"MUI2.nsh\"".into());
    line(String::new());
    line(format!("Name {}", q(i.name)));
    line(format!("OutFile {}", q(&i.out.display().to_string())));
    line(format!(
        "InstallDir \"$LOCALAPPDATA\\Programs\\{}\"",
        escaped(&folder)
    ));
    line(format!("InstallDirRegKey HKCU {} \"InstallDir\"", q(&own)));
    line(format!(
        "BrandingText {}",
        q(&format!("{} {}", i.name, i.version))
    ));
    line(format!("VIProductVersion \"{a}.{b}.{c}.{d}\""));
    line(format!("VIAddVersionKey \"ProductName\" {}", q(i.name)));
    line(format!(
        "VIAddVersionKey \"ProductVersion\" {}",
        q(i.version)
    ));
    line(format!("VIAddVersionKey \"FileVersion\" {}", q(i.version)));
    line(format!(
        "VIAddVersionKey \"FileDescription\" {}",
        q(&format!("{} Setup", i.name))
    ));
    if let Some(publisher) = i.publisher {
        line(format!("VIAddVersionKey \"CompanyName\" {}", q(publisher)));
    }
    line(String::new());
    if let Some(icon) = i.icon {
        let icon = q(&icon.display().to_string());
        line(format!("!define MUI_ICON {icon}"));
        line(format!("!define MUI_UNICON {icon}"));
    }
    line(format!(
        "!define MUI_FINISHPAGE_RUN \"$INSTDIR\\{}\"",
        escaped(exe)
    ));
    line("!insertmacro MUI_PAGE_DIRECTORY".into());
    line("!insertmacro MUI_PAGE_INSTFILES".into());
    line("!insertmacro MUI_PAGE_FINISH".into());
    line("!insertmacro MUI_UNPAGE_CONFIRM".into());
    line("!insertmacro MUI_UNPAGE_INSTFILES".into());
    line("!insertmacro MUI_LANGUAGE \"English\"".into());
    line(String::new());

    let shortcut = format!("\"$SMPROGRAMS\\{}.lnk\"", escaped(&folder));
    let installed = format!("\"$INSTDIR\\{}\"", escaped(exe));
    line("Section \"Install\"".into());
    line("  SetOutPath \"$INSTDIR\"".into());
    line(format!(
        "  File {} {}",
        q(&format!("/oname={exe}")),
        q(&i.exe_path.display().to_string())
    ));
    line("  WriteUninstaller \"$INSTDIR\\uninstall.exe\"".into());
    line(format!("  CreateShortCut {shortcut} {installed}"));
    line(format!(
        "  WriteRegStr HKCU {} \"InstallDir\" \"$INSTDIR\"",
        q(&own)
    ));
    let key = q(&uninstall);
    for (name, value) in [
        ("DisplayName", q(i.name)),
        ("DisplayVersion", q(i.version)),
        ("DisplayIcon", installed.clone()),
        ("InstallLocation", "\"$INSTDIR\"".to_string()),
        (
            "UninstallString",
            "'\"$INSTDIR\\uninstall.exe\"'".to_string(),
        ),
        (
            "QuietUninstallString",
            "'\"$INSTDIR\\uninstall.exe\" /S'".to_string(),
        ),
    ] {
        line(format!("  WriteRegStr HKCU {key} \"{name}\" {value}"));
    }
    if let Some(publisher) = i.publisher {
        line(format!(
            "  WriteRegStr HKCU {key} \"Publisher\" {}",
            q(publisher)
        ));
    }
    line(format!(
        "  WriteRegDWORD HKCU {key} \"EstimatedSize\" {}",
        i.size_kb
    ));
    line(format!("  WriteRegDWORD HKCU {key} \"NoModify\" 1"));
    line(format!("  WriteRegDWORD HKCU {key} \"NoRepair\" 1"));
    if let Some(scheme) = i.deep_link {
        // As `App::deep_link` registers it at launch, so the link works
        // before the app's first run.
        let classes = format!("Software\\Classes\\{scheme}");
        line(format!(
            "  WriteRegStr HKCU {} \"\" {}",
            q(&classes),
            q(&format!("URL:{scheme}"))
        ));
        line(format!(
            "  WriteRegStr HKCU {} \"URL Protocol\" \"\"",
            q(&classes)
        ));
        line(format!(
            "  WriteRegStr HKCU {} \"\" '\"$INSTDIR\\{}\",0'",
            q(&format!("{classes}\\DefaultIcon")),
            escaped(exe)
        ));
        line(format!(
            "  WriteRegStr HKCU {} \"\" '\"$INSTDIR\\{}\" \"%1\"'",
            q(&format!("{classes}\\shell\\open\\command")),
            escaped(exe)
        ));
    }
    line("SectionEnd".into());
    line(String::new());

    line("Section \"Uninstall\"".into());
    line(format!("  Delete {installed}"));
    // What an update left beside it (the updater keeps the replaced one).
    line(format!(
        "  Delete \"$INSTDIR\\{}.old\"",
        escaped(exe.trim_end_matches(".exe"))
    ));
    line("  Delete \"$INSTDIR\\uninstall.exe\"".into());
    line("  RMDir \"$INSTDIR\"".into());
    line(format!("  Delete {shortcut}"));
    line(format!("  DeleteRegKey HKCU {key}"));
    line(format!("  DeleteRegKey HKCU {}", q(&own)));
    // Launch at login (`autostart`), keyed by the app's name.
    line(format!(
        "  DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Run\" {}",
        q(i.name)
    ));
    if let Some(scheme) = i.deep_link {
        line(format!(
            "  DeleteRegKey HKCU {}",
            q(&format!("Software\\Classes\\{scheme}"))
        ));
    }
    // The name and icon of its notifications (RFC 0030), written at each start.
    if let Some(id) = i.app_id {
        line(format!(
            "  DeleteRegKey HKCU {}",
            q(&format!("Software\\Classes\\AppUserModelId\\{id}"))
        ));
    }
    line("SectionEnd".into());
    s
}

/// `text` escaped for inside an NSIS string, without the quotes.
fn escaped(text: &str) -> String {
    let q = quoted(text);
    q[1..q.len() - 1].to_string()
}

/// `makensis`: on the `PATH`, or where NSIS's own installer puts it.
#[cfg(target_os = "windows")]
pub fn makensis() -> Option<std::path::PathBuf> {
    let on_path = std::process::Command::new("makensis")
        .arg("/VERSION")
        .output()
        .is_ok_and(|o| o.status.success());
    if on_path {
        return Some("makensis".into());
    }
    ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|dir| Path::new(&dir).join("NSIS").join("makensis.exe"))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installer<'a>(exe_path: &'a Path, icon: Option<&'a Path>, out: &'a Path) -> Installer<'a> {
        Installer {
            name: "My \"App\"",
            identifier: "com.example.myapp",
            version: "1.2.3",
            publisher: Some("Example $Corp"),
            exe: "my_app.exe",
            exe_path,
            icon,
            deep_link: Some("myapp"),
            app_id: Some("com.example.myapp"),
            out,
            size_kb: 12_345,
        }
    }

    #[test]
    fn it_installs_for_the_user_alone() {
        let script = script(&installer(
            Path::new(r"C:\p\target\release\my_app.exe"),
            Some(Path::new(r"C:\p\app.ico")),
            Path::new(r"C:\p\out\My App_1.2.3_x64-setup.exe"),
        ));
        for line in [
            "RequestExecutionLevel user",
            "Unicode true",
            // No quote in a folder name; the name itself is kept, escaped.
            "InstallDir \"$LOCALAPPDATA\\Programs\\My -App-\"",
            "Name \"My $\\\"App$\\\"\"",
            "VIProductVersion \"1.2.3.0\"",
            "VIAddVersionKey \"CompanyName\" \"Example $$Corp\"",
            "!define MUI_ICON \"C:\\p\\app.ico\"",
            "!define MUI_UNICON \"C:\\p\\app.ico\"",
            "  File \"/oname=my_app.exe\" \"C:\\p\\target\\release\\my_app.exe\"",
            "  CreateShortCut \"$SMPROGRAMS\\My -App-.lnk\" \"$INSTDIR\\my_app.exe\"",
            "OutFile \"C:\\p\\out\\My App_1.2.3_x64-setup.exe\"",
        ] {
            assert!(script.contains(line), "missing `{line}` in:\n{script}");
        }
        assert!(!script.contains("HKLM"), "nothing machine-wide");
    }

    #[test]
    fn settings_lists_it_and_its_uninstall() {
        let script = script(&installer(Path::new("a.exe"), None, Path::new("o.exe")));
        let key =
            "HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\com.example.myapp\"";
        for value in [
            "\"DisplayName\" \"My $\\\"App$\\\"\"",
            "\"DisplayVersion\" \"1.2.3\"",
            "\"DisplayIcon\" \"$INSTDIR\\my_app.exe\"",
            "\"UninstallString\" '\"$INSTDIR\\uninstall.exe\"'",
            "\"QuietUninstallString\" '\"$INSTDIR\\uninstall.exe\" /S'",
            "\"Publisher\" \"Example $$Corp\"",
        ] {
            let line = format!("WriteRegStr {key} {value}");
            assert!(script.contains(&line), "missing `{line}` in:\n{script}");
        }
        assert!(script.contains(&format!("WriteRegDWORD {key} \"EstimatedSize\" 12345")));
        assert!(script.contains(&format!("DeleteRegKey {key}")));
        assert!(!script.contains("MUI_ICON"), "no icon, NSIS's own");
    }

    #[test]
    fn the_url_scheme_is_registered_and_removed() {
        let script = script(&installer(Path::new("a.exe"), None, Path::new("o.exe")));
        for line in [
            "WriteRegStr HKCU \"Software\\Classes\\myapp\" \"\" \"URL:myapp\"",
            "WriteRegStr HKCU \"Software\\Classes\\myapp\" \"URL Protocol\" \"\"",
            "WriteRegStr HKCU \"Software\\Classes\\myapp\\shell\\open\\command\" \"\" '\"$INSTDIR\\my_app.exe\" \"%1\"'",
            "DeleteRegKey HKCU \"Software\\Classes\\myapp\"",
        ] {
            assert!(script.contains(line), "missing `{line}` in:\n{script}");
        }
        let mut plain = installer(Path::new("a.exe"), None, Path::new("o.exe"));
        plain.deep_link = None;
        plain.app_id = None;
        assert!(!super::script(&plain).contains("Classes"));
    }

    #[test]
    fn uninstalling_leaves_nothing_of_the_app_but_its_data() {
        let script = script(&installer(Path::new("a.exe"), None, Path::new("o.exe")));
        let uninstall = &script[script.find("Section \"Uninstall\"").unwrap()..];
        for line in [
            "Delete \"$INSTDIR\\my_app.exe\"",
            "Delete \"$INSTDIR\\my_app.old\"",
            "Delete \"$INSTDIR\\uninstall.exe\"",
            "RMDir \"$INSTDIR\"",
            "Delete \"$SMPROGRAMS\\My -App-.lnk\"",
            "DeleteRegValue HKCU \"Software\\Microsoft\\Windows\\CurrentVersion\\Run\" \"My $\\\"App$\\\"\"",
            // What the app wrote at each start for its notifications (RFC 0030).
            "DeleteRegKey HKCU \"Software\\Classes\\AppUserModelId\\com.example.myapp\"",
        ] {
            assert!(uninstall.contains(line), "missing `{line}` in:\n{uninstall}");
        }
        assert!(!uninstall.contains("RMDir /r"), "never a whole folder");
    }

    #[test]
    fn a_name_becomes_a_file_name() {
        assert_eq!(file_name("My App"), "My App");
        assert_eq!(file_name("A/B: C?"), "A-B- C-");
        assert_eq!(file_name("Dots..."), "Dots");
        assert_eq!(file_name("  "), "App");
    }
}
