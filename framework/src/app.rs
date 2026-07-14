//! The application builder — Elyra's `Application`.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use crate::about::AboutInfo;
use crate::assets::AssetResolver;
use crate::command::{Command, CommandRegistry};
use crate::container::{Container, Ctx};
use crate::error::Error;
use crate::event::EventBus;
use crate::middleware::Middleware;
use crate::provider::Provider;
use crate::shell;
use crate::window::{UserEvent, WindowConfig, Windows};
use tao::event_loop::EventLoopBuilder;

/// Builds and runs an Elyra desktop application.
///
/// ```ignore
/// App::new()
///     .title("My App")
///     .bind(Db::connect()?)
///     .commands(commands![greet, add])
///     .assets(elyra::asset_resolver::<Assets>())
///     .run()
/// ```
///
/// An [`EventBus`] is created automatically, bound into the container (so
/// commands resolve it via `ctx.get::<EventBus>()`), and driven by the shell.
pub struct App {
    container: Container,
    registry: CommandRegistry,
    providers: Vec<Box<dyn Provider>>,
    assets: Option<AssetResolver>,
    bus: EventBus,
    windows: Vec<WindowConfig>,
    tray: Option<crate::tray::TrayConfig>,
    about: AboutInfo,
    persist_window: bool,
    #[cfg(feature = "updater")]
    updater: Option<crate::updater::UpdaterConfig>,
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    db_url: Option<String>,
}

/// The fully assembled application, ready to run (or inspect in tests).
#[doc(hidden)]
pub struct Prepared {
    pub ctx: Ctx,
    pub registry: Arc<CommandRegistry>,
    pub bus: EventBus,
    pub assets: Option<AssetResolver>,
    pub windows: Vec<WindowConfig>,
    pub tray: Option<crate::tray::TrayConfig>,
    pub about: AboutInfo,
    pub persist_window: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            container: Container::new(),
            registry: CommandRegistry::new(),
            providers: Vec::new(),
            assets: None,
            bus: EventBus::new(),
            windows: vec![WindowConfig::default()],
            tray: None,
            about: AboutInfo::default(),
            persist_window: false,
            #[cfg(feature = "updater")]
            updater: None,
            db_url: None,
        }
    }

    /// Bind a singleton into the container, resolvable via `ctx.get::<T>()`.
    pub fn bind<T: Any + Send + Sync>(mut self, value: T) -> Self {
        self.container.bind(value);
        self
    }

    /// Register commands, typically via the `commands![...]` macro.
    pub fn commands(mut self, cmds: Vec<Box<dyn Command>>) -> Self {
        self.registry.extend(cmds);
        self
    }

    /// Register a service provider (`register` runs before all `boot`s).
    pub fn provider(mut self, provider: impl Provider) -> Self {
        self.providers.push(Box::new(provider));
        self
    }

    /// Add a command middleware. Outermost-first: the first added wraps the rest.
    pub fn middleware(mut self, middleware: impl Middleware) -> Self {
        self.registry.add_middleware(Arc::new(middleware));
        self
    }

    /// Set the frontend asset resolver (usually `elyra::asset_resolver::<A>()`).
    pub fn assets(mut self, resolver: AssetResolver) -> Self {
        self.assets = Some(resolver);
        self
    }

    /// Set an explicit event coalescing window (default: none). A small window
    /// (~8ms) forces frame-level batching of sustained, time-spaced streams.
    pub fn batch_window(mut self, window: Duration) -> Self {
        self.bus = EventBus::with_batch_window(window);
        self
    }

    /// A clone of the application's event bus — for emitting from background
    /// threads or tasks started in `main`, outside any command.
    pub fn events(&self) -> EventBus {
        self.bus.clone()
    }

    /// Set the primary window's title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.windows[0].title = title.into();
        self
    }

    /// Set the primary window's initial inner size in logical pixels.
    pub fn size(mut self, width: f64, height: f64) -> Self {
        self.windows[0].width = width;
        self.windows[0].height = height;
        self
    }

    /// Set the primary window's minimum inner size in logical pixels.
    pub fn min_size(mut self, width: f64, height: f64) -> Self {
        self.windows[0].min_size = Some((width, height));
        self
    }

    /// Whether the primary window can be resized (default: true).
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.windows[0].resizable = resizable;
        self
    }

    /// Whether the primary window has native decorations (default: true).
    pub fn decorations(mut self, decorations: bool) -> Self {
        self.windows[0].decorations = decorations;
        self
    }

    /// Keep the primary window above others (default: false).
    pub fn always_on_top(mut self, always_on_top: bool) -> Self {
        self.windows[0].always_on_top = always_on_top;
        self
    }

    /// Add an additional window to open at startup.
    pub fn window(mut self, config: WindowConfig) -> Self {
        self.windows.push(config);
        self
    }

    /// Set the metadata shown in the framework's built-in About dialog.
    ///
    /// On macOS the standard **About <App>** menu item opens the dialog; from
    /// the frontend, call `openAbout()` (exported by `@elyra/runtime`) to open
    /// it from a button.
    pub fn about(mut self, about: AboutInfo) -> Self {
        self.about = about;
        self
    }

    /// Remember the primary window's size, position, and maximized state between
    /// runs (stored under the OS config directory, keyed by the About name).
    pub fn persist_window_state(mut self) -> Self {
        self.persist_window = true;
        self
    }

    /// Enable the framework's built-in update flow (`updater` feature).
    ///
    /// The shell exposes `/__update/check` + `/__update/install` and emits
    /// progress on the `elyra:update` event channel; `@elyra/runtime` renders
    /// the update toast from those events. A silent check on startup is opt-in
    /// via [`UpdaterConfig::auto_check`](crate::updater::UpdaterConfig::auto_check).
    #[cfg(feature = "updater")]
    pub fn updater(mut self, config: crate::updater::UpdaterConfig) -> Self {
        self.updater = Some(config);
        self
    }

    /// Configure a system tray icon + menu. Menu clicks arrive on the `"tray"`
    /// event channel; a `Quit` item closes the app.
    #[cfg(feature = "tray")]
    pub fn tray(mut self, config: crate::tray::TrayConfig) -> Self {
        self.tray = Some(config);
        self
    }

    /// Connect a database (lazily) and bind it as [`Database`] in the container.
    /// The URL scheme picks the driver: `sqlite:` / `mysql:` / `postgres:`.
    ///
    /// [`Database`]: elyra_db::Database
    #[cfg(feature = "database")]
    pub fn database(mut self, url: impl Into<String>) -> Self {
        self.db_url = Some(url.into());
        self
    }

    /// Open the window and run until it closes.
    ///
    /// If `ELYRA_CODEGEN_OUT` is set (as `rata codegen` does), this instead
    /// writes the TypeScript bindings to that path and returns without opening
    /// a window.
    pub fn run(self) -> crate::Result<()> {
        if let Some(out) = std::env::var_os("ELYRA_CODEGEN_OUT") {
            let ts = crate::codegen::generate(&self.registry).map_err(Error::Codegen)?;
            std::fs::write(&out, &ts).map_err(|e| Error::Io(e.to_string()))?;
            eprintln!(
                "codegen: wrote {} ({} bytes)",
                std::path::Path::new(&out).display(),
                ts.len()
            );
            return Ok(());
        }

        // The event loop must be created on the main thread; its proxy lets
        // `Windows` open more windows at runtime.
        let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

        // The runtime must exist before we build lazy DB pools (sqlx spawns a
        // pool maintenance task) and before `boot`, which may run async setup.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to build tokio runtime");

        let prepared = {
            let _guard = rt.enter();
            let mut app = self;
            app.container.bind(Windows::new(event_loop.create_proxy()));

            #[cfg(feature = "database")]
            if let Some(url) = app.db_url.clone() {
                let db =
                    elyra_db::Database::connect_lazy(&url).expect("failed to create database pool");
                app.container.bind(db);
            }

            app.prepare()
        };

        shell::run(
            rt,
            event_loop,
            prepared.registry,
            prepared.ctx,
            prepared.bus,
            prepared.assets,
            prepared.windows,
            prepared.tray,
            prepared.about,
            prepared.persist_window,
        )
    }

    /// Assemble the app: run every provider's `register`, bind the event bus,
    /// build the context, then run every provider's `boot`. Exposed (hidden) so
    /// tests can exercise wiring without opening a window.
    #[doc(hidden)]
    pub fn prepare(self) -> Prepared {
        let App {
            mut container,
            registry,
            providers,
            assets,
            bus,
            windows,
            tray,
            mut about,
            persist_window,
            #[cfg(feature = "updater")]
            updater,
            db_url: _,
        } = self;

        // Sensible fallbacks so the dialog is never blank.
        if about.name.is_empty() {
            about.name = windows.first().map(|w| w.title.clone()).unwrap_or_default();
        }

        // Build the update runtime and bind it so the shell can drive it.
        #[cfg(feature = "updater")]
        if let Some(cfg) = updater {
            match cfg.build() {
                Ok(u) => container.bind(crate::updater::UpdaterRuntime {
                    updater: u,
                    manifest_url: cfg.manifest_url.clone(),
                    target: crate::updater::Updater::current_target(),
                    auto_check: cfg.auto_check,
                }),
                Err(e) => eprintln!("updater: invalid config ({e}); update flow disabled"),
            }
        }

        // Phase 1: every provider binds its services.
        for provider in &providers {
            provider.register(&mut container);
        }

        // The bus is always resolvable from inside commands.
        container.bind(bus.clone());

        let ctx = Ctx::new(Arc::new(container));

        // Phase 2: boot with a fully populated context.
        for provider in &providers {
            provider.boot(&ctx);
        }

        Prepared {
            ctx,
            registry: Arc::new(registry),
            bus,
            assets,
            windows,
            tray,
            about,
            persist_window,
        }
    }
}
