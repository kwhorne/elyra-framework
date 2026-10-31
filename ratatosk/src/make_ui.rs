//! `rata make:ui <Component>` (RFC 0012): copy one of `@elyra/ui`'s
//! components into the app, to change. The copy comes from the package the
//! app has installed, so it matches the rest of `@elyra/ui`; its imports of
//! the other components and of the package's internals point back at the
//! package, so a copied `Field` still shares its form with `@elyra/ui`'s
//! `Form`. The file is the app's: rata never overwrites it (`--force` does).

use std::path::{Path, PathBuf};

use crate::config::Config;

pub fn make_ui(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    run(cfg, &args)
}

struct Options {
    name: Option<String>,
    list: bool,
    force: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        name: None,
        list: false,
        force: false,
    };
    for arg in args {
        match arg.as_str() {
            "--list" => opts.list = true,
            "--force" => opts.force = true,
            flag if flag.starts_with("--") => return Err(format!("unknown argument `{flag}`")),
            name if opts.name.is_none() => opts.name = Some(name.to_owned()),
            extra => return Err(format!("one component at a time (also got `{extra}`)")),
        }
    }
    if opts.name.is_none() && !opts.list {
        return Err("usage: rata make:ui <Component> [--force] | rata make:ui --list".into());
    }
    Ok(opts)
}

fn run(cfg: &Config, args: &[String]) -> Result<(), String> {
    let opts = parse_args(args)?;
    let app = cfg.root.join(&cfg.frontend_dir);
    let package = app.join("node_modules/@elyra/ui");
    let source = package.join("src");
    if !source.is_dir() {
        return Err(format!(
            "@elyra/ui isn't installed in {}: add it to package.json (`rata new` does) and run `npm install` there",
            app.display()
        ));
    }
    let components = components(&source)?;
    if opts.list {
        println!("@elyra/ui's components (rata make:ui <Component> copies one into the app):");
        for name in &components {
            println!("  {name}");
        }
        return Ok(());
    }
    let name = opts.name.unwrap_or_default();
    if !components.contains(&name) {
        return Err(format!(
            "@elyra/ui has no `{name}`. It has: {}",
            components.join(", ")
        ));
    }
    let version = version(&package);
    let original = std::fs::read_to_string(source.join(format!("{name}.svelte")))
        .map_err(|e| format!("{name}.svelte: {e}"))?;
    let target_dir = app.join("src/lib/ui");
    let target = target_dir.join(format!("{name}.svelte"));
    if target.exists() && !opts.force {
        return Err(format!(
            "{} exists and is yours: rata won't overwrite it (--force replaces it)",
            relative(cfg, &target)
        ));
    }
    std::fs::create_dir_all(&target_dir).map_err(|e| format!("{}: {e}", target_dir.display()))?;
    std::fs::write(&target, owned(&name, &version, &original))
        .map_err(|e| format!("{}: {e}", target.display()))?;
    println!("wrote {}", relative(cfg, &target));
    println!(
        "\nImport it from there instead of \"@elyra/ui\", in each view that should use your copy:\n  import {name} from \"<path to src>/lib/ui/{name}.svelte\";"
    );
    Ok(())
}

/// The component names in `@elyra/ui/src`: its public `.svelte` files.
fn components(source: &Path) -> Result<Vec<String>, String> {
    let mut names: Vec<String> = std::fs::read_dir(source)
        .map_err(|e| format!("{}: {e}", source.display()))?
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "svelte")
                .then(|| path.file_stem()?.to_str().map(str::to_owned))?
        })
        .collect();
    names.sort();
    Ok(names)
}

/// The installed package's version, for the copy's header.
fn version(package: &Path) -> String {
    std::fs::read_to_string(package.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json["version"].as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

/// The copy: a header saying where it came from, and the imports that were
/// relative to the package pointed back at it.
fn owned(name: &str, version: &str, original: &str) -> String {
    let mut out = format!(
        "<!-- Copied from @elyra/ui {version} by `rata make:ui {name}`: yours to change. -->\n"
    );
    for line in original.split_inclusive('\n') {
        out.push_str(&rewrite_import(line));
    }
    out
}

/// One line, with an import of a sibling component or an internal made an
/// import from the package: `import Icon from "./internal/Icon.svelte";`
/// stays the package's, and `import Input from "./Input.svelte";` becomes
/// `import { Input } from "@elyra/ui";`.
fn rewrite_import(line: &str) -> String {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let ending = if line.ends_with('\n') { "\n" } else { "" };
    let body = trimmed.trim_end();
    if !body.starts_with("import ") {
        return line.to_owned();
    }
    if let Some((head, rest)) = body.split_once("\"./internal/") {
        return format!("{indent}{head}\"@elyra/ui/internal/{rest}{ending}");
    }
    // `import X from "./Y.svelte";` — a default import of a sibling.
    let Some(rest) = body.strip_prefix("import ") else {
        return line.to_owned();
    };
    let Some((local, from)) = rest.split_once(" from \"./") else {
        return line.to_owned();
    };
    let Some(component) = from.strip_suffix(".svelte\";") else {
        return line.to_owned();
    };
    let local = local.trim();
    if local.starts_with('{') || local.contains(',') {
        return line.to_owned();
    }
    if local == component {
        format!("{indent}import {{ {component} }} from \"@elyra/ui\";{ending}")
    } else {
        format!("{indent}import {{ {component} as {local} }} from \"@elyra/ui\";{ending}")
    }
}

fn relative(cfg: &Config, path: &Path) -> String {
    path.strip_prefix(&cfg.root)
        .map(PathBuf::from)
        .unwrap_or_else(|_| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_and_internal_imports_point_at_the_package() {
        assert_eq!(
            rewrite_import("  import Input from \"./Input.svelte\";\n"),
            "  import { Input } from \"@elyra/ui\";\n"
        );
        assert_eq!(
            rewrite_import("  import Box from \"./Card.svelte\";"),
            "  import { Card as Box } from \"@elyra/ui\";"
        );
        assert_eq!(
            rewrite_import("  import Icon from \"./internal/Icon.svelte\";\n"),
            "  import Icon from \"@elyra/ui/internal/Icon.svelte\";\n"
        );
        assert_eq!(
            rewrite_import("  import { FIELD, FORM } from \"./internal/context.js\";\n"),
            "  import { FIELD, FORM } from \"@elyra/ui/internal/context.js\";\n"
        );
        for untouched in [
            "  import { getContext } from \"svelte\";\n",
            "  import { can as canStore } from \"@elyra/runtime\";\n",
            "  const x = \"./Input.svelte\";\n",
            "<!-- import Input from \"./Input.svelte\"; -->\n",
            "  import { helper } from \"./Shared.svelte\";\n",
            "  import Thing, { helper } from \"./Shared.svelte\";\n",
        ] {
            assert_eq!(rewrite_import(untouched), untouched);
        }
    }

    #[test]
    fn a_copy_says_where_it_came_from() {
        let copy = owned(
            "Table",
            "0.21.0",
            "<script>\n  import Callout from \"./Callout.svelte\";\n</script>\n",
        );
        assert_eq!(
            copy,
            "<!-- Copied from @elyra/ui 0.21.0 by `rata make:ui Table`: yours to change. -->\n\
             <script>\n  import { Callout } from \"@elyra/ui\";\n</script>\n"
        );
    }

    /// Every component in this repository's `ui/src`, copied: no import is
    /// left pointing next to it, where the copy has nothing.
    #[test]
    fn every_real_component_copies_cleanly() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui/src");
        let names = components(&source).unwrap();
        assert!(names.len() >= 30, "{names:?}");
        for name in names {
            let original = std::fs::read_to_string(source.join(format!("{name}.svelte"))).unwrap();
            let copy = owned(&name, "0", &original);
            for line in copy
                .lines()
                .filter(|l| l.trim_start().starts_with("import "))
            {
                assert!(!line.contains("\"./"), "{name}: {line}");
            }
            assert_eq!(copy.lines().count(), original.lines().count() + 1, "{name}");
        }
    }

    fn project(name: &str) -> (Config, PathBuf) {
        let root = std::env::temp_dir().join(format!("rata-make-ui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = root.join("app/node_modules/@elyra/ui/src/internal");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            root.join("app/node_modules/@elyra/ui/package.json"),
            r#"{"version":"0.21.0"}"#,
        )
        .unwrap();
        let ui = root.join("app/node_modules/@elyra/ui/src");
        std::fs::write(ui.join("Badge.svelte"), "<span><slot /></span>\n").unwrap();
        std::fs::write(ui.join("index.js"), "export {};\n").unwrap();
        std::fs::write(
            ui.join("Field.svelte"),
            "<script>\n  import { FORM } from \"./internal/context.js\";\n</script>\n",
        )
        .unwrap();
        std::fs::write(src.join("context.js"), "export const FORM = Symbol();\n").unwrap();
        let cfg = Config {
            root: root.clone(),
            app_crate: "app".into(),
            frontend_dir: "app".into(),
            codegen_out: "app/src/bindings.ts".into(),
            bundle_identifier: "com.example.app".into(),
            bundle_name: "App".into(),
            bundle_version: "0.1.0".into(),
            bundle_icon: None,
            bundle_deep_link: None,
            bundle_description: None,
            bundle_maintainer: None,
            database_url: None,
            migrations_dir: "migrations".into(),
        };
        (cfg, root)
    }

    #[test]
    fn copies_a_component_once() {
        let (cfg, root) = project("copy");
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        run(&cfg, &args(&["Field"])).unwrap();
        let copy = std::fs::read_to_string(root.join("app/src/lib/ui/Field.svelte")).unwrap();
        assert!(
            copy.starts_with("<!-- Copied from @elyra/ui 0.21.0"),
            "{copy}"
        );
        assert!(
            copy.contains("from \"@elyra/ui/internal/context.js\""),
            "{copy}"
        );

        // Yours now: not overwritten, unless asked.
        std::fs::write(root.join("app/src/lib/ui/Field.svelte"), "mine").unwrap();
        let err = run(&cfg, &args(&["Field"])).unwrap_err();
        assert!(err.contains("won't overwrite"), "{err}");
        assert_eq!(
            std::fs::read_to_string(root.join("app/src/lib/ui/Field.svelte")).unwrap(),
            "mine"
        );
        run(&cfg, &args(&["Field", "--force"])).unwrap();
        assert_ne!(
            std::fs::read_to_string(root.join("app/src/lib/ui/Field.svelte")).unwrap(),
            "mine"
        );

        // Only components: not the internals, not a name it doesn't have.
        let err = run(&cfg, &args(&["context"])).unwrap_err();
        assert!(err.contains("It has: Badge, Field"), "{err}");
        run(&cfg, &args(&["--list"])).unwrap();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn says_what_to_do_without_the_package() {
        let (cfg, root) = project("missing");
        std::fs::remove_dir_all(root.join("app/node_modules")).unwrap();
        let err = run(&cfg, &["Badge".to_string()]).unwrap_err();
        assert!(err.contains("npm install"), "{err}");
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--nope".into()]).is_err());
        assert!(parse_args(&["A".into(), "B".into()]).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}
