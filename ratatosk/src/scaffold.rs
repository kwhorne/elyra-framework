//! `rata new <name>` — scaffold a new Elyra workspace + Svelte app.
//!
//! Generates a self-contained project (its own `[workspace]`) with a Rust bin
//! crate, a Vite + Svelte 5 frontend, and an `elyra.toml`. The `elyra`
//! dependency points at a path when `--elyra <path>` is given (handy pre-publish
//! / for dogfooding), otherwise a version placeholder.

use std::path::{Path, PathBuf};

/// Where releases live. Scaffolded projects depend on a tag here, since the crates
/// are not published to a registry.
const REPO_URL: &str = "https://github.com/kwhorne/elyra-framework";

pub struct NewOptions {
    pub name: String,
    pub parent_dir: PathBuf,
    /// Optional path to the framework's `framework/` crate for a `path`
    /// dependency (pre-publish). `None` -> a version placeholder.
    pub elyra_path: Option<String>,
}

/// Whether a local `runtime/` checkout lacks the `dist/` its package points at.
fn runtime_needs_build(runtime: &Path) -> bool {
    !runtime.join("dist").join("index.js").is_file()
}

pub fn new_project(opts: NewOptions) -> Result<(), String> {
    let root = opts.parent_dir.join(&opts.name);
    if root.exists() {
        return Err(format!("`{}` already exists", root.display()));
    }

    // Pin the scaffold to *this* CLI's version. Elyra is distributed as tagged
    // GitHub releases rather than through crates.io/npm, so the default is a git
    // dependency on the matching tag — a registry version string would name
    // something that doesn't exist (see docs/releasing.md).
    let elyra_version = env!("CARGO_PKG_VERSION");
    let elyra_dep = match &opts.elyra_path {
        Some(path) => format!("elyra = {{ path = {path:?} }}"),
        None => format!("elyra = {{ git = \"{REPO_URL}\", tag = \"v{elyra_version}\" }}"),
    };

    // When --elyra points at a local framework crate, wire @elyra/runtime to the
    // sibling `runtime/` via a file: dependency so the app builds offline.
    // Otherwise use the tarball attached to the GitHub release: npm accepts a
    // remote tarball URL, and (unlike pnpm/yarn) cannot install a subdirectory of
    // a git repository, which is what `runtime/` is.
    let local_runtime: Option<PathBuf> = opts.elyra_path.as_ref().and_then(|p| {
        let framework = std::fs::canonicalize(p).ok()?;
        let runtime = framework.parent()?.join("runtime");
        runtime.is_dir().then_some(runtime)
    });
    let runtime_dep = match &local_runtime {
        Some(runtime) => format!("file:{}", runtime.display()),
        None => format!(
            "{REPO_URL}/releases/download/v{elyra_version}/elyra-runtime-{elyra_version}.tgz"
        ),
    };
    // @elyra/ui (RFC 0012) the same way: the sibling `ui/`, or the release's
    // tarball. It ships sources, so a checkout needs no build.
    let ui_dep = match opts
        .elyra_path
        .as_ref()
        .and_then(|p| std::fs::canonicalize(p).ok())
        .and_then(|framework| Some(framework.parent()?.join("ui")))
        .filter(|ui| ui.is_dir())
    {
        Some(ui) => format!("file:{}", ui.display()),
        None => {
            format!("{REPO_URL}/releases/download/v{elyra_version}/elyra-ui-{elyra_version}.tgz")
        }
    };

    let subst = |tpl: &str| {
        tpl.replace("{{name}}", &opts.name)
            .replace("{{elyra_dep}}", &elyra_dep)
            .replace("{{runtime_dep}}", &runtime_dep)
            .replace("{{ui_dep}}", &ui_dep)
    };

    for (rel, contents) in FILES {
        write(&root, rel, &subst(contents))?;
    }

    println!("Created {}", root.display());
    println!("\nNext:");
    // A `file:` dependency is a symlink to the checkout, whose package entry
    // points at `dist/` — build output that a fresh clone doesn't have. Without
    // it the frontend build fails with "failed to resolve import @elyra/runtime".
    if let Some(runtime) = local_runtime.as_deref().filter(|r| runtime_needs_build(r)) {
        println!(
            "  (cd {} && npm ci && npm run build)   # build @elyra/runtime first (no dist/ yet)",
            runtime.display()
        );
    }
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
pub(crate) const FILES: &[(&str, &str)] = &[
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
        r#"use elyra::{command, commands, AboutInfo, App, Ctx};
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
        .about(
            AboutInfo::new("{{name}}", env!("CARGO_PKG_VERSION"))
                .description("An Elyra desktop app.")
                .icon("/icon.svg"),
        )
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
        "app/public/icon.svg",
        r##"<svg width="512" height="512" viewBox="0 0 512 512" xmlns="http://www.w3.org/2000/svg">
  <defs><linearGradient id="e" x1="0" y1="0" x2="1" y2="1">
    <stop offset="0" stop-color="#fdba74"/><stop offset="1" stop-color="#f97316"/>
  </linearGradient></defs>
  <rect width="512" height="512" rx="116" fill="#1a1b26"/>
  <g stroke="url(#e)" stroke-width="22" fill="url(#e)" stroke-linejoin="round">
    <line x1="256" y1="150" x2="150" y2="350"/>
    <line x1="256" y1="150" x2="362" y2="350"/>
    <line x1="150" y1="350" x2="362" y2="350"/>
    <circle cx="256" cy="150" r="46"/>
    <circle cx="150" cy="350" r="46"/>
    <circle cx="362" cy="350" r="46"/>
  </g>
</svg>
"##,
    ),
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
    "@elyra/ui": "{{ui_dep}}",
    "@msgpack/msgpack": "^3.1.3"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "^7.3.0",
    "svelte": "^5.56.8",
    "vite": "^8.2.1"
  },
  "engines": {
    "node": "^20.19.0 || >=22.12.0"
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
  // One Svelte and one @elyra/runtime, the app's own, even when @elyra/ui is
  // linked from a checkout (`rata new --elyra`): two runtimes would keep two
  // sets of stores.
  resolve: { dedupe: ["svelte", "@elyra/runtime"] },
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
    <link rel="icon" type="image/svg+xml" href="/icon.svg" />
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
        r#"import "@elyra/ui/tokens.css";
import "./app.css";
import { mount } from "svelte";
import { initTheme } from "./theme.js";
import App from "./App.svelte";

initTheme();

export default mount(App, { target: document.getElementById("app") });
"#,
    ),
    (
        // Default theme (auto / light / dark). The palette is @elyra/ui's tokens.
        "app/src/theme.js",
        r#"// Theme handling: auto / light / dark. Dark is the default (no attribute);
// light sets data-theme="light". The palette is @elyra/ui's design tokens
// (`--elyra-*`, in @elyra/ui/tokens.css); override them in app.css.

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
        // The app's own styles, on @elyra/ui's design tokens (RFC 0012).
        "app/src/app.css",
        r#"/*
 * The app's styles. Colours, fonts and shapes are @elyra/ui's design tokens
 * (`--elyra-*`, imported in main.js before this file): the Tokyo Night
 * palette, dark by default and light under [data-theme="light"]. Override
 * any of them here, for both themes or one:
 *
 *   :root { --elyra-accent: #fb923c; }
 *   :root[data-theme="light"] { --elyra-accent: #c2410c; }
 */
:root {
  --brand: #fb923c;
}

* { box-sizing: border-box; }
html, body, #app { margin: 0; height: 100%; }
body {
  font-family: var(--elyra-font);
  background: var(--elyra-bg);
  color: var(--elyra-text);
  font-size: 14px;
}
button { font-family: inherit; cursor: pointer; }
code { font-family: var(--elyra-font-mono); color: var(--elyra-muted); }

::-webkit-scrollbar { width: 8px; height: 8px; }
::-webkit-scrollbar-thumb { background: var(--elyra-accent-soft); border-radius: 4px; }

.app { display: flex; flex-direction: column; height: 100vh; overflow: hidden; }

.toolbar {
  display: flex; align-items: center; gap: 14px;
  height: 44px; padding: 0 14px;
  background: var(--elyra-surface); border-bottom: 1px solid var(--elyra-border);
}
.brand { font-weight: 700; letter-spacing: 0.2px; color: var(--elyra-text); text-decoration: none; }
.nav { display: flex; gap: 4px; margin-left: 16px; }
.nav a { color: var(--elyra-muted); text-decoration: none; padding: 4px 10px; border-radius: 6px; }
.nav a:hover { color: var(--elyra-text); background: var(--elyra-inset); }
.nav a.active { color: var(--elyra-text); background: var(--elyra-accent-soft); }
.spacer { flex: 1; }

.content { flex: 1; overflow: auto; padding: 24px; }

.card {
  max-width: 480px;
  background: var(--elyra-raised);
  border: 1px solid var(--elyra-border);
  border-radius: 10px;
  padding: 20px;
}
.card h2 { margin: 0 0 4px; font-size: 18px; font-weight: 600; }
.subtitle { color: var(--elyra-muted); margin: 0 0 16px; font-size: 13px; }

.row { display: flex; gap: 8px; }
input {
  flex: 1; min-width: 0;
  background: var(--elyra-inset); color: var(--elyra-text);
  border: 1px solid var(--elyra-border); border-radius: 7px;
  padding: 7px 10px; font: inherit;
}
input:focus { outline: none; border-color: var(--elyra-accent); }

.btn {
  border: 1px solid var(--elyra-border); background: var(--elyra-inset); color: var(--elyra-text);
  padding: 7px 12px; border-radius: 7px; font-size: 13px;
}
.btn:hover { border-color: var(--elyra-accent); color: var(--elyra-accent); }
.btn.primary {
  background: color-mix(in srgb, var(--elyra-accent) 18%, transparent);
  border-color: var(--elyra-accent); color: var(--elyra-accent); font-weight: 600;
}

.result {
  margin: 14px 0 0; padding: 10px 12px;
  background: var(--elyra-inset); border: 1px dashed var(--elyra-border); border-radius: 8px;
  font-family: var(--elyra-font-mono); color: var(--elyra-success); font-size: 13px;
}
"#,
    ),
    (
        "app/src/App.svelte",
        r#"<script>
  import Router from "./Router.svelte";
  import NavCount from "./NavCount.svelte";
  import { routes } from "./routes.js";
  import { nav } from "./resources/index.js";
  import { href, route } from "@elyra/runtime";
  import { getTheme, applyTheme } from "./theme.js";

  let theme = $state(getTheme());

  function cycleTheme() {
    theme = theme === "auto" ? "light" : theme === "light" ? "dark" : "auto";
    applyTheme(theme);
  }
</script>

<div class="app">
  <header class="toolbar">
    <a class="brand" href={href("/")}>{{name}}</a>
    <nav class="nav">
      <a href={href("/")} class:active={$route.path === "/"}>Home</a>
      {#each nav as item (item.path)}
        <a href={href(item.path)} class:active={$route.path.startsWith(item.path)}
          >{item.label}{#if item.count}<NavCount count={item.count} />{/if}</a
        >
      {/each}
    </nav>
    <span class="spacer"></span>
    <button class="btn" onclick={cycleTheme}>theme: {theme}</button>
  </header>

  <main class="content">
    <Router {routes} />
  </main>
</div>
"#,
    ),
    (
        "app/src/NavCount.svelte",
        r#"<!--
  A count beside a navigation link — a nav item's `count`, a store of a number
  (the unread notifications, say). Nothing at zero.
-->
<script>
  let { count } = $props();
  let value = $state(0);
  $effect(() => count.subscribe((n) => (value = n)));
</script>

{#if value > 0}<span class="count">{value > 99 ? "99+" : value}</span>{/if}

<style>
  .count {
    margin-left: 6px; padding: 0 6px; border-radius: 999px;
    background: var(--accent); color: var(--bg); font-size: 11px; font-weight: 600;
  }
</style>
"#,
    ),
    (
        "app/src/Router.svelte",
        r#"<!--
  Renders the page for the current route. `routes` maps patterns
  ("/customers/:id") to loaders (`() => import("./pages/Customer.svelte")`),
  so each page is its own chunk. The page receives `params` and `query`.
-->
<script>
  import { route, resolveRoute } from "@elyra/runtime";
  import NotFound from "./pages/NotFound.svelte";

  let { routes } = $props();

  const match = $derived(resolveRoute(routes, $route.path));
</script>

{#if match}
  {#await match.value() then page}
    <page.default params={match.params} query={$route.query} />
  {:catch error}
    <p class="result">Could not load this page: {error.message}</p>
  {/await}
{:else}
  <NotFound path={$route.path} />
{/if}
"#,
    ),
    (
        "app/src/routes.js",
        r#"// The app's pages: a pattern ("/customers/:id") -> a loader. The most
// specific pattern wins, so "/customers/new" beats "/customers/:id".
// `rata make:resource` adds its pages through ./resources/index.js.
import { routes as resourceRoutes } from "./resources/index.js";

export const routes = {
  "/": () => import("./pages/Home.svelte"),
  ...resourceRoutes,
};
"#,
    ),
    (
        "app/src/resources/index.js",
        r#"// Resources — managed by rata. Regenerated by `rata make:resource` and
// `rata resources:sync` from the folders next to this file; change a
// resource in its own index.js, not here.

// The runtime as a namespace: one from before 0.25 has no registerSearch.
import * as runtime from "@elyra/runtime";

const all = [];

/** Every resource's pages, merged into the app's routes. */
export const routes = Object.assign({}, ...all.map((r) => r.routes ?? {}));

/** Links for the app's navigation: `{ label, path }`. */
export const nav = all.flatMap((r) => r.nav ?? []);

// Each resource's search, in the ⌘K palette.
for (const r of all) if (r.search) runtime.registerSearch?.(r.search);
"#,
    ),
    (
        "app/src/pages/Home.svelte",
        r#"<script>
  import { invoke } from "@elyra/runtime";

  let name = $state("world");
  let greeting = $state("");

  async function greet() {
    greeting = await invoke("greet", name);
  }
</script>

<div class="card">
  <h2>Hello from Elyra</h2>
  <p class="subtitle">Edit <code>src/pages/Home.svelte</code> to get started.</p>
  <div class="row">
    <input bind:value={name} placeholder="name" />
    <button class="btn primary" onclick={greet}>greet</button>
  </div>
  {#if greeting}<p class="result">{greeting}</p>{/if}
</div>
"#,
    ),
    (
        "app/src/pages/NotFound.svelte",
        r#"<script>
  import { href } from "@elyra/runtime";
  let { path } = $props();
</script>

<div class="card">
  <h2>Not found</h2>
  <p class="subtitle">Nothing lives at <code>{path}</code>.</p>
  <a class="btn" href={href("/")}>Back home</a>
</div>
"#,
    ),
];
