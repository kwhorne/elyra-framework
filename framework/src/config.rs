//! Configuration — Elyra's `Config` facade.
//!
//! `elyra.toml` used to be read only by the `rata` CLI, so an app had no way to
//! ask for its own configuration at runtime: values were either hard-coded or
//! pulled straight from `std::env::var`. [`Config`] is the runtime half.
//!
//! ## Sources, lowest priority first
//! 1. defaults registered by the app or its providers ([`Config::default_value`]),
//! 2. `elyra.toml` (the `[app] / [frontend] / …` tables, flattened to dotted keys),
//! 3. `config/*.toml` files (the file stem becomes the key prefix),
//! 4. a `.env` file,
//! 5. real process environment variables.
//!
//! A later source overrides an earlier one, so a shipped default can always be
//! overridden by an env var without touching the binary.
//!
//! ## Keys
//! Dotted, lowercase: `database.url`, `app.name`. An environment variable maps to
//! a key by lowercasing and turning `__` into `.`:
//! `ELYRA_DATABASE__URL=…` sets `database.url`. Plain `DATABASE_URL` is also
//! accepted for the handful of conventional names.
//!
//! ```ignore
//! let config = ctx.get::<Config>();
//! let url: String = config.get("database.url").unwrap_or_else(|| "sqlite://app.db".into());
//! let debug: bool = config.bool("app.debug").unwrap_or(false);
//! ```
//!
//! `${VAR}` inside a value is expanded from the environment, matching the CLI's
//! behaviour for `[database] url`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A resolved, read-only configuration map with dotted keys.
#[derive(Debug, Clone, Default)]
pub struct Config {
    values: BTreeMap<String, String>,
}

impl Config {
    /// An empty configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the standard sources, rooted at `dir` (usually the project root or
    /// the directory next to the executable).
    pub fn load(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        let mut config = Config::new();

        if let Ok(text) = std::fs::read_to_string(dir.join("elyra.toml")) {
            config.merge_toml(&text, None);
        }

        let config_dir = dir.join("config");
        if let Ok(entries) = std::fs::read_dir(&config_dir) {
            let mut files: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
                .collect();
            files.sort();
            for file in files {
                let prefix = file
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase());
                if let Ok(text) = std::fs::read_to_string(&file) {
                    config.merge_toml(&text, prefix.as_deref());
                }
            }
        }

        if let Ok(text) = std::fs::read_to_string(dir.join(".env")) {
            config.merge_dotenv(&text);
        }

        config.merge_env();
        config
    }

    /// Load from the current working directory, falling back to the directory
    /// containing the executable (a bundled app doesn't run from its source tree).
    pub fn load_default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        if cwd.join("elyra.toml").exists() || cwd.join(".env").exists() {
            return Config::load(cwd);
        }
        if let Some(exe_dir) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
        {
            return Config::load(exe_dir);
        }
        Config::load(cwd)
    }

    /// Register a fallback value (used when no source provides the key).
    pub fn default_value(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.values
            .entry(normalize_key(key))
            .or_insert_with(|| value.into());
        self
    }

    /// Set (or override) a value.
    pub fn set(&mut self, key: &str, value: impl Into<String>) -> &mut Self {
        self.values.insert(normalize_key(key), value.into());
        self
    }

    /// The raw string for `key`, with `${VAR}` expanded.
    pub fn raw(&self, key: &str) -> Option<String> {
        self.values.get(&normalize_key(key)).map(|v| expand(v))
    }

    /// A parsed value: `config.get::<u16>("server.port")`.
    pub fn get<T: std::str::FromStr>(&self, key: &str) -> Option<T> {
        self.raw(key)?.parse().ok()
    }

    /// A string value (the common case).
    pub fn string(&self, key: &str) -> Option<String> {
        self.raw(key)
    }

    /// A boolean: `true/1/yes/on` (case-insensitive) are true.
    pub fn bool(&self, key: &str) -> Option<bool> {
        let raw = self.raw(key)?.trim().to_ascii_lowercase();
        match raw.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    /// An integer value.
    pub fn int(&self, key: &str) -> Option<i64> {
        self.get(key)
    }

    /// A value or a fallback.
    pub fn get_or<T: std::str::FromStr>(&self, key: &str, fallback: T) -> T {
        self.get(key).unwrap_or(fallback)
    }

    /// Whether a key is present.
    pub fn has(&self, key: &str) -> bool {
        self.values.contains_key(&normalize_key(key))
    }

    /// Every key under `prefix`, with the prefix stripped.
    pub fn section(&self, prefix: &str) -> BTreeMap<String, String> {
        let prefix = format!("{}.", normalize_key(prefix));
        self.values
            .iter()
            .filter_map(|(k, v)| k.strip_prefix(&prefix).map(|k| (k.to_string(), expand(v))))
            .collect()
    }

    /// Every resolved key (sorted) — for `rata config:show` and debugging.
    pub fn all(&self) -> BTreeMap<String, String> {
        self.values.clone()
    }

    /// Merge a TOML document, flattening tables into dotted keys. `prefix`
    /// namespaces the file (used for `config/<name>.toml`).
    pub fn merge_toml(&mut self, text: &str, prefix: Option<&str>) {
        // A deliberately small TOML reader: tables, `key = value`, and the scalar
        // types a config file actually uses. Enough for `elyra.toml`-shaped files
        // without adding a parser dependency to the runtime.
        let mut table = prefix.map(|p| p.to_string()).unwrap_or_default();
        for line in text.lines() {
            let line = strip_comment(line).trim();
            if line.is_empty() {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                let name = name.trim().trim_matches('"').to_ascii_lowercase();
                table = match prefix {
                    Some(p) => format!("{p}.{name}"),
                    None => name,
                };
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().trim_matches('"').to_ascii_lowercase();
            let value = unquote(value.trim());
            let full = if table.is_empty() {
                key
            } else {
                format!("{table}.{key}")
            };
            self.values.insert(normalize_key(&full), value);
        }
    }

    /// Merge `KEY=value` lines from a `.env` file.
    pub fn merge_dotenv(&mut self, text: &str) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            self.values
                .insert(env_key_to_config_key(key), unquote(value.trim()));
        }
    }

    /// Merge real environment variables (highest priority).
    pub fn merge_env(&mut self) {
        for (key, value) in std::env::vars() {
            self.values.insert(env_key_to_config_key(&key), value);
        }
    }
}

/// `ELYRA_DATABASE__URL` / `DATABASE_URL` -> `database.url`.
fn env_key_to_config_key(key: &str) -> String {
    let key = key.trim();
    let stripped = key.strip_prefix("ELYRA_").unwrap_or(key);
    let dotted = stripped.replace("__", ".").replace('_', ".");
    normalize_key(&dotted)
}

fn normalize_key(key: &str) -> String {
    key.trim().trim_matches('.').to_ascii_lowercase()
}

fn strip_comment(line: &str) -> &str {
    // Only a `#` outside quotes starts a comment.
    let mut in_quotes = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            '#' if !in_quotes => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// Expand `${VAR}` (and `$VAR`) from the process environment.
fn expand(value: &str) -> String {
    if !value.contains('$') {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len());
    let bytes: Vec<char> = value.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '$' && i + 1 < bytes.len() {
            if bytes[i + 1] == '{' {
                if let Some(end) = bytes[i + 2..].iter().position(|c| *c == '}') {
                    let name: String = bytes[i + 2..i + 2 + end].iter().collect();
                    out.push_str(&std::env::var(&name).unwrap_or_default());
                    i += end + 3;
                    continue;
                }
            } else if bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == '_' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == '_') {
                    j += 1;
                }
                let name: String = bytes[i + 1..j].iter().collect();
                out.push_str(&std::env::var(&name).unwrap_or_default());
                i = j;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// A [`Provider`](crate::Provider) that loads [`Config`] and binds it.
///
/// ```no_run
/// use elyra::App;
/// use elyra::config::ConfigProvider;
/// App::new().provider(ConfigProvider::default()).run().unwrap();
/// // in a #[command]: ctx.get::<elyra::Config>().string("app.name")
/// ```
#[derive(Default)]
pub struct ConfigProvider {
    dir: Option<PathBuf>,
    defaults: Vec<(String, String)>,
}

impl ConfigProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from an explicit directory instead of auto-detecting.
    pub fn at(mut self, dir: impl Into<PathBuf>) -> Self {
        self.dir = Some(dir.into());
        self
    }

    /// Register a compiled-in default (lowest priority).
    pub fn with_default(mut self, key: &str, value: impl Into<String>) -> Self {
        self.defaults.push((key.to_string(), value.into()));
        self
    }
}

impl crate::Provider for ConfigProvider {
    fn register(&self, container: &mut crate::Container) {
        let mut config = match &self.dir {
            Some(dir) => Config::load(dir),
            None => Config::load_default(),
        };
        for (key, value) in &self.defaults {
            config.default_value(key, value.clone());
        }
        container.bind(config);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELYRA_TOML: &str = r#"
# Ratatosk project descriptor
[app]
crate = "elyra-example"

[frontend]
dir = "example/app"   # inline comment

[database]
url = "sqlite://example/app.db?mode=rwc"
migrations = "example/migrations"
"#;

    #[test]
    fn reads_elyra_toml_into_dotted_keys() {
        let mut config = Config::new();
        config.merge_toml(ELYRA_TOML, None);
        assert_eq!(config.string("app.crate").as_deref(), Some("elyra-example"));
        assert_eq!(
            config.string("frontend.dir").as_deref(),
            Some("example/app")
        );
        assert_eq!(
            config.string("database.url").as_deref(),
            Some("sqlite://example/app.db?mode=rwc")
        );
        assert!(config.has("database.migrations"));
        assert!(!config.has("nope.missing"));
    }

    #[test]
    fn a_config_file_is_namespaced_by_its_stem() {
        let mut config = Config::new();
        config.merge_toml("[cache]\nttl = 60\n", Some("services"));
        assert_eq!(config.int("services.cache.ttl"), Some(60));
    }

    #[test]
    fn dotenv_maps_to_config_keys() {
        let mut config = Config::new();
        config.merge_dotenv(
            "# comment\nexport DATABASE_URL=sqlite://x.db\nAPP__DEBUG=true\nEMPTY=\n",
        );
        assert_eq!(
            config.string("database.url").as_deref(),
            Some("sqlite://x.db")
        );
        assert_eq!(config.bool("app.debug"), Some(true));
        assert_eq!(config.string("empty").as_deref(), Some(""));
    }

    #[test]
    fn later_sources_win() {
        let mut config = Config::new();
        config.merge_toml("[database]\nurl = \"from-toml\"\n", None);
        config.merge_dotenv("DATABASE_URL=from-dotenv\n");
        assert_eq!(
            config.string("database.url").as_deref(),
            Some("from-dotenv")
        );

        // …and a registered default never overrides a real value.
        config.default_value("database.url", "from-default");
        assert_eq!(
            config.string("database.url").as_deref(),
            Some("from-dotenv")
        );
        config.default_value("database.pool", "5");
        assert_eq!(config.int("database.pool"), Some(5));
    }

    #[test]
    fn values_expand_environment_variables() {
        std::env::set_var("ELYRA_TEST_HOME", "/tmp/elyra-home");
        let mut config = Config::new();
        config.merge_toml("[storage]\nroot = \"${ELYRA_TEST_HOME}/data\"\n", None);
        assert_eq!(
            config.string("storage.root").as_deref(),
            Some("/tmp/elyra-home/data")
        );
        std::env::remove_var("ELYRA_TEST_HOME");
    }

    #[test]
    fn typed_accessors_parse() {
        let mut config = Config::new();
        config.set("server.port", "8080");
        config.set("app.debug", "off");
        assert_eq!(config.get::<u16>("server.port"), Some(8080));
        assert_eq!(config.bool("app.debug"), Some(false));
        assert_eq!(config.get_or("server.workers", 4u32), 4);
        assert_eq!(config.get::<u16>("app.debug"), None);
    }

    #[test]
    fn sections_are_extracted_with_the_prefix_stripped() {
        let mut config = Config::new();
        config.merge_toml(ELYRA_TOML, None);
        let db = config.section("database");
        assert_eq!(db.len(), 2);
        assert!(db.contains_key("url"));
        assert!(db.contains_key("migrations"));
    }

    #[test]
    fn env_keys_normalize() {
        assert_eq!(env_key_to_config_key("ELYRA_DATABASE__URL"), "database.url");
        assert_eq!(env_key_to_config_key("DATABASE_URL"), "database.url");
        assert_eq!(env_key_to_config_key("APP__NAME"), "app.name");
    }

    #[test]
    fn comments_outside_quotes_are_stripped() {
        assert_eq!(strip_comment("a = 1 # note").trim(), "a = 1");
        assert_eq!(strip_comment("a = \"x # y\"").trim(), "a = \"x # y\"");
    }

    #[test]
    fn load_reads_a_directory_tree() {
        let dir = std::env::temp_dir().join(format!("elyra-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("elyra.toml"), ELYRA_TOML).unwrap();
        std::fs::write(
            dir.join("config/services.toml"),
            "[mail]\nfrom = \"a@b.c\"\n",
        )
        .unwrap();
        std::fs::write(dir.join(".env"), "APP__DEBUG=1\n").unwrap();

        let config = Config::load(&dir);
        assert_eq!(config.string("app.crate").as_deref(), Some("elyra-example"));
        assert_eq!(
            config.string("services.mail.from").as_deref(),
            Some("a@b.c")
        );
        assert_eq!(config.bool("app.debug"), Some(true));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
