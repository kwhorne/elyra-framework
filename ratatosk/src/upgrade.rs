//! `rata upgrade` (RFC 0057): the framework's version moved everywhere the
//! app writes it, at once.
//!
//! An app names its Elyra release in `Cargo.toml` (a git dependency's
//! `tag`), in `app/package.json` (the `@elyra/runtime` and `@elyra/ui`
//! release tarballs), and, with a Laravel half, in the clone its
//! `composer.json` takes `elyracode/elyra-backend` from. `rata upgrade` reads
//! them, moves each to the version asked for, refreshes the lock files with
//! the tool that owns each one, and prints the CHANGELOG's Upgrading notes of
//! every release in between. A `path` or `file:` dependency (a checkout of
//! the framework) is left as it is.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::config::Config;
use crate::scaffold::REPO_URL;

/// The crates an app can depend on by the repository's tag.
const CRATES: &[&str] = &["elyra", "elyra-ai", "elyra-db", "elyra-macros"];
/// The packages an app can take from the release's tarballs.
const PACKAGES: &[(&str, &str)] = &[
    ("@elyra/runtime", "elyra-runtime"),
    ("@elyra/ui", "elyra-ui"),
    ("@elyra/charts", "elyra-charts"),
];
/// The Laravel package.
const BACKEND_PACKAGE: &str = "elyracode/elyra-backend";

/// A release: `0.58.0`, compared as numbers.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(u64, u64, u64);

impl Version {
    /// `0.58.0` or `v0.58.0`; anything after a `-` (a pre-release) is ignored.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim().trim_start_matches('v');
        let core = text.split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let version = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(version)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// One place the app names its release.
#[derive(Debug, PartialEq)]
pub enum Place {
    /// A `Cargo.toml`, and the crates in it on the repository's tag.
    Cargo {
        file: PathBuf,
        crates: Vec<String>,
        from: Version,
    },
    /// The frontend's `package.json`, and the packages on the release's tarballs.
    Npm {
        file: PathBuf,
        packages: Vec<String>,
        from: Version,
    },
    /// The Laravel app, and the clone of the repository it installs from.
    Laravel {
        app: PathBuf,
        clone: PathBuf,
        from: Version,
    },
}

impl Place {
    fn from(&self) -> &Version {
        match self {
            Place::Cargo { from, .. } | Place::Npm { from, .. } | Place::Laravel { from, .. } => {
                from
            }
        }
    }
}

/// What can't be moved, and why: a dependency on a checkout, a branch.
#[derive(Debug, PartialEq)]
pub struct LeftAlone {
    pub file: PathBuf,
    pub what: String,
    pub why: String,
}

/// Where the app names its release, and what it leaves alone.
#[derive(Debug, Default)]
pub struct Survey {
    pub places: Vec<Place>,
    pub left: Vec<LeftAlone>,
}

impl Survey {
    /// The release the app is on: the oldest of the places'.
    pub fn current(&self) -> Option<&Version> {
        self.places.iter().map(Place::from).min()
    }
}

/// A manifest read: the place it is (if it names a release), what it leaves
/// alone, and its new text when asked to move.
type Read = (Option<Place>, Vec<LeftAlone>, Option<String>);

// --- Cargo -------------------------------------------------------------------

/// The `[dependencies]`-like tables of a manifest: the workspace's, the
/// crate's (normal, dev, build) and each target's.
fn dependency_tables(doc: &toml_edit::DocumentMut) -> Vec<Vec<String>> {
    let kinds = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut paths: Vec<Vec<String>> = kinds.iter().map(|k| vec![(*k).to_owned()]).collect();
    paths.push(vec!["workspace".into(), "dependencies".into()]);
    if let Some(targets) = doc.get("target").and_then(|t| t.as_table_like()) {
        for (target, _) in targets.iter() {
            for kind in kinds {
                paths.push(vec!["target".into(), target.to_owned(), kind.to_owned()]);
            }
        }
    }
    paths
}

fn table_at<'a>(
    doc: &'a mut toml_edit::DocumentMut,
    path: &[String],
) -> Option<&'a mut dyn toml_edit::TableLike> {
    // Looked up, never inserted: `Item::get_mut` would add an empty table.
    let mut table: &mut dyn toml_edit::TableLike = doc.as_table_mut();
    for key in path {
        table = table.get_mut(key)?.as_table_like_mut()?;
    }
    Some(table)
}

/// Read one `Cargo.toml`: the crates on the repository's tag, and the ones in
/// another form. With `to`, also move their tags and give back the new text.
fn cargo_manifest(file: &Path, text: &str, to: Option<&Version>) -> Result<Read, String> {
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e| format!("{}: {e}", file.display()))?;
    let mut crates = Vec::new();
    let mut from: Option<Version> = None;
    let mut left = Vec::new();
    for path in dependency_tables(&doc) {
        let Some(table) = table_at(&mut doc, &path) else {
            continue;
        };
        for name in CRATES {
            let Some(dep) = table.get_mut(name).and_then(|d| d.as_table_like_mut()) else {
                continue;
            };
            let git = dep.get("git").and_then(|g| g.as_str()).map(str::to_owned);
            let tag = dep.get("tag").and_then(|t| t.as_str()).map(str::to_owned);
            let ours = git
                .as_deref()
                .is_some_and(|g| g.trim_end_matches(".git") == REPO_URL);
            match (ours, tag.as_deref().and_then(Version::parse)) {
                (true, Some(version)) => {
                    if let Some(to) = to {
                        let mut value = toml_edit::Value::from(format!("v{to}"));
                        if let Some(old) = dep.get("tag").and_then(|t| t.as_value()) {
                            *value.decor_mut() = old.decor().clone();
                        }
                        dep.insert("tag", toml_edit::Item::Value(value));
                    }
                    if !crates.iter().any(|c| c == name) {
                        crates.push((*name).to_owned());
                    }
                    from = Some(from.map_or(version.clone(), |f| f.min(version)));
                }
                _ => {
                    let why = if dep.contains_key("path") {
                        "a path to a checkout of the framework"
                    } else if dep.contains_key("branch") || dep.contains_key("rev") {
                        "a branch or a revision, not a release's tag"
                    } else if git.is_some() && !ours {
                        "another repository"
                    } else if !dep.contains_key("git") {
                        "a registry version, which names nothing: Elyra is only in its repository"
                    } else {
                        "a tag that isn't a release's"
                    };
                    left.push(LeftAlone {
                        file: file.to_owned(),
                        what: (*name).to_owned(),
                        why: why.to_owned(),
                    });
                }
            }
        }
    }
    let place = from.map(|from| Place::Cargo {
        file: file.to_owned(),
        crates,
        from,
    });
    let new = to.filter(|_| place.is_some()).map(|_| doc.to_string());
    Ok((place, left, new))
}

/// Every `Cargo.toml` of the workspace: the root's and its members'.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut walk = vec![(root.to_owned(), 0)];
    while let Some((dir, depth)) = walk.pop() {
        let manifest = dir.join("Cargo.toml");
        if manifest.is_file() {
            found.push(manifest);
        }
        if depth == 3 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if entry.path().is_dir()
                && !name.starts_with('.')
                && !["target", "node_modules", "vendor", "dist"].contains(&name.as_ref())
            {
                walk.push((entry.path(), depth + 1));
            }
        }
    }
    found.sort();
    found
}

// --- npm ---------------------------------------------------------------------

/// The release tarball's URL for a package, at `version`.
fn tarball(file_stem: &str, version: &Version) -> String {
    format!("{REPO_URL}/releases/download/v{version}/{file_stem}-{version}.tgz")
}

/// The version in a release tarball's URL, if it is one.
fn tarball_version(url: &str, file_stem: &str) -> Option<Version> {
    let rest = url.strip_prefix(&format!("{REPO_URL}/releases/download/v"))?;
    let (tag, file) = rest.split_once('/')?;
    let version = Version::parse(tag)?;
    (file == format!("{file_stem}-{version}.tgz")).then_some(version)
}

/// Read the frontend's `package.json`, as [`cargo_manifest`] does a
/// `Cargo.toml`. Only the URLs' text changes: its order and indentation stay.
fn npm_manifest(file: &Path, text: &str, to: Option<&Version>) -> Result<Read, String> {
    let json: Value = serde_json::from_str(text).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut packages = Vec::new();
    let mut from: Option<Version> = None;
    let mut left = Vec::new();
    let mut new = text.to_owned();
    for (package, stem) in PACKAGES {
        for section in ["dependencies", "devDependencies"] {
            let Some(spec) = json[section][package].as_str() else {
                continue;
            };
            match tarball_version(spec, stem) {
                Some(version) => {
                    if let Some(to) = to {
                        new = new.replace(
                            &format!("\"{spec}\""),
                            &format!("\"{}\"", tarball(stem, to)),
                        );
                    }
                    packages.push((*package).to_owned());
                    from = Some(from.map_or(version.clone(), |f| f.min(version)));
                }
                None => left.push(LeftAlone {
                    file: file.to_owned(),
                    what: (*package).to_owned(),
                    why: if spec.starts_with("file:") || spec.starts_with("link:") {
                        "a link to a checkout of the framework".into()
                    } else {
                        format!("`{spec}`, not a release's tarball")
                    },
                }),
            }
        }
    }
    let place = from.map(|from| Place::Npm {
        file: file.to_owned(),
        packages,
        from,
    });
    let new = to.filter(|_| place.is_some()).map(|_| new);
    Ok((place, left, new))
}

// --- Laravel -----------------------------------------------------------------

/// The clone a Laravel app's `composer.json` takes the package from: its
/// `path` repository whose folder is the repository's `laravel/`.
fn laravel_clone(app: &Path) -> Option<PathBuf> {
    let composer: Value =
        serde_json::from_str(&std::fs::read_to_string(app.join("composer.json")).ok()?).ok()?;
    let repositories = &composer["repositories"];
    let entries: Vec<&Value> = match repositories {
        Value::Array(list) => list.iter().collect(),
        Value::Object(map) => map.values().collect(),
        _ => Vec::new(),
    };
    entries.into_iter().find_map(|repo| {
        let url = repo["url"].as_str()?;
        if repo["type"].as_str() != Some("path") {
            return None;
        }
        let folder = app.join(url);
        folder
            .join("composer.json")
            .is_file()
            .then(|| std::fs::read_to_string(folder.join("composer.json")).ok())
            .flatten()
            .filter(|text| text.contains(&format!("\"{BACKEND_PACKAGE}\"")))
            .and_then(|_| folder.parent().map(Path::to_owned))
    })
}

/// The release tag the clone is checked out at.
fn clone_version(clone: &Path) -> Option<Version> {
    let out = Command::new("git")
        .args(["describe", "--tags", "--exact-match"])
        .current_dir(clone)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| Version::parse(String::from_utf8_lossy(&out.stdout).trim()))
        .flatten()
}

// --- The survey and the plan ---------------------------------------------------

/// Where the app at `cfg.root` names its release.
pub fn survey(cfg: &Config) -> Result<Survey, String> {
    let mut survey = Survey::default();
    for file in manifests(&cfg.root) {
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let (place, left, _) = cargo_manifest(&file, &text, None)?;
        survey.places.extend(place);
        survey.left.extend(left);
    }
    let package = cfg.root.join(&cfg.frontend_dir).join("package.json");
    if let Ok(text) = std::fs::read_to_string(&package) {
        let (place, left, _) = npm_manifest(&package, &text, None)?;
        survey.places.extend(place);
        survey.left.extend(left);
    }
    if let Some(app) = cfg
        .check_backend
        .as_deref()
        .map(|b| cfg.root.join(b))
        .filter(|p| p.is_dir())
    {
        match laravel_clone(&app).and_then(|clone| Some((clone_version(&clone)?, clone))) {
            Some((from, clone)) => survey.places.push(Place::Laravel { app, clone, from }),
            None => survey.left.push(LeftAlone {
                file: app.join("composer.json"),
                what: BACKEND_PACKAGE.into(),
                why: "not from a clone of the repository at a release's tag".into(),
            }),
        }
    }
    Ok(survey)
}

/// What `rata upgrade` would do, as it prints it.
pub fn plan_text(survey: &Survey, to: &Version, root: &Path) -> String {
    let shown = |p: &Path| p.strip_prefix(root).unwrap_or(p).display().to_string();
    let from = survey
        .current()
        .map_or_else(|| "?".to_owned(), ToString::to_string);
    let mut out = format!("elyra {from} → {to}\n");
    for place in &survey.places {
        let line = match place {
            Place::Cargo { file, crates, from } => {
                format!(
                    "{:<20}{:<28}tag v{from} → v{to}",
                    shown(file),
                    crates.join(", ")
                )
            }
            Place::Npm {
                file,
                packages,
                from,
            } => {
                format!(
                    "{:<20}{:<28}{from} → {to}",
                    shown(file),
                    packages.join(", ")
                )
            }
            Place::Laravel { app, from, .. } => {
                format!(
                    "{:<20}{:<28}v{from} → v{to}",
                    format!("{} (Laravel)", shown(app)),
                    BACKEND_PACKAGE
                )
            }
        };
        out.push_str(&format!("  {}\n", line.trim_end()));
    }
    for left in &survey.left {
        out.push_str(&format!(
            "  left alone: {} in {}: {}\n",
            left.what,
            shown(&left.file),
            left.why
        ));
    }
    let steps: Vec<String> = survey
        .places
        .iter()
        .map(|p| match p {
            Place::Cargo { crates, .. } => format!(
                "cargo update {}",
                crates
                    .iter()
                    .map(|c| format!("-p {c}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            Place::Npm { .. } => "npm install".into(),
            Place::Laravel { .. } => format!("composer update {BACKEND_PACKAGE}"),
        })
        .collect();
    if !steps.is_empty() {
        out.push_str(&format!("  {}\n", steps.join(" · ")));
    }
    out
}

// --- The Upgrading notes --------------------------------------------------------

/// The **Upgrading** paragraph of each release after `from`, up to and
/// including `to`, newest first, as the CHANGELOG has them.
pub fn upgrading_notes(changelog: &str, from: &Version, to: &Version) -> Vec<(Version, String)> {
    let mut notes = Vec::new();
    let mut sections = changelog.split("\n## [").skip(1);
    for section in sections.by_ref() {
        let Some((head, body)) = section.split_once('\n') else {
            continue;
        };
        let Some(version) = head.split(']').next().and_then(Version::parse) else {
            continue; // [Unreleased]
        };
        if &version <= from || &version > to {
            continue;
        }
        let Some(at) = body.find("**Upgrading") else {
            continue;
        };
        let rest = &body[at..];
        let end = rest
            .find("\n### ")
            .or_else(|| rest.find("\n## "))
            .unwrap_or(rest.len());
        notes.push((version, rest[..end].trim().to_owned()));
    }
    notes
}

/// The CHANGELOG at `version`'s tag, from the repository.
fn fetch_changelog(version: &Version) -> Option<String> {
    let raw = REPO_URL.replace("https://github.com/", "https://raw.githubusercontent.com/");
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "15",
            &format!("{raw}/v{version}/CHANGELOG.md"),
        ])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// --- The releases there are -------------------------------------------------------

/// The repository's release tags.
fn releases() -> Result<Vec<Version>, String> {
    let out = Command::new("git")
        .args(["ls-remote", "--tags", "--refs", REPO_URL])
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "couldn't list the releases: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let mut versions: Vec<Version> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split("refs/tags/").nth(1))
        .filter_map(Version::parse)
        .collect();
    versions.sort();
    Ok(versions)
}

// --- Doing it -----------------------------------------------------------------------

fn run_step(what: &str, dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    println!("  {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("{what}: failed to run {program}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "{what} failed in {}; the edits so far are kept, and `rata upgrade` again finishes the job",
            dir.display()
        ))
    }
}

/// Move every place in `survey` to `to`: the files, then each tool's lock.
fn apply(cfg: &Config, survey: &Survey, to: &Version) -> Result<(), String> {
    for place in &survey.places {
        match place {
            Place::Cargo { file, .. } => {
                let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
                if let (_, _, Some(new)) = cargo_manifest(file, &text, Some(to))? {
                    std::fs::write(file, new).map_err(|e| format!("{}: {e}", file.display()))?;
                }
            }
            Place::Npm { file, .. } => {
                let text = std::fs::read_to_string(file).map_err(|e| e.to_string())?;
                if let (_, _, Some(new)) = npm_manifest(file, &text, Some(to))? {
                    std::fs::write(file, new).map_err(|e| format!("{}: {e}", file.display()))?;
                }
            }
            Place::Laravel { .. } => {}
        }
    }
    for place in &survey.places {
        match place {
            Place::Cargo { crates, .. } => {
                let mut args = vec!["update"];
                for c in crates {
                    args.extend(["-p", c.as_str()]);
                }
                run_step("cargo update", &cfg.root, "cargo", &args)?;
            }
            Place::Npm { file, .. } => {
                let dir = file.parent().unwrap_or(&cfg.root);
                run_step("npm install", dir, "npm", &["install"])?;
            }
            Place::Laravel { app, clone, .. } => {
                let tag = format!("v{to}");
                run_step(
                    "git fetch",
                    clone,
                    "git",
                    &["fetch", "--depth", "1", "origin", "tag", &tag],
                )?;
                run_step("git checkout", clone, "git", &["checkout", "--quiet", &tag])?;
                run_step(
                    "composer update",
                    app,
                    "composer",
                    &["update", BACKEND_PACKAGE],
                )?;
            }
        }
    }
    Ok(())
}

/// `rata upgrade [<version>] [--dry-run]`.
pub fn upgrade(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let dry = args.iter().any(|a| a == "--dry-run");
    let named = args.iter().find(|a| !a.starts_with("--"));
    let rata = Version::parse(env!("CARGO_PKG_VERSION")).ok_or("rata's own version")?;
    let to = match named {
        Some(v) => Version::parse(v).ok_or_else(|| format!("`{v}` isn't a version (0.58.0)"))?,
        None => rata.clone(),
    };

    let known = releases().ok();
    if let Some(known) = &known {
        if !known.contains(&to) {
            return Err(format!("there's no release v{to} in {REPO_URL}"));
        }
        if let Some(newest) = known.last().filter(|n| *n > &to) {
            println!("(v{newest} is the newest release: `rata upgrade {newest}`)");
        }
    }
    if to > rata {
        return Err(format!(
            "rata is {rata}; {to}'s codegen and checks come with rata {to}:\n  \
             cargo install --git {REPO_URL} --tag v{to} ratatosk --force"
        ));
    }

    let survey = survey(cfg)?;
    let Some(from) = survey.current().cloned() else {
        return Err(
            "the app names no release of Elyra: no git dependency on its tag, no release tarball"
                .into(),
        );
    };
    print!("{}", plan_text(&survey, &to, &cfg.root));
    if cfg.check_backend.is_none() {
        println!(
            "  (a Laravel half: `[check] backend = \"../web\"` in elyra.toml moves it too; by hand,\n   \
             check its clone out at v{to} and run `composer update {BACKEND_PACKAGE}`)"
        );
    }

    if from < to {
        match fetch_changelog(&to) {
            Some(changelog) => {
                for (version, note) in upgrading_notes(&changelog, &from, &to) {
                    println!("\nUpgrading ({version}):\n{note}");
                }
            }
            None => println!(
                "\nUpgrading: see {REPO_URL}/blob/v{to}/CHANGELOG.md for each release after {from}"
            ),
        }
    }

    if dry {
        println!("\n(--dry-run: nothing changed)");
        return Ok(());
    }
    if survey.places.iter().all(|p| p.from() == &to) {
        println!("\nAlready on {to}.");
        return Ok(());
    }
    println!();
    apply(cfg, &survey, &to)?;
    println!("\nOn {to}. `rata check` says whether the app agrees.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(v("0.10.0") > v("0.9.9"));
        assert_eq!(v("v0.58.0"), v("0.58.0"));
        assert_eq!(v("0.58.0-rc.1"), v("0.58.0"));
        assert_eq!(Version::parse("0.58"), None);
        assert_eq!(Version::parse("main"), None);
    }

    const CARGO: &str = r#"[package]
name = "kver"

[dependencies]
# The framework, at its release.
elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.57.0", features = ["database"] } # keep
elyra-ai = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.57.0" }
serde = "1"

[dev-dependencies]
elyra-db = { path = "../elyra-framework/database" }

[target.'cfg(windows)'.dependencies]
elyra-macros = { git = "https://github.com/kwhorne/elyra-framework", branch = "main" }
"#;

    #[test]
    fn a_manifest_has_its_tags_moved_and_nothing_else() {
        let file = Path::new("Cargo.toml");
        let (place, left, new) = cargo_manifest(file, CARGO, Some(&v("0.58.0"))).unwrap();
        assert_eq!(
            place,
            Some(Place::Cargo {
                file: file.into(),
                crates: vec!["elyra".into(), "elyra-ai".into()],
                from: v("0.57.0"),
            })
        );
        let new = new.unwrap();
        assert_eq!(new, CARGO.replace("tag = \"v0.57.0\"", "tag = \"v0.58.0\""));
        assert!(new.contains("# The framework, at its release."));
        let left: Vec<(&str, &str)> = left
            .iter()
            .map(|l| (l.what.as_str(), l.why.as_str()))
            .collect();
        assert_eq!(
            left,
            [
                ("elyra-db", "a path to a checkout of the framework"),
                (
                    "elyra-macros",
                    "a branch or a revision, not a release's tag"
                ),
            ]
        );
    }

    #[test]
    fn a_workspace_dependency_is_moved_too() {
        let text = r#"[workspace]
members = ["app-rs"]

[workspace.dependencies]
elyra = { git = "https://github.com/kwhorne/elyra-framework.git", tag = "v0.56.1" }
"#;
        let (place, _, new) =
            cargo_manifest(Path::new("Cargo.toml"), text, Some(&v("0.58.0"))).unwrap();
        assert!(matches!(place, Some(Place::Cargo { from, .. }) if from == v("0.56.1")));
        assert!(new.unwrap().contains("tag = \"v0.58.0\""));
    }

    const PACKAGE: &str = r#"{
  "name": "app",
  "dependencies": {
    "@elyra/runtime": "https://github.com/kwhorne/elyra-framework/releases/download/v0.57.0/elyra-runtime-0.57.0.tgz",
    "@elyra/ui": "https://github.com/kwhorne/elyra-framework/releases/download/v0.57.0/elyra-ui-0.57.0.tgz",
    "@elyra/charts": "https://github.com/kwhorne/elyra-framework/releases/download/v0.57.0/elyra-charts-0.57.0.tgz",
    "svelte": "^5.0.0"
  }
}
"#;

    #[test]
    fn the_tarballs_are_moved_in_place() {
        let file = Path::new("app/package.json");
        let (place, left, new) = npm_manifest(file, PACKAGE, Some(&v("0.58.0"))).unwrap();
        assert!(left.is_empty());
        assert!(matches!(place, Some(Place::Npm { ref packages, .. }) if packages.len() == 3));
        assert_eq!(new.unwrap(), PACKAGE.replace("0.57.0", "0.58.0"));
    }

    #[test]
    fn a_linked_checkout_is_left_alone() {
        let text =
            r#"{ "dependencies": { "@elyra/runtime": "file:../../elyra-framework/runtime" } }"#;
        let (place, left, new) =
            npm_manifest(Path::new("package.json"), text, Some(&v("0.58.0"))).unwrap();
        assert_eq!(place, None);
        assert_eq!(new, None);
        assert_eq!(left[0].why, "a link to a checkout of the framework");
    }

    #[test]
    fn a_tarball_of_another_version_than_its_tag_is_not_a_release() {
        let url = format!("{REPO_URL}/releases/download/v0.57.0/elyra-ui-0.56.0.tgz");
        assert_eq!(tarball_version(&url, "elyra-ui"), None);
        assert_eq!(
            tarball_version(&tarball("elyra-ui", &v("0.57.0")), "elyra-ui"),
            Some(v("0.57.0"))
        );
    }

    const CHANGELOG: &str = "# Changelog

## [Unreleased]

## [0.58.0] — 2026-12-01

**Upgrading:** the words.

- One.

### Added

- Something.

## [0.57.1] — 2026-11-28

### Fixed

- Nothing to upgrade.

## [0.57.0] — 2026-11-26

**Upgrading:** menus.

### Added
";

    #[test]
    fn the_notes_are_each_release_after_the_old_one_up_to_the_new() {
        let notes = upgrading_notes(CHANGELOG, &v("0.56.0"), &v("0.58.0"));
        assert_eq!(
            notes,
            [
                (
                    v("0.58.0"),
                    "**Upgrading:** the words.\n\n- One.".to_owned()
                ),
                (v("0.57.0"), "**Upgrading:** menus.".to_owned()),
            ]
        );
        assert!(upgrading_notes(CHANGELOG, &v("0.58.0"), &v("0.58.0")).is_empty());
        assert_eq!(
            upgrading_notes(CHANGELOG, &v("0.57.0"), &v("0.58.0")).len(),
            1
        );
    }

    #[test]
    fn the_plan_says_each_place_and_what_runs() {
        let survey = Survey {
            places: vec![
                Place::Cargo {
                    file: "/a/Cargo.toml".into(),
                    crates: vec!["elyra".into()],
                    from: v("0.57.0"),
                },
                Place::Npm {
                    file: "/a/app/package.json".into(),
                    packages: vec!["@elyra/runtime".into(), "@elyra/ui".into()],
                    from: v("0.57.0"),
                },
            ],
            left: vec![],
        };
        let text = plan_text(&survey, &v("0.58.0"), Path::new("/a"));
        assert!(text.starts_with("elyra 0.57.0 → 0.58.0\n"), "{text}");
        assert!(
            text.contains("Cargo.toml          elyra                       tag v0.57.0 → v0.58.0"),
            "{text}"
        );
        assert!(
            text.contains("app/package.json    @elyra/runtime, @elyra/ui   0.57.0 → 0.58.0"),
            "{text}"
        );
        assert!(
            text.contains("cargo update -p elyra · npm install"),
            "{text}"
        );
    }

    #[test]
    fn a_laravel_app_installs_from_its_clone() {
        let dir = std::env::temp_dir().join(format!("rata-upgrade-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("web")).unwrap();
        std::fs::create_dir_all(dir.join("elyra-framework/laravel")).unwrap();
        std::fs::write(
            dir.join("elyra-framework/laravel/composer.json"),
            r#"{ "name": "elyracode/elyra-backend" }"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("web/composer.json"),
            r#"{ "repositories": { "elyra": { "type": "path", "url": "../elyra-framework/laravel" } } }"#,
        )
        .unwrap();
        let clone = laravel_clone(&dir.join("web")).unwrap();
        assert_eq!(
            clone.canonicalize().unwrap(),
            dir.join("elyra-framework").canonicalize().unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
