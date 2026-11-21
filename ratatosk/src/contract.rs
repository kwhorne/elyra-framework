//! `rata check --backend` (RFC 0040): the desktop app held against what its
//! Laravel API says it is.
//!
//! Laravel describes itself with `php artisan elyra:contract`: its routes,
//! the rules of each route's FormRequest, the abilities there are, and the
//! shape of what each GET answered for a real user. The app's check mode
//! gives each command's types as JSON Schema, and the sources say which
//! routes each command calls. [`findings`] compares them.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde_json::Value;
use syn::visit::Visit;

use crate::check::{Finding, Level};

/// A call a command makes to the backend: `GET /api/customers/{}`, and the
/// argument it sends as the body, if one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub method: String,
    /// The path, each `{…}` placeholder as `{}`; `None` when it isn't known
    /// from the source.
    pub path: Option<String>,
    pub body: Option<String>,
}

/// Each command's backend calls, read from the Rust sources under `src`.
pub fn calls(src: &Path) -> BTreeMap<String, Vec<Call>> {
    let mut files = Vec::new();
    crate::make_resource::rust_files(src, &mut files);
    let mut out = BTreeMap::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        if !text.contains("Backend") {
            continue;
        }
        if let Ok(parsed) = syn::parse_file(&text) {
            calls_in(&parsed, &mut out);
        }
    }
    out
}

/// The backend calls of each `#[command]` in `file`.
pub fn calls_in(file: &syn::File, out: &mut BTreeMap<String, Vec<Call>>) {
    let mut consts = HashMap::new();
    collect_consts(&file.items, &mut consts);
    commands_in(&file.items, &consts, out);
}

fn collect_consts(items: &[syn::Item], consts: &mut HashMap<String, String>) {
    for item in items {
        match item {
            syn::Item::Const(c) => {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &*c.expr
                {
                    consts.insert(c.ident.to_string(), s.value());
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    collect_consts(items, consts);
                }
            }
            _ => {}
        }
    }
}

fn commands_in(
    items: &[syn::Item],
    consts: &HashMap<String, String>,
    out: &mut BTreeMap<String, Vec<Call>>,
) {
    for item in items {
        match item {
            syn::Item::Fn(f)
                if f.attrs.iter().any(|a| {
                    a.path()
                        .segments
                        .last()
                        .is_some_and(|s| s.ident == "command")
                }) =>
            {
                let mut finder = Finder {
                    consts,
                    calls: Vec::new(),
                    edit_save: None,
                };
                finder.visit_block(&f.block);
                if !finder.calls.is_empty() {
                    out.insert(f.sig.ident.to_string(), finder.calls);
                }
            }
            syn::Item::Mod(m) => {
                if let Some((_, items)) = &m.content {
                    commands_in(items, consts, out);
                }
            }
            _ => {}
        }
    }
}

const METHODS: &[&str] = &["get", "post", "put", "patch", "delete"];

struct Finder<'a> {
    consts: &'a HashMap<String, String>,
    calls: Vec<Call>,
    /// The save of the last `.edit(..)`, until a `.save_back(..)` on it.
    edit_save: Option<usize>,
}

/// `Method::Post` → `POST`.
fn method_of(expr: &syn::Expr) -> Option<String> {
    let syn::Expr::Path(p) = expr else {
        return None;
    };
    let name = p.path.segments.last()?.ident.to_string().to_uppercase();
    METHODS
        .contains(&name.to_lowercase().as_str())
        .then_some(name)
}

impl<'ast> Visit<'ast> for Finder<'_> {
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        // `.get("/api/…")` on the backend: one argument, no turbofish (that's
        // `ctx.get::<Backend>()`), on a receiver that names the backend.
        if METHODS.contains(&name.as_str())
            && call.turbofish.is_none()
            && call.args.len() == 1
            && names_backend(&call.receiver)
        {
            let path = call.args.first().and_then(|a| self.path(a));
            self.calls.push(Call {
                method: name.to_uppercase(),
                path,
                body: None,
            });
        }
        // `.edit("/api/…/contract")` (RFC 0038): the file is downloaded with
        // a GET, and each save sent back with a PUT, to the same route
        // unless `.save_back(..)` says another.
        let edit = name == "edit" && call.args.len() == 1 && names_backend(&call.receiver);
        if edit {
            let path = call.args.first().and_then(|a| self.path(a));
            for method in ["GET", "PUT"] {
                self.calls.push(Call {
                    method: method.into(),
                    path: path.clone(),
                    body: None,
                });
            }
            self.edit_save = Some(self.calls.len() - 1);
        }
        syn::visit::visit_expr_method_call(self, call);
        // `.save_back(path)` / `.save_back_with(Method::Post, path)` after
        // `.edit(..)`: where its saves go.
        if name == "save_back" || name == "save_back_with" {
            if let Some(at) = self.edit_save.take() {
                let (method, path) = match (name.as_str(), call.args.len()) {
                    ("save_back", 1) => (Some("PUT".to_owned()), call.args.first()),
                    ("save_back_with", 2) => (
                        call.args.first().and_then(method_of),
                        call.args.iter().nth(1),
                    ),
                    _ => (None, None),
                };
                if let (Some(method), Some(path)) = (method, path) {
                    let path = self.path(path);
                    if let Some(save) = self.calls.get_mut(at) {
                        save.method = method;
                        save.path = path;
                    }
                }
            }
        }
        // `.body(&input)` after it: the last call's body.
        if name == "body" || name == "fields" {
            if let Some(arg) = call.args.first().and_then(ident_of) {
                if let Some(last) = self.calls.last_mut() {
                    last.body.get_or_insert(arg);
                }
            }
        }
    }
}

impl Finder<'_> {
    /// The path a call is made to: a literal, a `format!` of one, or a
    /// constant (`PATH`), with each placeholder as `{}`.
    fn path(&self, expr: &syn::Expr) -> Option<String> {
        match expr {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(s),
                ..
            }) => Some(placeholders(&s.value())),
            syn::Expr::Reference(r) => self.path(&r.expr),
            syn::Expr::Path(p) => p
                .path
                .get_ident()
                .and_then(|i| self.consts.get(&i.to_string()))
                .map(|v| placeholders(v)),
            syn::Expr::Macro(m) if m.mac.path.is_ident("format") => {
                let args = m
                    .mac
                    .parse_body_with(
                        syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated,
                    )
                    .ok()?;
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit),
                    ..
                }) = args.first()?
                else {
                    return None;
                };
                // `{PATH}` is the constant's value; any other `{…}` is an argument.
                let mut out = String::new();
                let mut rest = lit.value();
                while let Some(open) = rest.find('{') {
                    out.push_str(&rest[..open]);
                    let close = rest[open..].find('}')? + open;
                    let name = &rest[open + 1..close];
                    match self.consts.get(name.split(':').next().unwrap_or_default()) {
                        Some(value) => out.push_str(value),
                        None => out.push_str("{}"),
                    }
                    rest = rest[close + 1..].to_owned();
                }
                out.push_str(&rest);
                Some(placeholders(&out))
            }
            syn::Expr::MethodCall(m) if m.method == "to_string" || m.method == "into" => {
                self.path(&m.receiver)
            }
            _ => None,
        }
    }
}

/// Whether the receiver chain reaches the backend: `ctx.get::<Backend>()`,
/// a variable named `backend`, or a field of that name.
fn names_backend(expr: &syn::Expr) -> bool {
    let text = quote::ToTokens::to_token_stream(expr).to_string();
    text.contains("Backend")
        || text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .any(|w| w == "backend")
}

fn ident_of(expr: &syn::Expr) -> Option<String> {
    match expr {
        syn::Expr::Reference(r) => ident_of(&r.expr),
        syn::Expr::Path(p) => p.path.get_ident().map(ToString::to_string),
        _ => None,
    }
}

/// `{id}`, `{customer}` and `{}` alike, so two paths compare by shape.
fn placeholders(path: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for c in path.split(['?', '#']).next().unwrap_or(path).chars() {
        match c {
            '{' => {
                inside = true;
                out.push_str("{}");
            }
            '}' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out
}

/// What's wrong between the app (`shape`, with each command's `types`), its
/// backend `calls` and the server's `contract`.
pub fn findings(
    shape: &Value,
    calls: &BTreeMap<String, Vec<Call>>,
    contract: &Value,
) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut push = |check, level, command: &str, ability: Option<&str>, message, fix| {
        found.push(Finding {
            check,
            level,
            command: Some(command.to_owned()),
            ability: ability.map(str::to_owned),
            message,
            fix,
        });
    };
    let routes: Vec<&Value> = contract["routes"]
        .as_array()
        .into_iter()
        .flatten()
        .collect();
    let abilities: Vec<&str> = contract["abilities"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let ability_of: HashMap<&str, &str> = shape["commands"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| Some((c["name"].as_str()?, c["ability"].as_str()?)))
        .collect();

    for (command, made) in calls {
        let types = &shape["types"][command];
        for call in made {
            let Some(path) = &call.path else {
                push(
                    "backend-not-checked",
                    Level::Info,
                    command,
                    None,
                    format!("{command} calls the backend with a {} path built in a way rata can't read.", call.method),
                    "Use a literal, a format! of one, or a resource's PATH constant, or allow it.".into(),
                );
                continue;
            };
            let Some(route) = routes.iter().find(|r| {
                r["method"].as_str() == Some(call.method.as_str())
                    && r["uri"].as_str().map(placeholders).as_deref() == Some(path.as_str())
            }) else {
                push(
                    "backend-route-missing",
                    Level::Error,
                    command,
                    None,
                    format!(
                        "{command} calls {} {path}, which the server doesn't have.",
                        call.method
                    ),
                    "Add the route to routes/api.php, or call the one the server has.".into(),
                );
                continue;
            };
            if call.method == "GET" {
                answer(command, types, route, &mut push);
            } else if let Some(body) = &call.body {
                rules(command, types, body, route, &mut push);
            }
        }
        if let Some(ability) = ability_of.get(command.as_str()) {
            let known = abilities.iter().any(|a| {
                *a == "*"
                    || *a == *ability
                    || a.strip_suffix('*').is_some_and(|p| ability.starts_with(p))
            });
            if !abilities.is_empty() && !known {
                push(
                    "backend-ability-unknown",
                    Level::Warning,
                    command,
                    Some(ability),
                    format!("{command} needs `{ability}`, which no user on the server has."),
                    "Add the permission on the server, or correct the command's `can`.".into(),
                );
            }
        }
    }
    found
}

type Push<'a> = dyn FnMut(&'static str, Level, &str, Option<&str>, String, String) + 'a;

/// The command's return type against what the route answered.
fn answer(command: &str, types: &Value, route: &Value, push: &mut Push) {
    let Some(fields) = route["answer"]["fields"].as_object() else {
        return;
    };
    let defs = &types["$defs"];
    let Some(record) = record_schema(&types["returns"], defs) else {
        return;
    };
    let Some(properties) = record["properties"].as_object() else {
        return;
    };
    let uri = route["uri"].as_str().unwrap_or_default();
    for (name, schema) in properties {
        let schema = resolve(schema, defs);
        let nullable = nullable(schema);
        match fields.get(name).and_then(Value::as_str) {
            None if !nullable => {
                let closest = closest(name, fields.keys());
                push(
                    "backend-field-missing",
                    Level::Error,
                    command,
                    None,
                    format!(
                        "{command} decodes `{name}`, which GET {uri} doesn't send{}.",
                        closest
                            .map(|c| format!("; it sends `{c}`"))
                            .unwrap_or_default()
                    ),
                    match closest {
                        Some(c) => {
                            format!("Rename the field to `{c}`, or #[serde(rename = \"{c}\")] it.")
                        }
                        None => "Make it an Option, or have the server's resource send it.".into(),
                    },
                );
            }
            None => {}
            Some(sent) => {
                let sent: Vec<&str> = sent.split('|').collect();
                if sent.contains(&"null") && !nullable {
                    push(
                        "backend-field-nullable",
                        Level::Warning,
                        command,
                        None,
                        format!("{command} decodes `{name}` as never null, and GET {uri} sent null for it."),
                        "Make it an Option, or have the server never send null.".into(),
                    );
                }
                let wanted = kinds(schema, defs);
                if !wanted.is_empty() {
                    if let Some(bad) = sent.iter().find(|t| **t != "null" && !fits(t, &wanted)) {
                        push(
                            "backend-field-type",
                            Level::Error,
                            command,
                            None,
                            format!(
                                "{command} decodes `{name}` as {}, and GET {uri} sends {bad}.",
                                wanted.join(" or ")
                            ),
                            "Change the field's type to what the server sends.".into(),
                        );
                    }
                }
            }
        }
    }
}

/// The input the command sends against the route's rules.
fn rules(command: &str, types: &Value, body: &str, route: &Value, push: &mut Push) {
    let uri = route["uri"].as_str().unwrap_or_default();
    let method = route["method"].as_str().unwrap_or_default();
    let Some(rules) = route["rules"].as_object() else {
        push(
            "backend-not-checked",
            Level::Info,
            command,
            None,
            format!("{method} {uri} has no FormRequest, so its rules can't be read."),
            "Give the action a FormRequest, or allow it.".into(),
        );
        return;
    };
    let defs = &types["$defs"];
    let Some(arg) = types["args"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"].as_str() == Some(body))
    else {
        return;
    };
    let schema = resolve(&arg["schema"], defs);
    let Some(properties) = schema["properties"].as_object() else {
        return;
    };
    // A field's own rule says whether it's required; `field.*` and
    // `field.name` are its items' and its fields', and only name it.
    let own: BTreeMap<&str, &str> = rules
        .iter()
        .filter(|(k, _)| !k.contains('.'))
        .filter_map(|(k, v)| Some((k.as_str(), v.as_str()?)))
        .collect();
    let named: std::collections::BTreeSet<&str> =
        rules.keys().filter_map(|k| k.split('.').next()).collect();
    for (field, rule) in &own {
        let required = rule.split('|').any(|r| r == "required") && !rule.contains("sometimes");
        if required && !properties.contains_key(*field) {
            push(
                "backend-input-required",
                Level::Error,
                command,
                None,
                format!(
                    "{command} sends `{body}`, which has no `{field}`; {method} {uri} requires it."
                ),
                format!("Add `{field}` to the input type, or make the rule `sometimes`."),
            );
        }
    }
    for field in properties.keys() {
        if !named.contains(field.as_str()) {
            push(
                "backend-input-unknown",
                Level::Warning,
                command,
                None,
                format!("{command} sends `{field}`, which {method} {uri}'s rules don't name: Laravel's validated() drops it."),
                "Add a rule for it on the server, or leave it out of the input type.".into(),
            );
        }
    }
}

/// The record a return type decodes: `T`, `Option<T>`, `Vec<T>`, or a page
/// (`{ data: [T] }`, Laravel's paginator) or a resource (`{ data: T }`).
fn record_schema<'a>(schema: &'a Value, defs: &'a Value) -> Option<&'a Value> {
    let schema = resolve(schema, defs);
    if let Some(variants) = schema["anyOf"].as_array() {
        return variants
            .iter()
            .find(|v| v["type"] != "null")
            .and_then(|v| record_schema(v, defs));
    }
    if schema["type"] == "array" {
        return record_schema(&schema["items"], defs);
    }
    let data = &schema["properties"]["data"];
    if !data.is_null() {
        let data = resolve(data, defs);
        if data["type"] == "array" {
            return record_schema(&data["items"], defs);
        }
        if data["properties"].is_object() {
            return Some(data);
        }
    }
    schema["properties"].is_object().then_some(schema)
}

fn resolve<'a>(schema: &'a Value, defs: &'a Value) -> &'a Value {
    match schema["$ref"]
        .as_str()
        .and_then(|r| r.strip_prefix("#/$defs/"))
    {
        Some(name) => resolve(&defs[name], defs),
        None => schema,
    }
}

fn nullable(schema: &Value) -> bool {
    match &schema["type"] {
        Value::Array(types) => types.iter().any(|t| t == "null"),
        Value::String(t) => t == "null",
        _ => schema["anyOf"]
            .as_array()
            .is_some_and(|v| v.iter().any(|s| s["type"] == "null")),
    }
}

/// The JSON types a schema takes, `null` aside.
fn kinds(schema: &Value, defs: &Value) -> Vec<String> {
    let schema = resolve(schema, defs);
    let mut out: Vec<String> = match &schema["type"] {
        Value::String(t) => vec![t.clone()],
        Value::Array(types) => types
            .iter()
            .filter_map(|t| t.as_str().map(str::to_owned))
            .collect(),
        _ => schema["anyOf"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|s| kinds(s, defs))
            .collect(),
    };
    out.retain(|t| t != "null");
    out
}

/// Whether a sampled type (`int`, `string`, …) decodes as one of `wanted`.
fn fits(sent: &str, wanted: &[String]) -> bool {
    wanted.iter().any(|w| {
        matches!(
            (sent, w.as_str()),
            ("int", "integer" | "number")
                | ("float", "number")
                | ("string", "string")
                | ("bool", "boolean")
                | ("array", "array")
                | ("object", "object")
        )
    })
}

/// The name in `names` closest to `name`, when one is close: `phone` →
/// `phone_number`.
fn closest<'a>(name: &str, names: impl Iterator<Item = &'a String>) -> Option<&'a str> {
    names
        .map(|n| (distance(name, n), n))
        .filter(|(d, n)| {
            *d <= name.len().max(n.len()) / 2 || n.contains(name) || name.contains(n.as_str())
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, n)| n.as_str())
}

fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let current = row[j + 1];
            row[j + 1] = if ca == b[j] {
                previous
            } else {
                1 + previous.min(row[j]).min(row[j + 1])
            };
            previous = current;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn source() -> BTreeMap<String, Vec<Call>> {
        let file = syn::parse_file(
            r#"
            pub const PATH: &str = "/api/customers";

            #[command(live, can = "customers.view")]
            pub async fn customers_index(ctx: Ctx, query: CustomerQuery) -> Result<Page<Customer>> {
                Ok(ctx.get::<Backend>().get(PATH).query(&query).json().await?)
            }

            #[command(live, can = "customers.view")]
            pub async fn customers_show(ctx: Ctx, id: i64) -> Result<Customer> {
                Ok(ctx.get::<Backend>().get(format!("{PATH}/{id}")).resource().await?)
            }

            #[command(can = "customers.create")]
            pub async fn customers_store(ctx: Ctx, input: CustomerInput) -> Result<Customer> {
                let backend = ctx.get::<Backend>();
                Ok(backend.post("/api/customers").body(&input).queue_offline().resource().await?)
            }

            #[command(can = "notes.archive")]
            pub async fn notes_archive(ctx: Ctx, id: i64) -> Result<()> {
                ctx.get::<Backend>().post(format!("/api/notes/{id}/archive")).send().await?;
                Ok(())
            }

            #[command]
            pub async fn odd(ctx: Ctx, path: String) -> Result<()> {
                ctx.get::<Backend>().delete(path).send().await?;
                Ok(())
            }

            #[command]
            pub async fn local(ctx: Ctx) -> Result<i64> { Ok(ctx.get::<Database>().count().await) }

            #[command]
            pub async fn contract_edit(ctx: Ctx, id: i64) -> Result<Editing> {
                Ok(ctx.get::<Backend>().edit(format!("{PATH}/{id}/contract")).only(["docx"]).open().await?)
            }

            #[command]
            pub async fn sheet_edit(ctx: Ctx, id: i64) -> Result<Editing> {
                let backend = ctx.get::<Backend>();
                Ok(backend
                    .edit(format!("/api/sheets/{id}/file"))
                    .save_back_with(Method::Post, format!("/api/sheets/{id}/versions"))
                    .open()
                    .await?)
            }
            "#,
        )
        .unwrap();
        let mut out = BTreeMap::new();
        calls_in(&file, &mut out);
        out
    }

    #[test]
    fn reads_which_routes_each_command_calls() {
        let calls = source();
        let get = |c: &str| {
            calls[c]
                .iter()
                .map(|c| (c.method.as_str(), c.path.as_deref(), c.body.as_deref()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            get("customers_index"),
            [("GET", Some("/api/customers"), None)]
        );
        assert_eq!(
            get("customers_show"),
            [("GET", Some("/api/customers/{}"), None)]
        );
        assert_eq!(
            get("customers_store"),
            [("POST", Some("/api/customers"), Some("input"))]
        );
        assert_eq!(
            get("notes_archive"),
            [("POST", Some("/api/notes/{}/archive"), None)]
        );
        assert_eq!(get("odd"), [("DELETE", None, None)]);
        assert!(!calls.contains_key("local"));
        // A file edited in its own app: downloaded, and saved back.
        assert_eq!(
            get("contract_edit"),
            [
                ("GET", Some("/api/customers/{}/contract"), None),
                ("PUT", Some("/api/customers/{}/contract"), None)
            ]
        );
        assert_eq!(
            get("sheet_edit"),
            [
                ("GET", Some("/api/sheets/{}/file"), None),
                ("POST", Some("/api/sheets/{}/versions"), None)
            ]
        );
    }

    fn shape() -> Value {
        let customer = json!({
            "type": "object",
            "properties": {
                "id": { "type": "integer" },
                "name": { "type": "string" },
                "email": { "type": "string" },
                "phone": { "type": "string" },
                "notes": { "type": ["string", "null"] },
                "credit": { "type": "integer" },
            },
        });
        json!({
            "commands": [
                { "name": "customers_index", "ability": "customers.view" },
                { "name": "customers_show", "ability": "customers.view" },
                { "name": "customers_store", "ability": "customers.create" },
                { "name": "notes_archive", "ability": "notes.archive" },
            ],
            "types": {
                "customers_index": {
                    "args": [], "$defs": { "Customer": customer },
                    "returns": { "type": "object", "properties": { "data": { "type": "array", "items": { "$ref": "#/$defs/Customer" } } } },
                },
                "customers_show": { "args": [], "$defs": { "Customer": customer }, "returns": { "$ref": "#/$defs/Customer" } },
                "customers_store": {
                    "args": [{ "name": "input", "schema": { "type": "object", "properties": { "name": {}, "email": {}, "nickname": {} } } }],
                    "$defs": {}, "returns": {},
                },
            },
        })
    }

    fn contract() -> Value {
        let fields = json!({ "id": "int", "name": "string", "email": "string|null", "phone_number": "string", "notes": "null", "credit": "string" });
        json!({
            "routes": [
                { "method": "GET", "uri": "/api/customers", "answer": { "kind": "page", "fields": fields } },
                { "method": "GET", "uri": "/api/customers/{customer}", "answer": { "kind": "record", "fields": fields } },
                { "method": "POST", "uri": "/api/customers", "rules": { "name": "required|string", "email": "required|email", "country": "required|string", "tags.*": "string", "address": "required|array", "address.city": "nullable|string" } },
                { "method": "GET", "uri": "/api/customers/{customer}/contract" },
                { "method": "PUT", "uri": "/api/customers/{customer}/contract" },
            ],
            "abilities": ["customers.view", "customers.create"],
        })
    }

    #[test]
    fn finds_where_the_halves_disagree() {
        let found = findings(&shape(), &source(), &contract());
        let seen: Vec<(&str, &str, String)> = found
            .iter()
            .map(|f| (f.check, f.command.as_deref().unwrap(), f.message.clone()))
            .collect();
        let has = |check: &str, command: &str, needle: &str| {
            assert!(
                seen.iter()
                    .any(|(c, k, m)| *c == check && *k == command && m.contains(needle)),
                "no {check} for {command} with {needle:?} in {seen:#?}"
            )
        };
        has(
            "backend-field-missing",
            "customers_index",
            "`phone`, which GET /api/customers doesn't send; it sends `phone_number`",
        );
        has("backend-field-missing", "customers_show", "`phone`");
        has("backend-field-nullable", "customers_index", "`email`");
        has(
            "backend-field-type",
            "customers_index",
            "`credit` as integer",
        );
        has("backend-input-required", "customers_store", "no `country`");
        // `address.city` names `address`; it doesn't make it not required.
        has("backend-input-required", "customers_store", "no `address`");
        // The edited file's routes: the contract's are there, the sheet's
        // aren't.
        has(
            "backend-route-missing",
            "sheet_edit",
            "GET /api/sheets/{}/file",
        );
        has(
            "backend-route-missing",
            "sheet_edit",
            "POST /api/sheets/{}/versions",
        );
        assert!(!seen.iter().any(|(_, k, _)| *k == "contract_edit"));
        has("backend-input-unknown", "customers_store", "`nickname`");
        has(
            "backend-route-missing",
            "notes_archive",
            "POST /api/notes/{}/archive",
        );
        has(
            "backend-ability-unknown",
            "notes_archive",
            "`notes.archive`",
        );
        has("backend-not-checked", "odd", "can't read");
        // `notes` may be null; `tags.*` isn't a required field.
        assert!(!seen
            .iter()
            .any(|(_, _, m)| m.contains("`notes`") || m.contains("`tags`")));
        assert!(!seen
            .iter()
            .any(|(c, _, m)| *c == "backend-input-required" && m.contains("`name`")));
    }

    #[test]
    fn a_name_close_enough_is_offered() {
        let names = ["phone_number".to_owned(), "email".to_owned()];
        assert_eq!(closest("phone", names.iter()), Some("phone_number"));
        assert_eq!(closest("emial", names.iter()), Some("email"));
        assert_eq!(closest("country", names.iter()), None);
    }
}
