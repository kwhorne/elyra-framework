//! `rata make:resource <Model> --generate <fields>` — RFC 0001 step 5: the
//! model, its migration, factory and seeder from a field list, so the rest of
//! the resource (commands, views, tests) has something to stand on.
//!
//! Field syntax, Rails-style: `name:type[:modifier…][?][=default]`.
//!
//! | type | Rust | column | rules |
//! |---|---|---|---|
//! | `string` | `String` | `string` (255) | `string\|max:255` |
//! | `text` | `String` | `text` | `string` |
//! | `email` | `String` | `string` | `email\|max:255` |
//! | `integer` / `bigint` | `i64` | `integer` / `big_integer` | `integer` |
//! | `float` | `f64` | `float` | `numeric` |
//! | `bool` | `bool` | `boolean` | `boolean` |
//! | `date` | `String` (ISO) | `string` | `date` |
//! | `json` | `serde_json::Value` (`cast = "json"`) | `text` | — |
//! | `references:Team` | `i64` + `belongs_to(Team)` | `foreign_id` | `integer\|exists:teams,id` |
//!
//! Modifiers: `unique`, `index`; `?` makes the field nullable; `=value` a
//! default. Nothing is inferred from a field's *name*.

use std::path::Path;

use crate::make::{plural, snake};
use crate::make_resource::{
    find_model, migration_struct, Field, Format, Kind, ModelInfo, Names, Reference,
};

/// Column names the model manages itself.
const RESERVED: &[&str] = &["id", "created_at", "updated_at", "deleted_at"];

/// Rust keywords a field can't be named.
const KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "yield", "box", "gen", "try",
];

/// One `name:type…` argument, parsed. `find` resolves a `references:` model.
fn parse_field(
    spec: &str,
    find: &dyn Fn(&str) -> Result<ModelInfo, String>,
    src: &Path,
) -> Result<Field, String> {
    let bad = |why: &str| format!("field `{spec}`: {why}");
    let (head, default) = match spec.split_once('=') {
        Some((head, default)) => (head, Some(default.to_string())),
        None => (spec, None),
    };
    let (head, nullable) = match head.strip_suffix('?') {
        Some(head) => (head, true),
        None => (head, false),
    };
    let mut parts = head.split(':');
    let name = parts.next().unwrap_or_default().to_string();
    let valid_name = name.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if !valid_name {
        return Err(bad("the name must be snake_case"));
    }
    if RESERVED.contains(&name.as_str()) {
        return Err(bad("the model manages this column itself"));
    }
    if KEYWORDS.contains(&name.as_str()) {
        return Err(bad("the name is a Rust keyword"));
    }
    let ty = parts
        .next()
        .ok_or_else(|| bad("missing a type, e.g. `name:string`"))?;

    let (kind, rust, format) = match ty {
        "string" => (Kind::Text, "String", Format::Short),
        "text" => (Kind::Text, "String", Format::Long),
        "email" => (Kind::Text, "String", Format::Email),
        "date" => (Kind::Text, "String", Format::Date),
        "integer" | "bigint" => (Kind::Int, "i64", Format::Plain),
        "float" => (Kind::Float, "f64", Format::Plain),
        "bool" => (Kind::Bool, "bool", Format::Plain),
        "json" => (Kind::Other, "serde_json::Value", Format::Plain),
        "references" => (Kind::Int, "i64", Format::Reference),
        other => {
            return Err(bad(&format!(
                "unknown type `{other}` — expected string, text, email, integer, bigint, float, \
                 bool, date, json or references:<Model>"
            )))
        }
    };

    let mut references = None;
    if format == Format::Reference {
        let model = parts
            .next()
            .ok_or_else(|| bad("`references` names its model: `team_id:references:Team`"))?;
        if !name.ends_with("_id") {
            return Err(bad(
                "a reference is named `<model>_id`, e.g. `team_id:references:Team`",
            ));
        }
        let parent = find(&crate::make::pascal(model))?;
        // A parent made by `--generate` has a migration; the tests run it.
        let generated = parent.module == format!("crate::resources::{}", snake(&parent.name))
            && src
                .join("resources")
                .join(snake(&parent.name))
                .join("migration.rs")
                .is_file();
        references = Some(Reference {
            model: parent.name.clone(),
            module: parent.module.clone(),
            table: parent.table.clone(),
            pk: parent.pk.clone(),
            mirror: (!generated).then(|| Box::new(parent)),
        });
    }

    let (mut unique, mut index) = (false, false);
    for modifier in parts {
        match modifier {
            "unique" => unique = true,
            "index" => index = true,
            other => {
                return Err(bad(&format!(
                    "unknown modifier `{other}` — expected unique or index"
                )))
            }
        }
    }
    if let Some(value) = &default {
        let ok = match (kind, format) {
            (_, Format::Reference) | (Kind::Other, _) => false,
            (Kind::Bool, _) => matches!(value.as_str(), "true" | "false"),
            (Kind::Int, _) => value.parse::<i64>().is_ok(),
            (Kind::Float, _) => value.parse::<f64>().is_ok(),
            (Kind::Text, _) => true,
        };
        if !ok {
            return Err(bad(&format!("`{value}` isn't a default a `{ty}` can have")));
        }
    }

    Ok(Field {
        column: name.clone(),
        name,
        kind,
        nullable,
        ty: rust.to_string(),
        format,
        unique,
        index,
        default,
        references,
    })
}

/// The model `--generate` describes, as the rest of the generator sees one.
pub(crate) fn model_from_fields(
    name: &str,
    specs: &[String],
    src: &Path,
) -> Result<ModelInfo, String> {
    if specs.is_empty() {
        return Err(format!(
            "`--generate` needs the fields, e.g. `rata make:resource {name} --generate \
             name:string email:email:unique active:bool=true`"
        ));
    }
    let find = |model: &str| find_model(src, model);
    let mut editable: Vec<Field> = Vec::new();
    for spec in specs {
        let field = parse_field(spec, &find, src)?;
        if editable.iter().any(|f| f.name == field.name) {
            return Err(format!("field `{}` is listed twice", field.name));
        }
        editable.push(field);
    }
    let module = snake(name);
    let mut columns = vec!["id".to_string()];
    columns.extend(editable.iter().map(|f| f.column.clone()));
    columns.extend(["created_at".to_string(), "updated_at".to_string()]);
    Ok(ModelInfo {
        name: name.to_string(),
        module: format!("crate::resources::{module}"),
        table: plural(&module),
        pk: "id".into(),
        timestamps: true,
        soft_deletes: false,
        editable,
        columns,
        generated: true,
    })
}

fn rust_type(f: &Field) -> String {
    if f.nullable {
        format!("Option<{}>", f.ty)
    } else {
        f.ty.clone()
    }
}

/// The factory's value for a field, from `n`.
fn factory_value(f: &Field, human: &str) -> Option<String> {
    if f.nullable || f.format == Format::Reference {
        return None; // `None` / set by the seeder
    }
    Some(match (f.kind, f.format) {
        (_, Format::Email) => format!(
            "format!(\"{human}{{n}}@example.com\")",
            human = human.replace(' ', "")
        ),
        (_, Format::Date) => "\"2026-01-01\".to_string()".into(),
        (Kind::Text, _) => format!(
            "format!(\"{} {{n}}\")",
            crate::resource_views::humanize(&f.name)
        ),
        (Kind::Int, _) => match &f.default {
            Some(d) => d.clone(),
            None => "n as i64".into(),
        },
        (Kind::Float, _) => match &f.default {
            Some(d) if d.contains('.') => d.clone(),
            Some(d) => format!("{d}.0"),
            None => "n as f64".into(),
        },
        (Kind::Bool, _) => f.default.clone().unwrap_or_else(|| "false".into()),
        (Kind::Other, _) => return None,
    })
}

pub(crate) fn render_model(m: &ModelInfo, n: &Names) -> String {
    let ty = &n.ty;
    let mut attrs = format!("table = \"{}\", timestamps", m.table);
    let mut uses = String::new();
    for f in &m.editable {
        if let Some(r) = &f.references {
            attrs.push_str(&format!(", belongs_to({}, fk = \"{}\")", r.model, f.column));
            let line = format!("use {}::{};\n", r.module, r.model);
            if !uses.contains(&line) {
                uses.push_str(&line);
            }
        }
    }
    if !uses.is_empty() {
        uses.insert(0, '\n');
    }
    let fields: String = m
        .editable
        .iter()
        .map(|f| {
            let cast = if f.kind == Kind::Other {
                "    #[model(cast = \"json\")]\n"
            } else {
                ""
            };
            format!("{cast}    pub {}: {},\n", f.name, rust_type(f))
        })
        .collect();
    let factory: String = m
        .editable
        .iter()
        .filter_map(|f| {
            factory_value(f, &n.human).map(|v| format!("            {}: {v},\n", f.name))
        })
        .collect();
    format!(
        r#"//! The `{ty}` model — generated by `rata make:resource {ty} --generate`, and
//! yours to change. Its table comes from `migration.rs`.

use elyra::{{Factory, Model}};
use serde::{{Deserialize, Serialize}};
{uses}
#[derive(Model, Serialize, Deserialize, specta::Type, Debug, Default, Clone)]
#[model({attrs})]
pub struct {ty} {{
    #[model(id)]
    pub id: i64,
{fields}    pub created_at: i64,
    pub updated_at: i64,
}}

/// Valid rows for tests and the seeder: `{ty}::factory().count(3).create(&db)`.
impl Factory for {ty} {{
    fn definition(n: u64) -> Self {{
        {ty} {{
{factory}            ..Default::default()
        }}
    }}
}}
"#
    )
}

/// A default as SQL: the schema builder takes it raw.
fn sql_default(f: &Field) -> Option<String> {
    let value = f.default.as_ref()?;
    Some(match f.kind {
        Kind::Bool => if value == "true" { "1" } else { "0" }.to_string(),
        Kind::Int | Kind::Float => value.clone(),
        _ => format!("'{}'", value.replace('\'', "''")),
    })
}

pub(crate) fn render_migration(m: &ModelInfo, n: &Names, version: &str) -> String {
    let table = &m.table;
    let name = format!("create_{table}_table");
    let mut columns = String::new();
    let mut indexes = String::new();
    for f in &m.editable {
        let col = &f.column;
        if let Some(r) = &f.references {
            let method = if f.nullable {
                "nullable_foreign_id"
            } else {
                "foreign_id"
            };
            columns.push_str(&format!(
                "            t.{method}(\"{col}\", \"{}\");\n",
                r.table
            ));
            continue;
        }
        let method = match (f.kind, f.format) {
            (_, Format::Long) => "text",
            (Kind::Text, _) => "string",
            (Kind::Int, _) => "big_integer",
            (Kind::Float, _) => "float",
            (Kind::Bool, _) => "boolean",
            // TEXT, not `t.json` (JSONB on Postgres): the JSON cast binds text,
            // which a JSONB column rejects through the `Any` driver.
            (Kind::Other, _) => "text",
        };
        let mut chain = String::new();
        // The JSON cast stores `null` as SQL NULL, so a JSON column always
        // allows it — even when the field isn't an `Option`.
        if f.nullable || f.kind == Kind::Other {
            chain.push_str(".nullable()");
        }
        if f.unique {
            chain.push_str(".unique()");
        }
        if let Some(default) = sql_default(f) {
            chain.push_str(&format!(".default_value({default:?})"));
        }
        columns.push_str(&format!("            t.{method}(\"{col}\"){chain};\n"));
        if f.index && !f.unique {
            indexes.push_str(&format!("            t.index(\"{col}\");\n"));
        }
    }
    format!(
        r#"//! The `{table}` table — generated by `rata make:resource {ty} --generate`.
//! Run it with `ELYRA_MIGRATE=up cargo run`; the registry lists it.

use elyra::db::{{Driver, RustMigration, Schema}};

pub struct {st};

impl RustMigration for {st} {{
    fn version(&self) -> &str {{
        "{version}"
    }}

    fn name(&self) -> &str {{
        "{name}"
    }}

    fn up(&self, driver: Driver) -> Vec<String> {{
        Schema::create("{table}", |t| {{
            t.id();
{columns}{indexes}            t.timestamps();
        }})
        .to_sql(driver)
    }}

    fn down(&self, driver: Driver) -> Vec<String> {{
        Schema::drop_if_exists("{table}").to_sql(driver)
    }}
}}
"#,
        ty = n.ty,
        st = migration_struct(n),
    )
}

pub(crate) fn render_seeder(m: &ModelInfo, n: &Names) -> String {
    let ty = &n.ty;
    let mut uses = String::new();
    let mut parents = String::new();
    let mut state = String::new();
    for f in &m.editable {
        let Some(r) = &f.references else { continue };
        let var = snake(&r.model);
        if !uses.contains(&format!("::{};\n", r.model)) {
            uses.push_str(&format!("use {}::{};\n", r.module, r.model));
            parents.push_str(&format!(
                "            // Point at an existing {human}, or make one.\n            \
                 let {var}_id = match {model}::query().first(db).await? {{\n                \
                 Some({var}) => {var}.{pk},\n                \
                 None => {{\n                    \
                 let mut {var} = {model}::default();\n                    \
                 {var}.insert(db).await?;\n                    \
                 {var}.{pk}\n                \
                 }}\n            \
                 }};\n",
                human = var.replace('_', " "),
                model = r.model,
                pk = r.pk,
            ));
        }
        let value = if f.nullable {
            format!("Some({var}_id)")
        } else {
            format!("{var}_id")
        };
        state.push_str(&format!("                    row.{} = {value};\n", f.name));
    }
    let build = if state.is_empty() {
        format!("            {ty}::factory().count(20).create(db).await?;\n")
    } else {
        format!(
            "            {ty}::factory()\n                .count(20)\n                \
             .state(move |row| {{\n{state}                }})\n                \
             .create(db)\n                .await?;\n"
        )
    };
    format!(
        r#"//! Demo `{ty}` rows — generated by `rata make:resource {ty} --generate`.
//! `ELYRA_SEED=1 cargo run` runs it; the registry lists it.

use elyra::db::Database;
use elyra::seeder::{{BoxSeed, Seeder}};
use elyra::Factory;

use super::{ty};
{uses}
pub struct {ty}Seeder;

impl Seeder for {ty}Seeder {{
    fn name(&self) -> &str {{
        "{ty}Seeder"
    }}

    fn run<'a>(&'a self, db: &'a Database) -> BoxSeed<'a> {{
        Box::pin(async move {{
{parents}{build}            Ok(())
        }})
    }}
}}
"#
    )
}

/// The version an existing migration was written with, so regenerating it
/// (`--force`) doesn't make it look new to a database that already ran it.
pub(crate) fn existing_version(migration: &str) -> Option<String> {
    let after = migration.split("fn version(&self) -> &str {").nth(1)?;
    let start = after.find('"')? + 1;
    let end = start + after[start..].find('"')?;
    Some(after[start..end].to_string())
}

/// The versions of the resources' migrations already on disk.
pub(crate) fn resource_versions(resources: &Path) -> Vec<u64> {
    let Ok(read) = std::fs::read_dir(resources) else {
        return Vec::new();
    };
    read.flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("migration.rs")).ok())
        .filter_map(|text| existing_version(&text)?.parse().ok())
        .collect()
}

/// A version after every existing one. Versions are seconds, and two
/// resources generated in the same second would otherwise collide — and a
/// parent's table must come first, since its children reference it.
pub(crate) fn next_version(now: u64, existing: &[u64]) -> u64 {
    existing.iter().map(|v| v + 1).fold(now, u64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::make_resource::names;

    fn no_models(model: &str) -> Result<ModelInfo, String> {
        Err(format!("no model {model}"))
    }

    fn field(spec: &str) -> Result<Field, String> {
        parse_field(spec, &no_models, Path::new("/nowhere/src"))
    }

    #[test]
    fn parses_the_field_syntax() {
        let f = field("email:email:unique").unwrap();
        assert_eq!(
            (f.kind, f.format, f.unique, f.nullable),
            (Kind::Text, Format::Email, true, false)
        );
        let f = field("phone:string?").unwrap();
        assert!(f.nullable && f.format == Format::Short);
        let f = field("active:bool=true").unwrap();
        assert_eq!(f.default.as_deref(), Some("true"));
        let f = field("status:string:index?=draft").unwrap();
        assert!(f.index && f.nullable);
        assert_eq!(f.default.as_deref(), Some("draft"));
        let f = field("meta:json?").unwrap();
        assert_eq!((f.kind, f.ty.as_str()), (Kind::Other, "serde_json::Value"));
    }

    #[test]
    fn rejects_what_it_cant_generate() {
        for (spec, why) in [
            ("Name:string", "snake_case"),
            ("id:integer", "manages this column"),
            ("type:string", "Rust keyword"),
            ("name", "missing a type"),
            ("name:varchar", "unknown type"),
            ("name:string:primary", "unknown modifier"),
            ("count:integer=many", "isn't a default"),
            ("flag:bool=yes", "isn't a default"),
            ("team:references:Team", "`<model>_id`"),
            ("team_id:references", "names its model"),
            ("team_id:references:Team", "no model Team"),
        ] {
            let err = field(spec).unwrap_err();
            assert!(err.contains(why), "{spec}: {err}");
        }
    }

    fn sample() -> ModelInfo {
        let specs: Vec<String> = [
            "name:string",
            "email:email:unique",
            "bio:text?",
            "active:bool=true",
            "score:float=1.5",
            "visits:integer:index",
            "meta:json?",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        model_from_fields("Customer", &specs, Path::new("/nowhere/src")).unwrap()
    }

    #[test]
    fn the_model_matches_the_fields() {
        let src = render_model(&sample(), &names("Customer"));
        for needle in [
            "#[model(table = \"customers\", timestamps)]",
            "    pub email: String,\n",
            "    pub bio: Option<String>,\n",
            "    #[model(cast = \"json\")]\n    pub meta: Option<serde_json::Value>,\n",
            "            email: format!(\"customer{n}@example.com\"),\n",
            "            active: true,\n",
            "            score: 1.5,\n",
            "            name: format!(\"Name {n}\"),\n",
        ] {
            assert!(src.contains(needle), "missing {needle:?} in:\n{src}");
        }
        let factory = &src[src.find("impl Factory").unwrap()..];
        assert!(
            !factory.contains("bio:"),
            "a nullable field is left to Default"
        );
    }

    #[test]
    fn the_migration_matches_the_fields() {
        let src = render_migration(&sample(), &names("Customer"), "1790000000");
        for needle in [
            "pub struct CreateCustomersTable;",
            "\"1790000000\"",
            "\"create_customers_table\"",
            "t.string(\"name\");",
            "t.string(\"email\").unique();",
            "t.text(\"bio\").nullable();",
            "t.boolean(\"active\").default_value(\"1\");",
            "t.float(\"score\").default_value(\"1.5\");",
            "t.big_integer(\"visits\");",
            "t.index(\"visits\");",
            "t.text(\"meta\").nullable();",
            "Schema::drop_if_exists(\"customers\")",
        ] {
            assert!(src.contains(needle), "missing {needle:?} in:\n{src}");
        }
        assert_eq!(existing_version(&src).as_deref(), Some("1790000000"));
    }

    #[test]
    fn versions_always_move_forward() {
        assert_eq!(next_version(100, &[]), 100);
        assert_eq!(next_version(100, &[100]), 101, "same second");
        assert_eq!(next_version(100, &[100, 101, 50]), 102);
        assert_eq!(next_version(200, &[100]), 200);
    }

    #[test]
    fn a_string_default_is_quoted_for_sql() {
        let f = field("status:string=it's").unwrap();
        assert_eq!(sql_default(&f).as_deref(), Some("'it''s'"));
    }
}
