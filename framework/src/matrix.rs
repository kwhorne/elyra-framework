//! One screen in several states at once (RFC 0034): frames side by side,
//! each a real window of the app with its own look and limits, in a debug
//! build.
//!
//! ```ignore
//! App::new().matrix(|m| {
//!     m.frame("Admin");
//!     m.frame("Can't delete").light().deny_abilities(["*.delete"]);
//!     m.frame("More contrast").contrast();
//!     m.frame("Narrow").size(380.0, 720.0);
//! })
//! ```
//!
//! ⌘⌥⇧E (Ctrl+Alt+Shift+E), the inspector's **Matrix** button or `rata dev
//! --matrix` opens the frames on the route the window shows, and they follow
//! it. A release build has none of it: [`App::matrix`](crate::App::matrix)
//! does nothing there.

// A release opens no frames: what opens them isn't compiled there.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use serde::Serialize;

use crate::security::WindowScope;
use crate::window::WindowConfig;

/// The frames an app declares, with [`App::matrix`](crate::App::matrix).
#[derive(Clone, Debug, Default)]
pub struct Matrix {
    frames: Vec<Frame>,
}

impl Matrix {
    /// A frame named `name`: the app as it is until it's told otherwise.
    pub fn frame(&mut self, name: impl Into<String>) -> &mut Frame {
        self.frames.push(Frame::new(name));
        self.frames.last_mut().expect("just pushed")
    }

    /// The frames, in the order they were declared.
    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }
}

/// A frame's theme: `data-theme` on its `<html>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

/// One frame of the [`Matrix`]: a window of the app with a look, a size and
/// fewer abilities, all optional.
#[derive(Clone, Debug)]
pub struct Frame {
    name: String,
    theme: Option<Theme>,
    contrast: bool,
    size: Option<(f64, f64)>,
    scope: WindowScope,
}

impl Frame {
    fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            theme: None,
            contrast: false,
            size: None,
            scope: WindowScope::default(),
        }
    }

    /// Dark: `data-theme="dark"`.
    pub fn dark(&mut self) -> &mut Self {
        self.theme = Some(Theme::Dark);
        self
    }

    /// Light: `data-theme="light"`.
    pub fn light(&mut self) -> &mut Self {
        self.theme = Some(Theme::Light);
        self
    }

    /// More contrast: `data-contrast="more"`, in the frame's theme.
    pub fn contrast(&mut self) -> &mut Self {
        self.contrast = true;
        self
    }

    /// The frame's inner size in CSS pixels. Without one, the frames share
    /// the screen.
    pub fn size(&mut self, width: f64, height: f64) -> &mut Self {
        self.size = Some((width, height));
        self
    }

    /// Refuse the abilities `patterns` cover in this frame, as
    /// [`WindowConfig::deny_abilities`] does a window's (RFC 0016).
    pub fn deny_abilities<S: Into<String>>(
        &mut self,
        patterns: impl IntoIterator<Item = S>,
    ) -> &mut Self {
        self.scope.deny_abilities(patterns);
        self
    }

    /// Allow only the abilities `patterns` cover in this frame, as
    /// [`WindowConfig::only_abilities`] does a window's (RFC 0016).
    pub fn only_abilities<S: Into<String>>(
        &mut self,
        patterns: impl IntoIterator<Item = S>,
    ) -> &mut Self {
        self.scope.only_abilities(patterns);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// The start of a frame's window label: `matrix:can-t-delete`.
pub(crate) const PREFIX: &str = "matrix:";

/// Whether the window labelled `label` is a frame.
pub(crate) fn is_frame(label: &str) -> bool {
    label.starts_with(PREFIX)
}

/// The frames of an app that declares none: as it is, light, more
/// contrast, and, with abilities to take away, without them.
pub(crate) fn defaults(has_abilities: bool) -> Matrix {
    let mut m = Matrix::default();
    m.frame("As it is");
    m.frame("Light").light();
    m.frame("More contrast").contrast();
    if has_abilities {
        m.frame("Without abilities").deny_abilities(["*"]);
    }
    m
}

/// A frame as its page and `rata mcp dev` see it.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Shown {
    pub(crate) name: String,
    pub(crate) theme: Option<Theme>,
    pub(crate) contrast: bool,
    /// The patterns its abilities are narrowed by, as a window's are.
    pub(crate) abilities: Option<crate::security::WindowAbilities>,
}

/// One frame to open: its window, and what the page is told.
pub(crate) struct Opening {
    pub(crate) config: WindowConfig,
    pub(crate) shown: Shown,
    /// Whether the frame chose its size; otherwise the grid gives it one.
    pub(crate) sized: bool,
}

/// The windows for `matrix`'s frames, on `path`, titled after `app`.
pub(crate) fn openings(matrix: &Matrix, app: &str, path: &str) -> Vec<Opening> {
    let mut labels = std::collections::HashSet::new();
    matrix
        .frames
        .iter()
        .map(|frame| {
            let mut label = format!("{PREFIX}{}", slug(&frame.name));
            let mut n = 2;
            while !labels.insert(label.clone()) {
                label = format!("{PREFIX}{}-{n}", slug(&frame.name));
                n += 1;
            }
            let (width, height) = frame.size.unwrap_or((640.0, 480.0));
            let mut config = WindowConfig::new(label)
                .title(format!("{app} — {}", frame.name))
                .size(width, height)
                .path(path);
            config.scope = frame.scope.clone();
            Opening {
                shown: Shown {
                    name: frame.name.clone(),
                    theme: frame.theme,
                    contrast: frame.contrast,
                    abilities: frame.scope.abilities(),
                },
                config,
                sized: frame.size.is_some(),
            }
        })
        .collect()
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "frame".into()
    } else {
        out
    }
}

/// What a frame's page is told before its script runs, and its look on
/// `<html>` already, so it never shows another first. `@elyra/runtime`
/// keeps it there.
pub(crate) fn init_script(shown: &Shown) -> String {
    let json = serde_json::to_string(shown).unwrap_or_else(|_| "null".into());
    format!(
        "globalThis.__ELYRA_FRAME__ = {json};\n\
         (() => {{ const f = globalThis.__ELYRA_FRAME__, h = document.documentElement; \
         if (!h) return; if (f.theme) h.setAttribute(\"data-theme\", f.theme); \
         if (f.contrast) h.setAttribute(\"data-contrast\", \"more\"); }})();"
    )
}

/// An area of the screen, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Area {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// Room for a frame's title bar, under the grid's cell.
const TITLE_BAR: f64 = 28.0;
const GAP: f64 = 8.0;

/// Where each of `count` frames goes on `screen`, in a grid in their order:
/// its outer position, and the inner size it gets when it chose none.
pub(crate) fn grid(screen: Area, count: usize) -> Vec<Area> {
    if count == 0 {
        return Vec::new();
    }
    let columns = (count as f64).sqrt().ceil() as usize;
    let rows = count.div_ceil(columns);
    let cell_w = screen.width / columns as f64;
    let cell_h = screen.height / rows as f64;
    (0..count)
        .map(|i| {
            let (row, column) = (i / columns, i % columns);
            Area {
                x: screen.x + column as f64 * cell_w + GAP / 2.0,
                y: screen.y + row as f64 * cell_h + GAP / 2.0,
                width: (cell_w - GAP).max(200.0),
                height: (cell_h - GAP - TITLE_BAR).max(150.0),
            }
        })
        .collect()
}

/// The frames that are open, by label, for `rata mcp dev`'s `app_windows`.
#[derive(Default)]
pub(crate) struct Open {
    frames: parking_lot::Mutex<std::collections::HashMap<String, Shown>>,
}

impl Open {
    pub(crate) fn set(&self, frames: impl IntoIterator<Item = (String, Shown)>) {
        *self.frames.lock() = frames.into_iter().collect();
    }

    pub(crate) fn get(&self, label: &str) -> Option<Shown> {
        self.frames.lock().get(label).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_windows_with_their_look_size_and_limits() {
        let mut m = Matrix::default();
        m.frame("Admin");
        m.frame("Can't delete").light().deny_abilities(["*.delete"]);
        m.frame("Narrow").size(380.0, 720.0).contrast();
        let open = openings(&m, "CRM", "#/customers");
        let labels: Vec<_> = open.iter().map(|o| o.config.label.as_str()).collect();
        assert_eq!(
            labels,
            ["matrix:admin", "matrix:can-t-delete", "matrix:narrow"]
        );
        assert_eq!(open[1].config.title, "CRM — Can't delete");
        assert_eq!(open[1].config.path, "#/customers");
        assert_eq!(open[1].shown.theme, Some(Theme::Light));
        let abilities = open[1].shown.abilities.as_ref().expect("narrowed");
        assert_eq!(abilities.deny, ["*.delete"]);
        assert!(!open[0].sized && open[2].sized);
        assert_eq!(
            (open[2].config.width, open[2].config.height),
            (380.0, 720.0)
        );
        assert!(open[2].shown.contrast);
    }

    #[test]
    fn two_frames_with_one_name_get_two_labels() {
        let mut m = Matrix::default();
        m.frame("Light").light();
        m.frame("Light").light().contrast();
        m.frame("—");
        let labels: Vec<_> = openings(&m, "A", "/")
            .into_iter()
            .map(|o| o.config.label)
            .collect();
        assert_eq!(labels, ["matrix:light", "matrix:light-2", "matrix:frame"]);
    }

    #[test]
    fn without_frames_of_its_own_an_app_gets_four_or_three() {
        let names = |m: Matrix| {
            m.frames()
                .iter()
                .map(|f| f.name().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(defaults(true)),
            ["As it is", "Light", "More contrast", "Without abilities"]
        );
        assert_eq!(
            names(defaults(false)),
            ["As it is", "Light", "More contrast"]
        );
    }

    #[test]
    fn the_grid_shares_the_screen_in_order() {
        let screen = Area {
            x: 0.0,
            y: 30.0,
            width: 1600.0,
            height: 1000.0,
        };
        let cells = grid(screen, 4);
        assert_eq!(cells.len(), 4);
        assert_eq!((cells[0].x, cells[0].y), (4.0, 34.0));
        assert_eq!((cells[1].x, cells[1].y), (804.0, 34.0));
        assert_eq!((cells[2].x, cells[2].y), (4.0, 534.0));
        assert_eq!(cells[3].width, 792.0);
        assert_eq!(cells[3].height, 500.0 - 8.0 - 28.0);
        // Three in two columns: the last row has one.
        let three = grid(screen, 3);
        assert_eq!(three[2].x, 4.0);
        assert!(grid(screen, 0).is_empty());
    }

    #[test]
    fn a_frame_s_page_is_told_its_look() {
        let shown = Shown {
            name: "Light".into(),
            theme: Some(Theme::Light),
            contrast: true,
            abilities: None,
        };
        let script = init_script(&shown);
        assert!(script.starts_with(
            r#"globalThis.__ELYRA_FRAME__ = {"name":"Light","theme":"light","contrast":true,"abilities":null};"#
        ));
        assert!(
            script.contains(r#"h.setAttribute("data-theme", f.theme)"#),
            "{script}"
        );
    }
}
