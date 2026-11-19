//! Point at it, and hand it to the agent (RFC 0035): in a debug build, the
//! developer drags over part of a window and says what to change; the
//! shell makes a UI task of it for a coding agent.
//!
//! A task is the note, the window, route and frame (RFC 0034), the elements
//! pointed at with the Svelte files and lines that drew them, the live
//! queries the window shows, and pictures of the selection and the window.
//! It's copied to the clipboard, written to `.elyra/tasks/` in the
//! workspace (under `rata dev`) or the app's folder, and kept for `rata mcp
//! dev`'s `ui_tasks` until the agent says it's done.

// A release makes no tasks: what makes them isn't compiled there.
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A rectangle in CSS pixels, from the top left of the window's page.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// An element pointed at, as the page found it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub(crate) struct Pointed {
    /// Its lines in the accessibility tree, as `window_snapshot` writes them.
    pub(crate) tree: String,
    /// `src/views/Customers.svelte:41`, from Svelte's dev metadata.
    #[serde(default)]
    pub(crate) source: Option<String>,
    /// The components above it, outermost first: `App.svelte:88 ‹Customers›`.
    #[serde(default)]
    pub(crate) stack: Vec<String>,
}

/// What the page sends when the developer hands a task off.
#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Asked {
    pub(crate) note: String,
    #[serde(default)]
    pub(crate) route: Option<String>,
    pub(crate) rect: Rect,
    /// The page's size in CSS pixels, to cut the selection from the picture.
    pub(crate) viewport: Rect,
    #[serde(default)]
    pub(crate) elements: Vec<Pointed>,
    /// Whether the page has Svelte's dev metadata (a Vite dev build).
    #[serde(default)]
    pub(crate) sources: bool,
}

/// One task, kept until the agent says it's done.
#[derive(Clone, Debug)]
pub(crate) struct Task {
    pub(crate) id: u64,
    /// The window's label, for the toast when it's done.
    pub(crate) window: String,
    pub(crate) markdown: String,
    pub(crate) selection: Option<Vec<u8>>,
    pub(crate) done: Option<String>,
}

/// The tasks of this run.
#[derive(Default)]
pub(crate) struct Tasks {
    inner: parking_lot::Mutex<(u64, Vec<Task>)>,
}

impl Tasks {
    /// The next task's id.
    pub(crate) fn next_id(&self) -> u64 {
        let mut inner = self.inner.lock();
        inner.0 += 1;
        inner.0
    }

    pub(crate) fn keep(&self, task: Task) {
        let mut inner = self.inner.lock();
        inner.1.push(task);
        // A long day's pointing: the oldest done ones go first.
        if inner.1.len() > 100 {
            if let Some(at) = inner.1.iter().position(|t| t.done.is_some()) {
                inner.1.remove(at);
            } else {
                inner.1.remove(0);
            }
        }
    }

    /// The tasks not yet done, newest first.
    pub(crate) fn pending(&self) -> Vec<Task> {
        let inner = self.inner.lock();
        inner
            .1
            .iter()
            .rev()
            .filter(|t| t.done.is_none())
            .cloned()
            .collect()
    }

    /// Mark task `id` done with `summary`: its window, or `None` if there's
    /// no such task.
    pub(crate) fn done(&self, id: u64, summary: &str) -> Option<String> {
        let mut inner = self.inner.lock();
        let task = inner.1.iter_mut().find(|t| t.id == id)?;
        task.done = Some(summary.to_owned());
        Some(task.window.clone())
    }
}

/// Where a task's files go: `.elyra/tasks/` in the workspace when `rata
/// dev` said where that is, else `tasks/` in the app's folder.
pub(crate) fn folder(app: &str) -> Option<PathBuf> {
    if let Ok(workspace) = std::env::var("ELYRA_WORKSPACE") {
        if !workspace.is_empty() {
            return Some(Path::new(&workspace).join(".elyra").join("tasks"));
        }
    }
    Some(crate::winstate::app_dir(app)?.join("tasks"))
}

/// The files a task was written to, as the Markdown names them.
pub(crate) struct Files {
    pub(crate) selection: Option<String>,
    pub(crate) window: Option<String>,
}

/// Everything about the task but what the page sent.
pub(crate) struct About<'a> {
    pub(crate) id: u64,
    pub(crate) app: &'a str,
    pub(crate) window: &'a str,
    pub(crate) live: &'a [(String, serde_json::Value)],
    pub(crate) frame: Option<&'a crate::matrix::Shown>,
    pub(crate) files: &'a Files,
}

/// At most this many lines of elements: the agent can take a snapshot.
const MAX_ELEMENT_LINES: usize = 40;

/// The task as Markdown, for the clipboard, the file and the agent.
pub(crate) fn markdown(asked: &Asked, about: &About<'_>) -> String {
    let mut out = String::new();
    let route = asked.route.as_deref().unwrap_or("/");
    let _ = writeln!(
        out,
        "## UI task {} — {}, window `{}`, route {route}\n",
        about.id, about.app, about.window
    );
    let note = asked.note.trim();
    let _ = writeln!(
        out,
        "{}\n",
        if note.is_empty() { "(no note)" } else { note }
    );

    let mut pictures = Vec::new();
    if let Some(path) = &about.files.selection {
        pictures.push(format!("picture: {path}"));
    }
    if let Some(path) = &about.files.window {
        pictures.push(format!("the whole window: {path}"));
    }
    let r = asked.rect;
    let _ = write!(
        out,
        "Pointed at {}×{} CSS pixels at ({}, {})",
        r.width.round(),
        r.height.round(),
        r.x.round(),
        r.y.round()
    );
    if pictures.is_empty() {
        out.push_str(":\n\n");
    } else {
        let _ = writeln!(out, " ({}):\n", pictures.join(", "));
    }

    if asked.elements.is_empty() {
        out.push_str("- nothing the accessibility tree names\n");
    }
    let mut lines = 0;
    for element in &asked.elements {
        let mut tree = element.tree.lines();
        let Some(first) = tree.next() else { continue };
        if lines >= MAX_ELEMENT_LINES {
            out.push_str("- …\n");
            break;
        }
        let first = first.trim_start().trim_start_matches("- ");
        let _ = write!(out, "- {first}");
        if let Some(source) = &element.source {
            let _ = write!(out, " — {source}");
        }
        if !element.stack.is_empty() {
            let _ = write!(out, " (in {})", element.stack.join(" › "));
        }
        out.push('\n');
        lines += 1;
        for line in tree {
            if lines >= MAX_ELEMENT_LINES {
                out.push_str("  - …\n");
                break;
            }
            let _ = writeln!(out, "  {line}");
            lines += 1;
        }
    }
    if !asked.sources {
        out.push_str("\nNo source locations: not a Vite dev build.\n");
    }

    if about.live.is_empty() {
        out.push_str("\nThe window shows no live queries.\n");
    } else {
        let shown: Vec<String> = about
            .live
            .iter()
            .map(|(command, args)| match args {
                // No arguments: just the query.
                serde_json::Value::Null => command.clone(),
                serde_json::Value::Array(a) if a.is_empty() => command.clone(),
                serde_json::Value::Object(o) if o.is_empty() => command.clone(),
                args => format!("{command} {args}"),
            })
            .collect();
        let _ = writeln!(
            out,
            "\nThe window shows the live queries {}.",
            shown.join(", ")
        );
    }
    match about.frame {
        Some(frame) => {
            let mut look = Vec::new();
            if let Some(theme) = frame.theme {
                look.push(format!("{theme:?}").to_lowercase());
            }
            if frame.contrast {
                look.push("more contrast".into());
            }
            if let Some(abilities) = &frame.abilities {
                if let Some(only) = &abilities.only {
                    look.push(format!("only {}", only.join(", ")));
                }
                if !abilities.deny.is_empty() {
                    look.push(format!("without {}", abilities.deny.join(", ")));
                }
            }
            let look = if look.is_empty() {
                "as the app is".into()
            } else {
                look.join(", ")
            };
            let _ = writeln!(out, "Frame: {:?} ({look}).", frame.name);
        }
        None => out.push_str("Frame: none (the app as it is).\n"),
    }
    out
}

/// `rect` of the page, cut from `png` (the window's picture), where the
/// picture is `scale` of its pixels to a CSS pixel.
pub(crate) fn cut(png: &[u8], rect: Rect, scale: f64) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("the picture isn't a PNG: {e}"))?;
    let mut pixels = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or("the picture is too large")?
    ];
    let info = reader
        .next_frame(&mut pixels)
        .map_err(|e| format!("the picture can't be read: {e}"))?;
    let channels = info.color_type.samples();
    let clamp = |v: f64, max: u32| (v * scale).round().clamp(0.0, f64::from(max)) as u32;
    let (x0, y0) = (clamp(rect.x, info.width), clamp(rect.y, info.height));
    let (x1, y1) = (
        clamp(rect.x + rect.width, info.width),
        clamp(rect.y + rect.height, info.height),
    );
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return Err("the selection is outside the picture".into());
    }
    let mut out = Vec::with_capacity((w * h) as usize * channels);
    for y in y0..y1 {
        let start = y as usize * info.line_size + x0 as usize * channels;
        out.extend_from_slice(&pixels[start..start + w as usize * channels]);
    }
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, w, h);
        encoder.set_color(info.color_type);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&out).map_err(|e| e.to_string())?;
    }
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked() -> Asked {
        Asked {
            note: "Replace this table with a SplitPane.".into(),
            route: Some("/customers?search=acme".into()),
            rect: Rect {
                x: 10.4,
                y: 20.0,
                width: 300.0,
                height: 120.6,
            },
            viewport: Rect {
                width: 900.0,
                height: 640.0,
                ..Rect::default()
            },
            elements: vec![
                Pointed {
                    tree: "- table \"Customers\" (2 rows)\n  - row \"Acme AS Oslo\" [ref=e4]"
                        .into(),
                    source: Some("src/views/Customers.svelte:41".into()),
                    stack: vec!["src/App.svelte:88 ‹Customers›".into()],
                },
                Pointed {
                    tree: "- button \"New customer\" [ref=e1]".into(),
                    source: Some("src/views/Customers.svelte:12".into()),
                    stack: vec![],
                },
            ],
            sources: true,
        }
    }

    #[test]
    fn a_task_says_what_where_and_from_which_line() {
        let files = Files {
            selection: Some(".elyra/tasks/7-selection.png".into()),
            window: Some(".elyra/tasks/7-window.png".into()),
        };
        let live = [
            (
                "customers_index".to_owned(),
                serde_json::json!({ "search": "acme" }),
            ),
            ("list_todos".to_owned(), serde_json::json!([])),
        ];
        let frame = crate::matrix::Shown {
            name: "Can't delete".into(),
            theme: Some(crate::matrix::Theme::Light),
            contrast: false,
            abilities: Some(crate::security::WindowAbilities {
                only: None,
                deny: vec!["*.delete".into()],
            }),
        };
        let text = markdown(
            &asked(),
            &About {
                id: 7,
                app: "CRM",
                window: "matrix:can-t-delete",
                live: &live,
                frame: Some(&frame),
                files: &files,
            },
        );
        assert!(text.starts_with(
            "## UI task 7 — CRM, window `matrix:can-t-delete`, route /customers?search=acme\n\nReplace this table with a SplitPane.\n\n"
        ), "{text}");
        assert!(text.contains(
            "Pointed at 300×121 CSS pixels at (10, 20) (picture: .elyra/tasks/7-selection.png, the whole window: .elyra/tasks/7-window.png):"
        ), "{text}");
        assert!(text.contains(
            "- table \"Customers\" (2 rows) — src/views/Customers.svelte:41 (in src/App.svelte:88 ‹Customers›)\n    - row \"Acme AS Oslo\" [ref=e4]\n"
        ), "{text}");
        assert!(
            text.contains("- button \"New customer\" [ref=e1] — src/views/Customers.svelte:12\n")
        );
        assert!(text.contains(
            "The window shows the live queries customers_index {\"search\":\"acme\"}, list_todos."
        ));
        assert!(
            text.contains("Frame: \"Can't delete\" (light, without *.delete)."),
            "{text}"
        );
        assert!(!text.contains("No source locations"));
    }

    #[test]
    fn without_vite_pictures_or_queries_it_says_so() {
        let mut asked = asked();
        asked.sources = false;
        asked.elements.clear();
        asked.note = "  ".into();
        let text = markdown(
            &asked,
            &About {
                id: 1,
                app: "CRM",
                window: "main",
                live: &[],
                frame: None,
                files: &Files {
                    selection: None,
                    window: None,
                },
            },
        );
        assert!(text.contains("(no note)"));
        assert!(text.contains("at (10, 20):\n\n- nothing the accessibility tree names"));
        assert!(text.contains("No source locations: not a Vite dev build."));
        assert!(text.contains("The window shows no live queries."));
        assert!(text.contains("Frame: none (the app as it is)."));
    }

    #[test]
    fn the_lines_are_cut_at_forty() {
        let mut asked = asked();
        asked.elements = vec![Pointed {
            tree: (0..100)
                .map(|i| format!("- row \"{i}\""))
                .collect::<Vec<_>>()
                .join("\n"),
            ..Pointed::default()
        }];
        let files = Files {
            selection: None,
            window: None,
        };
        let text = markdown(
            &asked,
            &About {
                id: 1,
                app: "A",
                window: "main",
                live: &[],
                frame: None,
                files: &files,
            },
        );
        assert!(text.contains("row \"39\""));
        assert!(!text.contains("row \"40\""));
        assert!(text.contains("  - …"));
    }

    #[test]
    fn the_selection_is_cut_from_the_picture_at_its_scale() {
        // 4×4, each pixel's red its x and green its y.
        let mut pixels = Vec::new();
        for y in 0..4u8 {
            for x in 0..4u8 {
                pixels.extend_from_slice(&[x, y, 0, 255]);
            }
        }
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 4, 4);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&pixels)
                .unwrap();
        }
        // Two picture pixels per CSS pixel: CSS (1, 0) 1×2 is pixels x 2..4, y 0..4.
        let rect = Rect {
            x: 1.0,
            y: 0.0,
            width: 1.0,
            height: 2.0,
        };
        let out = cut(&png, rect, 2.0).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(&out))
            .read_info()
            .unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 4));
        assert_eq!(&buf[0..4], &[2, 0, 0, 255]);
        assert_eq!(&buf[buf.len() - 4..], &[3, 3, 0, 255]);
        assert!(cut(
            &png,
            Rect {
                x: 10.0,
                y: 10.0,
                width: 5.0,
                height: 5.0
            },
            2.0
        )
        .is_err());
    }

    #[test]
    fn tasks_wait_until_done_newest_first() {
        let tasks = Tasks::default();
        for window in ["main", "settings"] {
            let id = tasks.next_id();
            tasks.keep(Task {
                id,
                window: window.into(),
                markdown: format!("task {id}"),
                selection: None,
                done: None,
            });
        }
        let ids: Vec<_> = tasks.pending().iter().map(|t| t.id).collect();
        assert_eq!(ids, [2, 1]);
        assert_eq!(
            tasks.done(1, "Swapped it for a SplitPane").as_deref(),
            Some("main")
        );
        assert_eq!(tasks.pending().len(), 1);
        assert_eq!(tasks.done(9, "no such"), None);
    }
}
