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
use tao::dpi::{LogicalSize, PhysicalPosition};

use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use tao::window::{Fullscreen, Window, WindowBuilder, WindowId};
use wry::http::{header, Request, Response, StatusCode};
use wry::{WebView, WebViewBuilder};

use crate::about::AboutInfo;
use crate::assets::{AssetResolver, FALLBACK_HTML};
use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::event::EventBus;
use crate::window::{UserEvent, WindowAction, WindowConfig, Windows};

const SCHEME: &str = "elyra";
const CMD_PREFIX: &str = "/__cmd/";
const EVENTS_PATH: &str = "/__events";
const ABOUT_PATH: &str = "/__about";

/// Menu id of the built-in "About <App>" item; clicking it opens the dialog.
#[cfg(any(target_os = "macos", feature = "tray"))]
const ABOUT_MENU_ID: &str = "__elyra_about";

type Body = Response<Cow<'static, [u8]>>;

/// Shared state captured by the protocol handler.
struct Runner {
    registry: Arc<CommandRegistry>,
    ctx: Ctx,
    bus: EventBus,
    assets: Option<AssetResolver>,
    rt: tokio::runtime::Runtime,
    about: AboutInfo,
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
    mut window_configs: Vec<WindowConfig>,
    tray: Option<crate::tray::TrayConfig>,
    about: AboutInfo,
    persist_window: bool,
    #[cfg_attr(not(feature = "shortcuts"), allow(unused_variables))] shortcuts: Vec<String>,
) -> crate::Result<()> {
    // Route menu clicks (macOS app menu + tray) through the event loop. On macOS
    // both the app menu and the tray use muda under the hood, so one handler
    // covers both; elsewhere the tray uses tray_icon's own menu event.
    #[cfg(target_os = "macos")]
    {
        let proxy = event_loop.create_proxy();
        muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
            let _ = proxy.send_event(UserEvent::MenuClick(event.id.0));
        }));
    }
    #[cfg(all(not(target_os = "macos"), feature = "tray"))]
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

    // Route global-shortcut presses through the event loop (like menu clicks).
    #[cfg(feature = "shortcuts")]
    {
        let proxy = event_loop.create_proxy();
        global_hotkey::GlobalHotKeyEvent::set_event_handler(Some(
            move |event: global_hotkey::GlobalHotKeyEvent| {
                if event.state == global_hotkey::HotKeyState::Pressed {
                    let _ = proxy.send_event(UserEvent::Shortcut(event.id));
                }
            },
        ));
    }

    let runner = Arc::new(Runner {
        registry,
        ctx,
        bus,
        assets,
        rt,
        about,
    });

    // Silent update check on startup (emits `elyra:update` if one is available).
    #[cfg(feature = "updater")]
    spawn_startup_update_check(&runner);

    // Build the initial windows up front, keyed by id so we can drop each on
    // close and exit when none remain.
    // Restore saved geometry into the primary window's config before building.
    let mut restored: Option<crate::winstate::Geometry> = None;
    if persist_window {
        if let Some(g) = crate::winstate::load(&runner.about.name) {
            if let Some(c) = window_configs.first_mut() {
                c.width = g.width;
                c.height = g.height;
            }
            restored = Some(g);
        }
    }

    let mut windows: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    let mut id_label: HashMap<WindowId, String> = HashMap::new();
    let mut focused: Option<WindowId> = None;
    let mut primary_id: Option<WindowId> = None;
    for (i, config) in window_configs.iter().enumerate() {
        let (window, webview) = build_window(&event_loop, &runner, config);
        if i == 0 {
            primary_id = Some(window.id());
            if let Some(g) = restored {
                if let (Some(x), Some(y)) = (g.x, g.y) {
                    window.set_outer_position(PhysicalPosition::new(x, y));
                }
                if g.maximized {
                    window.set_maximized(true);
                }
            }
        }
        id_label.insert(window.id(), config.label.clone());
        windows.insert(window.id(), (window, webview));
    }

    // The tray must be created after the loop initializes (macOS); hold it alive.
    #[cfg(feature = "tray")]
    let mut tray_config = tray;
    #[cfg(feature = "tray")]
    let mut tray_handle: Option<tray_icon::TrayIcon> = None;

    // Global-shortcut manager (held for the program's lifetime) + id -> accelerator.
    #[cfg(feature = "shortcuts")]
    let mut _hotkey_manager: Option<global_hotkey::GlobalHotKeyManager> = None;
    #[cfg(feature = "shortcuts")]
    let mut shortcut_ids: HashMap<u32, String> = HashMap::new();

    // Native macOS app menu (an Edit menu is what makes ⌘C/⌘V/⌘X reach the
    // webview); held alive for the program's lifetime.
    #[cfg(target_os = "macos")]
    let app_name = if !runner.about.name.is_empty() {
        runner.about.name.clone()
    } else {
        window_configs
            .first()
            .map(|c| c.title.clone())
            .unwrap_or_else(|| "Elyra".to_string())
    };
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
                #[cfg(feature = "shortcuts")]
                {
                    match global_hotkey::GlobalHotKeyManager::new() {
                        Ok(manager) => {
                            for accel in &shortcuts {
                                match accel.parse::<global_hotkey::hotkey::HotKey>() {
                                    Ok(hk) => {
                                        if manager.register(hk).is_ok() {
                                            shortcut_ids.insert(hk.id(), accel.clone());
                                        }
                                    }
                                    Err(e) => eprintln!("shortcut '{accel}': {e}"),
                                }
                            }
                            _hotkey_manager = Some(manager);
                        }
                        Err(e) => eprintln!("global shortcuts unavailable: {e}"),
                    }
                }
            }
            #[cfg(feature = "shortcuts")]
            Event::UserEvent(UserEvent::Shortcut(id)) => {
                if let Some(accel) = shortcut_ids.get(&id) {
                    let _ = runner.bus.emit("elyra:shortcut", accel);
                }
            }
            #[cfg(any(target_os = "macos", feature = "tray"))]
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                if id == ABOUT_MENU_ID {
                    // Open the built-in About dialog (the runtime listens here).
                    let _ = runner.bus.emit("elyra:about", &runner.about);
                } else {
                    #[cfg(feature = "tray")]
                    if id == crate::tray::QUIT_ID {
                        *control_flow = ControlFlow::Exit;
                    } else {
                        let _ = runner.bus.emit("tray", &id);
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenWindow(config)) => {
                let (window, webview) = build_window(target, &runner, &config);
                id_label.insert(window.id(), config.label.clone());
                windows.insert(window.id(), (window, webview));
            }
            Event::UserEvent(UserEvent::Window(cmd)) => {
                let target_id = cmd
                    .label
                    .as_deref()
                    .and_then(|l| {
                        id_label
                            .iter()
                            .find(|(_, v)| v.as_str() == l)
                            .map(|(k, _)| *k)
                    })
                    .or(focused)
                    .or_else(|| windows.keys().next().copied());
                if let Some(id) = target_id {
                    apply_window_action(&mut windows, &mut id_label, id, cmd.action, control_flow);
                }
            }
            Event::WindowEvent {
                window_id, event, ..
            } => match event {
                WindowEvent::CloseRequested => {
                    if persist_window && Some(window_id) == primary_id {
                        if let Some((w, _)) = windows.get(&window_id) {
                            save_geometry(&runner.about.name, w);
                        }
                    }
                    windows.remove(&window_id);
                    id_label.remove(&window_id);
                    if windows.is_empty() {
                        *control_flow = ControlFlow::Exit;
                    }
                }
                WindowEvent::Focused(f) => {
                    if f {
                        focused = Some(window_id);
                    } else if persist_window && Some(window_id) == primary_id {
                        if let Some((w, _)) = windows.get(&window_id) {
                            save_geometry(&runner.about.name, w);
                        }
                    }
                    emit_window_state(&runner, &windows, &id_label, window_id, focused);
                }
                WindowEvent::Resized(_) | WindowEvent::Moved(_) => {
                    emit_window_state(&runner, &windows, &id_label, window_id, focused);
                }
                _ => {}
            },
            _ => {}
        }
    })
}

/// Install a standard macOS application menu with an Edit menu, so the system
/// routes Cut/Copy/Paste/Select-All/Undo/Redo to the focused webview text field.
/// Returns the menu, which must be kept alive.
#[cfg(target_os = "macos")]
fn macos_app_menu(app_name: &str) -> muda::Menu {
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();

    // A custom "About" item (instead of the system panel) so clicking it opens
    // the framework's themed dialog via the `elyra:about` event.
    let about = MenuItem::with_id(ABOUT_MENU_ID, format!("About {app_name}"), true, None);

    let app = Submenu::new(app_name, true);
    let _ = app.append_items(&[
        &about,
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
    let dnd = runner.clone();
    let webview = WebViewBuilder::new()
        .with_url(url)
        .with_drag_drop_handler(move |event| {
            if let wry::DragDropEvent::Drop { paths, .. } = event {
                let files: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
                let _ = dnd.bus.emit("elyra:file-drop", &files);
            }
            true
        })
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

async fn route(runner: &Arc<Runner>, request: Request<Vec<u8>>) -> Body {
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

    if path == ABOUT_PATH {
        return with_cors(serve_about(runner));
    }

    if let Some(op) = path.strip_prefix("/__window/") {
        let op = op.to_owned();
        return with_cors(serve_window(runner, &op, request.into_body()));
    }

    #[cfg(feature = "updater")]
    if path == "/__update/check" {
        return with_cors(serve_update_check(runner).await);
    }
    #[cfg(feature = "updater")]
    if path == "/__update/install" {
        return with_cors(serve_update_install(runner));
    }

    #[cfg(feature = "system")]
    if let Some(op) = path.strip_prefix("/__sys/") {
        let op = op.to_owned();
        return with_cors(serve_system(&op, request.into_body()).await);
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

/// Serve the app's About metadata as MessagePack (named map -> object).
fn serve_about(runner: &Runner) -> Body {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(runner.about.to_msgpack()))
        .unwrap()
}

/// Encode a serializable value as a MessagePack (named-map) response.
fn msgpack_ok<T: serde::Serialize>(value: &T) -> Body {
    let bytes = rmp_serde::to_vec_named(value).unwrap_or_default();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/msgpack")
        .header(header::CACHE_CONTROL, "no-store")
        .header("x-elyra-status", "ok")
        .body(Cow::Owned(bytes))
        .unwrap()
}

/// A phase update on the `elyra:update` channel, consumed by the runtime toast.
#[cfg(feature = "updater")]
#[derive(serde::Serialize)]
struct UpdatePhase {
    phase: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    notes: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    progress: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[cfg(feature = "updater")]
impl UpdatePhase {
    fn available(version: String, notes: Option<String>) -> Self {
        Self {
            phase: "available",
            version: Some(version),
            notes,
            progress: None,
            message: None,
        }
    }
    fn downloading(progress: u8) -> Self {
        Self {
            phase: "downloading",
            version: None,
            notes: None,
            progress: Some(progress),
            message: None,
        }
    }
    fn simple(phase: &'static str) -> Self {
        Self {
            phase,
            version: None,
            notes: None,
            progress: None,
            message: None,
        }
    }
    fn error(message: String) -> Self {
        Self {
            phase: "error",
            version: None,
            notes: None,
            progress: None,
            message: Some(message),
        }
    }
}

/// Spawn the silent startup update check (no-op if the updater isn't configured
/// or auto-check is disabled).
#[cfg(feature = "updater")]
fn spawn_startup_update_check(runner: &Arc<Runner>) {
    let Some(rt) = runner.ctx.try_get::<crate::updater::UpdaterRuntime>() else {
        return;
    };
    if !rt.auto_check {
        return;
    }
    let runner = Arc::clone(runner);
    let handle = runner.rt.handle().clone();
    handle.spawn(async move {
        update_check_and_emit(&runner).await;
    });
}

/// Run a check and, if an update is available, emit an `available` phase.
#[cfg(feature = "updater")]
async fn update_check_and_emit(runner: &Arc<Runner>) {
    use crate::updater::{UpdateStatus, UpdaterRuntime};
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return;
    };
    let rt2 = rt.clone();
    let result =
        tokio::task::spawn_blocking(move || rt2.updater.check(&rt2.manifest_url, &rt2.target))
            .await;
    if let Ok(Ok(UpdateStatus::Available(info))) = result {
        let _ = runner.bus.emit(
            "elyra:update",
            &UpdatePhase::available(info.version, info.notes),
        );
    }
}

/// `GET /__update/check` — report whether a newer release exists.
#[cfg(feature = "updater")]
async fn serve_update_check(runner: &Runner) -> Body {
    use crate::updater::{UpdateCheck, UpdaterRuntime};
    let err = |message: String| UpdateCheck {
        available: false,
        version: None,
        notes: None,
        error: Some(message),
    };
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return msgpack_ok(&err("updater not configured".into()));
    };
    let rt2 = rt.clone();
    let check = match tokio::task::spawn_blocking(move || {
        rt2.updater.check(&rt2.manifest_url, &rt2.target)
    })
    .await
    {
        Ok(Ok(status)) => UpdateCheck::from(status),
        Ok(Err(e)) => err(e.to_string()),
        Err(e) => err(e.to_string()),
    };
    msgpack_ok(&check)
}

/// `POST /__update/install` — download + verify + apply in the background,
/// streaming progress over `elyra:update`. Returns immediately.
#[cfg(feature = "updater")]
fn serve_update_install(runner: &Arc<Runner>) -> Body {
    let runner = Arc::clone(runner);
    tokio::spawn(async move { run_update_install(runner).await });
    msgpack_ok(&true)
}

#[cfg(feature = "updater")]
async fn run_update_install(runner: Arc<Runner>) {
    use crate::updater::{UpdateStatus, Updater, UpdaterRuntime};
    let Some(rt) = runner.ctx.try_get::<UpdaterRuntime>() else {
        return;
    };
    let bus = runner.bus.clone();

    // Re-check to obtain the signed artifact URL + signature.
    let rt_check = rt.clone();
    let status = match tokio::task::spawn_blocking(move || {
        rt_check
            .updater
            .check(&rt_check.manifest_url, &rt_check.target)
    })
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return emit_err(&bus, e.to_string()),
        Err(e) => return emit_err(&bus, e.to_string()),
    };
    let info = match status {
        UpdateStatus::Available(info) => info,
        UpdateStatus::UpToDate => {
            let _ = bus.emit("elyra:update", &UpdatePhase::simple("up-to-date"));
            return;
        }
    };

    let bus_dl = bus.clone();
    let rt_dl = rt.clone();
    let staged = tokio::task::spawn_blocking(move || {
        rt_dl
            .updater
            .download_verified_with_progress(&info, |got, total| {
                let pct = match total {
                    Some(t) if t > 0 => ((got.saturating_mul(100)) / t) as u8,
                    _ => 0,
                };
                let _ = bus_dl.emit("elyra:update", &UpdatePhase::downloading(pct));
            })
    })
    .await;

    let staged = match staged {
        Ok(Ok(path)) => path,
        Ok(Err(e)) => return emit_err(&bus, e.to_string()),
        Err(e) => return emit_err(&bus, e.to_string()),
    };

    let _ = bus.emit("elyra:update", &UpdatePhase::simple("ready"));
    // Let the frontend paint "Restarting…" before we re-exec.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    if let Err(e) = Updater::apply_and_relaunch(&staged) {
        emit_err(&bus, e.to_string());
    }
}

#[cfg(feature = "updater")]
fn emit_err(bus: &crate::event::EventBus, message: String) {
    let _ = bus.emit("elyra:update", &UpdatePhase::error(message));
}

/// A plain-text error response (mirrors the command error shape).
fn msgpack_err(message: String) -> Body {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header("x-elyra-status", "error")
        .body(Cow::Owned(message.into_bytes()))
        .unwrap()
}

/// Dispatch a `/__sys/<op>` native-system call. Args arrive as a MessagePack
/// body; results are MessagePack (errors surface via `x-elyra-status`).
#[cfg(feature = "system")]
async fn serve_system(op: &str, body: Vec<u8>) -> Body {
    use crate::system;
    match op {
        "dialog.open" => match rmp_serde::from_slice::<system::OpenDialog>(&body) {
            Ok(opt) => msgpack_ok(&system::open_dialog(opt).await),
            Err(e) => msgpack_err(e.to_string()),
        },
        "dialog.save" => match rmp_serde::from_slice::<system::SaveDialog>(&body) {
            Ok(opt) => msgpack_ok(&system::save_dialog(opt).await),
            Err(e) => msgpack_err(e.to_string()),
        },
        "shell.open" => match rmp_serde::from_slice::<String>(&body) {
            Ok(target) => match system::open_external(&target) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "clipboard.read" => match system::clipboard_read() {
            Ok(text) => msgpack_ok(&text),
            Err(e) => msgpack_err(e),
        },
        "clipboard.write" => match rmp_serde::from_slice::<String>(&body) {
            Ok(text) => match system::clipboard_write(&text) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "notify" => match rmp_serde::from_slice::<system::Notification>(&body) {
            Ok(n) => match system::notify(n) {
                Ok(()) => msgpack_ok(&()),
                Err(e) => msgpack_err(e),
            },
            Err(e) => msgpack_err(e.to_string()),
        },
        "paths" => msgpack_ok(&system::paths()),
        other => msgpack_err(format!("unknown system op: {other}")),
    }
}

/// Serializable window state pushed on the `elyra:window` channel.
#[derive(serde::Serialize)]
struct WindowState<'a> {
    label: &'a str,
    width: f64,
    height: f64,
    maximized: bool,
    fullscreen: bool,
    focused: bool,
}

/// Persist the primary window's geometry for the next run.
fn save_geometry(app: &str, window: &Window) {
    let scale = window.scale_factor();
    let size = window.inner_size();
    let pos = window.outer_position().ok();
    crate::winstate::save(
        app,
        crate::winstate::Geometry {
            width: size.width as f64 / scale,
            height: size.height as f64 / scale,
            x: pos.map(|p| p.x),
            y: pos.map(|p| p.y),
            maximized: window.is_maximized(),
        },
    );
}

fn emit_window_state(
    runner: &Runner,
    windows: &HashMap<WindowId, (Window, WebView)>,
    id_label: &HashMap<WindowId, String>,
    id: WindowId,
    focused: Option<WindowId>,
) {
    if let Some((window, _)) = windows.get(&id) {
        let scale = window.scale_factor();
        let size = window.inner_size();
        let _ = runner.bus.emit(
            "elyra:window",
            &WindowState {
                label: id_label.get(&id).map(String::as_str).unwrap_or(""),
                width: size.width as f64 / scale,
                height: size.height as f64 / scale,
                maximized: window.is_maximized(),
                fullscreen: window.fullscreen().is_some(),
                focused: Some(id) == focused,
            },
        );
    }
}

/// Apply a window action on the main thread. `Close` removes the window (and
/// exits when it was the last one).
fn apply_window_action(
    windows: &mut HashMap<WindowId, (Window, WebView)>,
    id_label: &mut HashMap<WindowId, String>,
    id: WindowId,
    action: WindowAction,
    control_flow: &mut ControlFlow,
) {
    if let WindowAction::Close = action {
        windows.remove(&id);
        id_label.remove(&id);
        if windows.is_empty() {
            *control_flow = ControlFlow::Exit;
        }
        return;
    }
    let Some((window, _)) = windows.get(&id) else {
        return;
    };
    match action {
        WindowAction::Minimize => window.set_minimized(true),
        WindowAction::ToggleMaximize => window.set_maximized(!window.is_maximized()),
        WindowAction::ToggleFullscreen => {
            let fs = if window.fullscreen().is_some() {
                None
            } else {
                Some(Fullscreen::Borderless(None))
            };
            window.set_fullscreen(fs);
        }
        WindowAction::Focus => window.set_focus(),
        WindowAction::Show => window.set_visible(true),
        WindowAction::Hide => window.set_visible(false),
        WindowAction::Center => {
            if let Some(monitor) = window.current_monitor() {
                let ms = monitor.size();
                let ws = window.outer_size();
                let x = monitor.position().x + (ms.width as i32 - ws.width as i32) / 2;
                let y = monitor.position().y + (ms.height as i32 - ws.height as i32) / 2;
                window.set_outer_position(PhysicalPosition::new(x, y));
            }
        }
        WindowAction::SetTitle(title) => window.set_title(&title),
        WindowAction::SetSize(w, h) => window.set_inner_size(LogicalSize::new(w, h)),
        WindowAction::Close => {}
    }
}

/// `POST /__window/<op>` — window control from the frontend.
fn serve_window(runner: &Runner, op: &str, body: Vec<u8>) -> Body {
    let Some(windows) = runner.ctx.try_get::<Windows>() else {
        return msgpack_err("window control unavailable".into());
    };
    let ok = match op {
        "minimize" => windows.minimize(None),
        "toggle_maximize" => windows.toggle_maximize(None),
        "toggle_fullscreen" => windows.toggle_fullscreen(None),
        "close" => windows.close(None),
        "focus" => windows.focus(None),
        "show" => windows.show(None),
        "hide" => windows.hide(None),
        "center" => windows.center(None),
        "set_title" => windows.set_title(
            None,
            rmp_serde::from_slice::<String>(&body).unwrap_or_default(),
        ),
        "set_size" => {
            let (w, h) = rmp_serde::from_slice::<(f64, f64)>(&body).unwrap_or((800.0, 600.0));
            windows.set_size(None, w, h)
        }
        other => return msgpack_err(format!("unknown window op: {other}")),
    };
    msgpack_ok(&ok)
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
