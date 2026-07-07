//! Ratatosk (`rata`) — the Elyra CLI, Artisan's counterpart.
//!
//! The squirrel that carries messages up and down Yggdrasil, between the Rust
//! root and the Svelte crown.
//!
//! M2 ships `codegen`, `build`, and `dev`, driven by an `elyra.toml` at the
//! workspace root. `new` (scaffolding) lands in M3.

mod bundle;
mod config;
mod migrate;
mod scaffold;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use config::Config;
use scaffold::NewOptions;

const HELP: &str = "\
rata — the Elyra CLI

USAGE:
    rata <command>

COMMANDS:
    new <name>    Scaffold a new workspace + Svelte app
                    [--elyra <path>]  path dep on the framework (pre-publish)
                    [--dir <parent>]  parent directory (default: cwd)
    dev           Vite dev server + app pointed at it (HMR)
    codegen       specta -> TypeScript types + typed api.* facade
    build         Vite build -> embedded assets -> release binary
    bundle        Package the release binary into a macOS .app

    migrate            Apply pending database migrations
    migrate:rollback   Roll back the most recent batch
    migrate:status     Show applied/pending migrations
    make:migration <name>   Scaffold up/down migration files

    help          Show this message

Configuration is read from `elyra.toml` at the workspace root.
";

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "help".into());

    let result = match cmd.as_str() {
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        "codegen" => run(codegen),
        "build" => run(build),
        "bundle" => run(bundle::bundle),
        "dev" => run(dev),
        "migrate" => run(migrate::migrate),
        "migrate:rollback" => run(migrate::rollback),
        "migrate:status" => run(migrate::status),
        "make:migration" => run(migrate::make_migration),
        "new" => new_command(),
        other => {
            eprintln!("rata: unknown command `{other}`\n");
            print!("{HELP}");
            std::process::exit(1);
        }
    };

    if let Err(err) = result {
        eprintln!("rata {cmd}: {err}");
        std::process::exit(1);
    }
}

/// Load config, then run the given step.
fn run(step: fn(&Config) -> Result<(), String>) -> Result<(), String> {
    let cfg = Config::load()?;
    step(&cfg)
}

/// `rata new <name> [--elyra <path>] [--dir <parent>]` (no config required).
fn new_command() -> Result<(), String> {
    let mut args = std::env::args().skip(2);
    let mut name = None;
    let mut elyra_path = None;
    let mut parent_dir = std::env::current_dir().map_err(|e| e.to_string())?;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--elyra" => elyra_path = args.next(),
            "--dir" => parent_dir = PathBuf::from(args.next().ok_or("--dir needs a value")?),
            other if !other.starts_with('-') => name = Some(other.to_string()),
            other => return Err(format!("unknown flag `{other}`")),
        }
    }

    let name = name.ok_or("usage: rata new <name> [--elyra <path>] [--dir <parent>]")?;
    scaffold::new_project(NewOptions {
        name,
        parent_dir,
        elyra_path,
    })
}

/// `rata codegen` — run the app in codegen mode; it writes the bindings and exits.
fn codegen(cfg: &Config) -> Result<(), String> {
    let out = cfg.root.join(&cfg.codegen_out);
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    println!("codegen: {} -> {}", cfg.app_crate, out.display());

    let status = Command::new("cargo")
        .args(["run", "--quiet", "-p", &cfg.app_crate])
        .env("ELYRA_CODEGEN_OUT", &out)
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;

    exit_ok(status, "codegen")
}

/// `rata build` — build the frontend, then the release binary that embeds it.
fn build(cfg: &Config) -> Result<(), String> {
    let frontend = cfg.root.join(&cfg.frontend_dir);
    println!("build: vite ({})", frontend.display());
    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&frontend)
        .status()
        .map_err(|e| format!("failed to run npm: {e}"))?;
    exit_ok(status, "npm run build")?;

    println!("build: cargo --release ({})", cfg.app_crate);
    let status = Command::new("cargo")
        .args(["build", "--release", "-p", &cfg.app_crate])
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    exit_ok(status, "cargo build")
}

/// `rata dev` — start Vite, wait for it, then run the app pointed at it for HMR.
fn dev(cfg: &Config) -> Result<(), String> {
    let frontend = cfg.root.join(&cfg.frontend_dir);
    let url = "http://localhost:5173";

    println!("dev: starting vite in {}", frontend.display());
    let mut vite = Command::new("npm")
        .args(["run", "dev"])
        .current_dir(&frontend)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to start vite: {e}"))?;

    wait_for_port(5173, 100);
    println!("dev: launching {} against {url}", cfg.app_crate);

    let status = Command::new("cargo")
        .args(["run", "-p", &cfg.app_crate])
        .env("ELYRA_DEV_URL", url)
        .current_dir(&cfg.root)
        .status();

    // Always tear down Vite when the app exits.
    let _ = vite.kill();
    let _ = vite.wait();

    exit_ok(status.map_err(|e| e.to_string())?, "app")
}

fn exit_ok(status: std::process::ExitStatus, what: &str) -> Result<(), String> {
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} exited with {status}"))
    }
}

/// Poll a localhost TCP port until it accepts a connection (or we give up).
fn wait_for_port(port: u16, tries: u32) {
    use std::net::{Ipv4Addr, SocketAddr, TcpStream};
    use std::time::Duration;

    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    for _ in 0..tries {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("dev: warning — vite not reachable on :{port} yet, launching anyway");
}
