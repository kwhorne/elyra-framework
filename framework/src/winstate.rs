//! Optional window-geometry persistence (opt-in via [`App::persist_window_state`]).
//!
//! Remembers the primary window's size, position, and maximized state between
//! runs, in a small file under the OS config directory. Dependency-free: the
//! record is five whitespace-separated fields, so no `serde_json` is pulled in.
//!
//! [`App::persist_window_state`]: crate::App::persist_window_state

use std::path::PathBuf;

/// Saved primary-window geometry.
#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    pub width: f64,
    pub height: f64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

/// `<config>/<app-slug>/` — the directory the state file lives in.
fn app_dir(app: &str) -> Option<PathBuf> {
    let slug: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let name = if slug.is_empty() { "elyra-app" } else { slug };
    Some(config_base()?.join(name))
}

fn config_base() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        return std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    }
    #[cfg(target_os = "windows")]
    {
        return std::env::var_os("APPDATA").map(PathBuf::from);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            return Some(PathBuf::from(x));
        }
        return std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config"));
    }
    #[allow(unreachable_code)]
    None
}

/// Load the saved geometry, if any.
pub(crate) fn load(app: &str) -> Option<Geometry> {
    let text = std::fs::read_to_string(app_dir(app)?.join("window-state")).ok()?;
    parse(&text)
}

/// Persist the geometry (best-effort; errors are ignored).
pub(crate) fn save(app: &str, g: Geometry) {
    let Some(dir) = app_dir(app) else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("window-state"), encode(g));
}

fn encode(g: Geometry) -> String {
    let opt = |v: Option<i32>| v.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
    format!(
        "{} {} {} {} {}",
        g.width,
        g.height,
        opt(g.x),
        opt(g.y),
        u8::from(g.maximized),
    )
}

fn parse(text: &str) -> Option<Geometry> {
    let mut it = text.split_whitespace();
    let width = it.next()?.parse().ok()?;
    let height = it.next()?.parse().ok()?;
    let coord = |s: &str| -> Option<Option<i32>> {
        if s == "-" {
            Some(None)
        } else {
            s.parse().ok().map(Some)
        }
    };
    let x = coord(it.next()?)?;
    let y = coord(it.next()?)?;
    let maximized = it.next()? != "0";
    Some(Geometry {
        width,
        height,
        x,
        y,
        maximized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_roundtrips() {
        let g = Geometry {
            width: 900.0,
            height: 640.5,
            x: Some(-12),
            y: None,
            maximized: true,
        };
        let p = parse(&encode(g)).expect("parse");
        assert_eq!(p.width, 900.0);
        assert_eq!(p.height, 640.5);
        assert_eq!(p.x, Some(-12));
        assert_eq!(p.y, None);
        assert!(p.maximized);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not a record").is_none());
        assert!(parse("").is_none());
    }
}
