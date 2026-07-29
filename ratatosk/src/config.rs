//! `elyra.toml` — the project descriptor Ratatosk reads.
//!
//! ```toml
//! [app]
//! crate = "elyra-example"
//!
//! [frontend]
//! dir = "example/app"
//!
//! [codegen]
//! out = "example/app/src/bindings.ts"
//! ```

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
struct Raw {
    app: App,
    frontend: Frontend,
    #[serde(default)]
    codegen: Codegen,
    #[serde(default)]
    bundle: Bundle,
    #[serde(default)]
    database: DatabaseCfg,
}

#[derive(Deserialize, Default)]
struct DatabaseCfg {
    url: Option<String>,
    migrations: Option<String>,
}

#[derive(Deserialize, Default)]
struct Bundle {
    identifier: Option<String>,
    name: Option<String>,
    version: Option<String>,
    /// Path (relative to the project root) to a source icon (`.svg` or a raster
    /// image). Used by `rata bundle` to generate the macOS `.icns`.
    icon: Option<String>,
    /// Custom URL scheme to register (`myapp` for `myapp://…`). Written into the
    /// macOS `Info.plist` and the Linux `.desktop` file.
    deep_link: Option<String>,
    /// Free-text description for the Linux package metadata.
    description: Option<String>,
    /// `Maintainer:` for the `.deb` control file (`Name <email>`).
    maintainer: Option<String>,
}

#[derive(Deserialize)]
struct App {
    #[serde(rename = "crate")]
    krate: String,
}

#[derive(Deserialize)]
struct Frontend {
    dir: String,
}

#[derive(Deserialize, Default)]
struct Codegen {
    out: Option<String>,
}

/// Resolved configuration, with the workspace root that contains `elyra.toml`.
pub struct Config {
    pub root: PathBuf,
    pub app_crate: String,
    pub frontend_dir: String,
    pub codegen_out: String,
    pub bundle_identifier: String,
    pub bundle_name: String,
    pub bundle_version: String,
    /// Resolved source icon for `rata bundle` (from `[bundle].icon` or a
    /// conventional location), if one exists.
    pub bundle_icon: Option<PathBuf>,
    /// Custom URL scheme to register with the OS, if any.
    pub bundle_deep_link: Option<String>,
    pub bundle_description: Option<String>,
    pub bundle_maintainer: Option<String>,
    /// Resolved database URL (from `[database].url` with `${VAR}` expansion, or
    /// the `DATABASE_URL` env var).
    pub database_url: Option<String>,
    pub migrations_dir: String,
}

/// Expand `${VAR}` occurrences in a string from the environment.
fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        rest = &rest[start + 2..];
        if let Some(end) = rest.find('}') {
            let var = &rest[..end];
            out.push_str(&std::env::var(var).unwrap_or_default());
            rest = &rest[end + 1..];
        } else {
            out.push_str("${");
            break;
        }
    }
    out.push_str(rest);
    out
}

impl Config {
    /// Find `elyra.toml` by walking up from the current directory, then parse it.
    pub fn load() -> Result<Config, String> {
        let start = std::env::current_dir().map_err(|e| e.to_string())?;
        let mut dir = start.as_path();
        let path = loop {
            let candidate = dir.join("elyra.toml");
            if candidate.is_file() {
                break candidate;
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => {
                    return Err("no `elyra.toml` found in this directory or any parent".to_string())
                }
            }
        };

        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let raw: Raw = toml::from_str(&text).map_err(|e| format!("invalid elyra.toml: {e}"))?;

        let root = path
            .parent()
            .expect("elyra.toml has a parent")
            .to_path_buf();

        let codegen_out = raw
            .codegen
            .out
            .unwrap_or_else(|| format!("{}/src/bindings.ts", raw.frontend.dir));

        let app_crate = raw.app.krate;
        let bundle_name = raw.bundle.name.unwrap_or_else(|| app_crate.clone());

        // Resolve a source icon: explicit `[bundle].icon`, else conventional spots.
        let bundle_icon = raw
            .bundle
            .icon
            .map(|p| root.join(p))
            .filter(|p| p.is_file())
            .or_else(|| {
                [
                    "app/public/icon.svg",
                    "app/public/icon.png",
                    "assets/icon.png",
                    "assets/icon.svg",
                    "icon.png",
                    "icon.svg",
                ]
                .iter()
                .map(|c| root.join(c))
                .find(|p| p.is_file())
            });

        let database_url = raw
            .database
            .url
            .map(|u| expand_env(&u))
            .or_else(|| std::env::var("DATABASE_URL").ok());

        Ok(Config {
            bundle_identifier: raw
                .bundle
                .identifier
                .unwrap_or_else(|| format!("com.example.{app_crate}")),
            bundle_version: raw.bundle.version.unwrap_or_else(|| "0.1.0".into()),
            bundle_name,
            bundle_icon,
            bundle_deep_link: raw.bundle.deep_link,
            bundle_description: raw.bundle.description,
            bundle_maintainer: raw.bundle.maintainer,
            database_url,
            migrations_dir: raw
                .database
                .migrations
                .unwrap_or_else(|| "migrations".into()),
            app_crate,
            frontend_dir: raw.frontend.dir,
            codegen_out,
            root,
        })
    }
}
