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
use crate::lifetime::{self, Close, Kind};
use crate::popover::{place, Popover, Rect, Screen, Space};
use crate::security::Policy;
use crate::window::{PopoverAction, UserEvent, WindowAction, WindowConfig};

use super::router::route;
use super::{ipc_handle, Runner, ABOUT_MENU_ID, SCHEME};

/// Run the event loop with the given initial windows. Diverges until the last
/// window closes, or, for an app that stays in the tray (RFC 0020), until a
/// quit. New windows can be opened at runtime via `Windows`.
// `tray_handle` is held for the program's lifetime to keep the tray icon
// visible; the unread count (RFC 0008) updates its tooltip.
#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "tray", allow(unused_assignments, unused_variables))]
pub(crate) fn run(
    rt: tokio::runtime::Runtime,
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))] mut event_loop: EventLoop<UserEvent>,
    registry: Arc<CommandRegistry>,
    ctx: Ctx,
    bus: EventBus,
    assets: Option<AssetResolver>,
    mut window_configs: Vec<WindowConfig>,
    tray: Option<crate::tray::TrayConfig>,
    icon: Option<crate::Icon>,
    about: AboutInfo,
    persist_window: bool,
    #[cfg_attr(not(feature = "shortcuts"), allow(unused_variables))] shortcuts: Vec<String>,
    menu: Option<crate::menu::Menu>,
    single_instance: bool,
    stay_in_tray: bool,
    deep_link: Option<String>,
    csp: Option<String>,
    policy: Policy,
) -> crate::Result<()> {
    // An app that stays in the tray (RFC 0020): out of the Dock until a
    // window shows, and not taking focus from the app in front at launch.
    // Cleared if the tray can't be made, when it's an ordinary app.
    #[cfg_attr(not(feature = "tray"), allow(unused_mut))]
    let mut stay = stay_in_tray;
    // Whether it's in the Dock now (macOS only; elsewhere unused).
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables, unused_mut))]
    let mut in_dock = lifetime::in_dock(stay, 0);
    #[cfg(target_os = "macos")]
    if stay {
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_activate_ignoring_other_apps(false);
    }

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
    // The popover's click (RFC 0019): the left button, on release. Linux's
    // icon sends none; its popover opens from the menu.
    #[cfg(all(feature = "tray", any(target_os = "macos", target_os = "windows")))]
    if tray.as_ref().is_some_and(|t| t.popover.is_some()) {
        let proxy = event_loop.create_proxy();
        tray_icon::TrayIconEvent::set_event_handler(Some(
            move |event: tray_icon::TrayIconEvent| {
                if let tray_icon::TrayIconEvent::Click {
                    rect,
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } = event
                {
                    let _ = proxy.send_event(UserEvent::TrayClick(Rect {
                        x: rect.position.x,
                        y: rect.position.y,
                        width: f64::from(rect.size.width),
                        height: f64::from(rect.size.height),
                    }));
                }
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
        clients: super::Clients::default(),
        icon,
        primary: window_configs.first().map(|w| w.label.clone()),
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

    // The framework's own menu labels, in the app's language (RFC 0056), and
    // again whenever the locale changes.
    if let Some(t) = runner.ctx.try_get::<crate::i18n::Translator>() {
        let proxy = parking_lot::Mutex::new(event_loop.create_proxy());
        t.on_change(move |_| {
            let _ = proxy.lock().send_event(UserEvent::Relabel);
        });
        MENU_WORDS.with(|w| *w.borrow_mut() = Some((t, runner.about.name.clone())));
    } else {
        MENU_WORDS.with(|w| *w.borrow_mut() = None);
    }

    // A change to the menu (RFC 0055), from the page or from Rust: the loop
    // applies it to every window's copy.
    if let Some(app_menu) = runner.ctx.try_get::<crate::menu::AppMenu>() {
        let proxy = parking_lot::Mutex::new(event_loop.create_proxy());
        app_menu.on_change(move |id| {
            let _ = proxy.lock().send_event(UserEvent::Menu(id.to_owned()));
        });
    }

    // Windows: the app's notifications show its name and icon (RFC 0030).
    #[cfg(all(target_os = "windows", feature = "system"))]
    crate::app_id::register(&runner.about.name, runner.icon.as_ref());

    // Deep-link: register the scheme (idempotent) so the OS routes URLs to us.
    if let Some(scheme) = &runner.deep_link {
        crate::deeplink::register(scheme, &runner.about.name);
    }

    // The documents named on the command line (RFC 0052): Windows and Linux
    // open a file with the app this way, and a terminal does on every
    // platform. They wait for the page's `documents.initial()`.
    if let Some(documents) = runner.ctx.try_get::<crate::documents::Documents>() {
        documents.open(
            &runner.ctx,
            crate::documents::in_args(std::env::args().skip(1), runner.deep_link.as_deref()),
        );
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
    // `rata dev --matrix` (RFC 0034): the first window opens the frames.
    let matrix_at_start = cfg!(debug_assertions)
        && std::env::var("ELYRA_MATRIX").is_ok_and(|v| v == "1")
        && runner
            .ctx
            .try_get::<crate::inspector::InspectorOff>()
            .is_none();
    for (i, config) in window_configs.iter().enumerate() {
        let start =
            (i == 0 && matrix_at_start).then_some("globalThis.__ELYRA_MATRIX_AT_START__ = true;");
        let (window, webview) = build_window(&event_loop, &runner, config, false, start);
        if i == 0 {
            primary_id = Some(window.id());
            if let Some(g) = restored {
                if let (Some(x), Some(y)) = (g.x, g.y) {
                    // Not on a screen that's since been unplugged: there the
                    // OS's own placement is better than a window nobody sees.
                    let screens: Vec<crate::winstate::Monitor> = event_loop
                        .available_monitors()
                        .map(|m| crate::winstate::Monitor {
                            x: m.position().x,
                            y: m.position().y,
                            width: m.size().width,
                            height: m.size().height,
                            scale: m.scale_factor(),
                        })
                        .collect();
                    if crate::winstate::reachable(x, y, g.width, &screens) {
                        window.set_outer_position(PhysicalPosition::new(x, y));
                    }
                }
                if g.maximized {
                    window.set_maximized(true);
                }
            }
        }
        id_label.insert(window.id(), config.label.clone());
        windows.insert(window.id(), (window, webview));
    }

    // The tray's popover (RFC 0019): made now, hidden, so the first click
    // shows it at once.
    let mut popover: Option<TrayPopover> = None;
    // The Quick Look window (RFC 0041), once made: its id, and the window it
    // was last shown beside.
    let mut quick: Option<(WindowId, Option<WindowId>)> = None;
    // The app's windows that have the focus now: none, the app is in the
    // background (Quick Look hides, RFC 0041).
    let mut with_focus: std::collections::HashSet<WindowId> = std::collections::HashSet::new();
    if let Some(config) = tray.as_ref().and_then(|t| t.popover.as_ref()) {
        let (window, webview) = build_window(&event_loop, &runner, config, true, None);
        popover = Some(TrayPopover {
            id: window.id(),
            state: Popover::default(),
            last: None,
        });
        id_label.insert(window.id(), config.label.clone());
        windows.insert(window.id(), (window, webview));
    }

    // The tray must be created after the loop initializes (macOS); hold it alive.
    #[cfg(feature = "tray")]
    let mut tray_config = tray;
    // What the tray shows, as `Tray` and the unread count change it (RFC 0028).
    #[cfg(feature = "tray")]
    let mut tray_live = tray_config
        .as_ref()
        .map(|c| crate::tray::Live::new(c, runner.icon.as_ref(), &runner.about.name));
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
                    _app_menu = Some(macos_app_menu(
                        &app_name,
                        runner.menu.as_ref(),
                        runner.ctx.try_get::<crate::menu::AppMenu>().as_deref(),
                        runner.ctx.has::<crate::diagnostics::Setup>(),
                    ));
                }
                #[cfg(feature = "tray")]
                if let (Some(config), Some(live)) = (tray_config.take(), tray_live.as_ref()) {
                    match crate::tray::build(&config, live) {
                        Ok(handle) => tray_handle = Some(handle),
                        Err(e) => {
                            crate::error!(target: "elyra::tray", "could not create the tray icon: {e}");
                            // Without an icon an app that stays in the tray
                            // couldn't be reached: it's an ordinary one.
                            if stay {
                                crate::warn!(
                                    target: "elyra::tray",
                                    "no tray icon: showing the main window, and ending with the last one"
                                );
                                stay = false;
                                if let Some(id) = primary_id {
                                    bring_back(
                                        target,
                                        &runner,
                                        &windows,
                                        &id_label,
                                        focused,
                                        id,
                                        &mut in_dock,
                                    );
                                }
                            }
                        }
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
            Event::UserEvent(UserEvent::Relabel) => relabel_menus(),
            Event::UserEvent(UserEvent::Menu(id)) => {
                if let Some(app_menu) = runner.ctx.try_get::<crate::menu::AppMenu>() {
                    apply_menu_state(&id, &app_menu);
                }
            }
            Event::UserEvent(UserEvent::MenuClick(id)) => {
                // A check item's mark is the app's to set (RFC 0055): put it
                // back as the app has it, whatever the click did to it.
                if is_check_item(&id) {
                    if let Some(app_menu) = runner.ctx.try_get::<crate::menu::AppMenu>() {
                        apply_menu_state(&id, &app_menu);
                    }
                }
                if id == crate::diagnostics::MENU_ID {
                    // Help → Save Diagnostics… (RFC 0033).
                    super::diagnose::save_from_menu(
                        &runner,
                        seen(target, &windows, &id_label, focused),
                    );
                } else if id == ABOUT_MENU_ID {
                    // Open the built-in About dialog (the runtime listens here).
                    let _ = runner.bus.emit("elyra:about", &runner.about);
                } else {
                    #[cfg(feature = "tray")]
                    if id == crate::tray::QUIT_ID {
                        quit(&runner, &windows, primary_id, persist_window, control_flow);
                    } else if id == crate::tray::POPOVER_ID {
                        // Linux: the icon a moment ago is by the cursor.
                        if let Some(p) = popover.as_mut() {
                            p.state.shown();
                            show_popover(target, &runner, &windows, &id_label, focused, p, Anchor::Cursor);
                        }
                    } else {
                        let _ = runner.bus.emit("elyra:menu", &id);
                        let _ = runner.bus.emit("tray", &id);
                    }
                    #[cfg(not(feature = "tray"))]
                    let _ = runner.bus.emit("elyra:menu", &id);
                }
            }
            Event::UserEvent(UserEvent::Badge(count)) => {
                let _label = crate::badge::label(count);
                #[cfg(target_os = "macos")]
                if let Some((w, _)) = windows.values().next() {
                    use tao::platform::macos::WindowExtMacOS;
                    w.set_badge_label(_label.clone());
                }
                #[cfg(target_os = "linux")]
                if let Some((w, _)) = windows.values().next() {
                    use tao::platform::unix::WindowExtUnix;
                    // The launcher finds the app by its `.desktop` entry,
                    // which `rata bundle` names after the binary.
                    let desktop = std::env::current_exe()
                        .ok()
                        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                        .map(|name| format!("{name}.desktop"));
                    w.set_badge_count((count > 0).then_some(i64::from(count)), desktop);
                }
                #[cfg(target_os = "windows")]
                {
                    use tao::platform::windows::WindowExtWindows;
                    let icon = (count > 0)
                        .then(|| {
                            tao::window::Icon::from_rgba(
                                crate::badge::overlay(count),
                                crate::badge::SIDE,
                                crate::badge::SIDE,
                            )
                            .ok()
                        })
                        .flatten();
                    for (w, _) in windows.values() {
                        w.set_overlay_icon(icon.as_ref());
                    }
                }
                // On the tray too (RFC 0028): an app in the tray with no
                // window open has no other place to show it.
                #[cfg(feature = "tray")]
                if let (Some(tray), Some(live)) = (&tray_handle, tray_live.as_mut()) {
                    live.set_unread(count);
                    live.apply(tray);
                }
            }
            Event::UserEvent(UserEvent::SecondInstance(payload)) => {
                // Raise the primary/focused window so the user sees the app.
                let popover_id = popover.as_ref().map(|p| p.id);
                if let Some(id) = focused
                    .filter(|f| Some(*f) != popover_id)
                    .or(primary_id)
                    .or_else(|| windows.keys().next().copied())
                {
                    if stay {
                        // In front of the app in front, and in the Dock.
                        bring_back(
                            target,
                            &runner,
                            &windows,
                            &id_label,
                            focused,
                            id,
                            &mut in_dock,
                        );
                    } else if let Some((w, _)) = windows.get(&id) {
                        w.set_visible(true);
                        w.set_minimized(false);
                        w.set_focus();
                    }
                }
                // The URL it was launched with, and the documents (RFC 0052).
                let launch = crate::documents::Launch::read(&payload);
                if let Some(url) = launch.url.as_deref().filter(|u| !u.is_empty()) {
                    let _ = runner.bus.emit("elyra:second-instance", &url);
                    if let Some(scheme) = &runner.deep_link {
                        // Validate the forwarded URL, don't just prefix-match it.
                        if crate::instance::is_deep_link(url, scheme) {
                            let _ = runner.bus.emit("elyra:deep-link", &url);
                        }
                    }
                }
                if !launch.files.is_empty() {
                    // Checked again here: the other process could be anyone's.
                    if let Some(documents) = runner.ctx.try_get::<crate::documents::Documents>() {
                        documents.open(&runner.ctx, launch.files);
                    }
                }
            }
            #[cfg(target_os = "macos")]
            Event::Opened { urls } => {
                // macOS delivers both here: a scheme's URLs (with the scheme in
                // the Info.plist) to the frontend as deep links, and the files
                // it opens with the app (RFC 0052) as documents, never as links.
                let (files, links): (Vec<_>, Vec<_>) =
                    urls.into_iter().partition(|u| u.scheme() == "file");
                for url in links {
                    let _ = runner.bus.emit("elyra:deep-link", &url.to_string());
                }
                if let Some(documents) = runner.ctx.try_get::<crate::documents::Documents>() {
                    documents.open(
                        &runner.ctx,
                        files.into_iter().filter_map(|u| u.to_file_path().ok()),
                    );
                }
            }
            // Opened again from Finder, Spotlight or the Dock while it runs: no
            // second process on macOS, so this is the way back to the main
            // window of an app that stays in the tray.
            #[cfg(target_os = "macos")]
            Event::Reopen { .. } if stay => {
                if let Some(id) = primary_id {
                    bring_back(target, &runner, &windows, &id_label, focused, id, &mut in_dock);
                }
            }
            Event::UserEvent(UserEvent::QuickLook { source, route }) => {
                let source = source.or(focused).filter(|id| Some(*id) != quick.map(|q| q.0));
                quick_look(target, &runner, &mut windows, &mut id_label, &mut quick, source, route);
            }
            Event::UserEvent(UserEvent::QuickLookBlur) => {
                let shown = quick
                    .and_then(|q| windows.get(&q.0))
                    .is_some_and(|(w, _)| w.is_visible());
                if shown && with_focus.is_empty() {
                    quick_look(target, &runner, &mut windows, &mut id_label, &mut quick, None, None);
                }
            }
            Event::UserEvent(UserEvent::Quit) => {
                quit(&runner, &windows, primary_id, persist_window, control_flow);
            }
            Event::UserEvent(UserEvent::Seen(reply)) => {
                let _ = reply.send(seen(target, &windows, &id_label, focused));
            }
            // A coding agent's question (RFC 0026), in a debug build.
            #[cfg(debug_assertions)]
            Event::UserEvent(UserEvent::Dev(ask)) => {
                let open = super::dev::Open {
                    windows: &windows,
                    labels: &id_label,
                    focused,
                    popover: popover.as_ref().map(|p| p.id),
                };
                super::dev::answer(ask, open, &runner);
            }
            Event::UserEvent(UserEvent::TrayClick(rect)) => {
                if let Some(p) = popover.as_mut() {
                    match p.state.clicked(std::time::Instant::now()) {
                        Some(true) => show_popover(target, &runner, &windows, &id_label, focused, p, Anchor::Icon(rect)),
                        Some(false) => hide_popover(&runner, &windows, &id_label, focused, p),
                        None => {}
                    }
                }
            }
            #[cfg(feature = "tray")]
            Event::UserEvent(UserEvent::Tray(change)) => {
                if let (Some(tray), Some(live)) = (&tray_handle, tray_live.as_mut()) {
                    live.change(change);
                    live.apply(tray);
                }
            }
            Event::UserEvent(UserEvent::Popover(action)) => {
                if let Some(p) = popover.as_mut() {
                    let show = match action {
                        PopoverAction::Show => true,
                        PopoverAction::Hide => false,
                        PopoverAction::Toggle => !p.state.is_shown(),
                    };
                    if show {
                        p.state.shown();
                        show_popover(target, &runner, &windows, &id_label, focused, p, Anchor::Last);
                    } else {
                        p.state.hidden();
                        hide_popover(&runner, &windows, &id_label, focused, p);
                    }
                }
            }
            Event::UserEvent(UserEvent::OpenWindow(config)) => {
                let (window, webview) = build_window(target, &runner, &config, false, None);
                let id = window.id();
                id_label.insert(id, config.label.clone());
                windows.insert(id, (window, webview));
                if stay && config.visible {
                    bring_back(
                        target,
                        &runner,
                        &windows,
                        &id_label,
                        focused,
                        id,
                        &mut in_dock,
                    );
                }
            }
            #[cfg(debug_assertions)]
            Event::UserEvent(UserEvent::Matrix {
                source,
                frames,
                reply,
            }) => {
                let open = toggle_matrix(
                    target,
                    &runner,
                    &mut windows,
                    &mut id_label,
                    source.as_deref(),
                    frames,
                );
                let _ = reply.send(open);
            }
            Event::UserEvent(UserEvent::Window(cmd)) => {
                // From a page (RFC 0016): its own window, or nothing if it's
                // gone, never the focused one.
                let target_id = match cmd.native {
                    Some(native) => windows.contains_key(&native).then_some(native),
                    None => cmd
                        .label
                        .as_deref()
                        .and_then(|l| {
                            id_label
                                .iter()
                                .find(|(_, v)| v.as_str() == l)
                                .map(|(k, _)| *k)
                        })
                        .or(focused)
                        .or_else(|| windows.keys().next().copied()),
                };
                let popover_id = popover.as_ref().map(|p| p.id);
                match (target_id, popover.as_mut()) {
                    // The popover hides rather than closes (Esc, `appWindow.hide()`),
                    // and shows where it opens.
                    (Some(id), Some(p)) if id == p.id => match cmd.action {
                        WindowAction::Close | WindowAction::Hide | WindowAction::Minimize => {
                            p.state.hidden();
                            hide_popover(&runner, &windows, &id_label, focused, p);
                        }
                        WindowAction::Show | WindowAction::Focus => {
                            p.state.shown();
                            show_popover(target, &runner, &windows, &id_label, focused, p, Anchor::Last);
                        }
                        action => apply_window_action(&mut windows, &mut id_label, id, action, popover_id, stay, control_flow),
                    },
                    // Shown in an app that stays in the tray: in front of the
                    // app in front, and in the Dock (RFC 0020).
                    (Some(id), _) if stay && matches!(cmd.action, WindowAction::Show) => {
                        bring_back(
                            target,
                            &runner,
                            &windows,
                            &id_label,
                            focused,
                            id,
                            &mut in_dock,
                        );
                    }
                    (Some(id), _) => {
                        // Closing the main window of an app that stays in the
                        // tray hides it, as its close button does.
                        let kind = kind_of(id, primary_id, popover_id);
                        let action = match cmd.action {
                            WindowAction::Close if lifetime::close(kind, stay) == Close::Hide => {
                                if persist_window {
                                    if let Some((w, _)) = windows.get(&id) {
                                        save_geometry(&runner.about.name, w);
                                    }
                                }
                                WindowAction::Hide
                            }
                            action => action,
                        };
                        let closing = matches!(action, WindowAction::Close);
                        let visibility = matches!(action, WindowAction::Show | WindowAction::Hide);
                        let hiding = matches!(action, WindowAction::Hide);
                        apply_window_action(&mut windows, &mut id_label, id, action, popover_id, stay, control_flow);
                        if closing {
                            let ids = runner.clients.release_native(id);
                            super::closed(&runner, &ids);
                        } else if visibility {
                            if hiding {
                                super::hidden(&runner, &runner.clients.of_native(id));
                            }
                            emit_window_state(&runner, &windows, &id_label, id, focused);
                        }
                    }
                    (None, _) => {}
                }
            }
            Event::WindowEvent {
                window_id, event, ..
            } => match event {
                WindowEvent::CloseRequested if popover.as_ref().is_some_and(|p| p.id == window_id) => {
                    // Alt+F4 on the popover: it hides.
                    if let Some(p) = popover.as_mut() {
                        p.state.hidden();
                        hide_popover(&runner, &windows, &id_label, focused, p);
                    }
                }
                WindowEvent::CloseRequested if quick.is_some_and(|q| q.0 == window_id) => {
                    // Its close button: it hides, made once (RFC 0041).
                    quick_look(target, &runner, &mut windows, &mut id_label, &mut quick, None, None);
                }
                WindowEvent::CloseRequested => {
                    // Quick Look goes with the window it was shown beside.
                    if quick.is_some_and(|q| q.1 == Some(window_id)) {
                        quick_look(target, &runner, &mut windows, &mut id_label, &mut quick, None, None);
                    }
                    if persist_window && Some(window_id) == primary_id {
                        if let Some((w, _)) = windows.get(&window_id) {
                            save_geometry(&runner.about.name, w);
                        }
                    }
                    let popover_id = popover.as_ref().map(|p| p.id);
                    let kind = kind_of(window_id, primary_id, popover_id);
                    match lifetime::close(kind, stay) {
                        // The main window of an app that stays in the tray
                        // (RFC 0020): hidden, its page kept.
                        Close::Hide => {
                            if let Some((w, _)) = windows.get(&window_id) {
                                w.set_visible(false);
                            }
                            super::hidden(&runner, &runner.clients.of_native(window_id));
                            emit_window_state(&runner, &windows, &id_label, window_id, focused);
                        }
                        Close::Drop => {
                            with_focus.remove(&window_id);
                            windows.remove(&window_id);
                            id_label.remove(&window_id);
                            let ids = runner.clients.release_native(window_id);
                            super::closed(&runner, &ids);
                            // The popover doesn't keep the app running (RFC 0019).
                            if lifetime::ends(stay, others(&windows, &id_label, popover_id)) {
                                *control_flow = ControlFlow::Exit;
                            }
                        }
                    }
                }
                WindowEvent::Focused(f) => {
                    if let Some(focus) = runner.ctx.try_get::<crate::window::Focus>() {
                        focus.set(f);
                    }
                    if f {
                        with_focus.insert(window_id);
                    } else {
                        with_focus.remove(&window_id);
                        if with_focus.is_empty() && quick.is_some() {
                            if let Some(w) = runner.ctx.try_get::<crate::window::Windows>() {
                                w.quick_look_blur_later();
                            }
                        }
                    }
                    // Back to the app: the notifications may have been read
                    // elsewhere (RFC 0008).
                    #[cfg(feature = "backend")]
                    if f {
                        if let Some(n) = runner.ctx.try_get::<crate::notifications::Notifications>() {
                            n.spawn(|n| async move {
                                let _ = n.refresh().await;
                            });
                        }
                        // …and the user's abilities changed (RFC 0010).
                        if let Some(a) = runner.ctx.try_get::<crate::abilities::Abilities>() {
                            a.spawn_refresh();
                        }
                    }
                    if !f {
                        if let Some(p) = popover.as_mut().filter(|p| p.id == window_id) {
                            // A click elsewhere: it hides (its state is sent below).
                            if p.state.blurred(std::time::Instant::now()) {
                                if let Some((w, _)) = windows.get(&window_id) {
                                    w.set_visible(false);
                                }
                                super::hidden(&runner, &runner.clients.of_native(window_id));
                            }
                        }
                    }
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
            // The loop has ended and tao ends the process next, without
            // running destructors: the app's exit hooks (RFC 0053).
            Event::LoopDestroyed => crate::exit::run(),
            _ => {}
        }

        // Out of the Dock again once no window shows (RFC 0020).
        #[cfg(target_os = "macos")]
        if stay && in_dock {
            let popover_id = popover.as_ref().map(|p| p.id);
            set_in_dock(target, &mut in_dock, lifetime::in_dock(stay, shown(&windows, popover_id)));
        }
    })
}

/// What the diagnostics say of the windows, the screens and the theme
/// (RFC 0033): each window's label, whether it shows and has focus, and its
/// page's path, never its query or what's on it.
fn seen(
    target: &EventLoopWindowTarget<UserEvent>,
    windows: &HashMap<WindowId, (Window, WebView)>,
    labels: &HashMap<WindowId, String>,
    focused: Option<WindowId>,
) -> crate::diagnostics::Seen {
    let mut list: Vec<crate::diagnostics::WindowSeen> = windows
        .iter()
        .map(|(id, (window, webview))| crate::diagnostics::WindowSeen {
            label: labels.get(id).cloned().unwrap_or_default(),
            visible: window.is_visible(),
            focused: focused == Some(*id),
            path: webview
                .url()
                .ok()
                .as_deref()
                .and_then(crate::diagnostics::page_path),
        })
        .collect();
    list.sort_by(|a, b| a.label.cmp(&b.label));
    crate::diagnostics::Seen {
        windows: list,
        screens: target
            .available_monitors()
            .map(|m| m.scale_factor())
            .collect(),
        dark: windows
            .values()
            .next()
            .map(|(w, _)| w.theme() == tao::window::Theme::Dark),
    }
}

/// A native menu item the app's [`crate::menu::Menu`] declared, kept so a
/// change to it reaches every window's copy (RFC 0055). A handle: cloning it
/// is the same item.
#[derive(Clone)]
enum NativeItem {
    Plain(muda::MenuItem),
    Check(muda::CheckMenuItem),
}

thread_local! {
    /// The app's items as built, by id: one per window's menu bar on Windows
    /// and Linux, one in the app menu on macOS. The event loop's thread only.
    static NATIVE_ITEMS: std::cell::RefCell<std::collections::HashMap<String, Vec<NativeItem>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
    /// The translator the framework's menu labels come from, and the app's
    /// name for `:app` (RFC 0056).
    static MENU_WORDS: std::cell::RefCell<Option<(std::sync::Arc<crate::i18n::Translator>, String)>> =
        const { std::cell::RefCell::new(None) };
    /// Every item the framework labelled, to label again in a new locale.
    static LABELLED: std::cell::RefCell<Vec<(crate::menu::MenuWord, Labelled)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// An item whose label is the framework's (RFC 0056).
#[derive(Clone)]
enum Labelled {
    Submenu(muda::Submenu),
    Item(muda::MenuItem),
    System(muda::PredefinedMenuItem),
}

/// `word` in the app's language, or `None` while that's the framework's
/// English (a system item then keeps the platform's own label).
fn menu_translated(word: crate::menu::MenuWord) -> Option<String> {
    MENU_WORDS.with(|w| {
        let w = w.borrow();
        let (t, app) = w.as_ref()?;
        word.translated(Some(t), app)
    })
}

/// `word` in the app's language, else in English.
fn menu_label(word: crate::menu::MenuWord, app: &str) -> String {
    menu_translated(word).unwrap_or_else(|| word.label(None, app))
}

/// Keep `item` to label again when the locale changes.
fn labelled<T>(word: crate::menu::MenuWord, item: T, wrap: impl Fn(T) -> Labelled) -> T
where
    T: Clone,
{
    LABELLED.with(|l| l.borrow_mut().push((word, wrap(item.clone()))));
    item
}

/// Label the framework's items in the locale as it is now.
fn relabel_menus() {
    let app = MENU_WORDS.with(|w| w.borrow().as_ref().map(|(_, app)| app.clone()));
    let Some(app) = app else { return };
    // Cloned out first: on Windows a label set can run the loop again.
    let items = LABELLED.with(|l| l.borrow().clone());
    for (word, item) in items {
        let text = menu_label(word, &app);
        match item {
            Labelled::Submenu(m) => m.set_text(&text),
            Labelled::Item(m) => m.set_text(&text),
            Labelled::System(m) => m.set_text(&text),
        }
    }
}

/// Make every copy of item `id` show its state now.
fn apply_menu_state(id: &str, state: &crate::menu::AppMenu) {
    let Some(now) = state.state(id) else { return };
    // Out of the list first: changing an item can redraw a window's menu bar
    // there and then (Windows), which may run the loop again, and that may
    // build a window's menu, adding to the list.
    let copies: Vec<NativeItem> =
        NATIVE_ITEMS.with(|items| items.borrow().get(id).cloned().unwrap_or_default());
    {
        for item in &copies {
            match item {
                NativeItem::Plain(item) => {
                    item.set_enabled(now.enabled);
                    item.set_text(&now.label);
                }
                NativeItem::Check(item) => {
                    item.set_enabled(now.enabled);
                    item.set_text(&now.label);
                    if let Some(checked) = now.checked {
                        item.set_checked(checked);
                    }
                }
            }
        }
    }
}

/// Whether `id` is one of the app's check items: its mark is set again after
/// a click, from the app's state, since the platform flips it on its own.
fn is_check_item(id: &str) -> bool {
    NATIVE_ITEMS.with(|items| {
        items
            .borrow()
            .get(id)
            .is_some_and(|copies| copies.iter().any(|c| matches!(c, NativeItem::Check(_))))
    })
}

/// Append the app's own submenus (from [`crate::menu::Menu`]) to `menu`, each
/// item as `state` has it now.
fn append_custom_submenus(
    menu: &muda::Menu,
    custom: Option<&crate::menu::Menu>,
    state: Option<&crate::menu::AppMenu>,
) {
    let Some(custom) = custom else { return };
    let here = crate::menu::Platform::current();
    for sm in custom.submenus.iter().filter(|sm| sm.only.includes(here)) {
        let _ = menu.append(&native_submenu(sm, state));
    }
}

/// One of the app's submenus, its entries built for this platform.
fn native_submenu(
    sm: &crate::menu::Submenu,
    state: Option<&crate::menu::AppMenu>,
) -> muda::Submenu {
    use crate::menu::{MenuEntry, Platform};
    use muda::{accelerator::Accelerator, CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};

    let here = Platform::current();
    let submenu = Submenu::new(&sm.title, true);
    for entry in &sm.items {
        match entry {
            MenuEntry::Separator => {
                let _ = submenu.append(&PredefinedMenuItem::separator());
            }
            MenuEntry::Submenu(inner) if inner.only.includes(here) => {
                let _ = submenu.append(&native_submenu(inner, state));
            }
            MenuEntry::Submenu(_) => {}
            MenuEntry::System(item) => {
                if let Some(native) = system_item(*item) {
                    let _ = submenu.append(&native);
                }
            }
            MenuEntry::Item(item) | MenuEntry::Check(item, _) if item.only.includes(here) => {
                let accel = item
                    .accelerator
                    .as_deref()
                    .and_then(|s| s.parse::<Accelerator>().ok());
                let now = state.and_then(|s| s.state(&item.id));
                let enabled = now.as_ref().map_or(item.enabled, |n| n.enabled);
                let label = now
                    .as_ref()
                    .map_or(item.label.as_str(), |n| n.label.as_str());
                let native = match entry {
                    MenuEntry::Check(_, checked) => {
                        let checked = now.as_ref().and_then(|n| n.checked).unwrap_or(*checked);
                        let check = CheckMenuItem::with_id(
                            item.id.as_str(),
                            label,
                            enabled,
                            checked,
                            accel,
                        );
                        let _ = submenu.append(&check);
                        NativeItem::Check(check)
                    }
                    _ => {
                        let plain = MenuItem::with_id(item.id.as_str(), label, enabled, accel);
                        let _ = submenu.append(&plain);
                        NativeItem::Plain(plain)
                    }
                };
                NATIVE_ITEMS.with(|items| {
                    items
                        .borrow_mut()
                        .entry(item.id.clone())
                        .or_default()
                        .push(native)
                });
            }
            MenuEntry::Item(_) | MenuEntry::Check(..) => {}
        }
    }
    submenu
}

/// The platform's own item for `item`, where it has one.
fn system_item(item: crate::menu::SystemItem) -> Option<muda::PredefinedMenuItem> {
    use crate::menu::{MenuWord, SystemItem};
    use muda::PredefinedMenuItem as P;
    let label = menu_translated(MenuWord::System(item));
    let text = label.as_deref();
    let native = match item {
        SystemItem::Undo => P::undo(text),
        SystemItem::Redo => P::redo(text),
        SystemItem::Cut => P::cut(text),
        SystemItem::Copy => P::copy(text),
        SystemItem::Paste => P::paste(text),
        SystemItem::SelectAll => P::select_all(text),
        SystemItem::Minimize => P::minimize(text),
        SystemItem::Fullscreen => P::fullscreen(text),
        SystemItem::CloseWindow => P::close_window(text),
        SystemItem::Hide => P::hide(text),
        SystemItem::HideOthers => P::hide_others(text),
        SystemItem::ShowAll => P::show_all(text),
        SystemItem::Services => P::services(text),
        SystemItem::Quit => P::quit(text),
        #[allow(unreachable_patterns)]
        _ => return None,
    };
    Some(labelled(MenuWord::System(item), native, Labelled::System))
}

/// Install a standard macOS application menu with an Edit menu, so the system
/// routes Cut/Copy/Paste/Select-All/Undo/Redo to the focused webview text field.
/// Returns the menu, which must be kept alive.
#[cfg(target_os = "macos")]
fn macos_app_menu(
    app_name: &str,
    custom: Option<&crate::menu::Menu>,
    state: Option<&crate::menu::AppMenu>,
    diagnostics: bool,
) -> muda::Menu {
    use crate::menu::{MenuWord, SystemItem as S};
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();
    let system = |item| system_item(item).expect("a macOS system item");

    // A custom "About" item (instead of the system panel) so clicking it opens
    // the framework's themed dialog via the `elyra:about` event.
    let about = labelled(
        MenuWord::About,
        MenuItem::with_id(
            ABOUT_MENU_ID,
            menu_label(MenuWord::About, app_name),
            true,
            None,
        ),
        Labelled::Item,
    );

    let app = Submenu::new(app_name, true);
    let _ = app.append_items(&[
        &about,
        &PredefinedMenuItem::separator(),
        &system(S::Services),
        &PredefinedMenuItem::separator(),
        &system(S::Hide),
        &system(S::HideOthers),
        &system(S::ShowAll),
        &PredefinedMenuItem::separator(),
        &system(S::Quit),
    ]);

    let edit = labelled(
        MenuWord::Edit,
        Submenu::new(menu_label(MenuWord::Edit, app_name), true),
        Labelled::Submenu,
    );
    let _ = edit.append_items(&[
        &system(S::Undo),
        &system(S::Redo),
        &PredefinedMenuItem::separator(),
        &system(S::Cut),
        &system(S::Copy),
        &system(S::Paste),
        &system(S::SelectAll),
    ]);

    let _ = menu.append(&app);
    let _ = menu.append(&edit);

    append_custom_submenus(&menu, custom, state);

    // Help → Save Diagnostics… (RFC 0033). macOS adds its own search field.
    if diagnostics {
        let help = labelled(
            MenuWord::Help,
            Submenu::new(menu_label(MenuWord::Help, app_name), true),
            Labelled::Submenu,
        );
        let _ = help.append(&labelled(
            MenuWord::Diagnostics,
            MenuItem::with_id(
                crate::diagnostics::MENU_ID,
                menu_label(MenuWord::Diagnostics, app_name),
                true,
                None,
            ),
            Labelled::Item,
        ));
        let _ = menu.append(&help);
        help.set_as_help_menu_for_nsapp();
    }

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
    state: Option<&crate::menu::AppMenu>,
    diagnostics: bool,
    window: &Window,
) -> Option<muda::Menu> {
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    // Nothing to show without app-provided submenus or the diagnostics (an
    // Edit menu isn't needed for clipboard shortcuts outside macOS).
    let custom = custom.filter(|c| !c.submenus.is_empty());
    if custom.is_none() && !diagnostics {
        return None;
    }

    let menu = Menu::new();
    append_custom_submenus(&menu, custom, state);

    // A Help > About item, mirroring the macOS "About <App>" entry, and
    // Save Diagnostics… (RFC 0033).
    use crate::menu::MenuWord;
    let help = labelled(
        MenuWord::Help,
        Submenu::new(menu_label(MenuWord::Help, app_name), true),
        Labelled::Submenu,
    );
    let about = labelled(
        MenuWord::About,
        MenuItem::with_id(
            ABOUT_MENU_ID,
            menu_label(MenuWord::About, app_name),
            true,
            None,
        ),
        Labelled::Item,
    );
    let _ = help.append(&about);
    if diagnostics {
        let _ = help.append_items(&[
            &PredefinedMenuItem::separator(),
            &labelled(
                MenuWord::Diagnostics,
                MenuItem::with_id(
                    crate::diagnostics::MENU_ID,
                    menu_label(MenuWord::Diagnostics, app_name),
                    true,
                    None,
                ),
                Labelled::Item,
            ),
        ]);
    }
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
    popover: bool,
    // A frame's look (RFC 0034), for its page before the app's script.
    frame: Option<&str>,
) -> (Window, WebView) {
    let mut builder = WindowBuilder::new()
        .with_title(&config.title)
        .with_inner_size(LogicalSize::new(config.width, config.height))
        .with_resizable(config.resizable)
        .with_decorations(config.decorations)
        .with_always_on_top(config.always_on_top)
        .with_visible(config.visible);
    // The app's icon (RFC 0028): the title bar and taskbar on Windows, the
    // window switcher on Linux. macOS takes it from the bundle.
    #[cfg(not(target_os = "macos"))]
    if let Some(pixels) = runner.icon.as_ref().and_then(crate::Icon::shared) {
        if let Ok(icon) =
            tao::window::Icon::from_rgba(pixels.rgba.clone(), pixels.width, pixels.height)
        {
            builder = builder.with_window_icon(Some(icon));
        }
    }
    #[cfg(target_os = "windows")]
    {
        use tao::platform::windows::WindowBuilderExtWindows;
        builder = builder.with_skip_taskbar(config.skip_taskbar);
    }
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowBuilderExtUnix;
        builder = builder.with_skip_taskbar(config.skip_taskbar);
    }
    if let Some((min_w, min_h)) = config.min_size {
        builder = builder.with_min_inner_size(LogicalSize::new(min_w, min_h));
    }
    let window = builder.build(target).expect("failed to build window");

    // Windows/Linux: the menu bar belongs to the window, so install it here.
    #[cfg(not(target_os = "macos"))]
    {
        if let Some(menu) = window_menu(
            &runner.about.name,
            runner.menu.as_ref(),
            runner.ctx.try_get::<crate::menu::AppMenu>().as_deref(),
            runner.ctx.has::<crate::diagnostics::Setup>(),
            &window,
        ) {
            // Held by the window's menu bar; dropping it would remove the menu.
            std::mem::forget(menu);
        }
    }

    // In `rata dev`, pages are served by Vite (HMR) at a cross-origin http://
    // URL; IPC still targets elyra://localhost, so CORS is added in `route`.
    // Debug builds only: a release build always loads its own page.
    let base = crate::security::dev_url().unwrap_or_else(|| format!("{SCHEME}://localhost"));
    let url = format!(
        "{}/{}",
        base.trim_end_matches('/'),
        config.path.trim_start_matches('/')
    );

    let handler = runner.clone();
    let dnd = runner.clone();
    // Which window this is (RFC 0016): every request from this webview comes
    // through this handler, whatever the page says about itself.
    let me = Arc::new(super::WindowRef::new(
        &config.label,
        config.scope.clone(),
        Some(window.id()),
    ));
    runner.clients.adopt(window.id(), me.id);
    let webview = WebViewBuilder::new()
        .with_url(url)
        // Hand the frontend this run's IPC token before any page script runs.
        // Every `/__*` request must present it (see `crate::security`).
        .with_initialization_script(runner.policy.init_script())
        // A coding agent's view of the page (RFC 0026): debug builds only.
        .with_initialization_script(agent_script())
        // The runtime hides a popover on Esc (RFC 0019).
        .with_initialization_script(if popover {
            "globalThis.__ELYRA_POPOVER__ = true;"
        } else {
            ""
        })
        .with_initialization_script(frame.unwrap_or(""))
        // The Theme Studio's draft (RFC 0037), for a window opened after it
        // changed: drawn before the app's script runs.
        .with_initialization_script(theme_draft(runner))
        // The webview stays on the app's own page (and the dev server's under
        // `rata dev`): any other page loaded here would get the IPC token. A
        // link away is offered to the OS, as `shell.open` would, if the
        // policy allows its scheme; anything else just doesn't happen.
        .with_navigation_handler({
            let policy = runner.policy.clone();
            move |url| {
                if policy.allows_navigation(&url) {
                    return true;
                }
                open_outside(&policy, &url);
                false
            }
        })
        // `window.open` and `target="_blank"`: never a webview of their own.
        .with_new_window_req_handler({
            let policy = runner.policy.clone();
            move |url, _features| {
                open_outside(&policy, &url);
                wry::NewWindowResponse::Deny
            }
        })
        .with_drag_drop_handler(move |event| {
            if let wry::DragDropEvent::Drop { paths, .. } = event {
                // Dropped is chosen: each is granted for this run (RFC 0009).
                let files: Vec<crate::files::ChosenFile> = paths
                    .into_iter()
                    .map(|path| {
                        let kind = if path.is_dir() {
                            crate::files::FileChoice::Folder
                        } else {
                            crate::files::FileChoice::Open
                        };
                        crate::files::ChosenFile::grant(path, kind)
                    })
                    .collect();
                let _ = dnd.bus.emit("elyra:file-drop", &files);
            }
            true
        })
        .with_asynchronous_custom_protocol(SCHEME.into(), move |_id, request, responder| {
            let runner = handler.clone();
            let me = me.clone();
            // Never touch the UI thread for real work — hand it to tokio.
            let handle = ipc_handle(&runner);
            handle.spawn(async move {
                let response = route(&runner, Some(&me), request).await;
                responder.respond(response);
            });
        });
    // On Linux the webview goes in the GTK box tao gives the window: what wry
    // asks for with tao (`build` would look for a raw window handle, which a
    // tao window under X11 doesn't have to give, and Wayland never does).
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window
            .default_vbox()
            .expect("tao gives a Linux window its GTK box");
        webview.build_gtk(vbox)
    };
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let webview = webview.build(&window);
    let webview = webview.expect("failed to build webview");

    (window, webview)
}

/// Close the matrix's frames if any is open, or open `frames` on the
/// screen of the window `source` (RFC 0034): whether they're open now.
#[cfg(debug_assertions)]
fn toggle_matrix(
    target: &EventLoopWindowTarget<UserEvent>,
    runner: &Arc<Runner>,
    windows: &mut HashMap<WindowId, (Window, WebView)>,
    id_label: &mut HashMap<WindowId, String>,
    source: Option<&str>,
    frames: Vec<crate::matrix::Opening>,
) -> bool {
    use crate::matrix::{grid, init_script, is_frame, Area};

    let state = runner.ctx.try_get::<crate::matrix::Open>();
    let open: Vec<WindowId> = id_label
        .iter()
        .filter(|(_, label)| is_frame(label))
        .map(|(id, _)| *id)
        .collect();
    if !open.is_empty() {
        for id in open {
            windows.remove(&id);
            id_label.remove(&id);
            let ids = runner.clients.release_native(id);
            super::closed(runner, &ids);
        }
        if let Some(state) = state {
            state.set([]);
        }
        return false;
    }

    // The menu bar (macOS) and the taskbar (Windows) aren't the frames'.
    let (top, bottom) = if cfg!(target_os = "macos") {
        (32.0, 0.0)
    } else if cfg!(target_os = "windows") {
        (0.0, 48.0)
    } else {
        (0.0, 0.0)
    };
    let monitor = source
        .and_then(|l| id_label.iter().find(|(_, v)| v.as_str() == l))
        .and_then(|(id, _)| windows.get(id))
        .and_then(|(w, _)| w.current_monitor())
        .or_else(|| target.primary_monitor());
    let screen = monitor.map_or(
        Area {
            x: 0.0,
            y: top,
            width: 1440.0,
            height: 900.0 - top - bottom,
        },
        |m| {
            let scale = m.scale_factor();
            let at = m.position().to_logical::<f64>(scale);
            let size = m.size().to_logical::<f64>(scale);
            Area {
                x: at.x,
                y: at.y + top,
                width: size.width,
                height: size.height - top - bottom,
            }
        },
    );
    let cells = grid(screen, frames.len());
    let mut shown = Vec::new();
    for (frame, cell) in frames.into_iter().zip(cells) {
        let mut config = frame.config;
        if !frame.sized {
            config.width = cell.width;
            config.height = cell.height;
        }
        let script = init_script(&frame.shown);
        let (window, webview) = build_window(target, runner, &config, false, Some(&script));
        window.set_outer_position(tao::dpi::LogicalPosition::new(cell.x, cell.y));
        let id = window.id();
        id_label.insert(id, config.label.clone());
        windows.insert(id, (window, webview));
        shown.push((config.label, frame.shown));
    }
    if let Some(state) = state {
        state.set(shown);
    }
    true
}

/// The Theme Studio's draft (RFC 0037) for a new window: debug builds only.
fn theme_draft(runner: &Runner) -> String {
    #[cfg(debug_assertions)]
    if let Some(script) = runner
        .ctx
        .try_get::<crate::theme::Drafts>()
        .and_then(|d| d.init_script())
    {
        return script;
    }
    let _ = runner;
    String::new()
}

/// The page's script for a coding agent (RFC 0026): in a debug build; a
/// release has none.
pub(super) fn agent_script() -> &'static str {
    #[cfg(debug_assertions)]
    return super::dev::AGENT_SCRIPT;
    #[cfg(not(debug_assertions))]
    ""
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
    /// Shown, or hidden (a popover between clicks, RFC 0019).
    visible: bool,
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
                visible: window.is_visible(),
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
    popover: Option<WindowId>,
    stay_in_tray: bool,
    control_flow: &mut ControlFlow,
) {
    if let WindowAction::Close = action {
        windows.remove(&id);
        id_label.remove(&id);
        if lifetime::ends(stay_in_tray, others(windows, id_label, popover)) {
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

/// The windows left besides the tray's popover, which doesn't keep the app
/// running (RFC 0019).
/// The windows besides the popover and the matrix's frames (RFC 0034):
/// neither keeps the app running.
/// Show the Quick Look window at `route` beside `source`, made the first
/// time and moved there after; or hide it with `None` (RFC 0041). The
/// window it's shown beside keeps the focus.
fn quick_look(
    target: &EventLoopWindowTarget<UserEvent>,
    runner: &Arc<Runner>,
    windows: &mut HashMap<WindowId, (Window, WebView)>,
    id_label: &mut HashMap<WindowId, String>,
    quick: &mut Option<(WindowId, Option<WindowId>)>,
    source: Option<WindowId>,
    route: Option<String>,
) {
    use crate::window::{QuickLookSpot, QUICK_LOOK};
    let Some(route) = route else {
        if let Some((w, _)) = quick.and_then(|q| windows.get(&q.0)) {
            w.set_visible(false);
        }
        let _ = runner.bus.emit(
            "elyra:quick-look",
            &serde_json::json!({ "open": false, "route": null }),
        );
        return;
    };
    let existing = quick.map(|q| q.0).filter(|id| windows.contains_key(id));
    let id = match existing {
        Some(id) => {
            if let Some((_, webview)) = windows.get(&id) {
                let hash = serde_json::to_string(&route).unwrap_or_default();
                let _ = webview.evaluate_script(&format!("location.hash = {hash};"));
            }
            id
        }
        None => {
            let config = WindowConfig::new(QUICK_LOOK)
                .title(&runner.about.name)
                .path(format!("#{route}"))
                .size(420.0, 640.0)
                .skip_taskbar(true)
                .visible(false)
                .read_only();
            let (window, webview) = build_window(
                target,
                runner,
                &config,
                false,
                Some("globalThis.__ELYRA_QUICK_LOOK__ = true;"),
            );
            let id = window.id();
            id_label.insert(id, QUICK_LOOK.to_owned());
            windows.insert(id, (window, webview));
            id
        }
    };
    // Beside the window it came from: right, else left, else over it.
    let beside = source.and_then(|s| windows.get(&s)).map(|(w, _)| {
        let scale = w.scale_factor();
        let at = w
            .outer_position()
            .unwrap_or_default()
            .to_logical::<f64>(scale);
        // Its outer width, to sit beside it; its inner height, so the two
        // windows, both with a title bar, are as tall.
        let outer = w.outer_size().to_logical::<f64>(scale);
        let inner = w.inner_size().to_logical::<f64>(scale);
        let screen = w.current_monitor().map(|m| {
            let s = m.scale_factor();
            let p = m.position().to_logical::<f64>(s);
            let z = m.size().to_logical::<f64>(s);
            (p.x, p.y, z.width, z.height)
        });
        let mut spot = QuickLookSpot::beside((at.x, at.y, outer.width, inner.height), screen);
        spot.height = inner.height;
        spot
    });
    if let Some((window, _)) = windows.get(&id) {
        // Shown first: macOS doesn't move or size a window that hasn't been.
        window.set_visible(true);
        if let Some(spot) = beside {
            window.set_outer_position(tao::dpi::LogicalPosition::new(spot.x, spot.y));
            window.set_inner_size(LogicalSize::new(spot.width, spot.height));
        }
    }
    if let Some((w, _)) = source.and_then(|s| windows.get(&s)) {
        w.set_focus();
    }
    *quick = Some((id, source));
    let _ = runner.bus.emit(
        "elyra:quick-look",
        &serde_json::json!({ "open": true, "route": route }),
    );
}

fn others(
    windows: &HashMap<WindowId, (Window, WebView)>,
    id_label: &HashMap<WindowId, String>,
    popover: Option<WindowId>,
) -> usize {
    windows
        .keys()
        .filter(|id| Some(**id) != popover)
        .filter(|id| !id_label.get(id).is_some_and(|l| crate::matrix::is_frame(l)))
        // Quick Look doesn't keep the app running (RFC 0041).
        .filter(|id| id_label.get(*id).map(String::as_str) != Some(crate::window::QUICK_LOOK))
        .count()
}

/// The windows shown besides the popover; a minimized one counts, it's in
/// the Dock.
#[cfg(target_os = "macos")]
fn shown(windows: &HashMap<WindowId, (Window, WebView)>, popover: Option<WindowId>) -> usize {
    windows
        .iter()
        .filter(|(id, (w, _))| Some(**id) != popover && (w.is_visible() || w.is_minimized()))
        .count()
}

/// Which kind of window `id` is, for what closing it does.
fn kind_of(id: WindowId, primary: Option<WindowId>, popover: Option<WindowId>) -> Kind {
    if Some(id) == popover {
        Kind::Popover
    } else if Some(id) == primary {
        Kind::Main
    } else {
        Kind::Other
    }
}

/// A regular app (in the Dock, ⌘Tab, its menu bar) or an accessory.
#[cfg(target_os = "macos")]
fn set_in_dock(target: &EventLoopWindowTarget<UserEvent>, in_dock: &mut bool, want: bool) {
    use tao::platform::macos::{ActivationPolicy, EventLoopWindowTargetExtMacOS};
    if *in_dock != want {
        target.set_activation_policy_at_runtime(if want {
            ActivationPolicy::Regular
        } else {
            ActivationPolicy::Accessory
        });
        *in_dock = want;
    }
}

/// Make an accessory a regular app. `true` if it was already active, when
/// it hands the activation to the Dock to take it back as a regular app.
#[cfg(target_os = "macos")]
fn into_the_dock(target: &EventLoopWindowTarget<UserEvent>, in_dock: &mut bool) -> bool {
    use objc2_app_kit::{NSApplication, NSApplicationActivationOptions, NSRunningApplication};
    if *in_dock {
        return false;
    }
    set_in_dock(target, in_dock, true);
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return false;
    };
    if !NSApplication::sharedApplication(mtm).isActive() {
        return false;
    }
    let dock = NSRunningApplication::runningApplicationsWithBundleIdentifier(
        objc2_foundation::ns_string!("com.apple.dock"),
    );
    match dock.firstObject() {
        Some(dock) => dock.activateWithOptions(NSApplicationActivationOptions::empty()),
        None => false,
    }
}

/// Show `id` in an app that stays in the tray (RFC 0020): in the Dock first,
/// so its menu bar comes with it, then in front of the app in front.
// `target` and `in_dock` are the Dock's, on macOS.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
fn bring_back(
    target: &EventLoopWindowTarget<UserEvent>,
    runner: &Runner,
    windows: &HashMap<WindowId, (Window, WebView)>,
    id_label: &HashMap<WindowId, String>,
    focused: Option<WindowId>,
    id: WindowId,
    in_dock: &mut bool,
) {
    let Some((w, _)) = windows.get(&id) else {
        return;
    };
    // Already active as an accessory, as when macOS activated it to send
    // `Reopen`, it doesn't take the menu bar as a regular app: the Dock is
    // given the activation, and the window shown again once it has it.
    #[cfg(target_os = "macos")]
    if into_the_dock(target, in_dock) {
        if let (Some(again), Some(label)) = (
            runner.ctx.try_get::<crate::window::Windows>(),
            id_label.get(&id).cloned(),
        ) {
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                again.show(Some(&label));
            });
        }
    }
    w.set_visible(true);
    w.set_minimized(false);
    focus_alone(w);
    emit_window_state(runner, windows, id_label, id, focused);
}

/// A link the webview won't follow: handed to the OS (the browser, the mail
/// app) when `shell.open`'s policy allows its scheme, and dropped otherwise.
/// Without the `system` feature nothing opens it.
fn open_outside(policy: &crate::security::Policy, url: &str) {
    if policy.allows_open(url).is_err() {
        eprintln!("elyra: the webview stays on the app's page; not opening {url}");
        return;
    }
    #[cfg(feature = "system")]
    if let Err(e) = crate::system::open_external(url) {
        eprintln!("elyra: couldn't open {url}: {e}");
    }
    #[cfg(not(feature = "system"))]
    eprintln!("elyra: the webview stays on the app's page; {url} needs the `system` feature to open outside it");
}

/// End the app: the main window's geometry saved first, as on its close.
fn quit(
    runner: &Runner,
    windows: &HashMap<WindowId, (Window, WebView)>,
    primary: Option<WindowId>,
    persist_window: bool,
    control_flow: &mut ControlFlow,
) {
    if persist_window {
        if let Some((w, _)) = primary.and_then(|id| windows.get(&id)) {
            save_geometry(&runner.about.name, w);
        }
    }
    *control_flow = ControlFlow::Exit;
}

/// The tray's popover in the event loop (RFC 0019).
struct TrayPopover {
    id: WindowId,
    state: Popover,
    /// The icon's rect at the last click, where it opens from code.
    last: Option<Rect>,
}

/// Where the popover opens from.
enum Anchor {
    /// The icon, clicked.
    Icon(Rect),
    /// The cursor (Linux's menu), else where it last opened.
    #[cfg_attr(not(feature = "tray"), allow(dead_code))]
    Cursor,
    /// Where it last opened, else the cursor.
    Last,
}

/// Place the popover by `anchor` (or at the top of the main monitor), show
/// it and give it focus.
#[allow(clippy::too_many_arguments)]
fn show_popover(
    target: &EventLoopWindowTarget<UserEvent>,
    runner: &Runner,
    windows: &HashMap<WindowId, (Window, WebView)>,
    id_label: &HashMap<WindowId, String>,
    focused: Option<WindowId>,
    popover: &mut TrayPopover,
    anchor: Anchor,
) {
    let Some((window, _)) = windows.get(&popover.id) else {
        return;
    };
    let space = Space::here();
    let screens: Vec<Screen> = target
        .available_monitors()
        .map(|m| Screen {
            bounds: Rect {
                x: f64::from(m.position().x),
                y: f64::from(m.position().y),
                width: f64::from(m.size().width),
                height: f64::from(m.size().height),
            },
            scale: m.scale_factor(),
        })
        .collect();
    let main = target.primary_monitor();
    let main_scale = main.as_ref().map_or(1.0, |m| m.scale_factor());
    // Wayland doesn't say where the cursor is (tao answers 0, 0).
    let cursor = || {
        target
            .cursor_position()
            .ok()
            .filter(|c| c.x != 0.0 || c.y != 0.0)
            .map(|c| space.cursor(c.x, c.y, main_scale))
            .and_then(|c| space.screen_at(c.center(), &screens).map(|on| (c, on)))
    };
    let at = match anchor {
        Anchor::Icon(rect) => {
            popover.last = Some(rect);
            space.icon(rect, &screens)
        }
        Anchor::Cursor => cursor().or_else(|| popover.last.and_then(|r| space.icon(r, &screens))),
        Anchor::Last => popover
            .last
            .and_then(|r| space.icon(r, &screens))
            .or_else(cursor),
    };
    // Else at the top of the main monitor.
    let at = at.or_else(|| {
        let main = main.as_ref()?;
        let bounds = space.screen(Screen {
            bounds: Rect {
                x: f64::from(main.position().x),
                y: f64::from(main.position().y),
                width: f64::from(main.size().width),
                height: f64::from(main.size().height),
            },
            scale: main.scale_factor(),
        });
        let top = Rect {
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y,
            width: 0.0,
            height: 0.0,
        };
        Some((top, bounds))
    });
    if let Some((icon, bounds)) = at {
        let size = window.outer_size();
        let (width, height) = match space {
            Space::Physical => (f64::from(size.width), f64::from(size.height)),
            Space::Points => {
                let size = size.to_logical::<f64>(window.scale_factor());
                (size.width, size.height)
            }
        };
        let (x, y) = place(icon, width, height, bounds);
        crate::debug!(
            target: "elyra::tray",
            "popover at ({x}, {y}), {width}×{height}, by {icon:?} on {bounds:?} ({space:?})"
        );
        match space {
            Space::Physical => {
                window.set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
            }
            Space::Points => window.set_outer_position(tao::dpi::LogicalPosition::new(x, y)),
        }
    }
    window.set_visible(true);
    focus_alone(window);
    emit_window_state(runner, windows, id_label, popover.id, focused);
}

/// Give the popover focus. On macOS, tao's `set_focus` activates the app,
/// which brings its main window forward too; so the popover is made key and
/// main first, and the app activated without its other windows.
fn focus_alone(window: &Window) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSWindow};
        use tao::platform::macos::WindowExtMacOS;
        let Some(mtm) = objc2::MainThreadMarker::new() else {
            return window.set_focus();
        };
        // SAFETY: tao's NSWindow, alive as long as `window`, on the main
        // thread (checked above).
        let ns_window = unsafe { &*window.ns_window().cast::<NSWindow>() };
        ns_window.makeKeyAndOrderFront(None);
        ns_window.makeMainWindow();
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
    #[cfg(not(target_os = "macos"))]
    window.set_focus();
}

fn hide_popover(
    runner: &Runner,
    windows: &HashMap<WindowId, (Window, WebView)>,
    id_label: &HashMap<WindowId, String>,
    focused: Option<WindowId>,
    popover: &TrayPopover,
) {
    if let Some((window, _)) = windows.get(&popover.id) {
        window.set_visible(false);
        super::hidden(runner, &runner.clients.of_native(popover.id));
        emit_window_state(runner, windows, id_label, popover.id, focused);
    }
}
