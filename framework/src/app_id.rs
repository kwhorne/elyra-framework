//! The app's identity in Windows ([RFC 0030]): the AppUserModelID its
//! notifications are shown under. Without one, a desktop app's notification
//! is shown as PowerShell's, with PowerShell's icon.
//!
//! It's [`App::identifier`] (`"com.example.notes"`), or one made of the app's
//! name (`Notes` → `notes`). At each start the app writes it under
//! `HKCU\Software\Classes\AppUserModelId\<id>`, with its name and icon, as
//! Windows reads them for a notification; the installer's uninstaller
//! removes it.
//!
//! [RFC 0030]: https://github.com/kwhorne/elyra-framework/blob/main/docs/proposals/0030-windows-app-identity.md
//! [`App::identifier`]: crate::App::identifier

use std::sync::OnceLock;

/// The running app's, once [`App::run`](crate::App::run) has set it.
static ID: OnceLock<String> = OnceLock::new();

/// Set once the app has said who it is (its window started): a
/// notification under an id Windows can't name may not show at all, so
/// one before (headless, `--mcp`) goes out as before.
#[cfg_attr(not(all(target_os = "windows", feature = "system")), allow(dead_code))]
static REGISTERED: OnceLock<()> = OnceLock::new();

/// `App::identifier`, or one made of the app's name.
pub(crate) fn of(identifier: Option<&str>, name: &str) -> String {
    match identifier {
        Some(id) => id.to_string(),
        None => from_name(name),
    }
}

/// `Elyra Example` → `elyra-example`: letters and digits, lowercase, the
/// rest a hyphen.
fn from_name(name: &str) -> String {
    let id: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let id = id.trim_matches('-');
    if id.is_empty() {
        "elyra-app".into()
    } else {
        id.into()
    }
}

/// What's wrong with an `App::identifier`: Windows takes up to 128
/// characters, and no spaces.
pub(crate) fn problem(id: &str) -> Option<String> {
    let fits = !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    (!fits).then(|| {
        format!(
            "App::identifier({id:?}): up to 128 letters, digits, dots, hyphens and \
             underscores, as in \"com.example.notes\""
        )
    })
}

pub(crate) fn set(id: String) {
    let _ = ID.set(id);
}

#[cfg_attr(not(all(target_os = "windows", feature = "system")), allow(dead_code))]
fn get() -> Option<&'static str> {
    ID.get().map(String::as_str)
}

/// The id notifications go out under: once the app has said who it is.
#[cfg_attr(not(all(target_os = "windows", feature = "system")), allow(dead_code))]
pub(crate) fn registered() -> Option<&'static str> {
    REGISTERED.get().and(get())
}

/// Say who the app is, for its notifications: its name and, when it has
/// one, its icon (written as a PNG beside its settings). Idempotent, at
/// every start.
#[cfg(all(target_os = "windows", feature = "system"))]
pub(crate) fn register(name: &str, icon: Option<&crate::Icon>) {
    let Some(id) = get() else {
        return;
    };
    let key = format!("Software\\Classes\\AppUserModelId\\{id}");
    if let Err(e) = registry::set(&key, "DisplayName", name) {
        crate::warn!(target: "elyra::notifications", "couldn't name the app's notifications: {e}");
        return;
    }
    let _ = REGISTERED.set(());
    let Some(png) = icon.and_then(crate::Icon::pixels).and_then(png) else {
        return;
    };
    let Some(dir) = crate::winstate::app_dir(name) else {
        return;
    };
    let path = dir.join("notification-icon.png");
    let unchanged = std::fs::read(&path).is_ok_and(|old| old == png);
    if !unchanged
        && (std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, &png).is_err())
    {
        return;
    }
    let _ = registry::set(&key, "IconUri", &path.display().to_string());
}

/// The icon as a PNG, for Windows to read from a file.
#[cfg_attr(not(all(target_os = "windows", feature = "system")), allow(dead_code))]
fn png(pixels: &crate::icon::Pixels) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, pixels.width, pixels.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&pixels.rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

#[cfg(all(target_os = "windows", feature = "system"))]
mod registry {
    use windows::core::PCWSTR;
    use windows::Win32::System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    /// A string value under `HKEY_CURRENT_USER\<key>`, the key made when
    /// it isn't there.
    pub(super) fn set(key: &str, name: &str, value: &str) -> Result<(), String> {
        let (key, name, value) = (wide(key), wide(name), wide(value));
        // SAFETY: each string is NUL-terminated UTF-16 that outlives the call,
        // and `cbdata` is the value's length in bytes, its NUL included.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(value.as_ptr().cast()),
                (value.len() * 2) as u32,
            )
        };
        if status.is_ok() {
            Ok(())
        } else {
            Err(format!("RegSetKeyValueW: {}", status.0))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identifier_or_one_made_of_the_name() {
        assert_eq!(of(Some("com.example.notes"), "Notes"), "com.example.notes");
        assert_eq!(of(None, "Elyra Example"), "elyra-example");
        assert_eq!(of(None, "Æ Notes!"), "notes");
        assert_eq!(of(None, "  "), "elyra-app");
        assert_eq!(of(None, ""), "elyra-app");
    }

    #[test]
    fn what_windows_takes() {
        assert_eq!(problem("com.example.notes"), None);
        assert_eq!(problem("my_app-2"), None);
        assert!(problem("").is_some());
        assert!(problem("com example").is_some(), "a space");
        assert!(problem("com.eksempel.notatær").is_some());
        assert!(problem(&"a".repeat(129)).is_some());
        assert_eq!(problem(&"a".repeat(128)), None);
    }

    #[test]
    fn the_icon_reads_back_as_it_was() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| i as u8).collect();
        let icon = crate::Icon::rgba(rgba.clone(), 4, 3);
        let bytes = png(icon.pixels().unwrap()).unwrap();
        let back = crate::Icon::png(&bytes);
        assert_eq!(back.size(), Some((4, 3)));
        assert_eq!(back.pixels().unwrap().rgba, rgba);
    }

    /// The whole way, on Windows: registered, a notification shown through
    /// the `Notifier` under the id, and Windows' own history of the id's
    /// notifications has it. It shows a notification, so CI runs it
    /// (`--ignored`).
    #[cfg(all(target_os = "windows", feature = "system"))]
    #[test]
    #[ignore = "shows a notification; CI runs it on Windows"]
    fn a_notification_is_shown_as_the_app() {
        let id = "dev.elyra.notification-test";
        set(id.into());
        register("Elyra notification test", None);
        let title = format!("Elyra test {}", std::process::id());
        crate::notifier::Notifier::system()
            .show(
                crate::notifier::Shown {
                    title: title.clone(),
                    body: Some("From the framework's tests".into()),
                },
                None,
            )
            .unwrap();
        let history = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, \
             ContentType = WindowsRuntime] > $null; \
             [Windows.UI.Notifications.ToastNotificationManager]::History.GetHistory('{id}') \
             | ForEach-Object {{ $_.Content.GetXml() }}"
        );
        let mut seen = String::new();
        for _ in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let out = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", &history])
                .output()
                .unwrap();
            seen = format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            if seen.contains(&title) {
                break;
            }
        }
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                &format!("HKCU\\Software\\Classes\\AppUserModelId\\{id}"),
                "/f",
            ])
            .output();
        assert!(
            seen.contains(&title),
            "Windows has no notification of {id} titled {title:?}: {seen}"
        );
    }
}
