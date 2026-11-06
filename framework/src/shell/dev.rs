//! A coding agent's questions (RFC 0026), answered on the event loop, which
//! holds the windows and their webviews. Debug builds only.

use std::collections::HashMap;

use tao::window::{Window, WindowId};
use wry::WebView;

use super::Runner;
use crate::dev::{Ask, WindowInfo};

/// The page's script for the agent: added to each page in a debug build.
pub(super) const AGENT_SCRIPT: &str = include_str!("agent.js");

/// The windows, as the event loop holds them.
pub(super) struct Open<'a> {
    pub(super) windows: &'a HashMap<WindowId, (Window, WebView)>,
    pub(super) labels: &'a HashMap<WindowId, String>,
    pub(super) focused: Option<WindowId>,
    pub(super) popover: Option<WindowId>,
}

impl Open<'_> {
    /// The window `label` names (a shown one first, labels aren't unique),
    /// or, without one, the focused window, else the first shown one.
    fn find(&self, label: Option<&str>) -> Result<WindowId, String> {
        let shown_first = |ids: &mut Vec<WindowId>| {
            ids.sort_by_key(|id| !self.windows.get(id).is_some_and(|(w, _)| w.is_visible()));
        };
        match label {
            Some(label) => {
                let mut ids: Vec<WindowId> = self
                    .labels
                    .iter()
                    .filter(|(_, l)| l.as_str() == label)
                    .map(|(id, _)| *id)
                    .collect();
                shown_first(&mut ids);
                ids.first().copied().ok_or_else(|| {
                    let mut labels: Vec<&str> = self.labels.values().map(String::as_str).collect();
                    labels.sort_unstable();
                    format!(
                        "no window `{label}`: the open ones are {}",
                        labels.join(", ")
                    )
                })
            }
            None => {
                if let Some(id) = self.focused.filter(|id| self.windows.contains_key(id)) {
                    return Ok(id);
                }
                let mut ids: Vec<WindowId> = self.windows.keys().copied().collect();
                shown_first(&mut ids);
                ids.first()
                    .copied()
                    .ok_or_else(|| "no window is open".to_owned())
            }
        }
    }

    fn webview(&self, label: Option<&str>) -> Result<(WindowId, &Window, &WebView), String> {
        let id = self.find(label)?;
        let (window, webview) = self.windows.get(&id).ok_or("the window closed")?;
        Ok((id, window, webview))
    }
}

/// One reply, from a script callback the platform may call more than once.
fn once<T: Send + 'static>(reply: tokio::sync::oneshot::Sender<T>) -> impl Fn(T) + Send + 'static {
    let reply = parking_lot::Mutex::new(Some(reply));
    move |value| {
        if let Some(reply) = reply.lock().take() {
            let _ = reply.send(value);
        }
    }
}

/// Answer `ask` from the open windows.
pub(super) fn answer(ask: Ask, open: Open<'_>, runner: &Runner) {
    match ask {
        Ask::Windows(reply) => {
            let mut list: Vec<WindowInfo> = open
                .windows
                .iter()
                .map(|(id, (window, _))| {
                    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
                    WindowInfo {
                        label: open.labels.get(id).cloned().unwrap_or_default(),
                        title: window.title(),
                        width: size.width,
                        height: size.height,
                        visible: window.is_visible(),
                        focused: open.focused == Some(*id),
                        popover: open.popover == Some(*id),
                    }
                })
                .collect();
            list.sort_by(|a, b| a.label.cmp(&b.label));
            let _ = reply.send(list);
        }
        Ask::Start {
            window,
            id,
            what,
            args,
            reply,
        } => {
            let answer = once(reply);
            match open.webview(window.as_deref()) {
                Ok((_, _, webview)) => {
                    let script = crate::dev::start_script(&id, what, &args);
                    let answered = {
                        let answer = std::sync::Arc::new(answer);
                        let callback = answer.clone();
                        let sent = webview.evaluate_script_with_callback(&script, move |result| {
                            callback(crate::dev::started(&result));
                        });
                        sent.map_err(|e| (answer, e))
                    };
                    if let Err((answer, e)) = answered {
                        answer(Err(format!("the page can't run the script: {e}")));
                    }
                }
                Err(why) => answer(Err(why)),
            }
        }
        Ask::Take { window, id, reply } => {
            let answer = std::sync::Arc::new(once(reply));
            match open.webview(window.as_deref()) {
                Ok((_, _, webview)) => {
                    let callback = answer.clone();
                    let sent = webview.evaluate_script_with_callback(
                        &crate::dev::take_script(&id),
                        move |result| {
                            callback(crate::dev::taken(&result));
                        },
                    );
                    if let Err(e) = sent {
                        answer(Err(format!("the page can't run the script: {e}")));
                    }
                }
                Err(why) => answer(Err(why)),
            }
        }
        Ask::Picture {
            window,
            max_width,
            reply,
        } => match open.webview(window.as_deref()) {
            Ok((_, window, webview)) => super::picture::take(window, webview, max_width, reply),
            Err(why) => {
                let _ = reply.send(Err(why));
            }
        },
        Ask::Showing { window, reply } => {
            let showing = open.find(window.as_deref()).map(|id| {
                // The page now shown: the newest client id the window's
                // pages used (a reload is a new one).
                let page = runner.clients.of_native(id).pop();
                #[cfg(feature = "database")]
                if let (Some(client), Some(live)) =
                    (page, runner.ctx.try_get::<crate::live::LiveRegistry>())
                {
                    return live
                        .showing(&client)
                        .into_iter()
                        .map(|(command, args)| {
                            let arguments = rmp_serde::from_slice::<serde_json::Value>(&args)
                                .unwrap_or(serde_json::Value::Null);
                            (command, arguments)
                        })
                        .collect();
                }
                #[cfg(not(feature = "database"))]
                let _ = page;
                Vec::new()
            });
            let _ = reply.send(showing);
        }
        Ask::Calls { window, reply } => {
            let rows = open.find(window.as_deref()).map(|id| {
                let Some(calls) = runner.ctx.try_get::<crate::inspector::Calls>() else {
                    return Vec::new();
                };
                // A window's calls are kept by the client ids its pages used
                // (a reload is a new one).
                let mut rows: Vec<crate::inspector::CallRow> = runner
                    .clients
                    .of_native(id)
                    .iter()
                    .flat_map(|client| calls.of(client))
                    .collect();
                rows.sort_by_key(|row| std::cmp::Reverse(row.at));
                rows.truncate(crate::inspector::Calls::KEEP);
                rows
            });
            let _ = reply.send(rows);
        }
    }
}
