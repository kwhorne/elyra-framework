//! `rata ai:install` / `rata ai:update` — what a coding agent needs to know
//! about an Elyra app, Laravel Boost's counterpart for the Rust side:
//!
//! - the framework's guidelines, as an `<elyra-framework-guidelines>` block in
//!   `AGENTS.md` (and in `CLAUDE.md` when it doesn't import `AGENTS.md`).
//!   Only the block is rata's; everything around it is kept.
//! - the framework's skills, in `.agents/skills/` and `.claude/skills/`.
//! - the `elyra-framework` server (`rata mcp dev`) in `.mcp.json`, next to
//!   whatever servers are there.
//!
//! `ai:install` writes all three; `ai:update` refreshes what's there. A skill
//! without rata's marker is the developer's and isn't overwritten.

use std::path::Path;

use crate::config::Config;

const GUIDELINES: &str = include_str!("../ai/guidelines.md");

/// The skills: (name, SKILL.md).
pub(crate) const SKILLS: &[(&str, &str)] = &[
    (
        "elyra-commands",
        include_str!("../ai/skills/elyra-commands/SKILL.md"),
    ),
    (
        "elyra-models-and-migrations",
        include_str!("../ai/skills/elyra-models-and-migrations/SKILL.md"),
    ),
    (
        "elyra-resources",
        include_str!("../ai/skills/elyra-resources/SKILL.md"),
    ),
    (
        "elyra-live-queries",
        include_str!("../ai/skills/elyra-live-queries/SKILL.md"),
    ),
    (
        "elyra-laravel-backend",
        include_str!("../ai/skills/elyra-laravel-backend/SKILL.md"),
    ),
    ("elyra-ui", include_str!("../ai/skills/elyra-ui/SKILL.md")),
];

const OPEN: &str = "<elyra-framework-guidelines>";
const CLOSE: &str = "</elyra-framework-guidelines>";

/// The last line of a skill rata wrote.
const MARKER: &str = "<!-- managed by rata: `rata ai:update` rewrites this file -->";

/// Where skills go: Elyra Workspace, Codex and Gemini CLI read `.agents`,
/// Claude Code `.claude`.
const SKILL_HOMES: &[&str] = &[".agents", ".claude"];

/// The MCP server's name in `.mcp.json`.
pub(crate) const SERVER: &str = "elyra-framework";

/// `rata ai:install`.
pub fn install(cfg: &Config) -> Result<(), String> {
    for line in apply(&cfg.root, false)? {
        println!("{line}");
    }
    println!(
        "\nAgents now find the framework's guidelines in AGENTS.md, its skills, and \
         `rata mcp dev` (search_docs, app_info, database_schema, …). `rata ai:update` \
         refreshes them after upgrading rata."
    );
    Ok(())
}

/// `rata ai:update`.
pub fn update(cfg: &Config) -> Result<(), String> {
    let done = apply(&cfg.root, true)?;
    if done.is_empty() {
        println!(
            "{}",
            if installed(&cfg.root) {
                "Up to date."
            } else {
                "Nothing to update — `rata ai:install` sets it up."
            }
        );
    }
    for line in done {
        println!("{line}");
    }
    Ok(())
}

/// Write (or, with `only_existing`, refresh) the guidelines, skills and the
/// MCP entry under `root`. Returns what it did, one line each.
fn apply(root: &Path, only_existing: bool) -> Result<Vec<String>, String> {
    let mut done = Vec::new();
    let block = format!("{OPEN}\n{}\n{CLOSE}", GUIDELINES.trim_end());

    let agents = root.join("AGENTS.md");
    let existing = read(&agents)?;
    if !only_existing || existing.as_deref().is_some_and(|t| t.contains(OPEN)) {
        let next = with_block(existing.as_deref().unwrap_or(""), &block);
        if existing.as_deref() != Some(next.as_str()) {
            write(&agents, &next)?;
            done.push(format!(
                "{} AGENTS.md",
                if existing.is_some() {
                    "updated"
                } else {
                    "created"
                }
            ));
        }
    }

    // CLAUDE.md gets the block only when it doesn't pull in AGENTS.md itself.
    let claude = root.join("CLAUDE.md");
    if let Some(text) = read(&claude)? {
        if !imports_agents(&text) && (!only_existing || text.contains(OPEN)) {
            let next = with_block(&text, &block);
            if next != text {
                write(&claude, &next)?;
                done.push("updated CLAUDE.md".into());
            }
        }
    }

    for home in SKILL_HOMES {
        for (name, body) in SKILLS {
            let path = root.join(home).join("skills").join(name).join("SKILL.md");
            let existing = read(&path)?;
            match &existing {
                None if only_existing => continue,
                Some(text) if !text.contains(MARKER) => {
                    done.push(format!(
                        "kept {} (it isn't rata's: delete it to get the framework's)",
                        relative(root, &path)
                    ));
                    continue;
                }
                _ => {}
            }
            let next = format!("{}\n\n{MARKER}\n", body.trim_end());
            if existing.as_deref() != Some(next.as_str()) {
                write(&path, &next)?;
                done.push(format!(
                    "{} {}",
                    if existing.is_some() {
                        "updated"
                    } else {
                        "wrote"
                    },
                    relative(root, &path)
                ));
            }
        }
    }

    let mcp = root.join(".mcp.json");
    let has_entry = read(&mcp)?.is_some_and(|text| {
        serde_json::from_str::<serde_json::Value>(&text)
            .is_ok_and(|v| v["mcpServers"].get(SERVER).is_some())
    });
    if !only_existing || has_entry {
        let server = serde_json::json!({ "command": "rata", "args": ["mcp", "dev"] });
        if merge_server(&mcp, SERVER, server)? {
            done.push(format!(
                "{} the `{SERVER}` server in .mcp.json (`rata mcp dev`)",
                if has_entry { "updated" } else { "added" }
            ));
        }
    }
    Ok(done)
}

/// Whether `ai:install` has run here: the block, one of rata's skills, or
/// the server entry.
fn installed(root: &Path) -> bool {
    let has = |path: &Path, needle: &str| {
        std::fs::read_to_string(path).is_ok_and(|text| text.contains(needle))
    };
    has(&root.join("AGENTS.md"), OPEN)
        || has(&root.join(".mcp.json"), &format!("\"{SERVER}\""))
        || SKILL_HOMES.iter().any(|home| {
            SKILLS.iter().any(|(name, _)| {
                has(
                    &root.join(home).join("skills").join(name).join("SKILL.md"),
                    MARKER,
                )
            })
        })
}

/// `text` with the guidelines `block` in place of the one it has, or added
/// at the end. Nothing outside the block changes.
pub(crate) fn with_block(text: &str, block: &str) -> String {
    if let (Some(start), Some(end)) = (text.find(OPEN), text.find(CLOSE)) {
        if start < end {
            let after = &text[end + CLOSE.len()..];
            return format!("{}{block}{after}", &text[..start]);
        }
    }
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        format!("{block}\n")
    } else {
        format!("{trimmed}\n\n{block}\n")
    }
}

/// Whether a `CLAUDE.md` imports `AGENTS.md` (`@AGENTS.md`), so it already
/// has the block.
fn imports_agents(text: &str) -> bool {
    text.lines().any(|line| {
        line.split_whitespace()
            .any(|word| word == "@AGENTS.md" || word == "@./AGENTS.md")
    })
}

/// Put `server` under `mcpServers.<name>` in the `.mcp.json` at `path`,
/// keeping every other server and key. Returns whether the file changed.
/// A file that isn't a JSON object is refused rather than replaced.
pub(crate) fn merge_server(
    path: &Path,
    name: &str,
    server: serde_json::Value,
) -> Result<bool, String> {
    let existing = read(path)?;
    let mut doc: serde_json::Value = match &existing {
        Some(text) => serde_json::from_str(text).map_err(|e| {
            format!(
                "{} isn't valid JSON ({e}); fix it, rata won't overwrite it",
                path.display()
            )
        })?,
        None => serde_json::json!({}),
    };
    let Some(object) = doc.as_object_mut() else {
        return Err(format!(
            "{} isn't a JSON object; rata won't overwrite it",
            path.display()
        ));
    };
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}));
    let Some(servers) = servers.as_object_mut() else {
        return Err(format!(
            "`mcpServers` in {} isn't an object; rata won't overwrite it",
            path.display()
        ));
    };
    if servers.get(name) == Some(&server) {
        return Ok(false);
    }
    servers.insert(name.to_owned(), server);
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?;
    write(path, &format!("{text}\n"))?;
    Ok(true)
}

fn read(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

#[cfg(test)]
mod tests {

    /// Every skill in `ai/skills/` is one `ai:install` writes.
    #[test]
    fn every_skill_folder_is_installed() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ai/skills");
        let mut folders: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect();
        folders.sort();
        let mut listed: Vec<String> = SKILLS.iter().map(|(name, _)| (*name).to_owned()).collect();
        listed.sort();
        assert_eq!(folders, listed);
    }

    use super::*;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rata-ai-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_block_replaces_only_itself() {
        let block = format!("{OPEN}\nnew\n{CLOSE}");
        assert_eq!(with_block("", &block), format!("{block}\n"));
        assert_eq!(
            with_block("# Mine\n\nKeep this.\n", &block),
            format!("# Mine\n\nKeep this.\n\n{block}\n")
        );
        let before = format!("# Mine\n\n{OPEN}\nold\n{CLOSE}\n\n## After\nstays\n");
        assert_eq!(
            with_block(&before, &block),
            format!("# Mine\n\n{block}\n\n## After\nstays\n")
        );
    }

    #[test]
    fn claude_md_that_imports_agents_md_is_left_alone() {
        assert!(imports_agents("@AGENTS.md\n"));
        assert!(imports_agents("See @AGENTS.md for more.\n"));
        assert!(!imports_agents("Read AGENTS.md.\n"));
    }

    #[test]
    fn installs_then_updates_without_touching_what_isnt_rata_s() {
        let root = temp("install");
        std::fs::write(root.join("CLAUDE.md"), "# Claude\n\nOur rules.\n").unwrap();
        std::fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers": {"laravel-boost": {"command": "php", "args": ["artisan", "boost:mcp"]}}, "other": 1}"#,
        )
        .unwrap();
        let mine = root.join(".claude/skills/elyra-resources/SKILL.md");
        std::fs::create_dir_all(mine.parent().unwrap()).unwrap();
        std::fs::write(&mine, "my own").unwrap();

        let done = apply(&root, false).unwrap();
        assert!(done.iter().any(|l| l == "created AGENTS.md"), "{done:?}");
        let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
        assert!(agents.starts_with(OPEN) && agents.contains("rata codegen"));
        let claude = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
        assert!(claude.starts_with("# Claude\n\nOur rules.\n\n") && claude.contains(OPEN));
        assert!(root
            .join(".agents/skills/elyra-live-queries/SKILL.md")
            .is_file());
        assert_eq!(std::fs::read_to_string(&mine).unwrap(), "my own");
        assert!(done
            .iter()
            .any(|l| l.starts_with("kept .claude/skills/elyra-resources")));
        let mcp: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(".mcp.json")).unwrap())
                .unwrap();
        assert_eq!(mcp["mcpServers"]["laravel-boost"]["command"], "php");
        assert_eq!(mcp["mcpServers"][SERVER]["args"][1], "dev");
        assert_eq!(mcp["other"], 1);

        // Nothing changed: nothing to do, apart from the skill that's kept.
        assert!(installed(&root));
        let again = apply(&root, true).unwrap();
        assert!(again.iter().all(|l| l.starts_with("kept ")), "{again:?}");

        // An update doesn't bring back what was removed.
        std::fs::remove_dir_all(root.join(".agents")).unwrap();
        apply(&root, true).unwrap();
        assert!(!root.join(".agents").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_broken_mcp_json_is_refused() {
        let root = temp("broken");
        let path = root.join(".mcp.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(merge_server(&path, "x", serde_json::json!({})).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn skills_have_their_frontmatter() {
        for (name, body) in SKILLS {
            let front = body
                .strip_prefix("---\n")
                .and_then(|rest| rest.split_once("\n---\n"))
                .map(|(front, _)| front)
                .unwrap_or_else(|| panic!("{name} has no frontmatter"));
            assert!(front.contains(&format!("name: {name}\n")), "{name}");
            assert!(front.contains("description: "), "{name}");
        }
    }
}
