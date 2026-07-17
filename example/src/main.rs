//! Elyra M1 demo — the DX benchmark.
//!
//! Commands over the MessagePack bridge (M0) *plus* the EventBus (M1): echo
//! round-trips for latency, synchronous bursts for batching, and a timed stream.
//!
//!   cargo run -p elyra-example          # serves the framework's fallback page
//!   (cd example/app && npm i && npm run build) then re-run  # serves Svelte

use std::time::{Duration, Instant};

use elyra::command::BoxFuture;
use elyra::updater::Updater;
use elyra::{
    command, commands, AboutInfo, App, CommandRequest, Container, Ctx, Database, EventBus, Menu,
    Middleware, Model, Next, Provider, Result, Submenu, TrayConfig, UpdaterConfig, WindowConfig,
    Windows,
};
use serde::{Deserialize, Serialize};

const DB_URL: &str = concat!(
    "sqlite://",
    env!("CARGO_MANIFEST_DIR"),
    "/elyra-example.db?mode=rwc"
);

/// The built Svelte frontend, embedded from memory. Empty until you run the
/// Vite build — the shell falls back to its built-in demo page in that case.
#[derive(rust_embed::RustEmbed)]
#[folder = "app/dist"]
struct Assets;

/// A trivial service resolved from the container inside `greet`.
struct Greeter {
    prefix: String,
}

/// A provider that binds the [`Greeter`] service (Laravel's ServiceProvider).
struct GreeterProvider;

impl Provider for GreeterProvider {
    fn register(&self, container: &mut Container) {
        container.bind(Greeter {
            prefix: "Hello".into(),
        });
    }

    fn boot(&self, _ctx: &Ctx) {
        eprintln!("boot: GreeterProvider ready");
    }
}

/// Logs every command and how long it took (the middleware pipeline).
struct Timing;

impl Middleware for Timing {
    fn handle(
        &self,
        ctx: Ctx,
        req: CommandRequest,
        next: Next,
    ) -> BoxFuture<'static, Result<Vec<u8>>> {
        Box::pin(async move {
            let name = req.name.clone();
            let started = Instant::now();
            let out = next.run(ctx, req).await;
            let status = if out.is_ok() { "ok" } else { "err" };
            eprintln!("cmd {name} [{status}] {:?}", started.elapsed());
            out
        })
    }
}

#[command]
async fn greet(ctx: Ctx, name: String) -> String {
    let greeter = ctx.get::<Greeter>();
    format!("{}, {}!", greeter.prefix, name)
}

#[command]
async fn add(_ctx: Ctx, a: i64, b: i64) -> i64 {
    a + b
}

/// A fallible command: `Err` becomes a rejected promise (`CommandError`) in JS.
#[command]
async fn checked_div(_ctx: Ctx, a: i64, b: i64) -> std::result::Result<i64, String> {
    if b == 0 {
        Err("cannot divide by zero".into())
    } else {
        Ok(a / b)
    }
}

// --- Database-backed todos (Result commands over the `Database` service) ----

/// A database model — CRUD + query builder come from `#[derive(Model)]`.
#[derive(Model, Serialize, Deserialize, specta::Type, Clone)]
#[model(table = "todos")]
struct Todo {
    id: i64,
    title: String,
    done: bool, // <-> the INTEGER `done` column
}

#[command]
async fn list_todos(ctx: Ctx) -> std::result::Result<Vec<Todo>, String> {
    let db = ctx.get::<Database>();
    Todo::query()
        .order_by("id")
        .get(&db)
        .await
        .map_err(|e| e.to_string())
}

#[command]
async fn add_todo(ctx: Ctx, title: String) -> std::result::Result<Todo, String> {
    let db = ctx.get::<Database>();
    let mut todo = Todo {
        id: 0,
        title,
        done: false,
    };
    todo.insert(&db).await.map_err(|e| e.to_string())?;
    Ok(todo)
}

/// The current updater target (e.g. `macos-aarch64`) + app version. Real update
/// checks call `Updater::check(manifest_url, target)`; see the updater docs.
#[command]
async fn update_target(_ctx: Ctx) -> String {
    format!(
        "{} v{}",
        Updater::current_target(),
        env!("CARGO_PKG_VERSION")
    )
}

/// Open a second window at runtime via the container-bound `Windows` handle.
#[command]
async fn open_window(ctx: Ctx) {
    ctx.get::<Windows>().open(
        WindowConfig::new("second")
            .title("Elyra — second window")
            .size(420.0, 340.0)
            .min_size(320.0, 240.0),
    );
}

/// A struct result — serialized as a named map, decoded to a plain JS object.
#[derive(Serialize, Deserialize, specta::Type)]
struct SystemInfo {
    os: String,
    arch: String,
    commands: Vec<String>,
}

#[command]
async fn system_info(_ctx: Ctx) -> SystemInfo {
    SystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        commands: vec![
            "greet".into(),
            "add".into(),
            "system_info".into(),
            "emit_echo".into(),
            "burst".into(),
            "stream".into(),
        ],
    }
}

/// Round-trip latency probe: bounce `seq` straight back over the bus.
#[command]
async fn emit_echo(ctx: Ctx, seq: u32) -> u32 {
    ctx.get::<EventBus>().emit("echo", &seq).ok();
    seq
}

/// Batching demo: emit `count` events synchronously. They land in one queue and
/// flush as a single batch — one IPC round for N changes.
#[command]
async fn burst(ctx: Ctx, count: u32) -> u32 {
    let bus = ctx.get::<EventBus>();
    for i in 0..count {
        bus.emit("burst", &i).ok();
    }
    count
}

/// Stream demo: push `count` events spaced `interval_ms` apart from a background
/// task, so the command returns immediately.
#[command]
async fn stream(ctx: Ctx, count: u32, interval_ms: u64) -> u32 {
    let bus = ctx.get::<EventBus>();
    tokio::spawn(async move {
        for i in 0..count {
            bus.emit("stream", &i).ok();
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    });
    count
}

// --- Opt-in latency harness (ELYRA_BENCH=1) --------------------------------

/// Whether the frontend should auto-run its input→paint benchmark on mount.
#[command]
async fn bench_enabled(_ctx: Ctx) -> bool {
    std::env::var("ELYRA_BENCH").is_ok()
}

/// Bounce `seq` back over the bus (the benchmark's per-sample round-trip).
#[command]
async fn bench_echo(ctx: Ctx, seq: u32) -> u32 {
    ctx.get::<EventBus>().emit("bench", &seq).ok();
    seq
}

#[derive(Serialize, Deserialize, specta::Type, Debug)]
struct BenchStats {
    n: u32,
    avg: f64,
    p50: f64,
    p95: f64,
    max: f64,
}

/// Receive the frontend's measured stats and print them to stderr.
#[command]
async fn bench_report(_ctx: Ctx, stats: BenchStats) {
    eprintln!("ELYRA_BENCH input->paint ms: {stats:?}");
}

/// Ask the built-in AI SDK a question. Needs `ANTHROPIC_API_KEY` (or
/// `OPENAI_API_KEY`) in the environment; errors are surfaced to the frontend.
#[command]
async fn ask(ctx: Ctx, prompt: String) -> Result<String, String> {
    let ai = ctx.get::<elyra::ai::Ai>();
    ai.chat()
        .instructions("You are a concise assistant inside a desktop app.")
        .prompt(prompt)
        .await
        .map(|r| r.text().to_string())
        .map_err(|e| e.to_string())
}

/// Increment a cache-backed visit counter (shared with the frontend `cache`).
#[command]
async fn visit_count(ctx: Ctx) -> i64 {
    ctx.get::<elyra::cache::Cache>().increment("visits", 1)
}

/// Save a note to the storage disk and read it back.
#[command]
async fn save_note(ctx: Ctx, text: String) -> Result<String, String> {
    let storage = ctx.get::<elyra::storage::Storage>();
    storage
        .put_str("note.txt", &text)
        .map_err(|e| e.to_string())?;
    storage.get_str("note.txt").map_err(|e| e.to_string())
}

/// Enqueue a background job (processed by `JobsProvider`; status on `elyra:queue`).
#[command]
async fn enqueue(ctx: Ctx, label: String) {
    ctx.get::<elyra::queue::Queue>()
        .push("log", serde_json::json!({ "label": label }));
}

/// Registers queue handlers once the container is booted.
struct JobsProvider;
impl Provider for JobsProvider {
    fn boot(&self, ctx: &Ctx) {
        ctx.get::<elyra::queue::Queue>()
            .on("log", |payload| async move {
                eprintln!("queue: log job -> {}", payload["label"]);
                Ok(())
            });
    }
}

/// Stream an AI answer to the frontend token-by-token over the `elyra:ai`
/// channel. Returns once the stream completes.
#[command]
async fn ask_stream(ctx: Ctx, prompt: String) -> Result<(), String> {
    use elyra::ai::StreamChunk;
    let ai = ctx.get::<elyra::ai::Ai>();
    let bus = ctx.get::<EventBus>();
    let mut chunks = ai
        .chat()
        .instructions("You are a concise assistant inside a desktop app.")
        .stream(prompt);
    while let Some(chunk) = chunks.next().await {
        if let StreamChunk::Delta(text) = chunk.map_err(|e| e.to_string())? {
            let _ = bus.emit("elyra:ai", &text);
        }
    }
    Ok(())
}

fn main() -> elyra::Result<()> {
    // Auto-migrate for the demo (in a real app you'd run `rata migrate`).
    // Skipped in codegen mode, which never opens a window.
    if std::env::var_os("ELYRA_CODEGEN_OUT").is_none() {
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        rt.block_on(async {
            let db = Database::connect(DB_URL).await.expect("db connect");
            db.migrator(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))
                .run()
                .await
                .expect("migrate");
        });
    }

    App::new()
        .title("Elyra M6")
        .size(560.0, 760.0)
        .min_size(420.0, 480.0)
        .persist_window_state()
        .single_instance()
        .deep_link("elyra-example")
        .global_shortcut("CmdOrCtrl+Shift+P")
        .menu(
            Menu::new().submenu(
                Submenu::new("File")
                    .item_accel("file.new", "New", "CmdOrCtrl+N")
                    .separator()
                    .item("file.export", "Export\u{2026}"),
            ),
        )
        .about(
            AboutInfo::new("Elyra Example", env!("CARGO_PKG_VERSION"))
                .description(
                    "A reference app for the Elyra framework \u{2014} commands, events, \
                     database, tray, windows, and this built-in About dialog.",
                )
                .website("elyracode.com")
                .repository("github.com/kwhorne/elyra-framework")
                .author("Knut W. Horne", "kwhorne.com"),
        )
        // Demo wiring. A real app points `manifest_url` at its release feed and
        // ships the matching public key; here auto-check is off so startup makes
        // no network call. `Check for updates` in the UI drives it manually.
        .updater(
            UpdaterConfig::new(
                "6kpsY+KcUgq+9VB7Ey7F+ZVHdq6+vnuSQh7qaRRG0iw=",
                "https://example.com/latest.json",
                env!("CARGO_PKG_VERSION"),
            )
            .auto_check(false),
        )
        .provider(GreeterProvider)
        .provider(elyra::cache::CacheProvider)
        .provider(elyra::storage::StorageProvider::at(
            std::env::temp_dir().join("elyra-example"),
        ))
        .provider(elyra::queue::QueueProvider)
        .provider(JobsProvider)
        .provider(elyra::ai::AiProvider)
        .middleware(Timing)
        .database(DB_URL)
        .tray(
            TrayConfig::new()
                .tooltip("Elyra M6")
                .title("Elyra")
                .item("open", "Open Elyra")
                .separator()
                .quit("Quit"),
        )
        .commands(commands![
            greet,
            add,
            checked_div,
            open_window,
            update_target,
            list_todos,
            add_todo,
            system_info,
            emit_echo,
            burst,
            stream,
            bench_enabled,
            bench_echo,
            bench_report,
            ask,
            ask_stream,
            visit_count,
            save_note,
            enqueue
        ])
        .assets(elyra::asset_resolver::<Assets>())
        .run()
}
