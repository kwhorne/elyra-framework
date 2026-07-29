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
    shortcuts: Vec<String>,
    menu: Option<crate::menu::Menu>,
    single_instance: bool,
    deep_link: Option<String>,
    csp: Option<String>,
    open_schemes: Vec<String>,
    open_paths: bool,
    sidecar_allow: Vec<String>,
    capabilities: std::collections::HashSet<crate::security::Capability>,
    #[allow(clippy::type_complexity)]
    event_types: Vec<(
        &'static str,
        fn(&mut specta::Types) -> specta::datatype::DataType,
    )>,
    numbers: crate::codegen::NumberPolicy,
    max_body: usize,
    csp_disabled: bool,
    #[cfg(feature = "updater")]
    updater: Option<crate::updater::UpdaterConfig>,
    #[cfg_attr(not(feature = "database"), allow(dead_code))]
    db_url: Option<String>,
    #[cfg(feature = "database")]
    migrations: Vec<Box<dyn elyra_db::RustMigration>>,
    #[cfg(feature = "database")]
    seeders: Vec<Box<dyn crate::seeder::Seeder>>,
}

/// The fully assembled application, ready to run (or inspect in tests).
#[doc(hidden)]
pub struct Prepared {
    pub ctx: Ctx,
    pub policy: crate::security::Policy,
    pub registry: Arc<CommandRegistry>,
    pub bus: EventBus,
    pub assets: Option<AssetResolver>,
    pub windows: Vec<WindowConfig>,
    pub tray: Option<crate::tray::TrayConfig>,
    pub about: AboutInfo,
    pub persist_window: bool,
    pub shortcuts: Vec<String>,
    pub menu: Option<crate::menu::Menu>,
    pub single_instance: bool,
    pub deep_link: Option<String>,
    pub csp: Option<String>,
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
            shortcuts: Vec::new(),
            menu: None,
            single_instance: false,
            deep_link: None,
            csp: None,
            open_schemes: Vec::new(),
            open_paths: true,
            sidecar_allow: Vec::new(),
            capabilities: crate::security::Capability::defaults()
                .iter()
                .copied()
                .collect(),
            event_types: Vec::new(),
            numbers: crate::codegen::NumberPolicy::default(),
            max_body: crate::wire::DEFAULT_MAX_BODY,
            csp_disabled: false,
            #[cfg(feature = "updater")]
            updater: None,
            db_url: None,
            #[cfg(feature = "database")]
            migrations: Vec::new(),
            #[cfg(feature = "database")]
            seeders: Vec::new(),
        }
    }

    /// Grant the frontend a native [`Capability`](crate::security::Capability).
    ///
    /// The everyday ones (commands, window control, store/cache reads + writes,
    /// storage reads + writes, queue, sidecar, system, autostart, update check)
    /// are granted by default. The destructive ones — `StoreClear`, `CacheFlush`,
    /// `StorageDelete`, `UpdaterInstall` — must be granted explicitly:
    ///
    /// ```no_run
    /// # use elyra::{App, security::Capability};
    /// App::new().allow_frontend(Capability::UpdaterInstall);
    /// ```
    pub fn allow_frontend(mut self, capability: crate::security::Capability) -> Self {
        self.capabilities.insert(capability);
        self
    }

    /// Revoke a capability from the frontend (e.g. keep the settings store
    /// Rust-only). Commands stay available unless you revoke `Capability::Commands`.
    pub fn deny_frontend(mut self, capability: crate::security::Capability) -> Self {
        self.capabilities.remove(&capability);
        self
    }

    /// Declare the payload type of an event channel, so `rata codegen` can emit a
    /// typed `channel()` for it.
    ///
    /// ```no_run
    /// # use elyra::App;
    /// # #[derive(serde::Serialize, specta::Type)] struct Progress { percent: u8 }
    /// App::new().event::<Progress>("progress");
    /// ```
    ///
    /// The frontend then gets `channel("progress")` typed as `Progress` — and an
    /// unknown channel name is a compile error instead of a silent `undefined`.
    pub fn event<T: specta::Type + 'static>(mut self, channel: &'static str) -> Self {
        self.event_types
            .push((channel, |types| T::definition(types)));
        self
    }

    /// Export 64-bit integers as `bigint` instead of `number`.
    ///
    /// MessagePack carries `i64` losslessly on the wire, but JS `number` starts
    /// losing precision past 2^53. Turn this on when the app really moves such
    /// values (ids from an external system, byte counts of huge files).
    pub fn codegen_bigint(mut self) -> Self {
        self.numbers = crate::codegen::NumberPolicy::BigInt;
        self
    }

    /// Maximum accepted request body on the IPC routes (default 16 MiB).
    /// Bodies above this are refused with `413` before any decoding.
    pub fn max_request_body(mut self, bytes: usize) -> Self {
        self.max_body = bytes;
        self
    }

    /// Serve HTML without any `Content-Security-Policy`.
    ///
    /// Elyra sends [`DEFAULT_CSP`](crate::security::DEFAULT_CSP) unless you
    /// override it with [`csp`](App::csp). Only disable it while debugging a
    /// policy problem — a webview with native capabilities and no CSP turns any
    /// injected script into an attacker's foothold.
    pub fn csp_disabled(mut self) -> Self {
        self.csp_disabled = true;
        self.csp = None;
        self
    }

    /// Allow extra URL schemes for `shell.open` from the frontend.
    ///
    /// `http`, `https`, and `mailto` are always allowed; everything else — and
    /// any executable file — is refused, because "open with the OS default
    /// handler" must not become "run this program". See [`crate::security`].
    ///
    /// ```no_run
    /// # use elyra::App;
    /// App::new().allow_open_schemes(["slack", "zoommtg"]);
    /// ```
    pub fn allow_open_schemes<I, S>(mut self, schemes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.open_schemes.extend(
            schemes
                .into_iter()
                .map(|s| s.into().trim().to_ascii_lowercase()),
        );
        self
    }

    /// Refuse local paths in `shell.open` (URL schemes only). Use this when the
    /// frontend never needs to reveal a file in Finder/Explorer.
    pub fn deny_open_paths(mut self) -> Self {
        self.open_paths = false;
        self
    }

    /// Allow the **frontend** to spawn `program` as a sidecar process.
    ///
    /// Default deny: without this, `/__sidecar/spawn` refuses everything, since
    /// an unrestricted spawn endpoint turns any script in the webview into
    /// arbitrary code execution. A bare name (`"ffmpeg"`) also matches an
    /// absolute path ending in that name; an entry given as a path must match
    /// exactly. Rust-side `Sidecar::spawn` is unaffected.
    ///
    /// ```no_run
    /// # use elyra::App;
    /// App::new().sidecar_allow("ffmpeg");
    /// ```
    pub fn sidecar_allow(mut self, program: impl Into<String>) -> Self {
        self.sidecar_allow.push(program.into());
        self
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

    /// Register an OS-level global keyboard shortcut (`shortcuts` feature).
    /// The accelerator (e.g. `"CmdOrCtrl+Shift+P"`) fires the `elyra:shortcut`
    /// event on the frontend, carrying the accelerator string.
    #[cfg(feature = "shortcuts")]
    pub fn global_shortcut(mut self, accelerator: impl Into<String>) -> Self {
        self.shortcuts.push(accelerator.into());
        self
    }

    /// Set a native application menu. Custom submenus are appended after the
    /// standard app + Edit menus; item clicks emit `elyra:menu` (rendered on
    /// macOS — see [`crate::menu`]).
    /// Ensure only one instance of the app runs. Later launches focus this
    /// window and forward their command line (e.g. a deep-link URL) on the
    /// `elyra:second-instance` channel, then exit. See [`crate::instance`].
    pub fn single_instance(mut self) -> Self {
        self.single_instance = true;
        self
    }

    /// Override the `Content-Security-Policy` served with HTML responses on the
    /// `elyra://` protocol.
    ///
    /// [`DEFAULT_CSP`](crate::security::DEFAULT_CSP) is applied automatically; use
    /// this when the app needs to widen it (e.g. an external image host), and
    /// [`csp_disabled`](App::csp_disabled) to turn it off entirely.
    pub fn csp(mut self, policy: impl Into<String>) -> Self {
        self.csp = Some(policy.into());
        self.csp_disabled = false;
        self
    }

    /// Register a custom URL scheme (e.g. `"myapp"` for `myapp://…` links).
    /// The launch URL is available via the runtime's `deepLink.initial()`; later
    /// URLs arrive on `elyra:deep-link`. See [`crate::deeplink`].
    pub fn deep_link(mut self, scheme: impl Into<String>) -> Self {
        self.deep_link = Some(scheme.into());
        self
    }

    pub fn menu(mut self, menu: crate::menu::Menu) -> Self {
        self.menu = Some(menu);
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

    /// Register Rust migrations (built with the
    /// [`Schema`](elyra_db::Schema) builder).
    ///
    /// Run them with `ELYRA_MIGRATE=up cargo run` (or `=down` to roll back the
    /// last batch): the app applies them and exits without opening a window, the
    /// same trick `rata codegen` uses. `.sql` migrations in the migrations
    /// directory keep working through `rata migrate` and share the same history
    /// table.
    #[cfg(feature = "database")]
    pub fn migrations(mut self, migrations: Vec<Box<dyn elyra_db::RustMigration>>) -> Self {
        self.migrations.extend(migrations);
        self
    }

    /// Register a database seeder, runnable with `ELYRA_SEED=1 cargo run`.
    #[cfg(feature = "database")]
    pub fn seeder(mut self, seeder: impl crate::seeder::Seeder + 'static) -> Self {
        self.seeders.push(Box::new(seeder));
        self
    }

    /// Open the window and run until it closes.
    ///
    /// If `ELYRA_CODEGEN_OUT` is set (as `rata codegen` does), this instead
    /// writes the TypeScript bindings to that path and returns without opening
    /// a window.
    pub fn run(self) -> crate::Result<()> {
        // Single-instance: if a primary is already running, hand it our payload
        // (any deep-link URL from argv) and exit before doing any real work.
        if self.single_instance {
            let app_id = if !self.about.name.is_empty() {
                self.about.name.clone()
            } else {
                self.windows
                    .first()
                    .map(|w| w.title.clone())
                    .unwrap_or_else(|| "Elyra".to_string())
            };
            let payload = self
                .deep_link
                .as_deref()
                .and_then(crate::deeplink::url_in_args)
                .unwrap_or_default();
            if crate::instance::notify_primary(&app_id, &payload) {
                return Ok(());
            }
        }

        if let Some(out) = std::env::var_os("ELYRA_CODEGEN_OUT") {
            let ts = crate::codegen::generate_with(&self.registry, &self.event_types, self.numbers)
                .map_err(Error::Codegen)?;
            std::fs::write(&out, &ts).map_err(|e| Error::Io(e.to_string()))?;
            eprintln!(
                "codegen: wrote {} ({} bytes)",
                std::path::Path::new(&out).display(),
                ts.len()
            );
            return Ok(());
        }

        // Migration / seeding modes: do the work, print a summary, and exit
        // without a window (so the CLI can drive app-side migrations + seeders).
        #[cfg(feature = "database")]
        {
            let migrate_mode = std::env::var("ELYRA_MIGRATE").ok();
            let seed_mode = std::env::var("ELYRA_SEED").ok();
            if migrate_mode.is_some() || seed_mode.is_some() {
                return self.run_database_task(migrate_mode.as_deref(), seed_mode.is_some());
            }
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
            prepared.shortcuts,
            prepared.menu,
            prepared.single_instance,
            prepared.deep_link,
            prepared.csp,
            prepared.policy,
        )
    }

    /// Apply Rust migrations and/or run seeders, then return (no window).
    #[cfg(feature = "database")]
    fn run_database_task(self, migrate: Option<&str>, seed: bool) -> crate::Result<()> {
        let url = self
            .db_url
            .clone()
            .or_else(|| std::env::var("DATABASE_URL").ok())
            .ok_or_else(|| {
                Error::Io("no database configured (App::database or DATABASE_URL)".into())
            })?;

        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::Io(e.to_string()))?;

        let migrations = self.migrations;
        let seeders = self.seeders;

        rt.block_on(async move {
            let db = elyra_db::Database::connect(&url)
                .await
                .map_err(|e| Error::Io(e.to_string()))?;
            let migrator = db.migrator(std::path::PathBuf::from("migrations"));

            match migrate {
                Some("down") | Some("rollback") => {
                    let rolled = migrator
                        .rollback_rust(&migrations, db.driver())
                        .await
                        .map_err(|e| Error::Io(e.to_string()))?;
                    for version in &rolled {
                        crate::info!(target: "elyra::migrate", "rolled back {version}");
                    }
                    if rolled.is_empty() {
                        crate::info!(target: "elyra::migrate", "nothing to roll back");
                    }
                }
                Some(_) => {
                    let applied = migrator
                        .run_rust(&migrations, db.driver())
                        .await
                        .map_err(|e| Error::Io(e.to_string()))?;
                    for version in &applied {
                        crate::info!(target: "elyra::migrate", "migrated {version}");
                    }
                    if applied.is_empty() {
                        crate::info!(target: "elyra::migrate", "nothing to migrate");
                    }
                }
                None => {}
            }

            if seed {
                for seeder in &seeders {
                    crate::info!(target: "elyra::seed", "seeding {}", seeder.name());
                    seeder
                        .run(&db)
                        .await
                        .map_err(|e| Error::Io(format!("seeder `{}`: {e}", seeder.name())))?;
                }
                if seeders.is_empty() {
                    crate::info!(target: "elyra::seed", "no seeders registered");
                }
            }
            Ok(())
        })
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
            shortcuts,
            menu,
            single_instance,
            deep_link,
            csp,
            open_schemes,
            open_paths,
            sidecar_allow,
            capabilities,
            event_types: _,
            numbers: _,
            max_body,
            csp_disabled,
            #[cfg(feature = "updater")]
            updater,
            db_url: _,
            #[cfg(feature = "database")]
                migrations: _,
            #[cfg(feature = "database")]
                seeders: _,
        } = self;

        // A local app gets a strict CSP unless it opted out or set its own.
        let csp = match (csp, csp_disabled) {
            (Some(policy), _) => Some(policy),
            (None, true) => None,
            (None, false) => Some(crate::security::DEFAULT_CSP.to_string()),
        };

        // The IPC security policy: a fresh token for this run, the dev-server
        // origin (if any), the capability grants, and the open/sidecar allowlists.
        let policy = crate::security::Policy::new(
            open_schemes,
            open_paths,
            sidecar_allow,
            capabilities,
            max_body,
        );

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
                Err(e) => crate::error!(
                    target: "elyra::updater",
                    "invalid config ({e}); the update flow is disabled"
                ),
            }
        }

        // Phase 1: every provider binds its services.
        for provider in &providers {
            provider.register(&mut container);
        }

        // The bus is always resolvable from inside commands.
        container.bind(bus.clone());

        // A small persistent settings store, keyed by the app's About name.
        container.bind(crate::store::Store::open(&about.name));

        // Commands can consult the policy (e.g. before handing a path to the OS).
        container.bind(policy.clone());

        // Sidecar process manager (streams output on the `elyra:sidecar` channel).
        #[cfg(feature = "sidecar")]
        container.bind(crate::sidecar::Sidecar::new(bus.clone()));

        let ctx = Ctx::new(Arc::new(container));

        // Phase 2: boot with a fully populated context.
        for provider in &providers {
            provider.boot(&ctx);
        }

        Prepared {
            ctx,
            policy,
            registry: Arc::new(registry),
            bus,
            assets,
            windows,
            tray,
            about,
            persist_window,
            shortcuts,
            menu,
            single_instance,
            deep_link,
            csp,
        }
    }
}
