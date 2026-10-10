//! Resource templates (RFC 0004): a live command that takes arguments, named
//! as a URI with variables — `app://customers_show/{id}`,
//! `app://customers_index{?direction,page,per_page,search,sort}`.
//!
//! The template is read off the tool's own input schema, so it can't disagree
//! with `tools/list`: each argument that's a scalar is a variable, and so is
//! each scalar field of a struct argument (one level deep). Required
//! variables are path segments, in argument order; optional ones are the
//! query. A struct's fields go in name order, the same in every build.

use serde_json::{Map, Value};

/// A command's resource template.
#[derive(Debug, Clone)]
pub struct Template {
    /// The RFC 6570 template, as `resources/templates/list` shows it.
    pub uri_template: String,
    // Read once expanded URIs are matched (RFC 0004 step 2).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) vars: Vec<Var>,
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
        for (index, arg) in args.iter().enumerate() {
            let schema = &input["properties"][arg.as_str()];
            match shape(schema, defs) {
                Shape::Scalar(kind, nullable) => vars.push(Var {
                    name: arg.clone(),
                    kind,
                    nullable,
                    required: required_args.contains(arg),
                    slot: Slot::Arg(index),
                }),
                Shape::Object {
                    properties,
                    required: required_fields,
                    nullable,
                } => {
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
        Ok(Some(Template { uri_template, vars }))
    }
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

    #[test]
    fn no_arguments_means_no_template() {
        assert!(derive(json!({ "type": "object", "properties": {} }), &[])
            .unwrap()
            .is_none());
    }
}
