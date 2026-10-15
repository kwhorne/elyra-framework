//! Embeds the framework's guides (`docs/*.md`) for `rata mcp dev`'s
//! `search_docs`, so an agent can read them with no network. Releases are
//! built from the repository, where `../docs` is; a build without it embeds
//! none, and the tool says so.

use std::path::Path;

fn main() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs");
    println!("cargo:rerun-if-changed={}", docs.display());
    let mut files: Vec<_> = std::fs::read_dir(&docs)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "md"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out = String::from(
        "/// The framework's guides: (file name, text).\npub const DOCS: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let name = path.file_name().unwrap().to_string_lossy();
        let absolute = path.canonicalize().unwrap_or_else(|_| path.clone());
        out.push_str(&format!(
            "    ({name:?}, include_str!({:?})),\n",
            absolute.display().to_string()
        ));
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("docs.rs");
    std::fs::write(dest, out).unwrap();
}
