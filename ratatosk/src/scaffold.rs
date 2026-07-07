//! `rata new <name>` — scaffold a new Elyra workspace + Svelte app.
//!
//! Generates a self-contained project (its own `[workspace]`) with a Rust bin
//! crate, a Vite + Svelte 5 frontend, and an `elyra.toml`. The `elyra`
//! dependency points at a path when `--elyra <path>` is given (handy pre-publish
//! / for dogfooding), otherwise a version placeholder.

use std::path::{Path, PathBuf};

pub struct NewOptions {
    pub name: String,
    pub parent_dir: PathBuf,
    /// Optional path to the framework's `framework/` crate for a `path`
    /// dependency (pre-publish). `None` -> a version placeholder.
    pub elyra_path: Option<String>,
}

pub fn new_project(opts: NewOptions) -> Result<(), String> {
    let root = opts.parent_dir.join(&opts.name);
    if root.exists() {
        return Err(format!("`{}` already exists", root.display()));
    }

    let elyra_dep = match &opts.elyra_path {
        Some(path) => format!("elyra = {{ path = {path:?} }}"),
        None => "elyra = \"0.1\"".to_string(),
    };

    // When --elyra points at a local framework crate, wire @elyra/runtime to the
    // sibling `runtime/` via a file: dependency so the app builds offline.
    let runtime_dep = opts
        .elyra_path
        .as_ref()
        .and_then(|p| {
            let framework = std::fs::canonicalize(p).ok()?;
            let runtime = framework.parent()?.join("runtime");
            runtime
                .is_dir()
                .then(|| format!("file:{}", runtime.display()))
        })
        .unwrap_or_else(|| "^0.0.0".to_string());

    let subst = |tpl: &str| {
        tpl.replace("{{name}}", &opts.name)
            .replace("{{elyra_dep}}", &elyra_dep)
            .replace("{{runtime_dep}}", &runtime_dep)
    };

    for (rel, contents) in FILES {
        write(&root, rel, &subst(contents))?;
    }

    println!("Created {}", root.display());
    println!("\nNext:");
    println!("  cd {}", opts.name);
    println!("  (cd app && npm install && npm run build)   # build the frontend");
    println!("  rata codegen                                # generate typed bindings");
    println!("  cargo run                                   # launch");
    Ok(())
}

fn write(root: &Path, rel: &str, contents: &str) -> Result<(), String> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// (relative path, template contents). `{{name}}` / `{{elyra_dep}}` are substituted.
const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        r#"# Standalone workspace so the project builds anywhere.
[workspace]

[package]
name = "{{name}}"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "{{name}}"
path = "src/main.rs"

[dependencies]
{{elyra_dep}}
serde = { version = "1", features = ["derive"] }
specta = { version = "=2.0.0-rc.25", features = ["derive"] }
rust-embed = "8"
"#,
    ),
    (
        "src/main.rs",
        r#"use elyra::{command, commands, App, Ctx};
use serde::{Deserialize, Serialize};

/// The built Svelte frontend, embedded from memory. Empty until `npm run build`
/// — the shell serves its built-in fallback page in that case.
#[derive(rust_embed::RustEmbed)]
#[folder = "app/dist"]
struct Assets;

#[command]
async fn greet(_ctx: Ctx, name: String) -> String {
    format!("Hello, {name}!")
}

#[derive(Serialize, Deserialize, specta::Type)]
struct AppInfo {
    name: String,
    os: String,
}

#[command]
async fn app_info(_ctx: Ctx) -> AppInfo {
    AppInfo {
        name: "{{name}}".into(),
        os: std::env::consts::OS.into(),
    }
}

fn main() -> elyra::Result<()> {
    App::new()
        .title("{{name}}")
        .commands(commands![greet, app_info])
        .assets(elyra::asset_resolver::<Assets>())
        .run()
}
"#,
    ),
    (
        "elyra.toml",
        r#"[app]
crate = "{{name}}"

[frontend]
dir = "app"

[codegen]
out = "app/src/bindings.ts"

[bundle]
identifier = "com.example.{{name}}"
name = "{{name}}"
version = "0.1.0"
"#,
    ),
    (
        ".gitignore",
        "/target\n**/node_modules\napp/dist/*\n!app/dist/.gitkeep\n.DS_Store\n",
    ),
    ("app/dist/.gitkeep", "placeholder — replaced by the Vite build\n"),
    (
        "app/package.json",
        r#"{
  "name": "{{name}}-app",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build"
  },
  "dependencies": {
    "@elyra/runtime": "{{runtime_dep}}",
    "@msgpack/msgpack": "^3.1.2"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^5.0.3",
    "svelte": "^5.20.0",
    "vite": "^6.1.0"
  }
}
"#,
    ),
    (
        "app/vite.config.js",
        r#"import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// `base: "./"` -> relative asset URLs (served from elyra://localhost/).
export default defineConfig({
  plugins: [svelte()],
  base: "./",
  build: { outDir: "dist", emptyOutDir: true, target: "esnext" },
  server: { port: 5173, strictPort: true },
});
"#,
    ),
    (
        "app/svelte.config.js",
        "import { vitePreprocess } from \"@sveltejs/vite-plugin-svelte\";\n\nexport default { preprocess: vitePreprocess() };\n",
    ),
    (
        "app/index.html",
        r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{{name}}</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.js"></script>
  </body>
</html>
"#,
    ),
    (
        "app/src/main.js",
        r#"import "./app.css";
import { mount } from "svelte";
import { initTheme } from "./theme.js";
import App from "./App.svelte";

initTheme();

export default mount(App, { target: document.getElementById("app") });
"#,
    ),
    (
        // Default theme (auto / light / dark). Palette lives in app.css.
        "app/src/theme.js",
        r#"// Theme handling: auto / light / dark. Dark is the default (no attribute);
// light sets data-theme="light". The palette lives in app.css.

const KEY = "{{name}}-theme";

export function getTheme() {
  return localStorage.getItem(KEY) || "auto";
}

export function applyTheme(theme) {
  localStorage.setItem(KEY, theme);
  const root = document.documentElement;
  const wantLight =
    theme === "light" ||
    (theme === "auto" &&
      window.matchMedia("(prefers-color-scheme: light)").matches);
  if (wantLight) root.setAttribute("data-theme", "light");
  else root.removeAttribute("data-theme");
}

export function initTheme() {
  applyTheme(getTheme());
  window
    .matchMedia("(prefers-color-scheme: light)")
    .addEventListener("change", () => {
      if (getTheme() === "auto") applyTheme("auto");
    });
}
"#,
    ),
    (
        // Default theme — Tokyo Night palette (shared with Grove / Conductor).
        "app/src/app.css",
        r#"/* Default theme — Tokyo Night palette. Dark by default; light via [data-theme="light"]. */
:root {
  --bg: #16161e;
  --bg-2: #1a1b26;
  --bg-3: #1f2030;
  --panel: #1e1f2b;
  --border: #2a2b3c;
  --text: #c0caf5;
  --text-dim: #787c99;
  --accent: #7aa2f7;
  --accent-2: #2f3650;
  --green: #9ece6a;
  --red: #f7768e;
  --amber: #e0af68;
  --brand: #fb923c;
  --font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
  --font-mono: "JetBrains Mono", "SF Mono", Menlo, monospace;
}

:root[data-theme="light"] {
  --bg: #ffffff;
  --bg-2: #f5f6fa;
  --bg-3: #eceef3;
  --panel: #ffffff;
  --border: #d8dbe3;
  --text: #2a2e3a;
  --text-dim: #6b7080;
  --accent: #3b6fd6;
  --accent-2: #dbe4f7;
  --green: #3f9142;
  --red: #d6435b;
}

* { box-sizing: border-box; }
html, body, #app { margin: 0; height: 100%; }
body {
  font-family: var(--font-ui);
  background: var(--bg);
  color: var(--text);
  font-size: 14px;
}
button { font-family: inherit; cursor: pointer; }
code { font-family: var(--font-mono); color: var(--text-dim); }

::-webkit-scrollbar { width: 8px; height: 8px; }
::-webkit-scrollbar-thumb { background: var(--accent-2); border-radius: 4px; }

.app { display: flex; flex-direction: column; height: 100vh; overflow: hidden; }

.toolbar {
  display: flex; align-items: center; gap: 14px;
  height: 44px; padding: 0 14px;
  background: var(--bg-2); border-bottom: 1px solid var(--border);
}
.brand { font-weight: 700; letter-spacing: 0.2px; }
.spacer { flex: 1; }

.content { flex: 1; overflow: auto; padding: 24px; }

.card {
  max-width: 480px;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 20px;
}
.card h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
.subtitle { color: var(--text-dim); margin: 0 0 16px; font-size: 13px; }

.row { display: flex; gap: 8px; }
input {
  flex: 1; min-width: 0;
  background: var(--bg-3); color: var(--text);
  border: 1px solid var(--border); border-radius: 7px;
  padding: 7px 10px; font: inherit;
}
input:focus { outline: none; border-color: var(--accent); }

.btn {
  border: 1px solid var(--border); background: var(--bg-3); color: var(--text);
  padding: 7px 12px; border-radius: 7px; font-size: 13px;
}
.btn:hover { border-color: var(--accent); color: var(--accent); }
.btn.primary {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  border-color: var(--accent); color: var(--accent); font-weight: 600;
}

.result {
  margin: 14px 0 0; padding: 10px 12px;
  background: var(--bg-3); border: 1px dashed var(--border); border-radius: 8px;
  font-family: var(--font-mono); color: var(--green); font-size: 13px;
}
"#,
    ),
    (
        "app/src/App.svelte",
        r#"<script>
  import { invoke } from "@elyra/runtime";
  import { getTheme, applyTheme } from "./theme.js";

  let name = $state("world");
  let greeting = $state("");
  let theme = $state(getTheme());

  async function greet() {
    greeting = await invoke("greet", name);
  }

  function cycleTheme() {
    theme = theme === "auto" ? "light" : theme === "light" ? "dark" : "auto";
    applyTheme(theme);
  }
</script>

<div class="app">
  <header class="toolbar">
    <span class="brand">{{name}}</span>
    <span class="spacer"></span>
    <button class="btn" onclick={cycleTheme}>theme: {theme}</button>
  </header>

  <main class="content">
    <div class="card">
      <h2>Hello from Elyra</h2>
      <p class="subtitle">Edit <code>src/App.svelte</code> to get started.</p>
      <div class="row">
        <input bind:value={name} placeholder="name" />
        <button class="btn primary" onclick={greet}>greet</button>
      </div>
      {#if greeting}<p class="result">{greeting}</p>{/if}
    </div>
  </main>
</div>
"#,
    ),
];
