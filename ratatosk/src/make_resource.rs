//! `rata make:resource <Model>` — the base tier of RFC 0001: the five resource
//! commands, their input/query types, validation, domain events and tests, for
//! a `#[derive(Model)]` struct that already exists.
//!
//! The model is read with `syn`, so the generated code matches its fields
//! exactly. Everything is checked before anything is written: a missing model,
//! a missing derive or a missing dependency stops the command with the exact
//! fix, and no file is touched. rata still never edits `main.rs` or
//! `Cargo.toml` — the registry (see [`crate::resource`]) keeps the wiring to a
//! one-time step.

use std::path::{Path, PathBuf};

use quote::ToTokens;

use crate::config::Config;
use crate::make::{pascal, plural, snake};
use crate::resource::{self, Layout};
use crate::resource_views as views;

/// Rows per page when the frontend doesn't ask, and the most it may ask for.
const PER_PAGE: i64 = 25;
const MAX_PER_PAGE: i64 = 100;

/// Derives the generated code relies on, and why — printed when one is missing.
const NEEDED_DERIVES: &[(&str, &str)] = &[
    ("Default", "`store` starts from `Model::default()`"),
    ("Clone", "the domain events carry a copy"),
    ("Serialize", "commands return it"),
    ("Deserialize", "the tests read it back"),
    ("Type", "codegen types it for the frontend (`specta::Type`)"),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Kind {
    Text,
    Int,
    Float,
    Bool,
    /// Anything else (a `cast` type, `serde_json::Value`, …): no type rule.
    Other,
}

#[derive(Debug)]
pub(crate) struct Field {
    pub(crate) name: String,
    pub(crate) column: String,
    pub(crate) kind: Kind,
    /// `Option<T>` on the model.
    pub(crate) nullable: bool,
    /// The Rust type, without the `Option`.
    pub(crate) ty: String,
}

#[derive(Debug)]
pub(crate) struct ModelInfo {
    pub(crate) name: String,
    /// `crate::customer`, for the `use`.
    pub(crate) module: String,
    pub(crate) table: String,
    pub(crate) pk: String,
    pub(crate) timestamps: bool,
    pub(crate) soft_deletes: bool,
    /// What a form may set: not the key, timestamps or relations.
    pub(crate) editable: Vec<Field>,
    /// Every stored column, for the sort allowlist.
    pub(crate) columns: Vec<String>,
}

#[derive(Debug)]
struct Options {
    model: String,
    force: bool,
    dry_run: bool,
    abilities: bool,
    view: bool,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let usage = "usage: rata make:resource <Model> [--view] [--force] [--dry-run] [--no-abilities]";
    let mut model = None;
    let mut opts = Options {
        model: String::new(),
        force: false,
        dry_run: false,
        abilities: true,
        view: false,
    };
    for arg in args {
        match arg.as_str() {
            "--force" => opts.force = true,
            "--dry-run" => opts.dry_run = true,
            "--no-abilities" => opts.abilities = false,
            "--view" => opts.view = true,
            "--generate" => return Err("`--generate` lands with RFC 0001 step 5 — not yet".into()),
            flag if flag.starts_with('-') => return Err(format!("unknown flag `{flag}`\n{usage}")),
            name if model.is_none() => model = Some(name.to_string()),
            extra => return Err(format!("unexpected argument `{extra}`\n{usage}")),
        }
    }
    opts.model = pascal(&model.ok_or(usage)?);
    if opts.model.is_empty() {
        return Err("the model name must contain letters".into());
    }
    Ok(opts)
}

// ---------------------------------------------------------------------------
// Reading the model
// ---------------------------------------------------------------------------

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `src/a/b.rs` -> `crate::a::b`; `src/a/mod.rs` -> `crate::a`; `src/main.rs` -> `crate`.
fn module_path(src: &Path, file: &Path) -> String {
    let rel = file.strip_prefix(src).unwrap_or(file);
    let mut parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if let Some(last) = parts.pop() {
        let stem = last.trim_end_matches(".rs");
        if !matches!(stem, "mod" | "main" | "lib") {
            parts.push(stem.to_string());
        }
    }
    std::iter::once("crate".to_string())
        .chain(parts)
        .collect::<Vec<_>>()
        .join("::")
}

fn derives(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for attr in attrs.iter().filter(|a| a.path().is_ident("derive")) {
        let _ = attr.parse_nested_meta(|meta| {
            if let Some(last) = meta.path.segments.last() {
                out.push(last.ident.to_string());
            }
            Ok(())
        });
    }
    out
}

fn type_text(ty: &syn::Type) -> String {
    ty.to_token_stream()
        .to_string()
        .replace(" :: ", "::")
        .replace(":: ", "::")
        .replace(" <", "<")
        .replace("< ", "<")
        .replace(" >", ">")
        .replace(" ,", ",")
}

/// `Option<T>` -> `Some(T)`.
fn option_inner(ty: &syn::Type) -> Option<&syn::Type> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    match &last.arguments {
        syn::PathArguments::AngleBracketed(args) => match args.args.first()? {
            syn::GenericArgument::Type(inner) => Some(inner),
            _ => None,
        },
        _ => None,
    }
}

fn kind_of(ty: &syn::Type) -> Kind {
    match type_text(ty).as_str() {
        "String" => Kind::Text,
        "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "isize" | "usize" => {
            Kind::Int
        }
        "f32" | "f64" => Kind::Float,
        "bool" => Kind::Bool,
        _ => Kind::Other,
    }
}

const RELATIONS: &[&str] = &["has_many", "has_one", "belongs_to", "belongs_to_many"];

/// Read `#[model(..)]` on the struct and its fields into a [`ModelInfo`].
fn model_info(item: &syn::ItemStruct, module: String) -> Result<ModelInfo, String> {
    let name = item.ident.to_string();
    // The derive's own default table name.
    let mut table = name.to_lowercase();
    let mut timestamps = false;
    let mut soft_deletes = false;
    for attr in item.attrs.iter().filter(|a| a.path().is_ident("model")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table") {
                let lit: syn::LitStr = meta.value()?.parse()?;
                table = lit.value();
            } else if meta.path.is_ident("timestamps") {
                timestamps = true;
            } else if meta.path.is_ident("soft_deletes") {
                soft_deletes = true;
            } else if meta.input.peek(syn::Token![=]) {
                let _: syn::Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let _ = meta.parse_nested_meta(|inner| {
                    if inner.input.peek(syn::Token![=]) {
                        let _: syn::Expr = inner.value()?.parse()?;
                    }
                    Ok(())
                });
            }
            Ok(())
        });
    }

    let syn::Fields::Named(fields) = &item.fields else {
        return Err(format!("`{name}` must be a struct with named fields"));
    };
    let mut pk = None;
    let mut editable = Vec::new();
    let mut columns = Vec::new();
    for field in &fields.named {
        let ident = field.ident.as_ref().expect("named").to_string();
        let ident = ident.trim_start_matches("r#").to_string();
        let mut column = ident.clone();
        let mut is_pk = false;
        let mut relation = false;
        for attr in field.attrs.iter().filter(|a| a.path().is_ident("model")) {
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("id") {
                    is_pk = true;
                } else if meta.path.is_ident("column") {
                    let lit: syn::LitStr = meta.value()?.parse()?;
                    column = lit.value();
                } else if RELATIONS.iter().any(|r| meta.path.is_ident(r)) {
                    relation = true;
                    if meta.input.peek(syn::token::Paren) {
                        let _ = meta.parse_nested_meta(|inner| {
                            if inner.input.peek(syn::Token![=]) {
                                let _: syn::Expr = inner.value()?.parse()?;
                            }
                            Ok(())
                        });
                    }
                } else if meta.input.peek(syn::Token![=]) {
                    let _: syn::Expr = meta.value()?.parse()?;
                }
                Ok(())
            });
        }
        if relation {
            continue;
        }
        // Without an explicit `#[model(id)]`, the derive's key is `id`.
        if is_pk || (pk.is_none() && ident == "id" && !fields_have_explicit_id(fields)) {
            if type_text(&field.ty) != "i64" {
                return Err(format!(
                    "`{name}.{ident}` is the primary key, and make:resource needs it to be `i64`"
                ));
            }
            pk = Some(column.clone());
            columns.push(column);
            continue;
        }
        // Sorting by the trash marker means nothing: trashed rows aren't listed.
        if !(soft_deletes && column == "deleted_at") {
            columns.push(column.clone());
        }
        let managed = (timestamps && matches!(column.as_str(), "created_at" | "updated_at"))
            || (soft_deletes && column == "deleted_at");
        if managed {
            continue;
        }
        let (nullable, inner) = match option_inner(&field.ty) {
            Some(inner) => (true, inner),
            None => (false, &field.ty),
        };
        editable.push(Field {
            name: ident,
            column,
            kind: kind_of(inner),
            nullable,
            ty: type_text(inner),
        });
    }
    let pk =
        pk.ok_or_else(|| format!("`{name}` has no primary key (`id: i64` or `#[model(id)]`)"))?;
    if editable.is_empty() {
        return Err(format!("`{name}` has no fields a form could set"));
    }
    Ok(ModelInfo {
        name,
        module,
        table,
        pk,
        timestamps,
        soft_deletes,
        editable,
        columns,
    })
}

fn fields_have_explicit_id(fields: &syn::FieldsNamed) -> bool {
    fields.named.iter().any(|f| {
        f.attrs
            .iter()
            .filter(|a| a.path().is_ident("model"))
            .any(|a| {
                let mut found = false;
                let _ = a.parse_nested_meta(|meta| {
                    if meta.path.is_ident("id") {
                        found = true;
                    } else if meta.input.peek(syn::Token![=]) {
                        let _: syn::Expr = meta.value()?.parse()?;
                    } else if meta.input.peek(syn::token::Paren) {
                        let _ = meta.parse_nested_meta(|inner| {
                            if inner.input.peek(syn::Token![=]) {
                                let _: syn::Expr = inner.value()?.parse()?;
                            }
                            Ok(())
                        });
                    }
                    Ok(())
                });
                found
            })
    })
}

/// Find `struct <name>` with `#[derive(Model)]` under `src/`.
fn find_model(src: &Path, name: &str) -> Result<ModelInfo, String> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    let mut found = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !text.contains(name) {
            continue;
        }
        let Ok(parsed) = syn::parse_file(&text) else {
            continue;
        };
        for item in parsed.items {
            if let syn::Item::Struct(item) = item {
                if item.ident == name && derives(&item.attrs).iter().any(|d| d == "Model") {
                    found.push((file.clone(), item));
                }
            }
        }
    }
    let (file, item) = match found.len() {
        0 => {
            return Err(format!(
                "no `#[derive(Model)] struct {name}` under {} — create it first \
                 (`rata make:model {name}`), or scaffold everything with \
                 `rata make:resource {name} --generate <fields>` (RFC 0001 step 5)",
                src.display()
            ))
        }
        1 => found.remove(0),
        _ => {
            let files: Vec<String> = found.iter().map(|(f, _)| f.display().to_string()).collect();
            return Err(format!(
                "`{name}` is a Model in more than one file: {}",
                files.join(", ")
            ));
        }
    };

    let have = derives(&item.attrs);
    let missing: Vec<String> = NEEDED_DERIVES
        .iter()
        .filter(|(d, _)| !have.iter().any(|h| h == d))
        .map(|(d, why)| format!("  {d:<12} {why}"))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "`{name}` ({}) is missing derives the resource needs:\n{}",
            file.display(),
            missing.join("\n")
        ));
    }
    model_info(&item, module_path(src, &file))
}

// ---------------------------------------------------------------------------
// Prerequisites in Cargo.toml
// ---------------------------------------------------------------------------

/// The lines `Cargo.toml` still needs, or empty. Read-only.
fn missing_dependencies(manifest: &str) -> Vec<String> {
    let Ok(doc) = manifest.parse::<toml::Table>() else {
        return vec!["(Cargo.toml doesn't parse)".into()];
    };
    let deps = doc.get("dependencies").and_then(|d| d.as_table());
    let dev = doc.get("dev-dependencies").and_then(|d| d.as_table());
    let dep = |name: &str| {
        deps.and_then(|d| d.get(name))
            .or_else(|| dev.and_then(|d| d.get(name)))
    };
    let features = |value: Option<&toml::Value>| -> Vec<String> {
        value
            .and_then(|v| v.get("features"))
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };

    let mut missing = Vec::new();
    if !features(deps.and_then(|d| d.get("elyra")))
        .iter()
        .any(|f| f == "database")
    {
        missing.push(
            "[dependencies] elyra: add `features = [\"database\"]` (and a `.database(url)` on the App)"
                .into(),
        );
    }
    if dep("serde_json").is_none() {
        missing.push("[dependencies] serde_json = \"1\"".into());
    }
    let tokio = features(dep("tokio"));
    if dep("tokio").is_none() || !tokio.iter().any(|f| f == "macros" || f == "full") {
        missing.push(
            "[dev-dependencies] tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }"
                .into(),
        );
    }
    missing
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

pub(crate) struct Names {
    /// `Customer`
    pub(crate) ty: String,
    /// `customer` — the folder and module
    pub(crate) module: String,
    /// `customers` — the command prefix and ability namespace
    pub(crate) plural: String,
    /// `customer` in messages
    pub(crate) human: String,
}

pub(crate) fn names(model: &str) -> Names {
    let module = snake(model);
    Names {
        ty: model.to_string(),
        plural: plural(&module),
        human: module.replace('_', " "),
        module,
    }
}

/// Whether the form must send the field. A JSON (`Other`) field isn't: its
/// empty value would fail `required`, and a missing one keeps what's there.
fn is_required(field: &Field) -> bool {
    !field.nullable && field.kind != Kind::Other
}

fn rules(field: &Field) -> String {
    let presence = if !is_required(field) {
        "nullable"
    } else {
        "required"
    };
    let ty = match field.kind {
        Kind::Text => "|string",
        Kind::Int => "|integer",
        Kind::Float => "|numeric",
        Kind::Bool => "|boolean",
        Kind::Other => "",
    };
    format!("{presence}{ty}")
}

fn quoted_list(items: &[String]) -> String {
    items
        .iter()
        .map(|s| format!("\"{s}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_mod(n: &Names, abilities: bool) -> String {
    let p = &n.plural;
    let ability_list = if abilities {
        format!("&[\n    \"{p}.view\",\n    \"{p}.create\",\n    \"{p}.update\",\n    \"{p}.delete\",\n]")
    } else {
        "&[]".into()
    };
    format!(
        r#"//! The `{ty}` resource — generated by `rata make:resource`, and yours to change.
//! rata lists it in `../mod.rs`; this file says what it contributes.

mod commands;
#[cfg(test)]
mod tests;

pub use commands::*;

/// The abilities its commands require. They're denied until the app grants
/// them — `.allow_abilities(resources::abilities())` grants every resource's.
pub const ABILITIES: &[&str] = {ability_list};

pub fn commands() -> Vec<Box<dyn elyra::Command>> {{
    elyra::commands![
        {p}_index,
        {p}_show,
        {p}_store,
        {p}_update,
        {p}_destroy,
    ]
}}

pub fn migrations() -> Vec<Box<dyn elyra::db::RustMigration>> {{
    Vec::new()
}}
"#,
        ty = n.ty,
    )
}

fn render_commands(m: &ModelInfo, n: &Names, abilities: bool) -> String {
    let ty = &n.ty;
    let p = &n.plural;
    let var = &n.module;
    let human = &n.human;
    let pk = &m.pk;
    let can = |verb: &str| {
        if abilities {
            format!("#[command(can = \"{p}.{verb}\")]")
        } else {
            "#[command]".into()
        }
    };

    let searchable: Vec<String> = m
        .editable
        .iter()
        .filter(|f| f.kind == Kind::Text)
        .map(|f| f.column.clone())
        .collect();
    let rules_list: String = m
        .editable
        .iter()
        .map(|f| format!("    (\"{}\", \"{}\"),\n", f.name, rules(f)))
        .collect();
    let input_fields: String = m
        .editable
        .iter()
        .map(|f| format!("    pub {}: Option<{}>,\n", f.name, f.ty))
        .collect();
    let apply: String = m
        .editable
        .iter()
        .map(|f| {
            if f.nullable {
                format!("        {var}.{0} = self.{0};\n", f.name)
            } else {
                format!("        if let Some({0}) = self.{0} {{\n            {var}.{0} = {0};\n        }}\n", f.name)
            }
        })
        .collect();
    let destroy = if m.soft_deletes {
        format!(
            "    // A soft delete: the row stays, with `deleted_at` set, and `with_trashed()`\n    \
             // still reaches it.\n    \
             {ty}::query().where_eq(\"{pk}\", id).soft_delete(&db).await?;\n"
        )
    } else {
        format!("    {var}.delete(&db).await?;\n")
    };
    // `find` first either way, so a missing row is "not found", not a no-op.
    let destroy_find = if m.soft_deletes {
        "    find(&db, id).await?;\n".to_string()
    } else {
        format!("    let {var} = find(&db, id).await?;\n")
    };
    let destroy_doc = if m.soft_deletes {
        format!("/// Soft-delete a {human}: it's hidden from the list and `show`, but kept.")
    } else {
        format!("/// Delete a {human}.")
    };

    format!(
        r#"//! `{ty}` commands — generated by `rata make:resource`, and yours to change.
//!
//! Laravel's resource verbs, one command each. Validation runs on create and
//! update alike, the list sorts through an allowlist and a page is capped, so
//! the frontend can't reach past what's written here.{abilities_note}

use elyra::db::model::Page;
use elyra::{{command, Ctx, Database, Error, Result, Translator, Validator}};
use serde::{{Deserialize, Serialize}};

use {module}::{ty};

/// Rows per page when the frontend doesn't say.
const PER_PAGE: i64 = {PER_PAGE};
/// The most rows a page holds, whatever the frontend asks for.
pub(super) const MAX_PER_PAGE: i64 = {MAX_PER_PAGE};
/// Columns the list sorts by. Anything else sorts by `{pk}`: a sort column from
/// the frontend never reaches SQL unchecked.
const SORTABLE: &[&str] = &[{sortable}];
/// Columns the search box matches.
const SEARCHABLE: &[&str] = &[{searchable}];
/// The rules for [`{ty}Input`], on create and update.
const RULES: &[(&str, &str)] = &[
{rules_list}];

/// What the list asks for: a search term, a sort and a page.
#[derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct {ty}Query {{
    pub search: Option<String>,
    /// A column from `SORTABLE`.
    pub sort: Option<String>,
    /// `"asc"` (the default) or `"desc"`.
    pub direction: Option<String>,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}}

/// What a form may set — no `{pk}` and no timestamps, so a request can't set
/// them. Every field is optional *here* so that a missing one comes back as a
/// validation message rather than a decode error; `RULES` says what's required.
#[derive(Debug, Default, Clone, Serialize, Deserialize, specta::Type)]
pub struct {ty}Input {{
{input_fields}}}

impl {ty}Input {{
    /// Check the input against `RULES`, in the app's language when it has an
    /// `I18nProvider`.
    async fn validate(&self, ctx: &Ctx, db: &Database) -> Result<()> {{
        let data = serde_json::to_value(self).map_err(Error::command)?;
        let translator = ctx.try_get::<Translator>();
        let mut validator = Validator::new(&data).rules(RULES);
        if let Some(translator) = translator.as_deref() {{
            validator = validator.translator(translator);
        }}
        validator.validate_with(db).await?;
        Ok(())
    }}

    /// Copy the validated fields onto `{var}`.
    fn apply(self, {var}: &mut {ty}) {{
{apply}    }}
}}

/// Dispatched after a {human} is created. React with `App::listen`, or send it
/// to every window with `App::broadcast`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct {ty}Created {{
    pub {var}: {ty},
}}

/// Dispatched after a {human} is updated.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct {ty}Updated {{
    pub {var}: {ty},
}}

/// Dispatched after a {human} is deleted.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct {ty}Deleted {{
    pub id: i64,
}}

/// A page of {p}, searched and sorted.
{can_view}
pub async fn {p}_index(ctx: Ctx, query: {ty}Query) -> Result<Page<{ty}>> {{
    let db = ctx.get::<Database>();
    let sort = query
        .sort
        .as_deref()
        .filter(|column| SORTABLE.contains(column))
        .unwrap_or("{pk}");
    let search = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let rows = {ty}::query()
        .when_some(search, |q, term| q.or_where_like(SEARCHABLE, format!("%{{term}}%")));
    let rows = if query.direction.as_deref() == Some("desc") {{
        rows.order_by_desc(sort)
    }} else {{
        rows.order_by(sort)
    }};
    // A stable order within equal values, so rows don't move between pages.
    let rows = rows.when(sort != "{pk}", |q| q.order_by("{pk}"));
    let per_page = query.per_page.unwrap_or(PER_PAGE).clamp(1, MAX_PER_PAGE);
    let page = query.page.unwrap_or(1).max(1);
    Ok(rows.paginate(&db, page, per_page).await?)
}}

/// One {human}.
{can_view}
pub async fn {p}_show(ctx: Ctx, id: i64) -> Result<{ty}> {{
    find(&ctx.get::<Database>(), id).await
}}

/// Create a {human}.
{can_create}
pub async fn {p}_store(ctx: Ctx, input: {ty}Input) -> Result<{ty}> {{
    let db = ctx.get::<Database>();
    input.validate(&ctx, &db).await?;
    let mut {var} = {ty}::default();
    input.apply(&mut {var});
    {var}.insert(&db).await?;
    ctx.dispatch({ty}Created {{
        {var}: {var}.clone(),
    }})
    .await?;
    Ok({var})
}}

/// Update a {human}, with the same rules as create.
{can_update}
pub async fn {p}_update(ctx: Ctx, id: i64, input: {ty}Input) -> Result<{ty}> {{
    let db = ctx.get::<Database>();
    let mut {var} = find(&db, id).await?;
    input.validate(&ctx, &db).await?;
    input.apply(&mut {var});
    {var}.update(&db).await?;
    ctx.dispatch({ty}Updated {{
        {var}: {var}.clone(),
    }})
    .await?;
    Ok({var})
}}

{destroy_doc}
{can_delete}
pub async fn {p}_destroy(ctx: Ctx, id: i64) -> Result<()> {{
    let db = ctx.get::<Database>();
{destroy_find}{destroy}    ctx.dispatch({ty}Deleted {{ id }}).await?;
    Ok(())
}}

async fn find(db: &Database, id: i64) -> Result<{ty}> {{
    {ty}::find(db, id)
        .await?
        .ok_or_else(|| Error::command(format!("{human} {{id}} not found")))
}}
"#,
        module = m.module,
        abilities_note = if abilities {
            "\n//!\n//! Every command needs an ability from `ABILITIES`, which the app must grant."
        } else {
            ""
        },
        sortable = quoted_list(&m.columns),
        searchable = quoted_list(&searchable),
        can_view = can("view"),
        can_create = can("create"),
        can_update = can("update"),
        can_delete = can("delete"),
    )
}

fn sample(field: &Field) -> String {
    match field.kind {
        Kind::Text => format!("Some(format!(\"{} {{n}}\"))", field.name),
        Kind::Int => format!("Some(n as {})", field.ty),
        Kind::Float => format!("Some(n as {} + 0.5)", field.ty),
        Kind::Bool => "Some(true)".into(),
        Kind::Other => "Some(Default::default())".into(),
    }
}

fn column_def(field: &Field) -> String {
    let method = match field.kind {
        Kind::Text => "string",
        Kind::Int => "big_integer",
        Kind::Float => "float",
        Kind::Bool => "boolean",
        Kind::Other => "text",
    };
    let nullable = if field.nullable { ".nullable()" } else { "" };
    format!("        t.{method}(\"{}\"){nullable};\n", field.column)
}

fn render_tests(m: &ModelInfo, n: &Names, abilities: bool) -> String {
    let ty = &n.ty;
    let p = &n.plural;
    let pk = &m.pk;
    let table = &m.table;
    let mut schema = if pk == "id" {
        "        t.id();\n".to_string()
    } else {
        format!("        t.id_named(\"{pk}\");\n")
    };
    for f in &m.editable {
        schema.push_str(&column_def(f));
    }
    if m.timestamps {
        schema.push_str("        t.timestamps();\n");
    }
    if m.soft_deletes {
        schema.push_str("        t.soft_deletes();\n");
    }
    let sample_fields: String = m
        .editable
        .iter()
        .map(|f| format!("        {}: {},\n", f.name, sample(f)))
        .collect();
    let required: Vec<String> = m
        .editable
        .iter()
        .filter(|f| is_required(f))
        .map(|f| f.name.clone())
        .collect();

    // The first text field doubles as a readable probe for update and search.
    let text = m.editable.iter().find(|f| f.kind == Kind::Text);
    let updated_check = match text {
        Some(f) if f.nullable => format!(
            "    assert_eq!(updated.{0}.as_deref(), Some(\"{0} 3\"));\n",
            f.name
        ),
        Some(f) => format!("    assert_eq!(updated.{0}, \"{0} 3\");\n", f.name),
        None => String::new(),
    };
    let search_block = match text {
        Some(f) => format!(
            r#"
    // Search matches any searchable column.
    let hits: Page<{ty}> = app
        .invoke_ok(
            "{p}_index",
            ({ty}Query {{
                search: Some("{name} 2".into()),
                ..Default::default()
            }},),
        )
        .await;
    assert_eq!(hits.total, 1);
"#,
            name = f.name
        ),
        None => String::new(),
    };
    let validation_test = if required.is_empty() {
        String::new()
    } else {
        format!(
            r#"
#[tokio::test]
async fn invalid_input_comes_back_field_by_field() {{
    let app = app().await;
    let bag = app
        .invoke_validation_errors("{p}_store", ({ty}Input::default(),))
        .await
        .expect("a validation bag");
    let required = [{required}];
    assert!(required.iter().all(|f| bag.contains_key(*f)), "{{bag:?}}");
    let page: Page<{ty}> = app.invoke_ok("{p}_index", ({ty}Query::default(),)).await;
    assert_eq!(page.total, 0, "nothing was written");

    // Update runs the same rules.
    let created: {ty} = app.invoke_ok("{p}_store", (input(1),)).await;
    let bag = app
        .invoke_validation_errors("{p}_update", (created.{pk}, {ty}Input::default()))
        .await
        .expect("a validation bag");
    assert!(required.iter().all(|f| bag.contains_key(*f)), "{{bag:?}}");
}}
"#,
            required = quoted_list(&required),
        )
    };
    let abilities_test = if abilities {
        r#"
#[tokio::test]
async fn every_command_needs_one_of_its_abilities() {
    for command in commands() {
        let ability = command
            .ability()
            .unwrap_or_else(|| panic!("`{}` has no `can`", command.name()));
        assert!(ABILITIES.contains(&ability), "{ability}");
    }
    let app = app().await;
    assert!(ABILITIES.iter().all(|a| app.policy().grants_ability(a)));
    // Denied by default: an app that doesn't grant them can't call them.
    assert!(!TestApp::new(App::new()).policy().grants_ability(ABILITIES[0]));
}
"#
        .to_string()
    } else {
        String::new()
    };

    format!(
        r#"//! `{ty}` end to end through `TestApp` — generated by `rata make:resource`.
//! Each test gets its own throwaway SQLite file.

use elyra::db::model::Page;
use elyra::db::schema::Schema;
use elyra::testing::TestApp;
use elyra::{{App, Database, Dispatcher}};

use super::*;
use {module}::{ty};

/// The `{table}` table as the model sees it. Keep it in step with your
/// migration.
fn schema() -> Schema {{
    Schema::create("{table}", |t| {{
{schema}    }})
}}

async fn app() -> TestApp {{
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{p}-test-{{}}-{{n}}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let db = Database::connect(&elyra::db::sqlite_url(&path))
        .await
        .expect("a SQLite file");
    schema().execute(&db).await.expect("the schema");
    TestApp::new(
        App::new()
            .bind(db)
            .swap(Dispatcher::fake())
            .commands(commands())
            .allow_abilities(ABILITIES.iter().copied()),
    )
}}

/// A valid input; `n` makes each one distinct.
fn input(n: u32) -> {ty}Input {{
    {ty}Input {{
{sample_fields}    }}
}}

#[tokio::test]
async fn create_list_show_update_delete() {{
    let app = app().await;
    let created: {ty} = app.invoke_ok("{p}_store", (input(1),)).await;
    assert!(created.{pk} > 0);
    app.invoke_ok::<{ty}>("{p}_store", (input(2),)).await;

    let page: Page<{ty}> = app.invoke_ok("{p}_index", ({ty}Query::default(),)).await;
    assert_eq!(page.total, 2);

    let shown: {ty} = app.invoke_ok("{p}_show", (created.{pk},)).await;
    assert_eq!(shown.{pk}, created.{pk});

    let updated: {ty} = app.invoke_ok("{p}_update", (created.{pk}, input(3))).await;
{updated_check}
    app.invoke_ok::<()>("{p}_destroy", (created.{pk},)).await;
    let gone = app.invoke_err("{p}_show", (created.{pk},)).await;
    assert!(gone.contains("not found"), "{{gone}}");

    let events = app.get::<Dispatcher>();
    events.assert_dispatched::<{ty}Created>();
    events.assert_dispatched::<{ty}Updated>();
    events.assert_dispatched_with(|e: &{ty}Deleted| e.id == created.{pk});
}}

#[tokio::test]
async fn search_sort_and_paging() {{
    let app = app().await;
    for n in 1..=3 {{
        app.invoke_ok::<{ty}>("{p}_store", (input(n),)).await;
    }}
{search_block}
    let newest_first: Page<{ty}> = app
        .invoke_ok(
            "{p}_index",
            ({ty}Query {{
                direction: Some("desc".into()),
                ..Default::default()
            }},),
        )
        .await;
    assert!(newest_first.data[0].{pk} > newest_first.data[1].{pk});

    // A column outside the allowlist is ignored, not interpolated.
    let unknown: Page<{ty}> = app
        .invoke_ok(
            "{p}_index",
            ({ty}Query {{
                sort: Some("{pk}; DROP TABLE {table}".into()),
                ..Default::default()
            }},),
        )
        .await;
    assert_eq!(unknown.total, 3);

    let second: Page<{ty}> = app
        .invoke_ok(
            "{p}_index",
            ({ty}Query {{
                page: Some(2),
                per_page: Some(2),
                ..Default::default()
            }},),
        )
        .await;
    assert_eq!((second.data.len(), second.last_page), (1, 2));

    let capped: Page<{ty}> = app
        .invoke_ok(
            "{p}_index",
            ({ty}Query {{
                per_page: Some(1_000_000),
                ..Default::default()
            }},),
        )
        .await;
    assert_eq!(capped.per_page, MAX_PER_PAGE);
}}
{validation_test}{abilities_test}"#,
        module = m.module,
    )
}

// ---------------------------------------------------------------------------
// The command
// ---------------------------------------------------------------------------

/// The files the resource consists of, rendered.
fn render(m: &ModelInfo, abilities: bool) -> Vec<(String, String)> {
    let n = names(&m.name);
    vec![
        ("mod.rs".into(), render_mod(&n, abilities)),
        ("commands.rs".into(), render_commands(m, &n, abilities)),
        ("tests.rs".into(), render_tests(m, &n, abilities)),
    ]
}

/// Format the generated files the way `cargo fmt` would. Without a `rustfmt`
/// they're left as rendered — valid, just not as tidy.
fn format(paths: &[PathBuf]) {
    let status = std::process::Command::new("rustfmt")
        .args(["--edition", "2021"])
        .args(paths)
        .status();
    if let Ok(status) = status {
        if !status.success() {
            eprintln!("warning: rustfmt couldn't format the generated files ({status})");
        }
    }
}

/// `rata make:resource <Model> [--force] [--dry-run] [--no-abilities]`
pub fn make_resource(cfg: &Config) -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(2).collect();
    run(cfg, &args)
}

fn run(cfg: &Config, args: &[String]) -> Result<(), String> {
    let opts = parse_args(args)?;
    let src = cfg.root.join("src");
    let model = find_model(&src, &opts.model)?;

    let manifest = std::fs::read_to_string(cfg.root.join("Cargo.toml"))
        .map_err(|e| format!("Cargo.toml: {e}"))?;
    let missing = missing_dependencies(&manifest);
    if !missing.is_empty() {
        return Err(format!(
            "Cargo.toml needs, before the resource can build (rata doesn't edit it):\n  {}",
            missing.join("\n  ")
        ));
    }

    let layout = Layout::new(&cfg.root, Path::new(&cfg.frontend_dir));
    let n = names(&model.name);
    let dir = layout.rust.join(&n.module);
    let files = render(&model, opts.abilities);

    // With --view, an existing Rust half is kept: the views are added to it.
    let keep_rust = dir.exists() && !opts.force;
    if keep_rust && !opts.view {
        return Err(format!(
            "{} already exists — pass --force to regenerate it (its {} are overwritten)",
            dir.display(),
            files
                .iter()
                .map(|(f, _)| f.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let views = if opts.view {
        Some(prepare_views(cfg, &model, &n, opts.force)?)
    } else {
        None
    };

    if opts.dry_run {
        println!("Would write (model {} in {}):", model.name, model.module);
        if keep_rust {
            println!("  (keeping {})", dir.display());
        } else {
            for (file, _) in &files {
                println!("  {}", dir.join(file).display());
            }
        }
        if let Some(views) = &views {
            for (file, _) in &views.files {
                println!("  {}", views.dir.join(file).display());
            }
            if let Some((path, _)) = &views.lang {
                println!("  {}  (+ \"{}\" labels)", path.display(), n.plural);
            }
        }
        println!("  and the registries");
        return Ok(());
    }

    if keep_rust {
        println!("Kept {} (it exists; --force regenerates it)", dir.display());
    } else {
        write_all(&dir, &files)?;
        format(&files.iter().map(|(f, _)| dir.join(f)).collect::<Vec<_>>());
    }
    if let Some(views) = &views {
        write_all(&views.dir, &views.files)?;
        match &views.lang {
            Some((path, Some(text))) => {
                std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("Added \"{}\" labels to {}", n.plural, path.display());
            }
            Some((path, None)) => println!(
                "Kept the \"{}\" labels already in {}",
                n.plural,
                path.display()
            ),
            None => {}
        }
    }
    for path in resource::sync(&layout)? {
        println!("Updated {}", path.display());
    }

    let p = &n.plural;
    println!(
        "\nCommands: {p}_index, {p}_show, {p}_store, {p}_update, {p}_destroy{}",
        if opts.abilities {
            format!(" — gated by {p}.view/create/update/delete")
        } else {
            String::new()
        }
    );
    if views.is_some() {
        println!("Pages: #/{p}, #/{p}/new, #/{p}/:id, #/{p}/:id/edit");
    }
    for hint in resource::wiring_hints(&layout) {
        println!("\n{hint}");
    }
    let router = cfg
        .root
        .join(&cfg.frontend_dir)
        .join("src")
        .join("Router.svelte");
    if views.is_some() && !router.is_file() {
        println!(
            "\nThe views need a router: `rata new` scaffolds `Router.svelte` + `routes.js` \
             since 0.8 — see docs/frontend-runtime.md#routing."
        );
    }
    println!(
        "\nThen: `cargo test {}::` and `rata codegen`{}.",
        n.module,
        if views.is_some() {
            " (the views import the typed `api`)"
        } else {
            ""
        }
    );
    Ok(())
}

fn write_all(dir: &Path, files: &[(String, String)]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for (file, contents) in files {
        let path = dir.join(file);
        std::fs::write(&path, contents).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("Created {}", path.display());
    }
    Ok(())
}

/// The Svelte half, rendered and checked but not written yet.
struct Views {
    dir: PathBuf,
    files: Vec<(String, String)>,
    /// `lang/en.json` and its merged text (`None`: the labels are already there).
    lang: Option<(PathBuf, Option<String>)>,
}

fn prepare_views(cfg: &Config, model: &ModelInfo, n: &Names, force: bool) -> Result<Views, String> {
    let frontend_src = cfg.root.join(&cfg.frontend_dir).join("src");
    if !frontend_src.is_dir() {
        return Err(format!("no frontend at {}", frontend_src.display()));
    }
    let rel = views::view_dir(Path::new(&cfg.frontend_dir), n);
    let dir = cfg.root.join(&rel);
    if dir.exists() && !force {
        return Err(format!(
            "{} already exists — pass --force to regenerate the views",
            dir.display()
        ));
    }
    let bindings = views::bindings_import(&rel, Path::new(&cfg.codegen_out));
    let en = cfg.root.join("lang").join("en.json");
    let i18n = en.is_file();
    let (files, labels) = views::render(model, n, &bindings, i18n);
    let lang = if i18n {
        let text = std::fs::read_to_string(&en).map_err(|e| format!("{}: {e}", en.display()))?;
        let merged = views::merge_labels(&text, &n.plural, &labels)
            .map_err(|e| format!("{} {e}", en.display()))?;
        Some((en, merged))
    } else {
        None
    };
    Ok(Views { dir, files, lang })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The test model, parsed.
    pub(crate) fn customer() -> ModelInfo {
        parse(CUSTOMER, "Customer").unwrap()
    }

    const CUSTOMER: &str = r#"
use elyra::Model;
use serde::{Deserialize, Serialize};

#[derive(Model, Serialize, Deserialize, specta::Type, Debug, Default, Clone)]
#[model(table = "customers", timestamps, soft_deletes, has_many(Order, fk = "customer_id"))]
pub struct Customer {
    #[model(id)]
    pub id: i64,
    pub name: String,
    #[model(column = "email_address")]
    pub email: String,
    pub phone: Option<String>,
    pub credit: f64,
    pub visits: i32,
    pub active: bool,
    #[model(cast = json)]
    pub tags: Vec<String>,
    #[model(belongs_to(Team))]
    pub team: Option<Team>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}
"#;

    fn parse(src: &str, name: &str) -> Result<ModelInfo, String> {
        let file = syn::parse_file(src).unwrap();
        let item = file
            .items
            .into_iter()
            .find_map(|i| match i {
                syn::Item::Struct(s) if s.ident == name => Some(s),
                _ => None,
            })
            .unwrap();
        model_info(&item, "crate::customer".into())
    }

    #[test]
    fn reads_the_model() {
        let m = parse(CUSTOMER, "Customer").unwrap();
        assert_eq!(m.table, "customers");
        assert_eq!(m.pk, "id");
        assert!(m.timestamps && m.soft_deletes);
        let editable: Vec<(&str, &str, Kind, bool)> = m
            .editable
            .iter()
            .map(|f| (f.name.as_str(), f.column.as_str(), f.kind, f.nullable))
            .collect();
        assert_eq!(
            editable,
            [
                ("name", "name", Kind::Text, false),
                ("email", "email_address", Kind::Text, false),
                ("phone", "phone", Kind::Text, true),
                ("credit", "credit", Kind::Float, false),
                ("visits", "visits", Kind::Int, false),
                ("active", "active", Kind::Bool, false),
                ("tags", "tags", Kind::Other, false),
            ],
            "no key, no timestamps, no relation"
        );
        assert_eq!(m.editable[6].ty, "Vec<String>");
        assert!(m.columns.contains(&"created_at".to_string()));
        assert!(
            !m.columns.contains(&"deleted_at".to_string()),
            "trashed rows aren't listed"
        );
        assert!(!m.columns.contains(&"team".to_string()));
    }

    #[test]
    fn the_key_must_be_i64() {
        let src = "#[derive(Model)] struct Tag { #[model(id)] slug: String, name: String }";
        assert!(parse(src, "Tag")
            .unwrap_err()
            .contains("needs it to be `i64`"));
        let src = "#[derive(Model)] struct Tag { name: String }";
        assert!(parse(src, "Tag").unwrap_err().contains("no primary key"));
    }

    #[test]
    fn default_table_matches_the_derive() {
        let src = "#[derive(Model)] struct BlogPost { id: i64, title: String }";
        assert_eq!(parse(src, "BlogPost").unwrap().table, "blogpost");
    }

    #[test]
    fn module_paths() {
        let src = Path::new("/p/src");
        assert_eq!(
            module_path(src, Path::new("/p/src/customer.rs")),
            "crate::customer"
        );
        assert_eq!(
            module_path(src, Path::new("/p/src/models/mod.rs")),
            "crate::models"
        );
        assert_eq!(
            module_path(src, Path::new("/p/src/models/customer.rs")),
            "crate::models::customer"
        );
        assert_eq!(module_path(src, Path::new("/p/src/main.rs")), "crate");
    }

    #[test]
    fn rules_follow_the_types() {
        let m = parse(CUSTOMER, "Customer").unwrap();
        let r: Vec<String> = m.editable.iter().map(rules).collect();
        assert_eq!(
            r,
            [
                "required|string",
                "required|string",
                "nullable|string",
                "required|numeric",
                "required|integer",
                "required|boolean",
                "nullable"
            ]
        );
    }

    #[test]
    fn generated_commands_use_the_safety_rails() {
        let m = parse(CUSTOMER, "Customer").unwrap();
        let src = render_commands(&m, &names("Customer"), true);
        for needle in [
            "#[command(can = \"customers.view\")]\npub async fn customers_index",
            "#[command(can = \"customers.delete\")]\npub async fn customers_destroy",
            ".filter(|column| SORTABLE.contains(column))",
            ".clamp(1, MAX_PER_PAGE)",
            "const SEARCHABLE: &[&str] = &[\"name\", \"email_address\", \"phone\"];",
            "input.validate(&ctx, &db).await?;\n    input.apply(&mut customer);\n    customer.update(&db)",
            "soft_delete(&db)",
            "use crate::customer::Customer;",
        ] {
            assert!(src.contains(needle), "missing {needle:?} in:\n{src}");
        }
        // Timestamps and the key aren't settable.
        let input = &src[src.find("pub struct CustomerInput").unwrap()..];
        let input = &input[..input.find('}').unwrap()];
        assert!(!input.contains("created_at") && !input.contains("pub id"));

        let open = render_commands(&m, &names("Customer"), false);
        assert!(!open.contains("can ="));
        assert!(
            render_mod(&names("Customer"), false).contains("pub const ABILITIES: &[&str] = &[];")
        );
    }

    #[test]
    fn a_hard_delete_without_soft_deletes() {
        let src = "#[derive(Model)] struct Note { id: i64, body: String }";
        let m = parse(src, "Note").unwrap();
        let out = render_commands(&m, &names("Note"), true);
        assert!(out.contains("let note = find(&db, id).await?;\n    note.delete(&db).await?;"));
    }

    #[test]
    fn missing_dependencies_are_listed() {
        let bare = "[package]\nname = \"a\"\n[dependencies]\nelyra = { path = \"x\" }\n";
        let missing = missing_dependencies(bare);
        assert_eq!(missing.len(), 3, "{missing:?}");
        let ready = "[dependencies]\nelyra = { path = \"x\", features = [\"database\"] }\n\
                     serde_json = \"1\"\n[dev-dependencies]\n\
                     tokio = { version = \"1\", features = [\"macros\", \"rt-multi-thread\"] }\n";
        assert!(missing_dependencies(ready).is_empty());
    }

    #[test]
    fn flags() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let o = parse_args(&args(&["blog_post", "--no-abilities", "--dry-run"])).unwrap();
        assert_eq!(o.model, "BlogPost");
        assert!(!o.abilities && o.dry_run && !o.force);
        assert!(parse_args(&args(&["Customer", "--view"])).unwrap().view);
        assert!(parse_args(&args(&["Customer", "--generate"]))
            .unwrap_err()
            .contains("step 5"));
        assert!(parse_args(&args(&[])).unwrap_err().contains("usage"));
    }
}
