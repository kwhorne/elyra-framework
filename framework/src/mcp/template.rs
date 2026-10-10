//! Resource templates (RFC 0004): a live command that takes arguments, named
//! as a URI with variables — `app://customers_show/{id}`,
//! `app://customers_index{?direction,page,per_page,search,sort}`.
//!
//! The template is read off the tool's own input schema, so it can't disagree
//! with `tools/list`: each argument that's a scalar is a variable, and so is
//! each scalar field of a struct argument (one level deep). Required
//! variables are path segments, in argument order; optional ones are the
//! query. A struct's fields go in name order, the same in every build.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

/// The longest expanded URI read.
const MAX_URI: usize = 2048;

/// Why an expanded URI's values don't fit, per variable.
pub(crate) type Problems = BTreeMap<String, Vec<String>>;

/// A command's resource template.
#[derive(Debug, Clone)]
pub struct Template {
    /// The RFC 6570 template, as `resources/templates/list` shows it.
    pub uri_template: String,
    pub(crate) vars: Vec<Var>,
    /// Each argument's shape, by position: what to rebuild it as.
    pub(crate) args: Vec<ArgShape>,
}

/// How an argument is rebuilt from its variables.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ArgShape {
    /// One variable, the value itself (`null` when an `Option` is left out).
    Scalar,
    /// An object of its fields' variables; `null` when it's an `Option` and
    /// none is given.
    Struct { nullable: bool },
}

/// One variable: what it holds, and where its value goes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Var {
    pub name: String,
    pub kind: Kind,
    pub nullable: bool,
    pub required: bool,
    pub slot: Slot,
}

/// A variable's type, as its schema states it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    String,
    Integer {
        unsigned: bool,
    },
    Number,
    Boolean,
    /// One of these string constants (a unit-variant enum).
    Enum(Vec<String>),
}

/// Where a variable's value goes in the command's arguments.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Slot {
    /// The argument at this position.
    Arg(usize),
    /// A field of the struct argument at this position. `nullable`: the
    /// argument is an `Option`, so it's `null` when none of its fields is given.
    Field {
        arg: usize,
        field: String,
        nullable: bool,
    },
}

impl Template {
    /// The template for command `name` with these argument names, from its
    /// input schema — or why it has none. `Ok(None)`: it takes no arguments,
    /// so its plain resource is all there is.
    pub(crate) fn derive(
        name: &str,
        input: &Value,
        args: &[String],
    ) -> Result<Option<Template>, String> {
        let defs = input.get("$defs").and_then(Value::as_object);
        let required_args = required(input);
        let mut vars: Vec<Var> = Vec::new();
        let mut shapes = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            let schema = &input["properties"][arg.as_str()];
            match shape(schema, defs) {
                Shape::Scalar(kind, nullable) => {
                    shapes.push(ArgShape::Scalar);
                    vars.push(Var {
                        name: arg.clone(),
                        kind,
                        nullable,
                        required: required_args.contains(arg),
                        slot: Slot::Arg(index),
                    })
                }
                Shape::Object {
                    properties,
                    required: required_fields,
                    nullable,
                } => {
                    shapes.push(ArgShape::Struct { nullable });
                    let mut fields: Vec<(&String, &Value)> = properties.iter().collect();
                    fields.sort_by(|a, b| a.0.cmp(b.0));
                    for (field, schema) in fields {
                        match shape(schema, defs) {
                            Shape::Scalar(kind, field_nullable) => vars.push(Var {
                                name: field.clone(),
                                kind,
                                nullable: field_nullable,
                                // A field the struct needs is needed — unless
                                // the whole struct may be left out.
                                required: !nullable && required_fields.contains(field),
                                slot: Slot::Field {
                                    arg: index,
                                    field: field.clone(),
                                    nullable,
                                },
                            }),
                            other => return Err(format!("`{arg}.{field}` {}", other.why())),
                        }
                    }
                }
                other => return Err(format!("`{arg}` {}", other.why())),
            }
        }
        if vars.is_empty() {
            return Ok(None);
        }
        for (i, var) in vars.iter().enumerate() {
            if !var
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err(format!(
                    "`{}` isn't a URI template variable name (letters, digits, `_`)",
                    var.name
                ));
            }
            if vars[..i].iter().any(|v| v.name == var.name) {
                return Err(format!("`{}` names two arguments", var.name));
            }
        }
        // Required ones in the path, in argument order; the rest the query.
        let mut uri_template = format!("app://{name}");
        for var in vars.iter().filter(|v| v.required) {
            uri_template.push_str(&format!("/{{{}}}", var.name));
        }
        let query: Vec<&str> = vars
            .iter()
            .filter(|v| !v.required)
            .map(|v| v.name.as_str())
            .collect();
        if !query.is_empty() {
            uri_template.push_str(&format!("{{?{}}}", query.join(",")));
        }
        Ok(Some(Template {
            uri_template,
            vars,
            args: shapes,
        }))
    }

    /// The command's arguments, when `uri` is an expansion of this template
    /// for command `name`: `None` when it's for something else, the problems
    /// when its values don't fit.
    pub(crate) fn arguments(&self, name: &str, uri: &str) -> Option<Result<Vec<Value>, Problems>> {
        let rest = uri.strip_prefix("app://")?.strip_prefix(name)?;
        if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('?')) {
            return None; // `customers_index_all`, not `customers_index`
        }
        let mut problems = Problems::new();
        let mut problem = |var: &str, why: String| {
            problems.entry(var.to_owned()).or_default().push(why);
        };
        if uri.len() > MAX_URI {
            problem("uri", format!("is longer than {MAX_URI} bytes"));
            return Some(Err(problems));
        }
        let (path, query) = match rest.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (rest, None),
        };

        let mut given: BTreeMap<&str, String> = BTreeMap::new();
        // The path: one segment per required variable, in order.
        let segments: Vec<&str> = if path.is_empty() {
            Vec::new()
        } else {
            path.strip_prefix('/').unwrap_or(path).split('/').collect()
        };
        let in_path: Vec<&Var> = self.vars.iter().filter(|v| v.required).collect();
        if segments.len() != in_path.len() {
            let expected: Vec<String> =
                in_path.iter().map(|v| format!("/{{{}}}", v.name)).collect();
            problem(
                "uri",
                format!(
                    "takes {} path segment(s) ({}), not {}",
                    in_path.len(),
                    expected.join(""),
                    segments.len()
                ),
            );
            return Some(Err(problems));
        }
        for (var, segment) in in_path.iter().zip(&segments) {
            match decode(segment) {
                Some(value) => {
                    given.insert(&var.name, value);
                }
                None => problem(&var.name, "isn't valid percent-encoded UTF-8".into()),
            }
        }
        // The query: optional variables, each at most once.
        for pair in query
            .into_iter()
            .flat_map(|q| q.split('&'))
            .filter(|p| !p.is_empty())
        {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let Some(key) = decode(key) else {
                problem("uri", format!("has a malformed parameter `{pair}`"));
                continue;
            };
            let Some(var) = self.vars.iter().find(|v| !v.required && v.name == key) else {
                problem(&key, "isn't a parameter of this resource".into());
                continue;
            };
            if given.contains_key(var.name.as_str()) {
                problem(&var.name, "is given more than once".into());
                continue;
            }
            match decode(value) {
                Some(value) => {
                    given.insert(&var.name, value);
                }
                None => problem(&var.name, "isn't valid percent-encoded UTF-8".into()),
            }
        }

        // Typed, and put back where the command takes them.
        let mut args: Vec<Value> = self
            .args
            .iter()
            .map(|shape| match shape {
                ArgShape::Scalar => Value::Null,
                ArgShape::Struct { .. } => json!({}),
            })
            .collect();
        for var in &self.vars {
            let Some(text) = given.get(var.name.as_str()) else {
                continue;
            };
            let value = match convert(&var.kind, text) {
                Ok(value) => value,
                Err(why) => {
                    problem(&var.name, why);
                    continue;
                }
            };
            match &var.slot {
                Slot::Arg(index) => args[*index] = value,
                Slot::Field { arg, field, .. } => {
                    args[*arg][field.as_str()] = value;
                }
            }
        }
        if !problems.is_empty() {
            return Some(Err(problems));
        }
        // An `Option` struct none of whose fields was given is `None`.
        for (arg, shape) in args.iter_mut().zip(&self.args) {
            if *shape == (ArgShape::Struct { nullable: true })
                && arg.as_object().is_some_and(Map::is_empty)
            {
                *arg = Value::Null;
            }
        }
        Some(Ok(args))
    }
}

impl Template {
    /// `completion/complete` for variable `var`: the values its schema allows
    /// that start with `prefix` — an enum's, or `true` / `false`. Anything
    /// else has no list to offer. `None`: no such variable.
    pub(crate) fn complete(&self, var: &str, prefix: &str) -> Option<Vec<String>> {
        let var = self.vars.iter().find(|v| v.name == var)?;
        let candidates: Vec<String> = match &var.kind {
            Kind::Enum(values) => values.clone(),
            Kind::Boolean => vec!["true".into(), "false".into()],
            _ => Vec::new(),
        };
        let prefix = prefix.to_lowercase();
        Some(
            candidates
                .into_iter()
                .filter(|c| c.to_lowercase().starts_with(&prefix))
                .take(100)
                .collect(),
        )
    }
}

/// A variable's text as its type.
fn convert(kind: &Kind, text: &str) -> Result<Value, String> {
    match kind {
        Kind::String => Ok(Value::from(text)),
        Kind::Integer { unsigned } => {
            let valid = !text.is_empty()
                && text
                    .strip_prefix('-')
                    .unwrap_or(text)
                    .chars()
                    .all(|c| c.is_ascii_digit());
            let n = valid
                .then(|| text.parse::<i64>().ok())
                .flatten()
                .ok_or_else(|| format!("must be an integer, not `{text}`"))?;
            if *unsigned && n < 0 {
                return Err(format!("must be 0 or more, not `{text}`"));
            }
            Ok(Value::from(n))
        }
        Kind::Number => text
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && !text.is_empty())
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| format!("must be a number, not `{text}`")),
        Kind::Boolean => match text {
            "true" => Ok(Value::Bool(true)),
            "false" => Ok(Value::Bool(false)),
            _ => Err(format!("must be `true` or `false`, not `{text}`")),
        },
        Kind::Enum(values) => values
            .iter()
            .find(|v| *v == text)
            .map(|v| Value::from(v.as_str()))
            .ok_or_else(|| format!("must be one of {}, not `{text}`", values.join(", "))),
    }
}

/// Percent-decode `s` (RFC 3986: `+` is a plus), as UTF-8.
fn decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What a schema is, as far as a template cares.
enum Shape<'a> {
    Scalar(Kind, bool),
    Object {
        properties: &'a Map<String, Value>,
        required: Vec<String>,
        nullable: bool,
    },
    List,
    Map,
    Any,
    Other,
}

impl Shape<'_> {
    fn why(&self) -> &'static str {
        match self {
            Shape::List => "is a list",
            Shape::Map => "is a map",
            Shape::Any => "takes any JSON",
            Shape::Object { .. } => "is a nested object",
            Shape::Scalar(..) | Shape::Other => "isn't a plain value",
        }
    }
}

fn required(schema: &Value) -> Vec<String> {
    schema["required"]
        .as_array()
        .map(|r| {
            r.iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Classify `schema`: through `$ref`s, a described wrapper (`allOf` of one)
/// and a nullable `anyOf`.
fn shape<'a>(schema: &'a Value, defs: Option<&'a Map<String, Value>>) -> Shape<'a> {
    let mut schema = schema;
    let mut nullable = false;
    for _ in 0..16 {
        if let Some(name) = schema["$ref"]
            .as_str()
            .and_then(|r| r.strip_prefix("#/$defs/"))
        {
            match defs.and_then(|d| d.get(name)) {
                Some(def) => {
                    schema = def;
                    continue;
                }
                None => return Shape::Other,
            }
        }
        if let Some([only]) = schema["allOf"].as_array().map(Vec::as_slice) {
            schema = only;
            continue;
        }
        if let Some(options) = schema["anyOf"].as_array() {
            let not_null: Vec<&Value> = options.iter().filter(|o| o["type"] != "null").collect();
            if not_null.len() < options.len() {
                nullable = true;
                if let [one] = not_null.as_slice() {
                    schema = one;
                    continue;
                }
            }
            // A unit-variant enum: every option a string constant.
            let constants: Option<Vec<String>> = not_null
                .iter()
                .map(|o| o["const"].as_str().map(str::to_owned))
                .collect();
            return match constants {
                Some(values) if !values.is_empty() => Shape::Scalar(Kind::Enum(values), nullable),
                _ => Shape::Other,
            };
        }
        break;
    }
    if let Some(value) = schema["const"].as_str() {
        return Shape::Scalar(Kind::Enum(vec![value.to_owned()]), nullable);
    }
    match schema["type"].as_str() {
        Some("string") => Shape::Scalar(Kind::String, nullable),
        Some("integer") => Shape::Scalar(
            Kind::Integer {
                unsigned: schema["minimum"].as_i64() == Some(0),
            },
            nullable,
        ),
        Some("number") => Shape::Scalar(Kind::Number, nullable),
        Some("boolean") => Shape::Scalar(Kind::Boolean, nullable),
        Some("array") => Shape::List,
        Some("object") => match schema["properties"].as_object() {
            Some(properties) => Shape::Object {
                properties,
                required: required(schema),
                nullable,
            },
            None => Shape::Map,
        },
        None if schema.as_object().is_some_and(Map::is_empty) => Shape::Any,
        _ => Shape::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn derive(input: Value, args: &[&str]) -> Result<Option<Template>, String> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        Template::derive("cmd", &input, &args)
    }

    #[test]
    fn required_scalars_are_the_path_optional_ones_the_query() {
        let t = derive(
            json!({ "type": "object", "required": ["from", "to"], "properties": {
                "from": { "type": "string" },
                "to": { "type": "string" },
                "status": { "anyOf": [{ "anyOf": [{ "const": "open" }, { "const": "paid" }] }, { "type": "null" }] },
            } }),
            &["from", "to", "status"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(t.uri_template, "app://cmd/{from}/{to}{?status}");
        assert_eq!(
            t.vars[2].kind,
            Kind::Enum(vec!["open".into(), "paid".into()])
        );
        assert!(t.vars[2].nullable && !t.vars[2].required);
    }

    #[test]
    fn a_struct_argument_contributes_its_fields_in_name_order() {
        let t = derive(
            json!({ "type": "object", "required": ["query"], "properties": {
                "query": { "$ref": "#/$defs/Query" },
            }, "$defs": { "Query": { "type": "object", "properties": {
                "search": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                "page": { "allOf": [{ "anyOf": [{ "type": "integer", "minimum": 0 }, { "type": "null" }] }], "description": "1-based" },
                "direction": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
            } } } }),
            &["query"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(t.uri_template, "app://cmd{?direction,page,search}");
        assert_eq!(t.vars[1].kind, Kind::Integer { unsigned: true });
        assert_eq!(
            t.vars[1].slot,
            Slot::Field {
                arg: 0,
                field: "page".into(),
                nullable: false
            }
        );
    }

    #[test]
    fn a_required_field_of_an_optional_struct_is_optional() {
        let t = derive(
            json!({ "type": "object", "properties": {
                "filter": { "anyOf": [{ "type": "object", "required": ["id"], "properties": {
                    "id": { "type": "integer" } } }, { "type": "null" }] },
            } }),
            &["filter"],
        )
        .unwrap()
        .unwrap();
        assert_eq!(t.uri_template, "app://cmd{?id}");
    }

    #[test]
    fn what_makes_a_command_a_tool_only() {
        let cases = [
            (
                json!({ "type": "array", "items": { "type": "string" } }),
                "is a list",
            ),
            (
                json!({ "type": "object", "additionalProperties": { "type": "string" } }),
                "is a map",
            ),
            (json!({}), "takes any JSON"),
            (
                json!({ "type": "object", "properties": { "inner": { "type": "object", "properties": {} } } }),
                "is a nested object",
            ),
            (
                json!({ "anyOf": [{ "type": "object", "properties": { "kind": { "const": "a" } } }, { "const": "b" }] }),
                "isn't a plain value",
            ),
        ];
        for (schema, why) in cases {
            let err = derive(
                json!({ "type": "object", "required": ["x"], "properties": { "x": schema } }),
                &["x"],
            )
            .unwrap_err();
            assert!(err.contains(why), "{err} / {why}");
        }
        // Two variables with one name: an argument and a struct's field.
        let err = derive(
            json!({ "type": "object", "required": ["page", "q"], "properties": {
                "page": { "type": "integer" },
                "q": { "type": "object", "properties": { "page": { "type": "integer" } } },
            } }),
            &["page", "q"],
        )
        .unwrap_err();
        assert!(err.contains("names two arguments"), "{err}");
        let err = derive(
            json!({ "type": "object", "required": ["per-page"], "properties": { "per-page": { "type": "integer" } } }),
            &["per-page"],
        )
        .unwrap_err();
        assert!(err.contains("variable name"), "{err}");
    }

    /// `orders(from: String, to: String, filter: Filter, flagged: Option<bool>)`
    /// with `Filter { min_total: Option<f64>, status: Option<Status> }`, and an
    /// `Option<Page { page: Option<u32> }>`.
    fn orders() -> Template {
        derive(
            json!({ "type": "object", "required": ["from", "to", "filter"], "properties": {
                "from": { "type": "string" },
                "to": { "type": "string" },
                "filter": { "type": "object", "properties": {
                    "min_total": { "anyOf": [{ "type": "number" }, { "type": "null" }] },
                    "status": { "anyOf": [{ "anyOf": [{ "const": "open" }, { "const": "paid" }] }, { "type": "null" }] },
                } },
                "flagged": { "anyOf": [{ "type": "boolean" }, { "type": "null" }] },
                "paging": { "anyOf": [{ "type": "object", "properties": {
                    "page": { "anyOf": [{ "type": "integer", "minimum": 0 }, { "type": "null" }] },
                } }, { "type": "null" }] },
            } }),
            &["from", "to", "filter", "flagged", "paging"],
        )
        .unwrap()
        .unwrap()
    }

    fn args(uri: &str) -> Result<Vec<Value>, Problems> {
        orders().arguments("cmd", uri).expect("this template's")
    }

    #[test]
    fn an_expanded_uri_becomes_the_commands_arguments() {
        let t = orders();
        assert_eq!(
            t.uri_template,
            "app://cmd/{from}/{to}{?min_total,status,flagged,page}"
        );
        assert_eq!(
            args("app://cmd/2026-01-01/2026-02-01").unwrap(),
            [
                json!("2026-01-01"),
                json!("2026-02-01"),
                json!({}),
                Value::Null,
                Value::Null
            ],
            "nothing optional given: an empty struct, nulls, and no `paging` at all"
        );
        assert_eq!(
            args("app://cmd/a%2Fb/%C3%A6?status=paid&min_total=10.5&flagged=true&page=2").unwrap(),
            [
                json!("a/b"),
                json!("æ"),
                json!({ "min_total": 10.5, "status": "paid" }),
                json!(true),
                json!({ "page": 2 }),
            ]
        );
        // `+` is a plus (RFC 3986), and an empty segment is an empty string.
        assert_eq!(
            args("app://cmd/a+b/").unwrap()[..2],
            [json!("a+b"), json!("")]
        );
    }

    #[test]
    fn values_that_dont_fit_are_named() {
        let problems =
            args("app://cmd/a/b?min_total=lots&status=gone&flagged=1&page=-2&page=3").unwrap_err();
        assert!(problems["min_total"][0].contains("number"), "{problems:?}");
        assert!(
            problems["status"][0].contains("one of open, paid"),
            "{problems:?}"
        );
        assert!(
            problems["flagged"][0].contains("`true` or `false`"),
            "{problems:?}"
        );
        let page = problems["page"].join(" / ");
        assert!(
            page.contains("0 or more") && page.contains("more than once"),
            "{page}"
        );

        assert!(args("app://cmd/a/b?typo=1").unwrap_err()["typo"][0].contains("isn't a parameter"));
        assert!(args("app://cmd/a?flagged=true").unwrap_err()["uri"][0].contains("/{from}/{to}"));
        assert!(args("app://cmd/a/b/c").unwrap_err().contains_key("uri"));
        assert!(args("app://cmd/%ZZ/b").unwrap_err().contains_key("from"));
        assert!(args("app://cmd/a/b?page=1e3")
            .unwrap_err()
            .contains_key("page"));
        let long = format!("app://cmd/a/b?min_total={}", "1".repeat(3000));
        assert!(args(&long).unwrap_err()["uri"][0].contains("longer than"));
    }

    #[test]
    fn another_commands_uri_isnt_this_templates() {
        let t = orders();
        assert!(t.arguments("cmd", "app://cmd_all/a/b").is_none());
        assert!(t.arguments("cmd", "app://other/a/b").is_none());
        assert!(t.arguments("cmd", "file:///cmd/a/b").is_none());
    }

    #[test]
    fn completion_offers_what_the_schema_allows() {
        let t = orders();
        assert_eq!(t.complete("status", "").unwrap(), ["open", "paid"]);
        assert_eq!(t.complete("status", "P").unwrap(), ["paid"]);
        assert_eq!(t.complete("flagged", "t").unwrap(), ["true"]);
        assert!(
            t.complete("from", "2026").unwrap().is_empty(),
            "a string: nothing to offer"
        );
        assert!(t.complete("nope", "").is_none());
    }

    #[test]
    fn no_arguments_means_no_template() {
        assert!(derive(json!({ "type": "object", "properties": {} }), &[])
            .unwrap()
            .is_none());
    }
}
