//! Ratatosk (`rata`) — the Elyra CLI, Artisan's counterpart.
//!
//! The squirrel that carries messages up and down Yggdrasil, between the Rust
//! root and the Svelte crown.
//!
//! M2 ships `codegen`, `build`, and `dev`, driven by an `elyra.toml` at the
//! workspace root. `new` (scaffolding) lands in M3.

mod ai;
mod bundle;
mod config;
mod make;
mod make_notifications;
mod make_resource;
mod mcp_dev;
mod migrate;
mod resource;
mod resource_backend;
mod resource_generate;
mod resource_views;
mod scaffold;

use std::path::PathBuf;
use std::process::{Command, Stdio};

use config::Config;
use scaffold::NewOptions;

pub(crate) const HELP: &str = "\
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
    make:command <name>     Scaffold a #[command] handler
    make:provider <name>    Scaffold a Provider
    make:middleware <name>  Scaffold a command Middleware
    make:model <name>       Scaffold a #[derive(Model)] struct
    make:resource <Model>   The commands, validation and tests for a model
                              [--view] [--dry-run] [--force] [--no-abilities]
                              [--generate name:string email:email:unique …]
                              [--backend /api/<plural>]  over a Laravel API
    make:notifications      The page for Laravel's notifications, and Laravel's side
                              [--dry-run] [--force]
    mcp inspect [--json]    The MCP tools the app exposes to AI agents
    mcp install [--client claude|cursor|vscode|workspace] [--release]
                            Print the config that connects an MCP client
                              (workspace: add the app to .mcp.json)
    mcp dev                 An MCP server for coding agents working on the app:
                              docs, commands, models, schema, migrations, log
    assistant inspect [--json]
                            The app's assistant's tools, and which ask first
    ai:install              Guidelines (AGENTS.md), skills and `mcp dev` for agents
    ai:update               Refresh what ai:install wrote
    resources:sync          Rebuild the resource registries (after removing one)

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
        "make:command" => run(make::command),
        "make:provider" => run(make::provider),
        "make:middleware" => run(make::middleware),
        "make:model" => run(make::model),
        "make:resource" => run(make_resource::make_resource),
        "make:notifications" => run(make_notifications::make_notifications),
        "mcp" => run(mcp),
        "assistant" => run(assistant),
        "ai:install" => run(ai::install),
        "ai:update" => run(ai::update),
        "resources:sync" => run(resource::sync_command),
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

/// `rata mcp inspect` / `rata mcp install` (RFC 0003).
fn mcp(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    match args.first().map(String::as_str) {
        Some("inspect") => mcp_inspect(cfg, &args),
        Some("install") => mcp_install(cfg, &args),
        Some("dev") => mcp_dev::serve(cfg),
        _ => Err("usage: rata mcp inspect [--json] | rata mcp install [--client claude|cursor|vscode|workspace] [--release] | rata mcp dev".into()),
    }
}

/// `rata mcp inspect [--json]` — run the app in inspect mode and list the MCP
/// tools it exposes: name, ability, hints (a resource's URI among them), and
/// the first line of each description (`--json`: the full definitions,
/// schemas included).
fn mcp_inspect(cfg: &Config, args: &[String]) -> Result<(), String> {
    let text = inspect(cfg, "ELYRA_MCP_INSPECT")?;
    if args.iter().any(|a| a == "--json") {
        println!("{text}");
        return Ok(());
    }
    let tools: Vec<serde_json::Value> =
        serde_json::from_str(&text).map_err(|e| format!("the catalog: {e}"))?;
    if tools.is_empty() {
        println!("No tools: grant abilities with `App::mcp(Mcp::new().allow_abilities([..]))`.");
        return Ok(());
    }
    print_tools(&tools, false);
    println!(
        "\n{} tool(s). `rata mcp inspect --json` shows the schemas.",
        tools.len()
    );
    Ok(())
}

/// `rata assistant inspect [--json]` (RFC 0011) — the commands the app's
/// assistant may use, and for each whether it asks the user first: for a
/// review of what a prompt-injected model could do.
fn assistant(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    if args.first().map(String::as_str) != Some("inspect") {
        return Err("usage: rata assistant inspect [--json]".into());
    }
    let text = inspect(cfg, "ELYRA_ASSISTANT_INSPECT")?;
    if args.iter().any(|a| a == "--json") {
        println!("{text}");
        return Ok(());
    }
    let Some(tools) = assistant_tools(&text)? else {
        println!("No assistant: add `App::assistant(Assistant::new().allow_abilities([..]))`.");
        return Ok(());
    };
    if tools.is_empty() {
        println!("No tools: grant abilities with `Assistant::new().allow_abilities([..])`.");
        return Ok(());
    }
    print_tools(&tools, true);
    let (asks, unasked) = confirm_counts(&tools);
    println!(
        "\n{} tool(s): {asks} ask the user first, {unasked} write without asking.",
        tools.len()
    );
    if unasked > 0 {
        println!(
            "A model can be talked into anything its tools allow: put what can't be undone \
             behind `Assistant::confirm(..)`."
        );
    }
    println!("A signed-in user is offered only the tools they have the ability for.");
    Ok(())
}

/// The assistant's catalog, or `None` when the app has no assistant.
fn assistant_tools(text: &str) -> Result<Option<Vec<serde_json::Value>>, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("the catalog: {e}"))?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Array(tools) => Ok(Some(tools)),
        other => Err(format!("the catalog isn't a list: {other}")),
    }
}

/// How many tools ask the user first, and how many may write without asking
/// (neither confirmed nor a read-only live command).
fn confirm_counts(tools: &[serde_json::Value]) -> (usize, usize) {
    let asks = |t: &&serde_json::Value| t["annotations"]["destructiveHint"].as_bool() == Some(true);
    let reads = |t: &&serde_json::Value| t["annotations"]["readOnlyHint"].as_bool() == Some(true);
    let confirmed = tools.iter().filter(asks).count();
    let unasked = tools.iter().filter(|t| !asks(t) && !reads(t)).count();
    (confirmed, unasked)
}

/// Run the app with `var` set to a scratch file it writes its catalog to, and
/// read it back.
fn inspect(cfg: &Config, var: &str) -> Result<String, String> {
    let out = std::env::temp_dir().join(format!("elyra-inspect-{}.json", std::process::id()));
    let status = Command::new("cargo")
        .args(["run", "--quiet", "-p", &cfg.app_crate])
        .env(var, &out)
        .current_dir(&cfg.root)
        .status()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    exit_ok(status, "the app")?;
    let text = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _ = std::fs::remove_file(&out);
    Ok(text)
}

/// One line per tool, and the first line of its description. For the
/// assistant, whether it asks first is what matters; for MCP, the hints.
fn print_tools(tools: &[serde_json::Value], assistant: bool) {
    for line in tool_lines(tools, assistant) {
        println!("{line}");
    }
}

fn tool_lines(tools: &[serde_json::Value], assistant: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for tool in tools {
        let name = tool["name"].as_str().unwrap_or_default();
        let ability = tool["ability"].as_str().unwrap_or_default();
        let read_only = tool["annotations"]["readOnlyHint"].as_bool() == Some(true);
        let confirm = tool["annotations"]["destructiveHint"].as_bool() == Some(true);
        let mut hints = Vec::new();
        if assistant {
            hints.push(if confirm {
                "asks first"
            } else if read_only {
                "read-only"
            } else {
                "runs without asking"
            });
        } else {
            if read_only {
                hints.push("read-only");
            }
            if confirm {
                hints.push("confirm");
            }
        }
        let args: Vec<String> = tool["inputSchema"]["properties"]
            .as_object()
            .map(|props| {
                let required = &tool["inputSchema"]["required"];
                props
                    .keys()
                    .map(|k| {
                        let needed = required
                            .as_array()
                            .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(k)));
                        if needed {
                            k.clone()
                        } else {
                            format!("{k}?")
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !assistant {
            if let Some(uri) = tool["resource"].as_str() {
                hints.push(uri);
            }
            if let Some(template) = tool["resourceTemplate"].as_str() {
                hints.push(template);
            }
        }
        let hints = if hints.is_empty() {
            String::new()
        } else {
            format!(" [{}]", hints.join(", "))
        };
        lines.push(format!("{name}({}) — {ability}{hints}", args.join(", ")));
        if let Some(line) = tool["description"].as_str().and_then(|d| d.lines().next()) {
            if !line.is_empty() {
                lines.push(format!("    {line}"));
            }
        }
        if !assistant {
            if let Some(why) = tool["noTemplate"].as_str() {
                lines.push(format!("    no resource template: {why}"));
            }
        }
    }
    lines
}

/// `rata mcp install [--client claude|cursor|vscode|workspace] [--release]` —
/// build the app and print the config that has an MCP client launch it with
/// `--mcp`. It prints, and edits nothing: the client's config is the user's.
/// `workspace` is the exception: Elyra Workspace gives the servers in the
/// project's own `.mcp.json` to every agent, so that's where it goes.
fn mcp_install(cfg: &Config, args: &[String]) -> Result<(), String> {
    let client = args
        .iter()
        .position(|a| a == "--client")
        .and_then(|i| args.get(i + 1))
        .map_or("claude", String::as_str);
    if !matches!(client, "claude" | "cursor" | "vscode" | "workspace") {
        return Err(format!(
            "unknown client `{client}` — claude, cursor, vscode or workspace"
        ));
    }
    let release = args.iter().any(|a| a == "--release");
    let exe = build_executable(cfg, release)?;
    let exe = exe.to_string_lossy().into_owned();
    let name = cfg.app_crate.clone();

    let mut server = serde_json::json!({ "command": exe, "args": ["--mcp"] });
    // Headless, the app reads `DATABASE_URL` when it has no `App::database`.
    if let Some(url) = &cfg.database_url {
        server["env"] = serde_json::json!({ "DATABASE_URL": url });
    }
    let pretty = |v: &serde_json::Value| serde_json::to_string_pretty(v).unwrap_or_default();
    if client == "workspace" {
        return mcp_install_workspace(cfg, &name, &exe, release);
    }
    match client {
        "claude" => {
            println!("Claude Code:\n");
            let env = cfg
                .database_url
                .as_ref()
                .map(|url| format!(" -e DATABASE_URL={}", shell_quote(url)))
                .unwrap_or_default();
            println!(
                "  claude mcp add {name}{env} -- {} --mcp\n",
                shell_quote(&exe)
            );
            println!(
                "Claude Desktop — in claude_desktop_config.json (Settings → Developer → Edit Config):\n"
            );
            println!(
                "{}",
                pretty(&serde_json::json!({ "mcpServers": { &name: server } }))
            );
        }
        "cursor" => {
            println!("Cursor — in ~/.cursor/mcp.json, or .cursor/mcp.json in a project:\n");
            println!(
                "{}",
                pretty(&serde_json::json!({ "mcpServers": { &name: server } }))
            );
        }
        _ => {
            server["type"] = serde_json::json!("stdio");
            println!("VS Code — in .vscode/mcp.json:\n");
            println!(
                "{}",
                pretty(&serde_json::json!({ "servers": { &name: server } }))
            );
        }
    }
    println!(
        "\nWhile the app is open, the agent works in it; otherwise it runs headless.{}",
        if release {
            ""
        } else {
            "\nThat's the debug build; `--release` for the optimized one."
        }
    );
    if cfg.database_url.is_some() {
        println!("The database URL is in the config above: keep it out of anything you share.");
    }
    Ok(())
}

/// `rata mcp install --client workspace`: the app's server in the project's
/// `.mcp.json`, next to the servers already there. A database URL goes in
/// only for SQLite: a server URL may carry a password, and `.mcp.json` is
/// often committed.
fn mcp_install_workspace(cfg: &Config, name: &str, exe: &str, release: bool) -> Result<(), String> {
    let mut server = serde_json::json!({ "command": exe, "args": ["--mcp"] });
    let mut env_note = None;
    if let Some(url) = &cfg.database_url {
        if url.starts_with("sqlite:") {
            server["env"] = serde_json::json!({ "DATABASE_URL": url });
        } else {
            env_note = Some(
                "The database URL isn't written there (it may hold a password): when the app has no \
                 `App::database`, give the agent's environment DATABASE_URL.",
            );
        }
    }
    let path = cfg.root.join(".mcp.json");
    let changed = ai::merge_server(&path, name, server)?;
    println!(
        "{} `{name}` in {} — Elyra Workspace gives it to every agent in the project once \
         you allow the file (Context tab).",
        if changed { "Added" } else { "Already had" },
        path.display()
    );
    println!(
        "It runs {exe} --mcp: the path is this machine's{}.",
        if release {
            ""
        } else {
            ", and the debug build (`--release` for the optimized one)"
        }
    );
    if let Some(note) = env_note {
        println!("{note}");
    }
    Ok(())
}

/// Build the app's binary and return its path, as cargo reports it.
fn build_executable(cfg: &Config, release: bool) -> Result<PathBuf, String> {
    let mut cargo = Command::new("cargo");
    cargo.args([
        "build",
        "--quiet",
        "-p",
        &cfg.app_crate,
        "--message-format=json-render-diagnostics",
    ]);
    if release {
        cargo.arg("--release");
    }
    let output = cargo
        .current_dir(&cfg.root)
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    exit_ok(output.status, "cargo build")?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|m| {
            m["reason"] == "compiler-artifact" && m["target"]["name"] == cfg.app_crate.as_str()
        })
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| format!("cargo built no executable for `{}`", cfg.app_crate))
}

/// Quote `s` for a POSIX shell when it needs it.
fn shell_quote(s: &str) -> String {
    if s.chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:=@%+".contains(c))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tools() -> Vec<serde_json::Value> {
        vec![
            json!({ "name": "customers_index", "ability": "customers.view",
                "description": "A page of customers.\n\nMore.",
                "annotations": { "readOnlyHint": true, "destructiveHint": false },
                "inputSchema": { "properties": { "query": {} }, "required": ["query"] },
                "resource": "app://customers_index" }),
            json!({ "name": "customers_store", "ability": "customers.create",
                "description": "",
                "annotations": { "readOnlyHint": false, "destructiveHint": false },
                "inputSchema": { "properties": { "input": {}, "tag": {} }, "required": ["input"] } }),
            json!({ "name": "customers_destroy", "ability": "customers.delete",
                "description": "Delete the customer with this `id`.",
                "annotations": { "readOnlyHint": false, "destructiveHint": true },
                "inputSchema": { "properties": { "id": {} }, "required": ["id"] } }),
        ]
    }

    #[test]
    fn the_assistant_lists_what_asks_first() {
        assert_eq!(
            tool_lines(&tools(), true),
            [
                "customers_index(query) — customers.view [read-only]",
                "    A page of customers.",
                "customers_store(input, tag?) — customers.create [runs without asking]",
                "customers_destroy(id) — customers.delete [asks first]",
                "    Delete the customer with this `id`.",
            ]
        );
        assert_eq!(confirm_counts(&tools()), (1, 1));
    }

    #[test]
    fn mcp_lists_its_hints() {
        let lines = tool_lines(&tools(), false);
        assert_eq!(
            lines[0],
            "customers_index(query) — customers.view [read-only, app://customers_index]"
        );
        assert_eq!(lines[2], "customers_store(input, tag?) — customers.create");
        assert_eq!(
            lines[3],
            "customers_destroy(id) — customers.delete [confirm]"
        );
    }

    #[test]
    fn no_assistant_is_null() {
        assert_eq!(assistant_tools("null").unwrap(), None);
        assert_eq!(assistant_tools("[]").unwrap(), Some(vec![]));
        assert!(assistant_tools("{}").is_err());
    }
}
