//! Native system integration (behind the `system` feature).
//!
//! The desktop capabilities almost every app needs: native **file dialogs**,
//! **opening** URLs/files in the OS, the **clipboard**, OS **notifications**,
//! and standard **paths**. The shell exposes these at `/__sys/*` and
//! `@elyra/runtime` wraps them as `dialog`, `shell`, `clipboard`, `notify`, and
//! `paths`.
//!
//! File dialogs use `rfd`'s async API (which marshals to the platform's main
//! thread internally), so they're safe to call from Elyra's tokio-driven IPC.

use serde::{Deserialize, Serialize};

/// A name + extensions pair for a file-dialog filter.
#[derive(Debug, Deserialize)]
pub struct Filter {
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// Options for an open dialog (file or folder, single or multiple).
#[derive(Debug, Default, Deserialize)]
pub struct OpenDialog {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub directory: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub start_dir: Option<String>,
}

/// Options for a save dialog.
#[derive(Debug, Default, Deserialize)]
pub struct SaveDialog {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub default_name: Option<String>,
    #[serde(default)]
    pub filters: Vec<Filter>,
    #[serde(default)]
    pub start_dir: Option<String>,
}

/// A desktop notification.
#[derive(Debug, Deserialize)]
pub struct Notification {
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
}

/// Standard OS directories + the running executable, as strings.
#[derive(Debug, Serialize)]
pub struct Paths {
    pub home: Option<String>,
    pub config: Option<String>,
    pub data: Option<String>,
    pub cache: Option<String>,
    pub temp: Option<String>,
    pub exe: Option<String>,
}

fn base(
    mut d: rfd::AsyncFileDialog,
    title: &Option<String>,
    start_dir: &Option<String>,
) -> rfd::AsyncFileDialog {
    if let Some(t) = title {
        d = d.set_title(t.as_str());
    }
    if let Some(s) = start_dir {
        d = d.set_directory(s.as_str());
    }
    d
}

fn with_filters(mut d: rfd::AsyncFileDialog, filters: &[Filter]) -> rfd::AsyncFileDialog {
    for f in filters {
        d = d.add_filter(f.name.as_str(), &f.extensions);
    }
    d
}

/// Show an open dialog. Returns the selected paths (empty if cancelled).
pub async fn open_dialog(opt: OpenDialog) -> Vec<String> {
    let d = with_filters(
        base(rfd::AsyncFileDialog::new(), &opt.title, &opt.start_dir),
        &opt.filters,
    );
    let to_str = |h: &rfd::FileHandle| h.path().display().to_string();
    match (opt.directory, opt.multiple) {
        (true, true) => d
            .pick_folders()
            .await
            .map(|v| v.iter().map(to_str).collect())
            .unwrap_or_default(),
        (true, false) => d
            .pick_folder()
            .await
            .map(|h| vec![to_str(&h)])
            .unwrap_or_default(),
        (false, true) => d
            .pick_files()
            .await
            .map(|v| v.iter().map(to_str).collect())
            .unwrap_or_default(),
        (false, false) => d
            .pick_file()
            .await
            .map(|h| vec![to_str(&h)])
            .unwrap_or_default(),
    }
}

/// Show a save dialog. Returns the chosen path, or `None` if cancelled.
pub async fn save_dialog(opt: SaveDialog) -> Option<String> {
    let mut d = with_filters(
        base(rfd::AsyncFileDialog::new(), &opt.title, &opt.start_dir),
        &opt.filters,
    );
    if let Some(name) = &opt.default_name {
        d = d.set_file_name(name.as_str());
    }
    d.save_file().await.map(|h| h.path().display().to_string())
}

/// Open a URL or path with the OS default handler.
pub fn open_external(target: &str) -> Result<(), String> {
    open::that(target).map_err(|e| e.to_string())
}

/// Read the clipboard's text contents.
pub fn clipboard_read() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_err(|e| e.to_string())
}

/// Replace the clipboard's text contents.
pub fn clipboard_write(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .map_err(|e| e.to_string())
}

/// Show an OS notification. (On macOS, delivery requires a bundled app.)
pub fn notify(n: Notification) -> Result<(), String> {
    let mut builder = notify_rust::Notification::new();
    builder.summary(&n.title);
    if let Some(body) = &n.body {
        builder.body(body);
    }
    builder.show().map(|_| ()).map_err(|e| e.to_string())
}

/// Standard OS directories + the running executable.
pub fn paths() -> Paths {
    let s = |p: std::path::PathBuf| p.display().to_string();
    Paths {
        home: dirs::home_dir().map(s),
        config: dirs::config_dir().map(s),
        data: dirs::data_dir().map(s),
        cache: dirs::cache_dir().map(s),
        temp: Some(std::env::temp_dir().display().to_string()),
        exe: std::env::current_exe().ok().map(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_reports_a_temp_dir() {
        // A pure, non-interactive smoke test: temp is always available.
        assert!(paths().temp.is_some());
    }
}
