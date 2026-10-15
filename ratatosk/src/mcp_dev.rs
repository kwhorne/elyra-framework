//! `rata mcp dev` — an MCP server for *developing* the app, Laravel Boost's
//! counterpart for the Rust side. Where `myapp --mcp` gives an agent the
//! app's own commands, this tells an agent working on the code what the app
//! is: its commands and abilities, models, schema, migrations, the latest
//! errors in its log, rata's commands, and the framework's guides.
//!
//! Every tool reads: nothing is written, migrated or run. A SQLite database
//! is opened read-only. The transport is stdio, one JSON-RPC message per
//! line; it answers `initialize`-era clients and 2026-07-28 ones (a request
//! carrying its version in `_meta`, and `server/discover`).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::config::Config;

include!(concat!(env!("OUT_DIR"), "/docs.rs"));

const MODERN: &str = "2026-07-28";
const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26"];
const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

/// How many log entries `read_log_entries` returns at most.
const MAX_ENTRIES: usize = 200;

/// `rata mcp dev`: serve on stdin/stdout until the client closes it.
pub fn serve(cfg: &Config) -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(cfg, &line) {
            writeln!(stdout, "{reply}").map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// One message in, maybe one out.
fn handle_line(cfg: &Config, line: &str) -> Option<String> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(e) => return Some(error(Value::Null, -32700, &format!("not JSON: {e}")).to_string()),
    };
    // Notifications (no id) and answers to requests we never send.
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str)?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let modern = params
        .pointer("/_meta/io.modelcontextprotocol~1protocolVersion")
        .and_then(Value::as_str);
    if let Some(version) = modern {
        if version != MODERN {
            let mut supported = vec![MODERN];
            supported.extend(LEGACY);
            let mut reply = error(id, -32022, "Unsupported protocol version");
            reply["error"]["data"] = json!({ "supported": supported, "requested": version });
            return Some(reply.to_string());
        }
    }
    let reply = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let version = asked.filter(|v| LEGACY.contains(v)).unwrap_or(LEGACY[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": server_info(),
                    "instructions": "Tools for developing this Elyra Framework app. Use search_docs before writing framework code you aren't sure of, app_info for its commands and models, database_schema before a migration, last_error when something fails.",
                }),
                false,
            )
        }
        "server/discover" => {
            let mut versions = vec![MODERN];
            versions.extend(LEGACY);
            result(
                id,
                json!({
                    "supportedVersions": versions,
                    "capabilities": { "tools": { "listChanged": false } },
                }),
                true,
            )
        }
        "ping" => result(id, json!({}), modern.is_some()),
        "tools/list" => result(id, json!({ "tools": tools() }), modern.is_some()),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            if !tools().iter().any(|t| t["name"] == name) {
                error(id, -32602, &format!("unknown tool `{name}`"))
            } else {
                let body = match call(cfg, name, &args) {
                    Ok(text) => json!({ "content": [{ "type": "text", "text": text }] }),
                    Err(text) => {
                        json!({ "content": [{ "type": "text", "text": text }], "isError": true })
                    }
                };
                result(id, body, modern.is_some())
            }
        }
        other => error(id, -32601, &format!("unknown method `{other}`")),
    };
    Some(reply.to_string())
}

fn server_info() -> Value {
    json!({ "name": "elyra-framework", "version": env!("CARGO_PKG_VERSION") })
}

fn result(id: Value, mut body: Value, modern: bool) -> Value {
    if let Some(map) = body.as_object_mut() {
        map.entry("resultType").or_insert_with(|| json!("complete"));
        if modern {
            map.insert("_meta".into(), json!({ META_SERVER: server_info() }));
        }
    }
    json!({ "jsonrpc": "2.0", "id": id, "result": body })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The tools, as `tools/list` gives them.
fn tools() -> Vec<Value> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": { "type": "object", "properties": properties, "required": required },
            "annotations": { "readOnlyHint": true },
        })
    };
    vec![
        tool(
            "app_info",
            "What this Elyra app is: its crate and version, the framework's version and the elyra features it enables, the frontend and bindings paths, the database driver, every #[command] with its options (live, the `can` ability, middleware) and file, every #[derive(Model)] with its table, and the resources. Read it before adding a command or a model.",
            json!({}),
            &[],
        ),
        tool(
            "database_schema",
            "The app's database (from elyra.toml or DATABASE_URL): its tables, and each column's type, whether it may be null, its default and whether it's the key. Read-only: SQLite is opened read-only. Check it before writing a migration or a model.",
            json!({ "table": { "type": "string", "description": "Only this table" } }),
            &[],
        ),
        tool(
            "migration_status",
            "Which migrations are applied (with their batch) and which are pending: the SQL files in the migrations directory and the Rust migrations under src/, against the `_elyra_migrations` table. Read-only, unlike `rata migrate:status` it never creates that table.",
            json!({}),
            &[],
        ),
        tool(
            "last_error",
            "The most recent ERROR entry in the app's log file (elyra::log), with the entries just before it. The log is found from the app's LogProvider (`to_app_dir` / `to_path`) or given as `path`. Use it when something failed while the app ran.",
            json!({ "path": { "type": "string", "description": "The log file, when it can't be found" } }),
            &[],
        ),
        tool(
            "read_log_entries",
            "The last entries of the app's log file (elyra::log), oldest first, optionally only from a level up.",
            json!({
                "count": { "type": "integer", "description": "How many (default 20, at most 200)" },
                "level": { "type": "string", "enum": ["error", "warn", "info", "debug", "trace"], "description": "Only this level and more severe" },
                "path": { "type": "string", "description": "The log file, when it can't be found" }
            }),
            &[],
        ),
        tool(
            "list_commands",
            "rata's own CLI commands (make:resource, make:migration, migrate, codegen, …) with their flags, to pick the generator instead of writing the code by hand.",
            json!({}),
            &[],
        ),
        tool(
            "search_docs",
            "Search the Elyra Framework guides for this version (commands, models, migrations, live queries, resources, the Laravel backend, MCP, testing, …), embedded in rata: the best-matching sections with their file and heading. Use it before writing framework code you aren't sure of; the API isn't Tauri's or Laravel's.",
            json!({
                "query": { "type": "string", "description": "Words to look for, e.g. \"live query depends_on\"" },
                "limit": { "type": "integer", "description": "How many sections (default 5)" }
            }),
            &["query"],
        ),
    ]
}

fn call(cfg: &Config, name: &str, args: &Value) -> Result<String, String> {
    match name {
        "app_info" => Ok(app_info(cfg)),
        "database_schema" => database_schema(cfg, args["table"].as_str()),
        "migration_status" => migration_status(cfg),
        "last_error" => last_error(&log_file(cfg, args["path"].as_str())?),
        "read_log_entries" => {
            let count = args["count"]
                .as_u64()
                .map_or(20, |c| c as usize)
                .clamp(1, MAX_ENTRIES);
            read_log_entries(
                &log_file(cfg, args["path"].as_str())?,
                count,
                args["level"].as_str(),
            )
        }
        "list_commands" => Ok(crate::HELP.to_string()),
        "search_docs" => {
            let query = args["query"].as_str().unwrap_or("").trim();
            if query.is_empty() {
                return Err("give a `query`".into());
            }
            let limit = args["limit"]
                .as_u64()
                .map_or(5, |l| l as usize)
                .clamp(1, 20);
            Ok(search_docs(DOCS, query, limit))
        }
        _ => Err(format!("unknown tool `{name}`")),
    }
}

// ---------------------------------------------------------------------------
// app_info
// ---------------------------------------------------------------------------

/// The app crate's folder: the root when its `Cargo.toml` is the app's,
/// else the workspace member that is.
fn app_dir(cfg: &Config) -> PathBuf {
    let is_app = |dir: &Path| {
        std::fs::read_to_string(dir.join("Cargo.toml"))
            .ok()
            .and_then(|text| text.parse::<toml::Table>().ok())
            .and_then(|doc| {
                doc.get("package")?
                    .get("name")?
                    .as_str()
                    .map(|n| n == cfg.app_crate)
            })
            .unwrap_or(false)
    };
    if is_app(&cfg.root) {
        return cfg.root.clone();
    }
    let members: Vec<String> = std::fs::read_to_string(cfg.root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|doc| {
            let members = doc.get("workspace")?.get("members")?.as_array()?.clone();
            Some(
                members
                    .iter()
                    .filter_map(|m| m.as_str().map(str::to_owned))
                    .collect(),
            )
        })
        .unwrap_or_default();
    for member in members {
        let dirs: Vec<PathBuf> = match member.strip_suffix("/*") {
            Some(parent) => std::fs::read_dir(cfg.root.join(parent))
                .map(|e| e.flatten().map(|e| e.path()).collect())
                .unwrap_or_default(),
            None => vec![cfg.root.join(&member)],
        };
        if let Some(dir) = dirs.into_iter().find(|d| is_app(d)) {
            return dir;
        }
    }
    cfg.root.clone()
}

/// A `#[command]` found in the source.
#[derive(Debug, PartialEq)]
struct FoundCommand {
    name: String,
    live: bool,
    can: Option<String>,
    middleware: Option<String>,
    file: String,
}

/// A `#[derive(Model)]` found in the source.
#[derive(Debug, PartialEq)]
struct FoundModel {
    name: String,
    table: String,
    file: String,
}

fn app_info(cfg: &Config) -> String {
    let dir = app_dir(cfg);
    // Forward slashes on every platform: they're for an agent to read.
    let rel = |p: &Path| {
        p.strip_prefix(&cfg.root)
            .unwrap_or(p)
            .display()
            .to_string()
            .replace('\\', "/")
    };
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok());
    // `version.workspace = true` and `elyra = { workspace = true, … }` come
    // from the root manifest.
    let workspace = std::fs::read_to_string(cfg.root.join("Cargo.toml"))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|doc| doc.get("workspace").cloned());
    let inherits = |v: &toml::Value| v.get("workspace").and_then(|w| w.as_bool()) == Some(true);
    let version = manifest
        .as_ref()
        .and_then(|m| m.get("package")?.get("version").cloned())
        .and_then(|v| {
            if inherits(&v) {
                workspace.as_ref()?.get("package")?.get("version").cloned()
            } else {
                Some(v)
            }
        })
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "?".into());
    let elyra = manifest
        .as_ref()
        .and_then(|m| m.get("dependencies")?.get("elyra").cloned());
    let elyra_source = elyra.as_ref().map(|dep| {
        match workspace
            .as_ref()
            .and_then(|w| w.get("dependencies")?.get("elyra"))
        {
            Some(shared) if inherits(dep) => dependency_source(shared),
            _ => dependency_source(dep),
        }
    });
    let features: Vec<String> = elyra
        .as_ref()
        .and_then(|e| e.get("features")?.as_array().cloned())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    let mut out = format!(
        "App: {} {version} (crate in {})\n",
        cfg.app_crate,
        if dir == cfg.root {
            ".".into()
        } else {
            rel(&dir)
        }
    );
    out.push_str(&format!(
        "Framework: elyra {} (rata {}), features: {}\n",
        elyra_source.unwrap_or_else(|| "not a dependency".into()),
        env!("CARGO_PKG_VERSION"),
        if features.is_empty() {
            "none".into()
        } else {
            features.join(", ")
        }
    ));
    out.push_str(&format!(
        "Frontend: {} (bindings: {})\n",
        cfg.frontend_dir, cfg.codegen_out
    ));
    out.push_str(&format!(
        "Database: {}; migrations in {}\n",
        cfg.database_url
            .as_deref()
            .map(describe_url)
            .unwrap_or_else(
                || "none configured (elyra.toml [database].url or DATABASE_URL)".into()
            ),
        cfg.migrations_dir
    ));

    let src = dir.join("src");
    let (commands, models) = scan(&src);
    out.push_str(&format!("\nCommands ({}):\n", commands.len()));
    for c in &commands {
        let mut options = Vec::new();
        if c.live {
            options.push("live".to_string());
        }
        if let Some(can) = &c.can {
            options.push(format!("can \"{can}\""));
        }
        if let Some(middleware) = &c.middleware {
            options.push(format!("middleware {middleware}"));
        }
        out.push_str(&format!(
            "- {}{} — {}\n",
            c.name,
            if options.is_empty() {
                String::new()
            } else {
                format!(" ({})", options.join(", "))
            },
            rel(&src.join(&c.file))
        ));
    }
    out.push_str(&format!("\nModels ({}):\n", models.len()));
    for m in &models {
        out.push_str(&format!(
            "- {} — table `{}` — {}\n",
            m.name,
            m.table,
            rel(&src.join(&m.file))
        ));
    }
    let mut resources: Vec<String> = std::fs::read_dir(src.join("resources"))
        .map(|e| {
            e.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    resources.sort();
    if !resources.is_empty() {
        out.push_str(&format!(
            "\nResources (rata make:resource): {}\n",
            resources.join(", ")
        ));
    }
    out
}

/// Where the `elyra` dependency comes from, in a few words.
fn dependency_source(dep: &toml::Value) -> String {
    match dep {
        toml::Value::String(version) => version.clone(),
        toml::Value::Table(t) => {
            if let Some(v) = t.get("version").and_then(|v| v.as_str()) {
                v.to_string()
            } else if let Some(p) = t.get("path").and_then(|v| v.as_str()) {
                format!("from {p}")
            } else if let Some(g) = t.get("git").and_then(|v| v.as_str()) {
                let at = ["tag", "rev", "branch"]
                    .iter()
                    .find_map(|k| t.get(*k).and_then(|v| v.as_str()))
                    .map(|r| format!(" at {r}"))
                    .unwrap_or_default();
                format!("from {g}{at}")
            } else if t.get("workspace").and_then(|w| w.as_bool()) == Some(true) {
                "(the workspace's)".into()
            } else {
                "?".into()
            }
        }
        _ => "?".into(),
    }
}

/// A database URL without its credentials: the driver, and for SQLite the
/// file.
fn describe_url(url: &str) -> String {
    match elyra_db::Driver::from_url(url) {
        Some(elyra_db::Driver::Sqlite) => format!("SQLite ({})", sqlite_file(url)),
        Some(elyra_db::Driver::MySql) => "MySQL".into(),
        Some(elyra_db::Driver::Postgres) => "PostgreSQL".into(),
        None => "an unknown driver".into(),
    }
}

/// The file of a `sqlite:` URL.
fn sqlite_file(url: &str) -> String {
    let rest = url.trim_start_matches("sqlite:");
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    rest.split('?').next().unwrap_or(rest).to_string()
}

/// The commands and models under `src`, with their files relative to it.
fn scan(src: &Path) -> (Vec<FoundCommand>, Vec<FoundModel>) {
    let mut files = Vec::new();
    crate::make_resource::rust_files(src, &mut files);
    let mut commands = Vec::new();
    let mut models = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if !text.contains("command") && !text.contains("Model") {
            continue;
        }
        let Ok(parsed) = syn::parse_file(&text) else {
            continue;
        };
        let name = file
            .strip_prefix(src)
            .unwrap_or(&file)
            .display()
            .to_string();
        scan_items(&parsed.items, &name, &mut commands, &mut models);
    }
    commands.sort_by(|a, b| a.name.cmp(&b.name));
    models.sort_by(|a, b| a.name.cmp(&b.name));
    (commands, models)
}

fn scan_items(
    items: &[syn::Item],
    file: &str,
    commands: &mut Vec<FoundCommand>,
    models: &mut Vec<FoundModel>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f) => {
                let Some(attr) = f.attrs.iter().find(|a| {
                    a.path()
                        .segments
                        .last()
                        .is_some_and(|s| s.ident == "command")
                }) else {
                    continue;
                };
                let mut found = FoundCommand {
                    name: f.sig.ident.to_string(),
                    live: false,
                    can: None,
                    middleware: None,
                    file: file.to_string(),
                };
                if let syn::Meta::List(_) = attr.meta {
                    let _ = attr.parse_nested_meta(|meta| {
                        if meta.path.is_ident("live") {
                            found.live = true;
                        } else if meta.path.is_ident("can") {
                            let lit: syn::LitStr = meta.value()?.parse()?;
                            found.can = Some(lit.value());
                        } else if meta.path.is_ident("middleware") {
                            let value: syn::Expr = meta.value()?.parse()?;
                            found.middleware = Some(
                                quote::ToTokens::to_token_stream(&value)
                                    .to_string()
                                    .replace(" ,", ","),
                            );
                        } else if meta.input.peek(syn::Token![=]) {
                            let _: syn::Expr = meta.value()?.parse()?;
                        }
                        Ok(())
                    });
                }
                commands.push(found);
            }
            syn::Item::Struct(s)
                if crate::make_resource::derives(&s.attrs)
                    .iter()
                    .any(|d| d == "Model") =>
            {
                let mut table = s.ident.to_string().to_lowercase();
                for attr in s.attrs.iter().filter(|a| a.path().is_ident("model")) {
                    let _ = attr.parse_nested_meta(|meta| {
                        if meta.path.is_ident("table") {
                            let lit: syn::LitStr = meta.value()?.parse()?;
                            table = lit.value();
                        } else if meta.input.peek(syn::Token![=]) {
                            let _: syn::Expr = meta.value()?.parse()?;
                        } else if meta.input.peek(syn::token::Paren) {
                            let _ = meta.parse_nested_meta(|inner| {
                                if inner.input.peek(syn::Token![=]) {
                                    let _: syn::Expr = inner.value()?.parse()?;
                                }
                                Ok(())
                            });
                        }
                        Ok(())
                    });
                }
                models.push(FoundModel {
                    name: s.ident.to_string(),
                    table,
                    file: file.to_string(),
                });
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    scan_items(items, file, commands, models);
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// The database
// ---------------------------------------------------------------------------

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {e}"))
}

/// The configured database URL, made read-only for SQLite. A SQLite file
/// that isn't there isn't created.
fn read_only_url(cfg: &Config) -> Result<String, String> {
    let url = cfg
        .database_url
        .clone()
        .ok_or("no database configured — `[database].url` in elyra.toml, or DATABASE_URL")?;
    if elyra_db::Driver::from_url(&url) != Some(elyra_db::Driver::Sqlite) {
        return Ok(url);
    }
    let (base, query) = url.split_once('?').unwrap_or((&url, ""));
    let mut base = base.to_string();
    let file = sqlite_file(&url);
    if !file.contains(":memory:") {
        let path = if Path::new(&file).is_absolute() {
            PathBuf::from(&file)
        } else {
            cfg.root.join(&file)
        };
        if !path.is_file() {
            return Err(format!(
                "the database {file} doesn't exist yet — `rata migrate` creates it"
            ));
        }
        // A relative file is the project's, not the one next to wherever
        // `rata mcp dev` was started.
        base = elyra_db::sqlite_url_opts(&path, false);
    }
    let mut params: Vec<&str> = query
        .split('&')
        .filter(|p| !p.is_empty() && !p.starts_with("mode="))
        .collect();
    params.push("mode=ro");
    Ok(format!("{base}?{}", params.join("&")))
}

/// A column, as `database_schema` lists it.
#[derive(Debug, PartialEq)]
struct Column {
    table: String,
    name: String,
    kind: String,
    nullable: bool,
    default: Option<String>,
    key: bool,
}

fn database_schema(cfg: &Config, only: Option<&str>) -> Result<String, String> {
    let url = read_only_url(cfg)?;
    let columns = runtime()?.block_on(async {
        let db = elyra_db::Database::connect(&url)
            .await
            .map_err(|e| format!("connect: {e}"))?;
        columns(&db)
            .await
            .map_err(|e| format!("reading the schema: {e}"))
    })?;
    Ok(format_schema(&columns, only, &describe_url(&url)))
}

async fn columns(db: &elyra_db::Database) -> Result<Vec<Column>, elyra_db::sqlx::Error> {
    use elyra_db::sqlx::{self, Row};
    let mut out = Vec::new();
    match db.driver() {
        elyra_db::Driver::Sqlite => {
            let tables = sqlx::query(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .fetch_all(db.pool())
            .await?;
            for table in tables {
                let table: String = table.try_get(0)?;
                let rows = sqlx::query(
                    "SELECT name, type, \"notnull\", dflt_value, pk FROM pragma_table_info(?) ORDER BY cid",
                )
                .bind(table.clone())
                .fetch_all(db.pool())
                .await?;
                for row in rows {
                    out.push(Column {
                        table: table.clone(),
                        name: row.try_get(0)?,
                        kind: row.try_get(1)?,
                        nullable: row.try_get::<i64, _>(2)? == 0,
                        default: row.try_get(3)?,
                        key: row.try_get::<i64, _>(4)? > 0,
                    });
                }
            }
        }
        driver => {
            let sql = if driver == elyra_db::Driver::MySql {
                "SELECT CAST(table_name AS CHAR), CAST(column_name AS CHAR), CAST(column_type AS CHAR), \
                 CAST(is_nullable AS CHAR), CAST(column_default AS CHAR), CAST(column_key AS CHAR) \
                 FROM information_schema.columns WHERE table_schema = DATABASE() \
                 ORDER BY table_name, ordinal_position"
            } else {
                "SELECT c.table_name::text, c.column_name::text, c.data_type::text, c.is_nullable::text, \
                 c.column_default::text, \
                 CASE WHEN EXISTS (SELECT 1 FROM information_schema.key_column_usage k \
                   JOIN information_schema.table_constraints t ON t.constraint_name = k.constraint_name \
                   AND t.table_schema = k.table_schema \
                   WHERE t.constraint_type = 'PRIMARY KEY' AND k.table_schema = c.table_schema \
                   AND k.table_name = c.table_name AND k.column_name = c.column_name) \
                 THEN 'PRI' ELSE '' END \
                 FROM information_schema.columns c WHERE c.table_schema = current_schema() \
                 ORDER BY c.table_name, c.ordinal_position"
            };
            for row in sqlx::query(sql).fetch_all(db.pool()).await? {
                out.push(Column {
                    table: row.try_get(0)?,
                    name: row.try_get(1)?,
                    kind: row.try_get(2)?,
                    nullable: row.try_get::<String, _>(3)? == "YES",
                    default: row.try_get(4)?,
                    key: row.try_get::<Option<String>, _>(5)?.as_deref() == Some("PRI"),
                });
            }
        }
    }
    Ok(out)
}

fn format_schema(columns: &[Column], only: Option<&str>, database: &str) -> String {
    let mut out = format!("Database: {database}\n");
    let mut tables: Vec<&str> = columns.iter().map(|c| c.table.as_str()).collect();
    tables.dedup();
    let tables: Vec<&str> = tables
        .into_iter()
        .filter(|t| only.is_none_or(|o| o.eq_ignore_ascii_case(t)))
        .collect();
    if tables.is_empty() {
        out.push_str(&match only {
            Some(table) => format!("No table `{table}`.\n"),
            None => "No tables yet.\n".into(),
        });
        return out;
    }
    for table in tables {
        out.push_str(&format!("\n{table}\n"));
        for c in columns.iter().filter(|c| c.table == table) {
            let mut notes = Vec::new();
            if c.key {
                notes.push("key".to_string());
            }
            notes.push(if c.nullable { "null" } else { "not null" }.to_string());
            if let Some(default) = &c.default {
                notes.push(format!("default {default}"));
            }
            out.push_str(&format!(
                "  {} {} ({})\n",
                c.name,
                if c.kind.is_empty() { "?" } else { &c.kind },
                notes.join(", ")
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Migrations
// ---------------------------------------------------------------------------

/// A migration the project has: (version, name, kind).
fn known_migrations(cfg: &Config) -> Vec<(String, String, &'static str)> {
    let mut known = Vec::new();
    if let Ok(entries) = std::fs::read_dir(cfg.root.join(&cfg.migrations_dir)) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(stem) = file.strip_suffix(".sql") else {
                continue;
            };
            if stem.ends_with(".down") {
                continue;
            }
            if let Some((version, name)) = stem.split_once('_') {
                known.push((version.to_string(), name.to_string(), "sql"));
            }
        }
    }
    let mut files = Vec::new();
    crate::make_resource::rust_files(&app_dir(cfg).join("src"), &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if !text.contains("RustMigration for") {
            continue;
        }
        let Ok(parsed) = syn::parse_file(&text) else {
            continue;
        };
        known.extend(rust_migrations(&parsed.items));
    }
    known.sort();
    known
}

/// `impl RustMigration for X` blocks whose `version` and `name` return a
/// string literal.
fn rust_migrations(items: &[syn::Item]) -> Vec<(String, String, &'static str)> {
    let mut out = Vec::new();
    for item in items {
        match item {
            syn::Item::Impl(imp)
                if imp.trait_.as_ref().is_some_and(|(_, path, _)| {
                    path.segments
                        .last()
                        .is_some_and(|s| s.ident == "RustMigration")
                }) =>
            {
                let literal = |name: &str| {
                    imp.items.iter().find_map(|i| match i {
                        syn::ImplItem::Fn(f) if f.sig.ident == name => {
                            f.block.stmts.iter().find_map(|s| match s {
                                syn::Stmt::Expr(
                                    syn::Expr::Lit(syn::ExprLit {
                                        lit: syn::Lit::Str(s),
                                        ..
                                    }),
                                    None,
                                ) => Some(s.value()),
                                _ => None,
                            })
                        }
                        _ => None,
                    })
                };
                if let (Some(version), Some(name)) = (literal("version"), literal("name")) {
                    out.push((version, name, "rust"));
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    out.extend(rust_migrations(items));
                }
            }
            _ => {}
        }
    }
    out
}

fn migration_status(cfg: &Config) -> Result<String, String> {
    let known = known_migrations(cfg);
    let applied: Vec<(String, String, i64)> = match read_only_url(cfg) {
        Ok(url) => runtime()?.block_on(async {
            use elyra_db::sqlx::{self, Row};
            let db = elyra_db::Database::connect(&url)
                .await
                .map_err(|e| format!("connect: {e}"))?;
            // No table yet: nothing applied. It isn't created here.
            let rows = sqlx::query("SELECT version, name, batch FROM _elyra_migrations")
                .fetch_all(db.pool())
                .await
                .unwrap_or_default();
            rows.iter()
                .map(|r| {
                    Ok((
                        r.try_get::<String, _>(0)?,
                        r.try_get::<String, _>(1)?,
                        r.try_get::<i64, _>(2)?,
                    ))
                })
                .collect::<Result<Vec<_>, sqlx::Error>>()
                .map_err(|e| e.to_string())
        })?,
        Err(e) if e.contains("doesn't exist yet") => Vec::new(),
        Err(e) => return Err(e),
    };
    Ok(format_migrations(&known, &applied))
}

fn format_migrations(
    known: &[(String, String, &str)],
    applied: &[(String, String, i64)],
) -> String {
    if known.is_empty() && applied.is_empty() {
        return "No migrations.".into();
    }
    let mut lines = Vec::new();
    let mut pending = 0;
    for (version, name, kind) in known {
        match applied.iter().find(|(v, _, _)| v == version) {
            Some((_, _, batch)) => lines.push(format!(
                "applied (batch {batch})  {version}_{name} [{kind}]"
            )),
            None => {
                pending += 1;
                lines.push(format!("pending            {version}_{name} [{kind}]"));
            }
        }
    }
    for (version, name, batch) in applied {
        if !known.iter().any(|(v, _, _)| v == version) {
            lines.push(format!(
                "applied (batch {batch})  {version}_{name} [not in the project any more]"
            ));
        }
    }
    format!(
        "{}\n\n{pending} pending — `rata migrate` applies them.",
        lines.join("\n")
    )
}

// ---------------------------------------------------------------------------
// The log
// ---------------------------------------------------------------------------

/// The app's log file: `path`, else where its `LogProvider` writes
/// (`to_path("…")`, `to_app_dir("…")`), else `<config dir>/<app>/app.log`.
fn log_file(cfg: &Config, path: Option<&str>) -> Result<PathBuf, String> {
    if let Some(path) = path {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() {
            path
        } else {
            cfg.root.join(path)
        };
        return if path.is_file() {
            Ok(path)
        } else {
            Err(format!("{} doesn't exist", path.display()))
        };
    }
    let mut candidates = Vec::new();
    let mut files = Vec::new();
    crate::make_resource::rust_files(&app_dir(cfg).join("src"), &mut files);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (call, in_app_dir) in [(".to_path(", false), (".to_app_dir(", true)] {
            for (at, _) in text.match_indices(call) {
                let rest = text[at + call.len()..].trim_start();
                let Some(literal) = rest
                    .strip_prefix('"')
                    .and_then(|r| r.split_once('"'))
                    .map(|(s, _)| s)
                else {
                    continue;
                };
                if in_app_dir {
                    if let Some(dir) = app_config_dir(literal) {
                        candidates.push(dir.join("app.log"));
                    }
                } else {
                    let p = PathBuf::from(literal);
                    candidates.push(if p.is_absolute() { p } else { cfg.root.join(p) });
                }
            }
        }
    }
    for name in [&cfg.bundle_name, &cfg.app_crate] {
        if let Some(dir) = app_config_dir(name) {
            candidates.push(dir.join("app.log"));
        }
    }
    candidates.dedup();
    candidates
        .iter()
        .find(|p| p.is_file())
        .cloned()
        .ok_or_else(|| {
            format!(
                "no log file found (looked for {}). The app writes one with \
                 `LogProvider::new().to_app_dir(\"My App\")`; pass `path` if it's elsewhere.",
                candidates
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
}

/// `<config dir>/<slug>`, as the framework's `LogProvider::to_app_dir`
/// names it.
fn app_config_dir(app: &str) -> Option<PathBuf> {
    let slug: String = app
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-');
    let name = if slug.is_empty() { "elyra-app" } else { slug };
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library").join("Application Support"));
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(all(unix, not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    Some(base?.join(name))
}

/// The log's entries, oldest first: the current file after its rotated
/// predecessor. A line that doesn't start with a timestamp belongs to the
/// entry before it (a multi-line message).
fn entries(path: &Path) -> Vec<String> {
    let rotated = PathBuf::from(format!("{}.1", path.display()));
    let mut text = std::fs::read_to_string(&rotated).unwrap_or_default();
    text.push_str(&std::fs::read_to_string(path).unwrap_or_default());
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let starts_entry = line.len() > 20
            && line.as_bytes()[..4].iter().all(u8::is_ascii_digit)
            && line.as_bytes()[10] == b'T';
        match out.last_mut() {
            Some(last) if !starts_entry => {
                last.push('\n');
                last.push_str(line);
            }
            _ => out.push(line.to_string()),
        }
    }
    out
}

/// An entry's level, from `YYYY-MM-DDTHH:MM:SS.mmmZ LEVEL [target] …`.
fn level_of(entry: &str) -> Option<&str> {
    entry.split_whitespace().nth(1)
}

fn severity(level: &str) -> u8 {
    match level.to_ascii_uppercase().as_str() {
        "ERROR" => 0,
        "WARN" => 1,
        "INFO" => 2,
        "DEBUG" => 3,
        _ => 4,
    }
}

fn last_error(path: &Path) -> Result<String, String> {
    let all = entries(path);
    let Some(at) = all.iter().rposition(|e| level_of(e) == Some("ERROR")) else {
        return Ok(format!(
            "No ERROR in {} ({} entries).",
            path.display(),
            all.len()
        ));
    };
    let from = at.saturating_sub(5);
    Ok(format!(
        "{}\n\nThe last error:\n{}\n\nJust before it:\n{}",
        path.display(),
        all[at],
        all[from..at].join("\n")
    ))
}

fn read_log_entries(path: &Path, count: usize, level: Option<&str>) -> Result<String, String> {
    let limit = level.map(severity).unwrap_or(u8::MAX);
    let all: Vec<String> = entries(path)
        .into_iter()
        .filter(|e| level_of(e).is_some_and(|l| severity(l) <= limit))
        .collect();
    let from = all.len().saturating_sub(count);
    Ok(format!(
        "{} — {} of {} entries:\n{}",
        path.display(),
        all.len() - from,
        all.len(),
        all[from..].join("\n")
    ))
}

// ---------------------------------------------------------------------------
// The guides
// ---------------------------------------------------------------------------

/// A section of a guide: its file, heading and text.
struct Section<'a> {
    file: &'a str,
    heading: String,
    text: String,
}

/// The guides split at their headings (not at `#` lines in code blocks).
fn sections<'a>(docs: &[(&'a str, &str)]) -> Vec<Section<'a>> {
    let mut out = Vec::new();
    for (file, text) in docs {
        let mut fenced = false;
        let mut title = String::new();
        let mut current = Section {
            file,
            heading: String::new(),
            text: String::new(),
        };
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                fenced = !fenced;
            }
            if !fenced && line.starts_with('#') {
                let heading = line.trim_start_matches('#').trim().to_string();
                if !current.text.trim().is_empty() {
                    out.push(current);
                }
                if line.starts_with("# ") {
                    title = heading.clone();
                }
                current = Section {
                    file,
                    heading: if line.starts_with("# ") || title.is_empty() {
                        heading
                    } else {
                        format!("{title} › {heading}")
                    },
                    text: String::new(),
                };
                continue;
            }
            current.text.push_str(line);
            current.text.push('\n');
        }
        if !current.text.trim().is_empty() {
            out.push(current);
        }
    }
    out
}

/// The best sections for `query`: each word counts, a word in the heading
/// counts more, and sections with every word come first.
fn search_docs(docs: &[(&str, &str)], query: &str, limit: usize) -> String {
    if docs.is_empty() {
        return "This rata was built without the guides; read them at https://elyracode.com/framework."
            .into();
    }
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric() && c != '_' && c != ':')
        .map(str::to_lowercase)
        .filter(|w| w.len() > 1)
        .collect();
    if words.is_empty() {
        return "Give words to look for.".into();
    }
    let mut scored: Vec<(usize, usize, Section)> = sections(docs)
        .into_iter()
        .filter_map(|s| {
            let heading = s.heading.to_lowercase();
            let text = s.text.to_lowercase();
            let mut matched = 0;
            let mut score = 0;
            for word in &words {
                let in_heading = heading.matches(word.as_str()).count();
                let in_text = text.matches(word.as_str()).count();
                if in_heading + in_text > 0 {
                    matched += 1;
                }
                score += in_heading * 10 + in_text;
            }
            (matched > 0).then_some((matched, score, s))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    if scored.is_empty() {
        return format!("Nothing in the guides matches “{query}”.");
    }
    scored
        .into_iter()
        .take(limit)
        .map(|(_, _, s)| {
            let text = s.text.trim();
            let text = if text.chars().count() > 2500 {
                format!("{}…", text.chars().take(2500).collect::<String>())
            } else {
                text.to_string()
            };
            format!("## docs/{} — {}\n\n{text}", s.file, s.heading)
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rata-mcp-dev-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config(root: &Path, database_url: Option<String>) -> Config {
        Config {
            root: root.to_path_buf(),
            app_crate: "crm".into(),
            frontend_dir: "app".into(),
            codegen_out: "app/src/bindings.ts".into(),
            bundle_identifier: "com.example.crm".into(),
            bundle_name: "CRM".into(),
            bundle_version: "0.1.0".into(),
            bundle_icon: None,
            bundle_deep_link: None,
            bundle_description: None,
            bundle_maintainer: None,
            database_url,
            migrations_dir: "migrations".into(),
        }
    }

    fn app(root: &Path) {
        std::fs::create_dir_all(root.join("src/resources/customer")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"crm\"\nversion = \"0.3.0\"\n\n[dependencies]\nelyra = { git = \"https://github.com/kwhorne/elyra-framework\", tag = \"v0.15.0\", features = [\"database\", \"backend\"] }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/main.rs"),
            "#[command]\nasync fn greet(_ctx: Ctx, name: String) -> String { name }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/resources/customer/commands.rs"),
            "#[command(live, can = \"customers.view\")]\nasync fn customers_index(ctx: Ctx) -> Page<Customer> { todo!() }\n\
             #[elyra::command(can = \"customers.delete\", middleware = [\"audit\"])]\nasync fn customers_destroy(ctx: Ctx, id: i64) {}\n\
             fn helper() {}\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/resources/customer/model.rs"),
            "#[derive(Model, Default)]\n#[model(table = \"customers\", timestamps, belongs_to(Team, fk = \"team_id\"))]\npub struct Customer { id: i64 }\n\
             #[derive(Model)]\nstruct Tag { id: i64 }\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/resources/customer/migration.rs"),
            "impl RustMigration for CreateCustomersTable {\n fn version(&self) -> &str { \"1760000000\" }\n fn name(&self) -> &str { \"create_customers_table\" }\n}\n",
        )
        .unwrap();
    }

    #[test]
    fn app_info_lists_commands_models_and_features() {
        let root = temp("info");
        app(&root);
        let text = app_info(&config(&root, Some("postgres://u:secret@db/crm".into())));
        assert!(text.starts_with("App: crm 0.3.0 (crate in .)\n"), "{text}");
        assert!(text.contains("elyra from https://github.com/kwhorne/elyra-framework at v0.15.0"));
        assert!(text.contains("features: database, backend"));
        assert!(
            text.contains("Database: PostgreSQL;"),
            "no credentials: {text}"
        );
        assert!(!text.contains("secret"));
        assert!(text.contains(
            "- customers_index (live, can \"customers.view\") — src/resources/customer/commands.rs"
        ));
        assert!(
            text.contains("- customers_destroy (can \"customers.delete\", middleware [\"audit\"])"),
            "{text}"
        );
        assert!(text.contains("- greet — src/main.rs"));
        assert!(!text.contains("helper"));
        assert!(text.contains("- Customer — table `customers`"));
        assert!(text.contains("- Tag — table `tag`"), "the derive's default");
        assert!(text.contains("Resources (rata make:resource): customer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reads_a_sqlite_schema_and_migrations_without_writing() {
        let root = temp("schema");
        app(&root);
        std::fs::create_dir_all(root.join("migrations")).unwrap();
        std::fs::write(root.join("migrations/0001_create_teams.sql"), "").unwrap();
        std::fs::write(root.join("migrations/0001_create_teams.down.sql"), "").unwrap();
        let file = root.join("crm.db");
        let url = elyra_db::sqlite_url(&file);
        let cfg = config(&root, Some(url.clone()));

        // Before the database exists: nothing created.
        assert!(database_schema(&cfg, None)
            .unwrap_err()
            .contains("doesn't exist yet"));
        assert!(!file.exists());
        let status = migration_status(&cfg).unwrap();
        assert!(
            status.contains("pending            0001_create_teams [sql]"),
            "{status}"
        );
        assert!(status.contains("pending            1760000000_create_customers_table [rust]"));

        runtime().unwrap().block_on(async {
            let db = elyra_db::Database::connect(&url).await.unwrap();
            for sql in [
                "CREATE TABLE teams (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, note TEXT DEFAULT 'x')",
                "CREATE TABLE _elyra_migrations (version VARCHAR(64) PRIMARY KEY, name VARCHAR(255) NOT NULL, batch INTEGER NOT NULL, applied_at BIGINT NOT NULL)",
                "INSERT INTO _elyra_migrations VALUES ('0001', 'create_teams', 1, 0)",
            ] {
                elyra_db::sqlx::query(sql).execute(db.pool()).await.unwrap();
            }
        });
        let schema = database_schema(&cfg, None).unwrap();
        assert!(
            schema.contains("\nteams\n  id INTEGER (key, null)"),
            "{schema}"
        );
        assert!(schema.contains("  name TEXT (not null)"));
        assert!(schema.contains("  note TEXT (null, default 'x')"));
        assert!(database_schema(&cfg, Some("nope"))
            .unwrap()
            .contains("No table `nope`."));
        let status = migration_status(&cfg).unwrap();
        assert!(
            status.contains("applied (batch 1)  0001_create_teams [sql]"),
            "{status}"
        );
        assert!(status.ends_with("1 pending — `rata migrate` applies them."));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_relative_sqlite_file_is_the_project_s_wherever_it_starts() {
        let root = temp("relative");
        let file = root.join("crm.db");
        runtime().unwrap().block_on(async {
            let db = elyra_db::Database::connect(&elyra_db::sqlite_url(&file))
                .await
                .unwrap();
            elyra_db::sqlx::query("CREATE TABLE teams (id INTEGER PRIMARY KEY)")
                .execute(db.pool())
                .await
                .unwrap();
        });
        // The tests run in ratatosk/, where there's no crm.db.
        assert!(!Path::new("crm.db").exists());
        let cfg = config(&root, Some("sqlite:crm.db?mode=rwc".into()));
        let url = read_only_url(&cfg).unwrap();
        assert!(url.ends_with("?mode=ro"), "{url}");
        let schema = database_schema(&cfg, None).unwrap();
        assert!(schema.contains("\nteams\n"), "{schema}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn finds_the_last_error_in_the_log() {
        let root = temp("log");
        let log = root.join("app.log");
        std::fs::write(
            format!("{}.1", log.display()),
            "2026-10-15T09:00:00.000Z INFO  [app] older\n",
        )
        .unwrap();
        std::fs::write(
            &log,
            "2026-10-15T10:00:00.000Z INFO  [app] started\n\
             2026-10-15T10:00:01.000Z ERROR [elyra::command] customers_store panicked: boom\n  at src/x.rs\n\
             2026-10-15T10:00:02.000Z WARN  [sync] retrying\n\
             2026-10-15T10:00:03.000Z DEBUG [elyra] dispatch\n",
        )
        .unwrap();
        let text = last_error(&log).unwrap();
        assert!(text.contains("The last error:\n2026-10-15T10:00:01.000Z ERROR [elyra::command] customers_store panicked: boom\n  at src/x.rs"), "{text}");
        assert!(text.contains("Just before it:\n2026-10-15T09:00:00.000Z INFO  [app] older\n2026-10-15T10:00:00.000Z INFO  [app] started"));
        let warn = read_log_entries(&log, 10, Some("warn")).unwrap();
        assert!(
            warn.contains("2 of 2 entries") && warn.ends_with("retrying"),
            "{warn}"
        );
        let last = read_log_entries(&log, 1, None).unwrap();
        assert!(last.ends_with("DEBUG [elyra] dispatch"));
        let cfg = config(&root, None);
        assert_eq!(log_file(&cfg, Some("app.log")).unwrap(), log);
        assert!(log_file(&cfg, Some("missing.log")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn searches_the_guides_by_section() {
        let docs: &[(&str, &str)] = &[
            (
                "live-queries.md",
                "# Live queries\n\nIntro.\n\n## What the model layer can't see\n\nUse `ctx.depends_on` for raw SQL.\n\n```bash\n# not a heading\n```\n",
            ),
            ("models.md", "# Models\n\nRaw SQL is possible.\n"),
        ];
        let text = search_docs(docs, "depends_on raw sql", 5);
        assert!(
            text.starts_with(
                "## docs/live-queries.md — Live queries › What the model layer can't see"
            ),
            "{text}"
        );
        assert!(text.contains("# not a heading"), "code isn't split");
        assert!(text.contains("## docs/models.md — Models"));
        assert!(search_docs(docs, "zzz", 5).starts_with("Nothing"));
        assert!(
            DOCS.iter().any(|(f, _)| *f == "live-queries.md"),
            "the real guides are embedded"
        );
    }

    #[test]
    fn speaks_both_protocol_eras() {
        let root = temp("protocol");
        let cfg = config(&root, None);
        let send = |message: Value| -> Value {
            serde_json::from_str(&handle_line(&cfg, &message.to_string()).unwrap()).unwrap()
        };
        let init = send(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {} } }));
        assert_eq!(init["result"]["protocolVersion"], "2025-06-18");
        assert!(handle_line(
            &cfg,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
        )
        .is_none());
        let list = send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "app_info",
                "database_schema",
                "migration_status",
                "last_error",
                "read_log_entries",
                "list_commands",
                "search_docs"
            ]
        );
        let docs = send(json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "search_docs", "arguments": { "query": "make:resource" } } }));
        assert!(docs["result"]["isError"].is_null());
        assert!(docs["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("make:resource"));
        let failed = send(json!({ "jsonrpc": "2.0", "id": 4, "method": "tools/call",
            "params": { "name": "database_schema", "arguments": {} } }));
        assert_eq!(
            failed["result"]["isError"], true,
            "a tool error the model can read"
        );
        let modern = send(
            json!({ "jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {
            "_meta": { "io.modelcontextprotocol/protocolVersion": MODERN },
            "name": "list_commands", "arguments": {} } }),
        );
        assert_eq!(
            modern["result"]["_meta"][META_SERVER]["name"],
            "elyra-framework"
        );
        let old = send(
            json!({ "jsonrpc": "2.0", "id": 6, "method": "tools/list", "params": {
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2024-01-01" } } }),
        );
        assert_eq!(old["error"]["code"], -32022);
        let unknown = send(json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call",
            "params": { "name": "drop_tables" } }));
        assert_eq!(unknown["error"]["code"], -32602);
        let _ = std::fs::remove_dir_all(root);
    }
}
