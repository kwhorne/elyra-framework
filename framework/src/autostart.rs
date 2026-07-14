//! Launch-at-login (behind the `autostart` feature), via the `auto-launch`
//! crate — LaunchAgents on macOS, the registry on Windows, and a `.desktop`
//! autostart entry on Linux.
//!
//! Exposed to the frontend as `autostart` (`enable` / `disable` / `isEnabled`)
//! and usable from Rust here. The entry points at the current executable and is
//! keyed by the [About](crate::AboutInfo) name.
//!
//! On macOS, reliable behavior generally requires a bundled `.app`.

fn instance(app: &str) -> Result<auto_launch::AutoLaunch, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    auto_launch::AutoLaunchBuilder::new()
        .set_app_name(app)
        .set_app_path(&exe.display().to_string())
        .build()
        .map_err(|e| e.to_string())
}

/// Enable launching the app at login.
pub fn enable(app: &str) -> Result<(), String> {
    instance(app)?.enable().map_err(|e| e.to_string())
}

/// Disable launching at login.
pub fn disable(app: &str) -> Result<(), String> {
    instance(app)?.disable().map_err(|e| e.to_string())
}

/// Whether launch-at-login is currently enabled.
pub fn is_enabled(app: &str) -> Result<bool, String> {
    instance(app)?.is_enabled().map_err(|e| e.to_string())
}
