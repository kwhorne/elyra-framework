//! The running app, for the agent working on it (RFC 0026): a debug build's
//! dev endpoint, which `rata mcp dev` reaches to list the windows, read a
//! window's page as an accessibility snapshot, act on it, take its picture
//! and read the inspector's call log.
//!
//! Debug builds only, as the inspector (RFC 0014): the module, the endpoint
//! and the page's script (`shell/agent.js`) aren't compiled into a release.
//!
//! The transport is the MCP endpoint's ([`crate::mcp::endpoint`]): a `0600`
//! socket in the app's folder (loopback on Windows), a challenge-response on
//! the per-install token, then one JSON request per line:
//!
//! ```text
//! → {"id":1,"op":"snapshot","window":"main"}
//! ← {"id":1,"ok":"route: /customers\n- banner\n  …"}
//! → {"id":2,"op":"act","window":"main","args":{"action":"click","ref":"e12"}}
//! ← {"id":2,"error":"no element e12: take a snapshot first, …"}
//! ```

use std::time::Duration;

use serde_json::{json, Value};
use tao::event_loop::EventLoopProxy;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
use tokio::sync::oneshot;

use crate::mcp::endpoint::{Reader, Writer};
use crate::window::UserEvent;

/// A request line is a small JSON object: an action's text is the longest.
const MAX_REQUEST: u64 = 64 * 1024;

/// How long the event loop has to answer one question (a page that hasn't
/// loaded never answers its script).
const ANSWER_TIMEOUT: Duration = Duration::from_secs(5);

/// How long a snapshot or an action may take in the page, settling included.
const JOB_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a job's answer is asked for.
const POLL: Duration = Duration::from_millis(40);

/// A question for the event loop, which holds the windows.
pub(crate) enum Ask {
    /// The open windows.
    Windows(oneshot::Sender<Vec<WindowInfo>>),
    /// Start `what` (`snapshot` or `act`) as job `id` in `window`'s page.
    Start {
        window: Option<String>,
        id: String,
        what: &'static str,
        args: Value,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Job `id`'s answer, once the page has it: `None` before.
    Take {
        window: Option<String>,
        id: String,
        reply: oneshot::Sender<Result<Option<Value>, String>>,
    },
    /// `window`'s picture, at most `max_width` CSS pixels wide.
    Picture {
        window: Option<String>,
        max_width: f64,
        reply: oneshot::Sender<Result<Picture, String>>,
    },
    /// The live queries `window`'s page shows, newest first, each with its
    /// arguments (RFC 0013's `showing`).
    Showing {
        window: Option<String>,
        reply: oneshot::Sender<Result<Vec<(String, Value)>, String>>,
    },
    /// The calls `window`'s pages made, newest first.
    Calls {
        window: Option<String>,
        reply: oneshot::Sender<Result<Vec<crate::inspector::CallRow>, String>>,
    },
}

/// A window, as `app_windows` lists it.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct WindowInfo {
    pub(crate) label: String,
    pub(crate) title: String,
    /// In CSS pixels.
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) visible: bool,
    pub(crate) focused: bool,
    /// The tray's popover (RFC 0019).
    pub(crate) popover: bool,
}

/// A window's picture: a PNG.
#[derive(Debug)]
pub(crate) struct Picture {
    pub(crate) png: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The widest picture, in CSS pixels (RFC 0026): an image costs the agent
/// tokens by its size.
pub(crate) const MAX_PICTURE_WIDTH: f64 = 1280.0;

/// Bind the dev endpoint for `app` and serve it on `rt`, asking `proxy`'s
/// event loop. A second debug build of the same app finds it held, and goes
/// without.
pub(crate) fn start(rt: &tokio::runtime::Runtime, app: &str, proxy: EventLoopProxy<UserEvent>) {
    let Some(endpoint) = crate::mcp::endpoint::bind_dev(app) else {
        crate::warn!(
            target: "elyra::dev",
            "no dev endpoint for coding agents (another debug build holds it, or there's \
             no app dir for its token): `rata mcp dev` can't see this one"
        );
        return;
    };
    let proxy = std::sync::Arc::new(parking_lot::Mutex::new(proxy));
    rt.spawn(endpoint.serve_with(move |reader, writer| {
        let proxy = proxy.clone();
        async move {
            crate::debug!(target: "elyra::dev", "a coding agent connected");
            if let Err(e) = serve(reader, writer, proxy).await {
                crate::debug!(target: "elyra::dev", "a dev connection ended: {e}");
            }
        }
    }));
}

type Proxy = std::sync::Arc<parking_lot::Mutex<EventLoopProxy<UserEvent>>>;

/// Answer one connection's requests, one line each, in order.
async fn serve(mut reader: Reader, mut writer: Writer, proxy: Proxy) -> std::io::Result<()> {
    loop {
        let mut line = String::new();
        let read = (&mut reader).take(MAX_REQUEST).read_line(&mut line).await?;
        if read == 0 {
            return Ok(());
        }
        if !line.ends_with('\n') {
            let answer = json!({ "id": Value::Null, "error": "the request is too long" });
            writer.write_all(format!("{answer}\n").as_bytes()).await?;
            return Ok(());
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(request) => {
                let id = request.get("id").cloned().unwrap_or(Value::Null);
                match answer(&request, &proxy).await {
                    Ok(ok) => json!({ "id": id, "ok": ok }),
                    Err(error) => json!({ "id": id, "error": error }),
                }
            }
            Err(e) => json!({ "id": Value::Null, "error": format!("not JSON: {e}") }),
        };
        writer.write_all(format!("{answer}\n").as_bytes()).await?;
        writer.flush().await?;
    }
}

/// One request's answer.
pub(crate) async fn answer(request: &Value, proxy: &Proxy) -> Result<Value, String> {
    let window = request
        .get("window")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let args = request.get("args").cloned().unwrap_or_else(|| json!({}));
    match request.get("op").and_then(Value::as_str) {
        Some("windows") => {
            let windows = ask(proxy, Ask::Windows).await?;
            Ok(json!(windows))
        }
        Some("snapshot") => {
            let page = job(proxy, window.clone(), "snapshot", Value::Null).await?;
            Ok(with_showing(page, proxy, window).await)
        }
        Some("act") => {
            let page = job(proxy, window.clone(), "act", args).await?;
            Ok(with_showing(page, proxy, window).await)
        }
        Some("screenshot") => {
            let picture = ask(proxy, |reply| Ask::Picture {
                window,
                max_width: MAX_PICTURE_WIDTH,
                reply,
            })
            .await??;
            use base64::Engine as _;
            Ok(json!({
                "png": base64::engine::general_purpose::STANDARD.encode(&picture.png),
                "width": picture.width,
                "height": picture.height,
            }))
        }
        Some("calls") => {
            let calls = ask(proxy, |reply| Ask::Calls { window, reply }).await??;
            Ok(json!(calls))
        }
        Some(other) => Err(format!(
            "unknown op `{other}`: windows, snapshot, act, screenshot or calls"
        )),
        None => Err("no `op`".into()),
    }
}

/// A snapshot with what the window shows in the app's own words under it:
/// its live queries and their arguments, as the in-app assistant's note of
/// the window has them (RFC 0013). Without any, the snapshot as it is.
async fn with_showing(page: Value, proxy: &Proxy, window: Option<String>) -> Value {
    let Value::String(page) = page else {
        return page;
    };
    let showing = ask(proxy, |reply| Ask::Showing { window, reply })
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    Value::String(showing_note(page, &showing))
}

/// `page`, then `showing:` and a line per live query: at most 16, each
/// query's arguments cut at 200 characters.
pub(crate) fn showing_note(mut page: String, showing: &[(String, Value)]) -> String {
    if showing.is_empty() {
        return page;
    }
    page.push_str("\nshowing (the window's live queries, newest first):");
    for (command, arguments) in showing.iter().take(16) {
        let mut arguments = arguments.to_string();
        if arguments.len() > 200 {
            let mut end = 200;
            while !arguments.is_char_boundary(end) {
                end -= 1;
            }
            arguments.truncate(end);
            arguments.push('…');
        }
        page.push_str(&format!("\n- {command} {arguments}"));
    }
    if showing.len() > 16 {
        page.push_str(&format!("\n- … {} more", showing.len() - 16));
    }
    page
}

/// Ask the event loop, and wait for its answer.
async fn ask<T>(proxy: &Proxy, make: impl FnOnce(oneshot::Sender<T>) -> Ask) -> Result<T, String> {
    let (reply, answer) = oneshot::channel();
    if proxy
        .lock()
        .send_event(UserEvent::Dev(make(reply)))
        .is_err()
    {
        return Err("the app is closing".into());
    }
    match tokio::time::timeout(ANSWER_TIMEOUT, answer).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(_)) => Err("the window closed before it answered".into()),
        Err(_) => Err("the window didn't answer: is its page still loading?".into()),
    }
}

/// Start `what` in the page, then ask for its answer until it's there.
async fn job(
    proxy: &Proxy,
    window: Option<String>,
    what: &'static str,
    args: Value,
) -> Result<Value, String> {
    let id = crate::security::random_token();
    ask(proxy, |reply| Ask::Start {
        window: window.clone(),
        id: id.clone(),
        what,
        args,
        reply,
    })
    .await??;
    let until = tokio::time::Instant::now() + JOB_TIMEOUT;
    loop {
        tokio::time::sleep(POLL).await;
        let taken = ask(proxy, |reply| Ask::Take {
            window: window.clone(),
            id: id.clone(),
            reply,
        })
        .await??;
        if let Some(job) = taken {
            return match job.get("error").and_then(Value::as_str) {
                Some(error) => Err(error.to_owned()),
                None => Ok(job.get("value").cloned().unwrap_or(Value::Null)),
            };
        }
        if tokio::time::Instant::now() >= until {
            return Err(format!(
                "the page didn't finish the {what} in {}s",
                JOB_TIMEOUT.as_secs()
            ));
        }
    }
}

/// The script that starts job `id` in a page: `true` once started, `false`
/// when the page has no agent script (still loading, or not Elyra's).
pub(crate) fn start_script(id: &str, what: &str, args: &Value) -> String {
    format!(
        "(() => !!window.__elyraAgent && window.__elyraAgent.start({}, {}, {}))()",
        json!(id),
        json!(what),
        args
    )
}

/// The script that reads job `id`'s answer as a JSON string (`"null"` before
/// it's done), so every platform's script callback hands back the same thing.
pub(crate) fn take_script(id: &str) -> String {
    format!(
        "JSON.stringify(window.__elyraAgent ? window.__elyraAgent.take({}) : null)",
        json!(id)
    )
}

/// A script callback's result (JSON of the script's value), as the job's
/// answer: the page returns a JSON string, so it's decoded twice.
pub(crate) fn taken(result: &str) -> Result<Option<Value>, String> {
    if result.trim().is_empty() {
        return Ok(None);
    }
    let outer: Value =
        serde_json::from_str(result).map_err(|e| format!("the page answered `{result}`: {e}"))?;
    let inner = match outer {
        Value::String(text) => {
            serde_json::from_str(&text).map_err(|e| format!("the page answered `{text}`: {e}"))?
        }
        other => other,
    };
    Ok((!inner.is_null()).then_some(inner))
}

/// Whether a start script's result says the job started.
pub(crate) fn started(result: &str) -> Result<(), String> {
    match serde_json::from_str::<Value>(result.trim()) {
        Ok(Value::Bool(true)) => Ok(()),
        _ => Err("the page has no agent script: is it still loading, or not an Elyra page?".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scripts_quote_what_they_carry() {
        let script = start_script(
            "j\"1",
            "act",
            &json!({ "action": "type", "text": "</script>\"" }),
        );
        assert_eq!(
            script,
            r#"(() => !!window.__elyraAgent && window.__elyraAgent.start("j\"1", "act", {"action":"type","text":"</script>\""}))()"#
        );
        assert_eq!(
            take_script("j\"1"),
            r#"JSON.stringify(window.__elyraAgent ? window.__elyraAgent.take("j\"1") : null)"#
        );
    }

    #[test]
    fn a_job_is_read_from_the_callbacks_json() {
        // Not done yet: the page's `null`, stringified, then JSON-encoded.
        assert_eq!(taken(r#""null""#), Ok(None));
        // No result at all (a page that answered nothing).
        assert_eq!(taken(""), Ok(None));
        let done = taken(r#""{\"done\":true,\"value\":\"route: /\"}""#)
            .unwrap()
            .unwrap();
        assert_eq!(done["value"], "route: /");
        // A platform that hands back the value itself rather than a string.
        assert_eq!(
            taken(r#"{"done":true,"value":1}"#).unwrap().unwrap()["value"],
            1
        );
        assert!(taken("nonsense").is_err());
    }

    #[test]
    fn what_the_window_shows_goes_under_the_snapshot() {
        let page = "route: /customers\n- main".to_owned();
        assert_eq!(showing_note(page.clone(), &[]), page);
        let note = showing_note(
            page,
            &[
                ("customers_index".into(), json!(["acme", 2])),
                ("customers_count".into(), json!([])),
            ],
        );
        assert_eq!(
            note,
            "route: /customers\n- main\nshowing (the window's live queries, newest first):\n- customers_index [\"acme\",2]\n- customers_count []"
        );
        // Bounded: 16 queries, and long arguments cut on a character.
        let many: Vec<(String, Value)> = (0..20).map(|i| (format!("q{i}"), json!(["é".repeat(150)]))).collect();
        let note = showing_note(String::new(), &many);
        assert!(note.ends_with("\n- … 4 more"), "{note}");
        assert!(note.contains("- q15 ") && !note.contains("- q16 "));
        assert!(note.lines().all(|l| l.chars().count() <= 220), "{note}");
    }

    #[test]
    fn a_start_says_whether_the_page_has_the_agent() {
        assert_eq!(started("true"), Ok(()));
        assert!(started("false").unwrap_err().contains("still loading"));
        assert!(started("").is_err());
    }
}
