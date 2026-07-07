//! The native shell: tao window + wry webview + the `elyra://` custom protocol.
//!
//! Everything the frontend touches lives under a single origin,
//! `elyra://localhost` — the app is served from `/`, commands from `/__cmd/*`,
//! and the event stream from `/__events`. Same origin means no CORS, no
//! preflight, no `data:`-URL games.
//!
//! ## Threading (macOS)
//! The event loop and webview live on the main thread. A separate multi-thread
//! tokio runtime owns all IPC work: the **asynchronous** custom-protocol handler
//! spawns each request onto the runtime and responds from there, so the UI
//! thread never blocks on a command or a long-poll (this replaces M0's
//! `block_on`).

use std::borrow::Cow;
use std::sync::Arc;

use std::collections::HashMap;
use tao::dpi::LogicalSize;

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder, WindowId};
use wry::http::{header, Request, Response, StatusCode};
use wry::{WebView, WebViewBuilder};

use crate::assets::{AssetResolver, FALLBACK_HTML};
use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::event::EventBus;
use crate::window::{UserEvent, WindowConfig};

const SCHEME: &str = "elyra";
const CMD_PREFIX: &str = "/__cmd/";
const EVENTS_PATH: &str = "/__events";

type Body = Response<Cow<'static, [u8]>>;

/// Shared state captured by the protocol handler.
struct Runner {
    registry: Arc<CommandRegistry>,
    ctx: Ctx,
    bus: EventBus,
    assets: Option<AssetResolver>,
    rt: tokio::runtime::Runtime,
}

/// Run the event loop with the given initial windows. Diverges until the last
/// window closes. New windows can be opened at runtime via `Windows`.
// `tray_handle` is intentionally write-only: it's held for the program's
// lifetime to keep the tray icon visible, never read again.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "tray", allow(unused_assignments, unused_variables))]
pub(crate) fn run(
    rt: tokio::runtime::Runtime,
    event_loop: EventLoop<UserEvent>,
    registry: Arc<CommandRegistry>,
    ctx: Ctx,
    bus: EventBus,
    assets: Option<AssetResolver>,
    window_configs: Vec<WindowConfig>,
    tray: Option<crate::tray::TrayConfig>,
) -> crate::Result<()> {
    // Route tray menu clicks through the event loop via a user event.
    #[cfg(feature = "tray")]
    if tray.is_some() {
        let proxy = event_loop.create_proxy();
        tray_icon::menu::MenuEvent::set_event_handler(Some(
            move |event: tray_icon::menu::MenuEvent| {
                let _ = proxy.send_event(UserEvent::MenuClick(event.id.0));
            },
        ));
    }
    #[cfg(not(feature = "tray"))]
    let _ = &tray;

    let runner = Arc::new(Runner {
        registry,
        ctx,
        bus,
        assets,
        rt,
    });

    // Build the initial windows up front, keyed by id so we can drop each on
    // close and exit when none remain.
    let mut windows: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    for config in &window_configs {
        let (window, webview) = build_window(&event_loop, &runner, config);
        windows.insert(window.id(), (window, webview));
    }

    // The tray must be created after the loop initializes (macOS); hold it alive.
    #[cfg(feature = "tray")]
    let mut tray_config = tray;
    #[cfg(feature = "tray")]
    let mut tray_handle: Option<tray_icon::TrayIcon> = None;

    // Native macOS app menu (an Edit menu is what makes ⌘C/⌘V/⌘X reach the
    // webview); held alive for the program's lifetime.
    #[cfg(target_os = "macos")]
    let app_name = window_configs
        .first()
        .map(|c| c.title.clone())
        .unwrap_or_else(|| "Elyra".to_string());
    #[cfg(target_os = "macos")]
    let mut _app_menu: Option<muda::Menu> = None;

    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                #[cfg(target_os = "macos")]
                {
                    _app_menu = Some(macos_app_menu(&app_name));
                }
                #[cfg(feature = "tray")]
                if let Some(config) = tray_config.take() {
                    match crate::tray::build(&config) {
                        Ok(handle) => tray_handle = Some(handle),
                        Err(e) => eprintln!("tray: {e}"),
                    }
                }
            }
            #[cfg(feature = "tray")]
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                if id == crate::tray::QUIT_ID {
                    *control_flow = ControlFlow::Exit;
                } else {
                    let _ = runner.bus.emit("tray", &id);
                }
            }
            Event::UserEvent(UserEvent::OpenWindow(config)) => {
                let (window, webview) = build_window(target, &runner, &config);
                windows.insert(window.id(), (window, webview));
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::CloseRequested,
                ..
            } => {
                windows.remove(&window_id);
                if windows.is_empty() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            _ => {}
        }
    })
}

/// Install a standard macOS application menu with an Edit menu, so the system
/// routes Cut/Copy/Paste/Select-All/Undo/Redo to the focused webview text field.
/// Returns the menu, which must be kept alive.
#[cfg(target_os = "macos")]
fn macos_app_menu(app_name: &str) -> muda::Menu {
    use muda::{Menu, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    let app = Submenu::new(app_name, true);
    let _ = app.append_items(&[
        &PredefinedMenuItem::about(None, None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::services(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::hide(None),
        &PredefinedMenuItem::hide_others(None),
        &PredefinedMenuItem::show_all(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::quit(None),
    ]);

    let edit = Submenu::new("Edit", true);
    let _ = edit.append_items(&[
        &PredefinedMenuItem::undo(None),
        &PredefinedMenuItem::redo(None),
        &PredefinedMenuItem::separator(),
        &PredefinedMenuItem::cut(None),
        &PredefinedMenuItem::copy(None),
        &PredefinedMenuItem::paste(None),
        &PredefinedMenuItem::select_all(None),
    ]);

    let _ = menu.append(&app);
    let _ = menu.append(&edit);
    menu.init_for_nsapp();
    menu
}

/// Build a window + its webview, wired to the shared protocol handler.
fn build_window(
    target: &EventLoopWindowTarget<UserEvent>,
    runner: &Arc<Runner>,
    config: &WindowConfig,
) -> (Window, WebView) {
    let mut builder = WindowBuilder::new()
        .with_title(&config.title)
        .with_inner_size(LogicalSize::new(config.width, config.height))
        .with_resizable(config.resizable)
        .with_decorations(config.decorations)
        .with_always_on_top(config.always_on_top);
    if let Some((min_w, min_h)) = config.min_size {
        builder = builder.with_min_inner_size(LogicalSize::new(min_w, min_h));
    }
    let window = builder.build(target).expect("failed to build window");

    // In `rata dev`, pages are served by Vite (HMR) at a cross-origin http://
    // URL; IPC still targets elyra://localhost, so CORS is added in `route`.
    let base = std::env::var("ELYRA_DEV_URL").unwrap_or_else(|_| format!("{SCHEME}://localhost"));
    let url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        config.path.trim_start_matches('/')
    );

    let handler = runner.clone();
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_asynchronous_custom_protocol(SCHEME.into(), move |_id, request, responder| {
            let runner = handler.clone();
            // Never touch the UI thread for real work — hand it to tokio.
            let handle = runner.rt.handle().clone();
            handle.spawn(async move {
                let response = route(&runner, request).await;
                responder.respond(response);
            });
        })
        .build(&window)
        .expect("failed to build webview");

    (window, webview)
}

async fn route(runner: &Runner, request: Request<Vec<u8>>) -> Body {
    // CORS preflight (only reachable from the cross-origin dev server).
    if request.method() == wry::http::Method::OPTIONS {
        return with_cors(
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Cow::Borrowed(b"".as_slice()))
                .unwrap(),
        );
    }

    let path = request.uri().path().to_owned();

    if path == EVENTS_PATH {
        return with_cors(serve_events(runner).await);
    }

    if let Some(name) = path.strip_prefix(CMD_PREFIX) {
        return with_cors(serve_command(runner, name, request.into_body()).await);
    }

    serve_asset(runner, &path)
}

/// Add permissive CORS headers so the dev server's http:// origin can reach the
/// elyra:// IPC endpoints. Harmless in production (same-origin).
fn with_cors(mut response: Body) -> Body {
    let headers = response.headers_mut();
    headers.insert("access-control-allow-origin", "*".parse().unwrap());
    headers.insert(
        "access-control-allow-methods",
        "GET, POST, OPTIONS".parse().unwrap(),
    );
    headers.insert(
        "access-control-allow-headers",
        "content-type, accept".parse().unwrap(),
    );
    response
}

/// Long-poll: block until the next event batch is ready, then respond. The
/// frontend reconnects immediately, giving a continuous binary stream.
async fn serve_events(runner: &Runner) -> Body {
    let batch = runner.bus.next_batch().await;
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(batch))
        .unwrap()
}

async fn serve_command(runner: &Runner, name: &str, body: Vec<u8>) -> Body {
    match runner
        .registry
        .clone()
        .dispatch(runner.ctx.clone(), name, &body)
        .await
    {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/msgpack")
            .header("x-elyra-status", "ok")
            .body(Cow::Owned(bytes))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .header("x-elyra-status", "error")
            .body(Cow::Owned(err.to_string().into_bytes()))
            .unwrap(),
    }
}

fn serve_asset(runner: &Runner, path: &str) -> Body {
    let rel = match path.trim_start_matches('/') {
        "" => "index.html",
        other => other,
    };

    if let Some(resolver) = &runner.assets {
        if let Some(asset) = resolver(rel) {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, asset.mime)
                .body(Cow::Owned(asset.bytes))
                .unwrap();
        }
    }

    // No embedded frontend yet — serve the dependency-free demo page.
    if rel == "index.html" {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Cow::Borrowed(FALLBACK_HTML.as_bytes()))
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Cow::Borrowed(b"not found".as_slice()))
        .unwrap()
}
