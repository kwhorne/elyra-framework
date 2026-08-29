//! The native side: the tao event loop, window construction, the app menu, and
//! the wry webview each window carries.
//!
//! ## Threading (macOS)
//! The event loop and webview live on the main thread. A separate multi-thread
//! tokio runtime owns all IPC work: the **asynchronous** custom-protocol handler
//! spawns each request onto the runtime and responds from there, so the UI
//! thread never blocks on a command or a long-poll.

use std::collections::HashMap;
use std::sync::Arc;

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopWindowTarget};
use tao::window::{Fullscreen, Window, WindowBuilder, WindowId};
use wry::{WebView, WebViewBuilder};

use crate::about::AboutInfo;
use crate::assets::AssetResolver;
use crate::command::CommandRegistry;
use crate::container::Ctx;
use crate::event::EventBus;
use crate::security::Policy;
use crate::window::{UserEvent, WindowAction, WindowConfig};

use super::router::route;
use super::{ipc_handle, Runner, ABOUT_MENU_ID, SCHEME};

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
    menu: Option<crate::menu::Menu>,
    single_instance: bool,
    deep_link: Option<String>,
    csp: Option<String>,
    policy: Policy,
) -> crate::Result<()> {
    // Route menu clicks through the event loop. The app menu is muda on every
    // platform; the tray is muda on macOS and tray_icon's own menu elsewhere.
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
        rt: Some(rt),
        about,
        deep_link,
        csp,
        policy,
        menu,
        cancellations: parking_lot::Mutex::new(std::collections::HashMap::new()),
    });

    // Single-instance: become primary and forward later launches into the loop.
    if single_instance {
        if let Some(listener) = crate::instance::bind_primary(&runner.about.name) {
            let proxy = event_loop.create_proxy();
            crate::instance::serve(listener, runner.about.name.clone(), move |payload| {
                let _ = proxy.send_event(UserEvent::SecondInstance(payload));
            });
        }
    }

    // Deep-link: register the scheme (idempotent) so the OS routes URLs to us.
    if let Some(scheme) = &runner.deep_link {
        crate::deeplink::register(scheme, &runner.about.name);
    }

    // Silent update check on startup (emits `elyra:update` if one is available).
    #[cfg(feature = "updater")]
    super::update::spawn_startup_update_check(&runner);

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
                    _app_menu = Some(macos_app_menu(&app_name, runner.menu.as_ref()));
                }
                #[cfg(feature = "tray")]
                if let Some(config) = tray_config.take() {
                    match crate::tray::build(&config) {
                        Ok(handle) => tray_handle = Some(handle),
                        Err(e) => crate::error!(target: "elyra::tray", "could not create the tray icon: {e}"),
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
                                    Err(e) => crate::warn!(target: "elyra::shortcuts", "invalid accelerator `{accel}`: {e}"),
                                }
                            }
                            _hotkey_manager = Some(manager);
                        }
                        Err(e) => crate::warn!(target: "elyra::shortcuts", "global shortcuts unavailable: {e}"),
                    }
                }
            }
            #[cfg(feature = "shortcuts")]
            Event::UserEvent(UserEvent::Shortcut(id)) => {
                if let Some(accel) = shortcut_ids.get(&id) {
                    let _ = runner.bus.emit("elyra:shortcut", accel);
                }
            }
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                if id == ABOUT_MENU_ID {
                    // Open the built-in About dialog (the runtime listens here).
                    let _ = runner.bus.emit("elyra:about", &runner.about);
                } else {
                    #[cfg(feature = "tray")]
                    if id == crate::tray::QUIT_ID {
                        *control_flow = ControlFlow::Exit;
                    } else {
                        let _ = runner.bus.emit("elyra:menu", &id);
                        let _ = runner.bus.emit("tray", &id);
                    }
                    #[cfg(not(feature = "tray"))]
                    let _ = runner.bus.emit("elyra:menu", &id);
                }
            }
            Event::UserEvent(UserEvent::SecondInstance(payload)) => {
                // Raise the primary/focused window so the user sees the app.
                if let Some(id) = focused
                    .or(primary_id)
                    .or_else(|| windows.keys().next().copied())
                {
                    if let Some((w, _)) = windows.get(&id) {
                        w.set_visible(true);
                        w.set_minimized(false);
                        w.set_focus();
                    }
                }
                if !payload.is_empty() {
                    let _ = runner.bus.emit("elyra:second-instance", &payload);
                    if let Some(scheme) = &runner.deep_link {
                        // Validate the forwarded URL, don't just prefix-match it.
                        if crate::instance::is_deep_link(&payload, scheme) {
                            let _ = runner.bus.emit("elyra:deep-link", &payload);
                        }
                    }
                }
            }
            #[cfg(target_os = "macos")]
            Event::Opened { urls } => {
                // macOS delivers scheme/file opens here (needs the scheme in the
                // bundle Info.plist). Forward each URL to the frontend.
                for url in urls {
                    let _ = runner.bus.emit("elyra:deep-link", &url.to_string());
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

/// Append the app's own submenus (from [`crate::menu::Menu`]) to `menu`.
fn append_custom_submenus(menu: &muda::Menu, custom: Option<&crate::menu::Menu>) {
    use muda::{accelerator::Accelerator, MenuItem, PredefinedMenuItem, Submenu};

    let Some(custom) = custom else { return };
    for sm in &custom.submenus {
        let submenu = Submenu::new(&sm.title, true);
        for entry in &sm.items {
            match entry {
                crate::menu::MenuEntry::Separator => {
                    let _ = submenu.append(&PredefinedMenuItem::separator());
                }
                crate::menu::MenuEntry::Item {
                    id,
                    label,
                    accelerator,
                } => {
                    let accel = accelerator
                        .as_deref()
                        .and_then(|s| s.parse::<Accelerator>().ok());
                    let item = MenuItem::with_id(id.as_str(), label, true, accel);
                    let _ = submenu.append(&item);
                }
            }
        }
        let _ = menu.append(&submenu);
    }
}

/// Install a standard macOS application menu with an Edit menu, so the system
/// routes Cut/Copy/Paste/Select-All/Undo/Redo to the focused webview text field.
/// Returns the menu, which must be kept alive.
#[cfg(target_os = "macos")]
fn macos_app_menu(app_name: &str, custom: Option<&crate::menu::Menu>) -> muda::Menu {
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

    append_custom_submenus(&menu, custom);

    menu.init_for_nsapp();
    menu
}

/// Build a per-window menu bar for Windows/Linux, where menus belong to a window
/// rather than the application.
///
/// `App::menu(..)` used to be a macOS-only no-op elsewhere, even though the API and
/// the docs presented it as cross-platform.
#[cfg(not(target_os = "macos"))]
fn window_menu(
    app_name: &str,
    custom: Option<&crate::menu::Menu>,
    window: &Window,
) -> Option<muda::Menu> {
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    // Nothing to show without app-provided submenus (an Edit menu isn't needed
    // for clipboard shortcuts outside macOS).
    let custom = custom?;
    if custom.submenus.is_empty() {
        return None;
    }

    let menu = Menu::new();
    append_custom_submenus(&menu, Some(custom));

    // A Help > About item, mirroring the macOS "About <App>" entry.
    let help = Submenu::new("Help", true);
    let about = MenuItem::with_id(ABOUT_MENU_ID, format!("About {app_name}"), true, None);
    let _ = help.append_items(&[&about, &PredefinedMenuItem::separator()]);
    let _ = menu.append(&help);

    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowExtWindows;
        // SAFETY: the HWND comes from the window we were handed and outlives the
        // menu, which is kept alive by the caller.
        let _ = unsafe { menu.init_for_hwnd(window.hwnd()) };
    }
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        // On GTK the menu bar is packed into the window's vertical box.
        let _ = menu.init_for_gtk_window(window.gtk_window(), window.default_vbox());
    }

    Some(menu)
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

    // Windows/Linux: the menu bar belongs to the window, so install it here.
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(menu) = window_menu(&runner.about.name, runner.menu.as_ref(), &window) {
            // Held by the window's menu bar; dropping it would remove the menu.
            std::mem::forget(menu);
        }
    }

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
        // Hand the frontend this run's IPC token before any page script runs.
        // Every `/__*` request must present it (see `crate::security`).
        .with_initialization_script(runner.policy.init_script())
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
            let handle = ipc_handle(&runner);
            handle.spawn(async move {
                let response = route(&runner, request).await;
                responder.respond(response);
            });
        })
        .build(&window)
        .expect("failed to build webview");

    (window, webview)
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
