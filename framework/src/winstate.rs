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

/// `<config>/<app-slug>/` — the per-app config directory (shared with the store).
pub(crate) fn app_dir(app: &str) -> Option<PathBuf> {
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

/// A screen, in the physical pixels window positions are in.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Monitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// Whether a window put back at `(x, y)` (its outer top-left, physical px),
/// `width` logical px wide, can be reached: a piece of its title bar, 64 by
/// 24 logical px, is on one of `screens`, so it can be dragged. A position
/// saved on a screen that's since been unplugged isn't, and the window would
/// open where nobody can see it. With no screens known, it's trusted.
pub(crate) fn reachable(x: i32, y: i32, width: f64, screens: &[Monitor]) -> bool {
    if screens.is_empty() {
        return true;
    }
    screens.iter().any(|m| {
        let bar = (
            f64::from(x),
            f64::from(y),
            f64::from(x) + width * m.scale,
            f64::from(y) + 24.0 * m.scale,
        );
        let screen = (
            f64::from(m.x),
            f64::from(m.y),
            f64::from(m.x) + f64::from(m.width),
            f64::from(m.y) + f64::from(m.height),
        );
        let wide = bar.2.min(screen.2) - bar.0.max(screen.0);
        let high = bar.3.min(screen.3) - bar.1.max(screen.1);
        wide >= 64.0 * m.scale && high >= 24.0 * m.scale
    })
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

    const LAPTOP: Monitor = Monitor {
        x: 0,
        y: 0,
        width: 2880,
        height: 1800,
        scale: 2.0,
    };
    const EXTERNAL: Monitor = Monitor {
        x: 2880,
        y: 0,
        width: 2560,
        height: 1440,
        scale: 1.0,
    };

    #[test]
    fn a_window_on_a_screen_is_reachable() {
        assert!(reachable(200, 100, 900.0, &[LAPTOP]));
        assert!(reachable(3000, 200, 900.0, &[LAPTOP, EXTERNAL]));
    }

    #[test]
    fn a_window_saved_on_an_unplugged_screen_is_not() {
        // Closed on the external screen, which is gone now.
        assert!(!reachable(3000, 200, 900.0, &[LAPTOP]));
        assert!(!reachable(-2000, 200, 900.0, &[LAPTOP]));
    }

    #[test]
    fn its_title_bar_must_be_on_screen_not_just_a_corner() {
        // Above the screen: the bar is out of reach though the window shows.
        assert!(!reachable(200, -100, 900.0, &[LAPTOP]));
        // Only its last 40 px on the screen's left edge: too little to grab.
        assert!(!reachable(-1760, 100, 900.0, &[LAPTOP]));
        // 200 px of it: enough.
        assert!(reachable(-1600, 100, 900.0, &[LAPTOP]));
    }

    #[test]
    fn with_no_screens_known_it_is_trusted() {
        assert!(reachable(-5000, -5000, 900.0, &[]));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not a record").is_none());
        assert!(parse("").is_none());
    }
}
