//! `rata check` (RFC 0036): the app checked as a whole, for CI and coding
//! agents.
//!
//! The app reports its shape in an inspect mode (`ELYRA_CHECK`): each
//! command with its ability, whether it's a read (live), and who it's
//! granted to; the frontend's grants; the startup problems; the bindings
//! codegen would write. rata reads the frontend's sources for the commands
//! and abilities they name, and [`findings`] turns it all into what's
//! wrong, why it matters and how to fix it.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

use serde::Serialize;
use serde_json::Value;

use crate::config::{Allow, Config};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    fn word(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warning => "warning",
            Level::Info => "info",
        }
    }
}

/// One thing `rata check` found.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub check: &'static str,
    pub level: Level,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ability: Option<String>,
    pub message: String,
    pub fix: String,
}

/// A finding the app said no to, with its reason.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Allowed {
    #[serde(flatten)]
    pub finding: Finding,
    pub why: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub allowed: Vec<Allowed>,
}

impl Report {
    pub fn count(&self, level: Level) -> usize {
        self.findings.iter().filter(|f| f.level == level).count()
    }
}

/// What the frontend's sources name: words (command names among them) and
/// quoted strings (abilities among them).
#[derive(Debug, Default)]
pub struct Frontend {
    words: HashSet<String>,
    strings: HashSet<String>,
}

impl Frontend {
    /// Read `text`'s identifiers and its quoted strings.
    pub fn read(&mut self, text: &str) {
        let mut word = String::new();
        for c in text.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                word.push(c);
            } else if !word.is_empty() {
                self.words.insert(std::mem::take(&mut word));
            }
        }
        if !word.is_empty() {
            self.words.insert(word);
        }
        for quote in ['"', '\'', '`'] {
            let mut parts = text.split(quote);
            parts.next();
            // Every other part is inside the quotes.
            while let Some(inside) = parts.next() {
                if inside.len() <= 200 {
                    self.strings.insert(inside.to_owned());
                }
                parts.next();
            }
        }
    }

    fn calls(&self, command: &str) -> bool {
        self.words.contains(command)
    }

    fn names(&self, ability: &str) -> bool {
        self.strings.contains(ability)
    }
}

/// What's wrong with the app `shape` (from `ELYRA_CHECK`), its frontend and
/// its bindings file (`None` when there's none), with `allow`'s findings
/// set apart.
#[cfg_attr(not(test), allow(dead_code))]
pub fn findings(
    shape: &Value,
    frontend: &Frontend,
    bindings_path: &str,
    bindings: Option<&str>,
    allow: &[Allow],
) -> Report {
    findings_with(shape, frontend, bindings_path, bindings, allow, Vec::new())
}

/// [`findings`], with `more` found elsewhere (the backend's, RFC 0040),
/// held to the same `allow`.
pub fn findings_with(
    shape: &Value,
    frontend: &Frontend,
    bindings_path: &str,
    bindings: Option<&str>,
    allow: &[Allow],
    more: Vec<Finding>,
) -> Report {
    let mut found = more;
    let mut push = |check,
                    level,
                    command: Option<&str>,
                    ability: Option<&str>,
                    message: String,
                    fix: String| {
        found.push(Finding {
            check,
            level,
            command: command.map(str::to_owned),
            ability: ability.map(str::to_owned),
            message,
            fix,
        });
    };

    for problem in shape["problems"].as_array().into_iter().flatten() {
        push(
            "startup-problem",
            Level::Error,
            None,
            None,
            problem.as_str().unwrap_or_default().to_owned(),
            "The app won't start until it's fixed; `cargo run` says the same.".into(),
        );
    }

    match (shape["bindings"]["ts"].as_str(), bindings) {
        (Some(want), Some(have)) if want == have => {}
        (Some(_), Some(_)) => push(
            "bindings-stale",
            Level::Error,
            None,
            None,
            format!(
                "{bindings_path} isn't what `rata codegen` writes for the commands as they are."
            ),
            "Run `rata codegen`.".into(),
        ),
        (Some(_), None) => push(
            "bindings-stale",
            Level::Error,
            None,
            None,
            format!("{bindings_path} doesn't exist."),
            "Run `rata codegen`.".into(),
        ),
        (None, _) => push(
            "bindings-stale",
            Level::Error,
            None,
            None,
            format!(
                "codegen fails: {}",
                shape["bindings"]["error"]
                    .as_str()
                    .unwrap_or("for no reason given")
            ),
            "Fix what it says, then run `rata codegen`.".into(),
        ),
    }

    for command in shape["commands"].as_array().into_iter().flatten() {
        let name = command["name"].as_str().unwrap_or_default();
        let ability = command["ability"].as_str();
        let live = command["live"] == true;
        let assistant = &command["assistant"];
        let mcp = &command["mcp"];
        let reached = command["frontend"] == true || assistant.is_object() || mcp.is_object();

        if let Some(ability) = ability {
            if !reached {
                push(
                    "ability-not-granted",
                    Level::Error,
                    Some(name),
                    Some(ability),
                    format!(
                        "{name} needs `{ability}`, which the app grants nowhere: the frontend, \
                         the assistant and MCP all refuse it."
                    ),
                    format!(
                        "Grant it with App::allow_ability(\"{ability}\") (Laravel's abilities \
                         only narrow it per user), or to the assistant or MCP."
                    ),
                );
            }
        } else if !live {
            push(
                "write-without-ability",
                Level::Warning,
                Some(name),
                None,
                format!(
                    "{name} needs no ability and isn't a live query, so it may write: any page \
                     script can call it."
                ),
                "Give it #[command(can = \"…\")] and grant that ability, or allow it in \
                 elyra.toml's [check] with the reason."
                    .into(),
            );
        }
        if !live {
            for (check, who, unasked, fix) in [
                (
                    "assistant-writes-unasked",
                    "The assistant",
                    assistant,
                    "Assistant::confirm",
                ),
                ("mcp-writes-unasked", "An MCP agent", mcp, "Mcp::confirm"),
            ] {
                if unasked["confirm"] == false {
                    let ability = ability.unwrap_or_default();
                    push(
                        check,
                        Level::Warning,
                        Some(name),
                        Some(ability),
                        format!("{who} can call {name} without asking the user."),
                        format!("Put it behind {fix}(\"{ability}\")."),
                    );
                }
            }
        }
        if !frontend.calls(name) && !assistant.is_object() && !mcp.is_object() {
            push(
                "uncalled",
                Level::Info,
                Some(name),
                None,
                format!(
                    "{name} isn't called by the frontend, and the assistant and MCP can't reach it."
                ),
                "Remove it if nothing needs it, or allow it with the reason (Rust code may call \
                 it)."
                    .into(),
            );
        }
    }

    for grant in shape["grants"].as_array().into_iter().flatten() {
        let pattern = grant["pattern"].as_str().unwrap_or_default();
        if grant["used"] == true || grant["wildcard"] == true || frontend.names(pattern) {
            continue;
        }
        push(
            "granted-unused",
            Level::Warning,
            None,
            Some(pattern),
            format!("The app grants `{pattern}`, but no command or page asks for it."),
            "Remove it, or correct it to the ability it was meant for.".into(),
        );
    }

    let mut report = Report::default();
    let mut used = vec![false; allow.len()];
    for finding in found {
        let matched = allow.iter().position(|a| {
            a.check == finding.check
                && a.command
                    .as_ref()
                    .is_none_or(|c| Some(c) == finding.command.as_ref())
                && a.ability
                    .as_ref()
                    .is_none_or(|x| Some(x) == finding.ability.as_ref())
        });
        match matched {
            Some(at) => {
                used[at] = true;
                report.allowed.push(Allowed {
                    finding,
                    why: allow[at].why.clone(),
                });
            }
            None => report.findings.push(finding),
        }
    }
    for (a, used) in allow.iter().zip(used) {
        if !used {
            let what = [a.command.as_deref(), a.ability.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ");
            report.findings.push(Finding {
                check: "allow-unused",
                level: Level::Warning,
                command: a.command.clone(),
                ability: a.ability.clone(),
                message: format!(
                    "[check] allows {}{} but nothing is found for it.",
                    a.check,
                    if what.is_empty() {
                        String::new()
                    } else {
                        format!(" for {what}")
                    }
                ),
                fix: "Remove it from elyra.toml: what it was for is gone.".into(),
            });
        }
    }
    report.findings.sort_by_key(|f| f.level);
    report
}

/// The report as `rata check` prints it.
pub fn text(report: &Report) -> String {
    let mut out = String::new();
    for f in &report.findings {
        out.push_str(&format!(
            "{:<9}{}\n{:<9}{}\n",
            f.level.word(),
            f.message,
            "",
            f.fix
        ));
    }
    if !report.allowed.is_empty() {
        out.push_str("\nAllowed in elyra.toml:\n");
        for a in &report.allowed {
            out.push_str(&format!(
                "- {}: {} ({})\n",
                a.finding.check, a.finding.message, a.why
            ));
        }
    }
    let (errors, warnings, notes) = (
        report.count(Level::Error),
        report.count(Level::Warning),
        report.count(Level::Info),
    );
    if errors + warnings + notes == 0 {
        out.push_str(if report.allowed.is_empty() {
            "No findings.\n"
        } else {
            "\nNo other findings.\n"
        });
    } else {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        out.push_str(&format!(
            "\n{}, {}, {}.\n",
            plural(errors, "error", "errors"),
            plural(warnings, "warning", "warnings"),
            plural(notes, "note", "notes")
        ));
    }
    out
}

/// Run the app in its check mode, read the frontend and the bindings, and
/// find what's wrong. The app's output goes to stderr, so `rata mcp dev`'s
/// stdout stays its own.
pub fn run(cfg: &Config) -> Result<Report, String> {
    run_with(cfg, cfg.check_backend.as_deref())
}

/// [`run`], with the server's contract from `backend` (RFC 0040): a file
/// `php artisan elyra:contract` wrote, or the Laravel app's folder, where
/// it's run as `[check] backend_as`.
pub fn run_with(cfg: &Config, backend: Option<&str>) -> Result<Report, String> {
    let out = std::env::temp_dir().join(format!("elyra-check-{}.json", std::process::id()));
    let output = Command::new("cargo")
        .args(["run", "--quiet", "-p", &cfg.app_crate])
        .env("ELYRA_CHECK", &out)
        .current_dir(&cfg.root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run cargo: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        return Err(format!(
            "the app didn't build or run:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    let text = std::fs::read_to_string(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _ = std::fs::remove_file(&out);
    let shape: Value = serde_json::from_str(&text).map_err(|e| format!("the app's shape: {e}"))?;

    let mut frontend = Frontend::default();
    let bindings_file = cfg.root.join(&cfg.codegen_out);
    read_sources(
        &cfg.root.join(&cfg.frontend_dir).join("src"),
        &bindings_file,
        &mut frontend,
    );
    let bindings = std::fs::read_to_string(&bindings_file).ok();
    let mut more = match backend {
        Some(backend) => {
            let contract = contract_of(cfg, backend)?;
            crate::contract::findings(&shape, &backend_calls(cfg), &contract)
        }
        None => Vec::new(),
    };
    more.extend(document_findings(&shape, &cfg.bundle_documents));
    more.extend(word_findings(&shape));
    Ok(findings_with(
        &shape,
        &frontend,
        &cfg.codegen_out,
        bindings.as_deref(),
        &cfg.check_allow,
        more,
    ))
}

/// The framework's words a language of the app's leaves in English (RFC
/// 0056): `@elyra/ui`'s, the runtime's panels' and the menus', which the
/// framework ships in English and Norwegian only. An app on an Elyra
/// before 0.58 says nothing of them, and nothing is found.
pub fn word_findings(shape: &Value) -> Vec<Finding> {
    let Some(locales) = shape["untranslated"].as_object() else {
        return Vec::new();
    };
    locales
        .iter()
        .filter_map(|(locale, missing)| {
            let missing: Vec<&str> = missing
                .as_array()?
                .iter()
                .filter_map(Value::as_str)
                .collect();
            if missing.is_empty() {
                return None;
            }
            let shown: Vec<&str> = missing.iter().take(3).copied().collect();
            let more = if missing.len() > shown.len() {
                ", …"
            } else {
                ""
            };
            Some(Finding {
                check: "framework-words-untranslated",
                level: Level::Warning,
                command: None,
                ability: None,
                message: format!(
                    "lang/{locale}.json: {} of elyra's words fall back to English ({}{more})",
                    missing.len(),
                    shown.join(", "),
                ),
                fix: format!(
                    "add them to lang/{locale}.json; every key is in elyra's framework/lang/en.json"
                ),
            })
        })
        .collect()
}

/// `[[bundle.documents]]` beside what the app `opens` (RFC 0052): a type
/// the OS offers the app for but the app turns away, and one it takes that
/// the OS never offers. An app on an Elyra before 0.54 says nothing of
/// what it opens, and nothing is found.
pub fn document_findings(
    shape: &Value,
    declared: &[crate::documents::DocumentType],
) -> Vec<Finding> {
    let Some(opens) = shape["opens"].as_array() else {
        return Vec::new();
    };
    let opens: Vec<&str> = opens.iter().filter_map(Value::as_str).collect();
    let mut found = Vec::new();
    for doc in declared {
        for ext in &doc.extensions {
            if !opens.contains(&ext.as_str()) {
                found.push(Finding {
                    check: "document-not-opened",
                    level: Level::Error,
                    command: None,
                    ability: None,
                    message: format!(
                        "[[bundle.documents]] \"{}\" registers .{ext}, which the app doesn't open: \
                         the OS offers the app, and the file is turned away",
                        doc.name
                    ),
                    fix: format!("add \"{ext}\" to App::opens([..]), or remove it from elyra.toml"),
                });
            }
        }
    }
    for ext in opens {
        if !declared
            .iter()
            .any(|d| d.extensions.iter().any(|e| e == ext))
        {
            found.push(Finding {
                check: "document-not-registered",
                level: Level::Warning,
                command: None,
                ability: None,
                message: format!(
                    "the app opens .{ext}, which no [[bundle.documents]] registers: \
                     only the command line hands it over"
                ),
                fix: format!(
                    "add [[bundle.documents]] with extensions = [\"{ext}\"] to elyra.toml"
                ),
            });
        }
    }
    found
}

/// The app's calls to the backend, read from the app crate's own folder: in
/// a workspace, the member that is the app.
fn backend_calls(cfg: &Config) -> BTreeMap<String, Vec<crate::contract::Call>> {
    crate::contract::calls(&crate::mcp_dev::app_dir(cfg).join("src"))
}

/// The server's contract: the file at `backend`, or what
/// `php artisan elyra:contract` says in the Laravel app there.
fn contract_of(cfg: &Config, backend: &str) -> Result<Value, String> {
    let path = cfg.root.join(backend);
    let text = if path.is_dir() {
        let mut command = Command::new("php");
        command
            .args(["artisan", "elyra:contract"])
            .current_dir(&path);
        match &cfg.check_backend_as {
            Some(user) => {
                command.arg(format!("--as={user}"));
            }
            None => eprintln!(
                "rata check: no [check] backend_as, so the server's answers aren't sampled"
            ),
        }
        let output = command
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("failed to run php artisan elyra:contract: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "php artisan elyra:contract failed in {}: {}",
                path.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        // What it says beside the contract (a FormRequest whose rules
        // couldn't be made) is the user's to read.
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?
    };
    serde_json::from_str(&text).map_err(|e| format!("the server's contract: {e}"))
}

/// Every `.svelte`, `.ts` and `.js` under `dir`, but the bindings.
fn read_sources(dir: &Path, bindings: &Path, frontend: &mut Frontend) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name != "node_modules" && !name.starts_with('.') {
                read_sources(&path, bindings, frontend);
            }
        } else if [".svelte", ".ts", ".js", ".mjs"]
            .iter()
            .any(|e| name.ends_with(e))
            && path != bindings
        {
            if let Ok(text) = std::fs::read_to_string(&path) {
                frontend.read(&text);
            }
        }
    }
}

/// `rata check [--json] [--strict]`.
pub fn check(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    let backend = args
        .iter()
        .position(|a| a == "--backend")
        .map(|at| {
            args.get(at + 1)
                .cloned()
                .ok_or("--backend needs the contract's file or the Laravel app's folder")
        })
        .transpose()?
        .or_else(|| cfg.check_backend.clone());
    let report = run_with(cfg, backend.as_deref())?;
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        print!("{}", text(&report));
    }
    let strict = args.iter().any(|a| a == "--strict");
    let errors = report.count(Level::Error);
    let warnings = report.count(Level::Warning);
    if errors > 0 || (strict && warnings > 0) {
        return Err(format!(
            "rata check found {errors} error(s){}",
            if strict {
                format!(" and {warnings} warning(s)")
            } else {
                String::new()
            }
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shape() -> Value {
        json!({
            "app": "CRM",
            "commands": [
                { "name": "customers_index", "ability": "customers.view", "live": true, "frontend": true, "assistant": { "confirm": false }, "mcp": null },
                { "name": "customers_destroy", "ability": "customers.delete", "live": false, "frontend": false, "assistant": null, "mcp": null },
                { "name": "notes_purge", "ability": null, "live": false, "frontend": true, "assistant": null, "mcp": null },
                { "name": "invoices_send", "ability": "invoices.send", "live": false, "frontend": true, "assistant": { "confirm": false }, "mcp": { "confirm": true } },
                { "name": "reports_rebuild", "ability": "reports.build", "live": false, "frontend": true, "assistant": null, "mcp": null },
                { "name": "export_csv", "ability": "reports.export", "live": false, "frontend": false, "assistant": null, "mcp": { "confirm": true } },
            ],
            "grants": [
                { "pattern": "customers.view", "used": true, "wildcard": false },
                { "pattern": "custmers.delete", "used": false, "wildcard": false },
                { "pattern": "reports.*", "used": true, "wildcard": true },
                { "pattern": "settings.open", "used": false, "wildcard": false },
            ],
            "problems": ["Backend::transcription() needs elyra's `ai` feature"],
            "bindings": { "ts": "// bindings\n" },
        })
    }

    fn frontend() -> Frontend {
        let mut f = Frontend::default();
        f.read(r#"<Button can="settings.open" onclick={() => api.notes_purge()}>Purge</Button>
            const rows = live.customers_index(); await api.customers_destroy(id); api.invoices_send(7);"#);
        f
    }

    fn checks(report: &Report) -> Vec<(&'static str, Option<&str>)> {
        report
            .findings
            .iter()
            .map(|f| (f.check, f.command.as_deref().or(f.ability.as_deref())))
            .collect()
    }

    #[test]
    fn finds_what_is_wrong_between_the_pieces() {
        let report = findings(
            &shape(),
            &frontend(),
            "app/src/bindings.ts",
            Some("// old\n"),
            &[],
        );
        assert_eq!(
            checks(&report),
            [
                ("startup-problem", None),
                ("bindings-stale", None),
                ("ability-not-granted", Some("customers_destroy")),
                ("write-without-ability", Some("notes_purge")),
                ("assistant-writes-unasked", Some("invoices_send")),
                ("granted-unused", Some("custmers.delete")),
                ("uncalled", Some("reports_rebuild")),
            ]
        );
        let not_granted = &report.findings[2];
        assert_eq!(not_granted.ability.as_deref(), Some("customers.delete"));
        assert!(not_granted
            .fix
            .contains("App::allow_ability(\"customers.delete\")"));
        // A live read the assistant may call unasked, an MCP tool behind
        // confirm, a grant a page names (`can="settings.open"`): all fine.
        assert_eq!(report.count(Level::Error), 3);
        assert_eq!(report.count(Level::Warning), 3);
        assert_eq!(report.count(Level::Info), 1);
    }

    #[test]
    fn bindings_missing_or_codegen_failing_are_errors() {
        let report = findings(&shape(), &frontend(), "app/src/bindings.ts", None, &[]);
        assert!(report
            .findings
            .iter()
            .any(|f| f.message == "app/src/bindings.ts doesn't exist."));
        let mut broken = shape();
        broken["bindings"] = json!({ "error": "codegen: a type can't be named" });
        let report = findings(&broken, &frontend(), "b.ts", Some("x"), &[]);
        assert!(report
            .findings
            .iter()
            .any(|f| f.message == "codegen fails: codegen: a type can't be named"));
        let current = findings(&shape(), &frontend(), "b.ts", Some("// bindings\n"), &[]);
        assert!(!current.findings.iter().any(|f| f.check == "bindings-stale"));
    }

    #[test]
    fn an_allow_sets_a_finding_apart_with_its_reason_and_an_unused_one_is_found() {
        let allow = [
            Allow {
                check: "write-without-ability".into(),
                command: Some("notes_purge".into()),
                ability: None,
                why: "only purges the user's own drafts".into(),
            },
            Allow {
                check: "uncalled".into(),
                command: Some("gone".into()),
                ability: None,
                why: "old".into(),
            },
        ];
        let report = findings(&shape(), &frontend(), "b.ts", Some("// bindings\n"), &allow);
        assert_eq!(report.allowed.len(), 1);
        assert_eq!(report.allowed[0].why, "only purges the user's own drafts");
        assert!(!report
            .findings
            .iter()
            .any(|f| f.check == "write-without-ability"));
        let unused = report
            .findings
            .iter()
            .find(|f| f.check == "allow-unused")
            .unwrap();
        assert_eq!(
            unused.message,
            "[check] allows uncalled for gone but nothing is found for it."
        );
        let printed = text(&report);
        assert!(printed.contains(
            "Allowed in elyra.toml:\n- write-without-ability: notes_purge needs no ability"
        ));
    }

    #[test]
    fn reads_words_and_quoted_strings() {
        let mut f = Frontend::default();
        f.read("api.customers_index(x); can=\"customers.view\" ; 'a.b' `c.d` api.greeting()");
        assert!(f.calls("customers_index"));
        assert!(!f.calls("greet"), "a word is whole");
        assert!(f.names("customers.view") && f.names("a.b") && f.names("c.d"));
        assert!(!f.names("customers"));
    }

    #[test]
    fn the_text_says_each_finding_and_counts_them() {
        let report = findings(&shape(), &frontend(), "b.ts", Some("// bindings\n"), &[]);
        let printed = text(&report);
        assert!(
            printed.starts_with("error    Backend::transcription()"),
            "{printed}"
        );
        assert!(printed.contains(
            "warning  notes_purge needs no ability and isn't a live query, so it may write: any page script can call it.\n         Give it"
        ));
        assert!(
            printed.ends_with("\n2 errors, 3 warnings, 1 note.\n"),
            "{printed}"
        );
        let empty = Report::default();
        assert_eq!(text(&empty), "No findings.\n");
    }

    #[test]
    fn the_backend_calls_are_read_from_the_app_crate_in_a_workspace() {
        let root =
            std::env::temp_dir().join(format!("rata-check-workspace-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("desktop/src")).unwrap();
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"desktop\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("desktop/Cargo.toml"),
            "[package]\nname = \"crm\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            root.join("desktop/src/main.rs"),
            "#[command]\nasync fn customers_index(ctx: Ctx) -> Value {\n    ctx.get::<Backend>().get(\"/api/customers\").send().await\n}\n",
        )
        .unwrap();
        let cfg = Config {
            root: root.clone(),
            app_crate: "crm".into(),
            frontend_dir: "desktop/app".into(),
            codegen_out: "desktop/app/src/bindings.ts".into(),
            bundle_identifier: "com.example.crm".into(),
            bundle_name: "CRM".into(),
            bundle_version: "0.1.0".into(),
            bundle_icon: None,
            bundle_deep_link: None,
            bundle_documents: Vec::new(),
            bundle_description: None,
            bundle_maintainer: None,
            bundle_publisher: None,
            database_url: None,
            migrations_dir: "migrations".into(),
            check_allow: Vec::new(),
            check_backend: None,
            check_backend_as: None,
        };
        let calls = backend_calls(&cfg);
        let _ = std::fs::remove_dir_all(&root);
        assert!(calls.contains_key("customers_index"), "{calls:?}");
    }

    #[test]
    fn document_types_and_what_the_app_opens_are_set_side_by_side() {
        let doc = |name: &str, ext: &str| crate::documents::DocumentType {
            name: name.into(),
            extensions: vec![ext.into()],
            mime: format!("application/x-{ext}"),
            role: crate::documents::Role::Editor,
            icon: None,
            uti: None,
            owned: true,
        };
        let declared = vec![doc("Note", "note"), doc("Draft", "draft")];
        let found = document_findings(&json!({ "opens": ["note", "md"] }), &declared);
        let checks: Vec<_> = found.iter().map(|f| (f.check, f.level)).collect();
        assert_eq!(
            checks,
            [
                ("document-not-opened", Level::Error),
                ("document-not-registered", Level::Warning)
            ]
        );
        assert!(found[0].message.contains(".draft"));
        assert!(found[1].message.contains(".md"));
        // An app that doesn't say (before 0.54): nothing to compare.
        assert!(document_findings(&json!({}), &declared).is_empty());
        assert!(document_findings(&json!({ "opens": ["note", "draft"] }), &declared).is_empty());
    }

    #[test]
    fn a_language_missing_the_frameworks_words_is_a_warning() {
        let shape = serde_json::json!({
            "untranslated": {
                "de": ["elyra.datepicker.clear", "elyra.datepicker.today", "elyra.modal.close", "elyra.menu.edit"]
            }
        });
        let found = word_findings(&shape);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].level, Level::Warning);
        assert_eq!(
            found[0].message,
            "lang/de.json: 4 of elyra's words fall back to English (elyra.datepicker.clear, elyra.datepicker.today, elyra.modal.close, …)"
        );
        assert!(
            word_findings(&serde_json::json!({})).is_empty(),
            "an older Elyra says nothing"
        );
    }
}
