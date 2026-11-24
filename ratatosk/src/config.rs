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
    #[serde(default)]
    check: CheckCfg,
}

/// `[check]` (RFC 0036): the findings an app says no to, with the reason.
#[derive(Deserialize, Default)]
struct CheckCfg {
    #[serde(default)]
    allow: Vec<Allow>,
    /// RFC 0040: the server's contract, a file or the Laravel app's folder.
    #[serde(default)]
    backend: Option<String>,
    /// …and the user whose answers `elyra:contract` samples in that folder.
    #[serde(default)]
    backend_as: Option<String>,
}

/// One finding `rata check` doesn't count: `{ check = "write-without-ability",
/// command = "greet", why = "harmless, no data" }`.
#[derive(Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Allow {
    pub check: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub ability: Option<String>,
    pub why: String,
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
    /// image). `rata bundle` makes the platform's icons from it: the macOS
    /// `.icns`, the Windows `.ico` and `.exe` icon, the Linux PNGs.
    icon: Option<String>,
    /// Custom URL scheme to register (`myapp` for `myapp://…`). Written into the
    /// macOS `Info.plist` and the Linux `.desktop` file.
    deep_link: Option<String>,
    /// Free-text description for the Linux package metadata.
    description: Option<String>,
    /// `Maintainer:` for the `.deb` control file (`Name <email>`).
    maintainer: Option<String>,
    /// `[[bundle.documents]]`: the file types the OS offers the app for
    /// (RFC 0052).
    #[serde(default)]
    documents: Vec<crate::documents::RawDocument>,
    /// Who publishes the app: the Windows `.exe`'s company name and the
    /// publisher Settings → Apps shows for its installer.
    publisher: Option<String>,
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
    /// Only read by the macOS `Info.plist` writer and the `.deb` control file.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub bundle_identifier: String,
    pub bundle_name: String,
    pub bundle_version: String,
    /// Resolved source icon for `rata bundle` (from `[bundle].icon` or a
    /// conventional location), if one exists.
    pub bundle_icon: Option<PathBuf>,
    /// Custom URL scheme to register with the OS, if any.
    pub bundle_deep_link: Option<String>,
    /// Document types to register with the OS (RFC 0052).
    pub bundle_documents: Vec<crate::documents::DocumentType>,
    pub bundle_description: Option<String>,
    pub bundle_maintainer: Option<String>,
    /// Read by the Windows bundle (the `.exe`'s version resource, the installer).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub bundle_publisher: Option<String>,
    /// Resolved database URL (from `[database].url` with `${VAR}` expansion, or
    /// the `DATABASE_URL` env var).
    pub database_url: Option<String>,
    pub migrations_dir: String,
    /// `[check] allow` (RFC 0036).
    pub check_allow: Vec<Allow>,
    /// `[check] backend` and `backend_as` (RFC 0040).
    pub check_backend: Option<String>,
    pub check_backend_as: Option<String>,
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

        let bundle_identifier = raw
            .bundle
            .identifier
            .unwrap_or_else(|| format!("com.example.{app_crate}"));
        let bundle_documents =
            crate::documents::resolve(raw.bundle.documents, &root, &bundle_identifier, &app_crate)?;

        Ok(Config {
            bundle_identifier,
            bundle_documents,
            bundle_version: raw.bundle.version.unwrap_or_else(|| "0.1.0".into()),
            bundle_name,
            bundle_icon,
            bundle_deep_link: raw.bundle.deep_link,
            bundle_description: raw.bundle.description,
            bundle_maintainer: raw.bundle.maintainer,
            bundle_publisher: raw.bundle.publisher,
            database_url,
            migrations_dir: raw
                .database
                .migrations
                .unwrap_or_else(|| "migrations".into()),
            app_crate,
            frontend_dir: raw.frontend.dir,
            codegen_out,
            check_allow: raw.check.allow,
            check_backend: raw.check.backend,
            check_backend_as: raw.check.backend_as,
            root,
        })
    }
}
