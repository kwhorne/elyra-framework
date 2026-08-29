//! `rata make:*` — Artisan-style generators that scaffold a source file and
//! print the one wiring step Rust needs (a `mod` line + registration). We never
//! rewrite `main.rs` automatically — editing a user's source is too fragile.

use std::path::PathBuf;

use crate::config::Config;

/// The item name passed on the command line (`rata make:command <name>`).
fn arg_name(usage: &str) -> Result<String, String> {
    std::env::args().nth(2).ok_or_else(|| usage.to_string())
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_lower = false;
        }
    }
    out.trim_matches('_').to_string()
}

fn pascal(name: &str) -> String {
    snake(name)
        .split('_')
        .filter(|s| !s.is_empty())
        .map(|s| {
            let mut c = s.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// Naive English pluralization, enough for table names.
fn plural(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let consonant_y =
        chars.last() == Some(&'y') && chars.len() >= 2 && !"aeiou".contains(chars[chars.len() - 2]);
    if consonant_y {
        format!("{}ies", &word[..word.len() - 1])
    } else if word.ends_with('s')
        || word.ends_with('x')
        || word.ends_with('z')
        || word.ends_with("ch")
        || word.ends_with("sh")
    {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

/// Resolve `<root>/src`, erroring if this doesn't look like an app project.
fn src_dir(cfg: &Config) -> Result<PathBuf, String> {
    let dir = cfg.root.join("src");
    if !dir.is_dir() {
        return Err(format!(
            "no `src/` directory at {} — run `rata make:*` from an app project",
            cfg.root.display()
        ));
    }
    Ok(dir)
}

fn write_new(path: &std::path::Path, contents: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// `rata make:command <name>`
pub fn command(cfg: &Config) -> Result<(), String> {
    let raw = arg_name("usage: rata make:command <name>")?;
    let name = snake(&raw);
    if name.is_empty() {
        return Err("command name must contain alphanumeric characters".into());
    }
    let path = src_dir(cfg)?.join(format!("{name}.rs"));
    let body = format!(
        "use elyra::{{command, Ctx}};\n\n\
         /// TODO: describe what `{name}` does.\n\
         #[command]\n\
         pub async fn {name}(_ctx: Ctx, message: String) -> String {{\n\
         \x20   format!(\"{name}: {{message}}\")\n\
         }}\n"
    );
    write_new(&path, &body)?;
    println!("Created {}", path.display());
    println!("\nNext steps (in src/main.rs):");
    println!("  mod {name};");
    println!("  use {name}::{name};");
    println!("  // then add `{name}` to your commands![ ... ] list");
    Ok(())
}

/// `rata make:provider <name>`
pub fn provider(cfg: &Config) -> Result<(), String> {
    let raw = arg_name("usage: rata make:provider <name>")?;
    let file = snake(&raw);
    if file.is_empty() {
        return Err("provider name must contain alphanumeric characters".into());
    }
    // `Payments` / `payments` -> `PaymentsProvider`.
    let base = pascal(&raw);
    let base = base.strip_suffix("Provider").unwrap_or(&base);
    let ty = format!("{base}Provider");
    let path = src_dir(cfg)?.join(format!("{file}.rs"));
    let body = format!(
        "use elyra::{{Container, Ctx, Provider}};\n\n\
         pub struct {ty};\n\n\
         impl Provider for {ty} {{\n\
         \x20   /// Bind services into the container.\n\
         \x20   fn register(&self, container: &mut Container) {{\n\
         \x20       let _ = container;\n\
         \x20       // container.bind(MyService::new());\n\
         \x20   }}\n\n\
         \x20   /// Run setup once the container is fully populated.\n\
         \x20   fn boot(&self, ctx: &Ctx) {{\n\
         \x20       let _ = ctx;\n\
         \x20   }}\n\
         }}\n"
    );
    write_new(&path, &body)?;
    println!("Created {}", path.display());
    println!("\nNext steps (in src/main.rs):");
    println!("  mod {file};");
    println!("  use {file}::{ty};");
    println!("  // then add `.provider({ty})` to your App builder");
    Ok(())
}

/// `rata make:middleware <name>`
pub fn middleware(cfg: &Config) -> Result<(), String> {
    let raw = arg_name("usage: rata make:middleware <name>")?;
    let file = snake(&raw);
    if file.is_empty() {
        return Err("middleware name must contain alphanumeric characters".into());
    }
    // `Timing` / `timing` -> `Timing`; `LogCalls` stays `LogCalls`.
    let ty = pascal(&raw);
    let path = src_dir(cfg)?.join(format!("{file}.rs"));
    let body = format!(
        "use elyra::command::BoxFuture;\n\
         use elyra::{{CommandRequest, Ctx, Middleware, Next, Result}};\n\n\
         /// TODO: describe what `{ty}` does to every command call.\n\
         pub struct {ty};\n\n\
         impl Middleware for {ty} {{\n\
         \x20   fn handle(&self, ctx: Ctx, req: CommandRequest, next: Next)\n\
         \x20       -> BoxFuture<'static, Result<Vec<u8>>>\n\
         \x20   {{\n\
         \x20       Box::pin(async move {{\n\
         \x20           // Before the command: `req.name` is the command being called.\n\
         \x20           let out = next.run(ctx, req).await;\n\
         \x20           // After the command: inspect or replace `out`.\n\
         \x20           out\n\
         \x20       }})\n\
         \x20   }}\n\
         }}\n"
    );
    write_new(&path, &body)?;
    println!("Created {}", path.display());
    println!("\nNext steps (in src/main.rs):");
    println!("  mod {file};");
    println!("  use {file}::{ty};");
    println!("  // then add `.middleware({ty})` to your App builder");
    Ok(())
}

/// `rata make:model <name>`
pub fn model(cfg: &Config) -> Result<(), String> {
    let raw = arg_name("usage: rata make:model <name>")?;
    let file = snake(&raw);
    if file.is_empty() {
        return Err("model name must contain alphanumeric characters".into());
    }
    let ty = pascal(&raw);
    let table = plural(&file);
    let path = src_dir(cfg)?.join(format!("{file}.rs"));
    let body = format!(
        "use elyra::Model;\n\
         use serde::{{Deserialize, Serialize}};\n\n\
         #[derive(Model, Serialize, Deserialize, specta::Type, Debug, Default, Clone)]\n\
         #[model(table = \"{table}\", timestamps)]\n\
         pub struct {ty} {{\n\
         \x20   #[model(id)]\n\
         \x20   pub id: i64,\n\
         \x20   pub name: String,\n\
         }}\n"
    );
    write_new(&path, &body)?;
    println!("Created {}", path.display());
    println!("\nNext steps:");
    println!("  mod {file};   // in src/main.rs");
    println!("  rata make:migration create_{table}   // then edit the .sql");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_helpers() {
        assert_eq!(snake("GreetUser"), "greet_user");
        assert_eq!(snake("greet user"), "greet_user");
        assert_eq!(snake("HTTPServer"), "httpserver"); // acronyms collapse (fine for filenames)
        assert_eq!(pascal("blog_post"), "BlogPost");
        assert_eq!(pascal("payments"), "Payments");
    }

    #[test]
    fn pluralization() {
        assert_eq!(plural("post"), "posts");
        assert_eq!(plural("category"), "categories");
        assert_eq!(plural("box"), "boxes");
        assert_eq!(plural("dish"), "dishes");
        assert_eq!(plural("day"), "days");
    }
}
