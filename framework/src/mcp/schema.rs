//! specta datatypes → JSON Schema (2020-12), for MCP tool definitions.
//!
//! The types first go through specta-serde, as for codegen, so the schema
//! describes the JSON serde actually reads and writes: renames, tagging and
//! flattening are already applied, and an enum is "untagged" — each variant
//! is the shape it has on the wire.
//!
//! Named types become `$defs` entries referenced with `$ref`; a generic one
//! (`Page<Customer>`) is inlined with its arguments substituted, since JSON
//! Schema has no generics. `serde_json::Value` is any JSON (`{}`).

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use specta::datatype::{DataType, Fields, Generic, NamedReferenceType, Primitive, Reference};
use specta::Types;

/// Which way the value travels — it decides whether an `Option` field may be
/// left out (serde reads a missing one as `None`; it always writes it).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Direction {
    /// What the agent sends: arguments.
    Input,
    /// What the command returns.
    Output,
}

/// Builds schemas against one type collection, gathering the `$defs` they use.
pub(crate) struct SchemaBuilder<'a> {
    types: &'a Types,
    direction: Direction,
    defs: BTreeMap<String, Value>,
    /// Definitions being built, so a recursive type refers to itself.
    building: Vec<String>,
}

impl<'a> SchemaBuilder<'a> {
    pub(crate) fn new(types: &'a Types, direction: Direction) -> Self {
        Self {
            types,
            direction,
            defs: BTreeMap::new(),
            building: Vec::new(),
        }
    }

    /// The `$defs` gathered so far, to attach to the root schema.
    pub(crate) fn defs(&self) -> &BTreeMap<String, Value> {
        &self.defs
    }

    /// The schema for one datatype.
    pub(crate) fn schema(&mut self, dt: &DataType) -> Value {
        self.with_generics(dt, &[])
    }

    fn with_generics(&mut self, dt: &DataType, generics: &[(Generic, DataType)]) -> Value {
        match dt {
            DataType::Primitive(p) => primitive(p),
            DataType::Nullable(inner) => {
                let inner = self.with_generics(inner, generics);
                json!({ "anyOf": [inner, { "type": "null" }] })
            }
            DataType::List(list) => {
                let mut schema = json!({
                    "type": "array",
                    "items": self.with_generics(&list.ty, generics),
                });
                if let Some(n) = list.length {
                    schema["minItems"] = json!(n);
                    schema["maxItems"] = json!(n);
                }
                if list.unique {
                    schema["uniqueItems"] = json!(true);
                }
                schema
            }
            DataType::Map(map) => json!({
                "type": "object",
                "additionalProperties": self.with_generics(map.value_ty(), generics),
            }),
            DataType::Tuple(tuple) => match tuple.elements.as_slice() {
                [] => json!({ "type": "null" }),
                elements => {
                    let items: Vec<Value> = elements
                        .iter()
                        .map(|e| self.with_generics(e, generics))
                        .collect();
                    json!({
                        "type": "array",
                        "prefixItems": items,
                        "minItems": elements.len(),
                        "maxItems": elements.len(),
                    })
                }
            },
            DataType::Struct(strct) => self.fields(&strct.fields, generics),
            DataType::Enum(enm) => {
                let variants: Vec<Value> = enm
                    .variants
                    .iter()
                    .filter(|(_, v)| !v.skip)
                    .filter_map(|(name, variant)| {
                        let mut schema = match &variant.fields {
                            Fields::Unit if name.is_empty() => return None,
                            Fields::Unit => json!({ "const": name }),
                            fields => self.fields(fields, generics),
                        };
                        describe(&mut schema, &variant.docs);
                        Some(schema)
                    })
                    .collect();
                match variants.len() {
                    0 => json!({ "not": {} }),
                    1 => variants.into_iter().next().expect("one"),
                    _ => json!({ "anyOf": variants }),
                }
            }
            DataType::Intersection(parts) => {
                let parts: Vec<Value> = parts
                    .iter()
                    .map(|p| self.with_generics(p, generics))
                    .collect();
                json!({ "allOf": parts })
            }
            DataType::Generic(g) => generics
                .iter()
                .find(|(name, _)| name == g)
                .map(|(_, dt)| dt.clone())
                .map(|dt| self.with_generics(&dt, &[]))
                // An unbound generic: anything goes.
                .unwrap_or_else(|| json!({})),
            DataType::Reference(Reference::Named(named)) => {
                let ndt = self.types.get(named).cloned();
                if let Some(ndt) = &ndt {
                    if ndt.name == "Value" && ndt.module_path.starts_with("serde_json") {
                        return json!({});
                    }
                }
                match &named.inner {
                    NamedReferenceType::Inline { dt, .. } => self.with_generics(dt, generics),
                    NamedReferenceType::Recursive(_) => match &ndt {
                        Some(ndt) => json!({ "$ref": format!("#/$defs/{}", ndt.name) }),
                        None => json!({}),
                    },
                    NamedReferenceType::Reference { generics: args, .. } => {
                        let Some(ndt) = ndt else { return json!({}) };
                        let Some(ty) = &ndt.ty else { return json!({}) };
                        // Arguments may mention the caller's own generics.
                        let args: Vec<(Generic, DataType)> = args
                            .iter()
                            .map(|(g, dt)| (g.clone(), substitute(dt, generics)))
                            .collect();
                        if !args.is_empty() {
                            // No generics in JSON Schema: inline, substituted.
                            let mut schema = self.with_generics(ty, &args);
                            describe(&mut schema, &ndt.docs);
                            return schema;
                        }
                        let key = ndt.name.to_string();
                        if !self.defs.contains_key(&key) && !self.building.contains(&key) {
                            self.building.push(key.clone());
                            let mut schema = self.with_generics(ty, &[]);
                            describe(&mut schema, &ndt.docs);
                            self.building.retain(|k| k != &key);
                            self.defs.insert(key.clone(), schema);
                        }
                        json!({ "$ref": format!("#/$defs/{key}") })
                    }
                }
            }
            // An exporter-specific opaque type: anything goes.
            DataType::Reference(_) => json!({}),
        }
    }

    fn fields(&mut self, fields: &Fields, generics: &[(Generic, DataType)]) -> Value {
        match fields {
            Fields::Unit => json!({ "type": "null" }),
            Fields::Unnamed(unnamed) => {
                let live: Vec<&DataType> = unnamed
                    .fields
                    .iter()
                    .filter_map(|f| f.ty.as_ref())
                    .collect();
                match live.as_slice() {
                    // A newtype is its inner value.
                    [one] if unnamed.fields.len() == 1 => self.with_generics(one, generics),
                    [] => json!({ "type": "array", "maxItems": 0 }),
                    many => {
                        let items: Vec<Value> = many
                            .iter()
                            .map(|dt| self.with_generics(dt, generics))
                            .collect();
                        json!({
                            "type": "array",
                            "prefixItems": items,
                            "minItems": many.len(),
                            "maxItems": many.len(),
                        })
                    }
                }
            }
            Fields::Named(named) => {
                let mut properties = Map::new();
                let mut required = Vec::new();
                for (name, field) in &named.fields {
                    let Some(ty) = &field.ty else { continue };
                    let mut schema = self.with_generics(ty, generics);
                    describe(&mut schema, &field.docs);
                    properties.insert(name.to_string(), schema);
                    let may_be_missing = field.optional
                        || (self.direction == Direction::Input
                            && matches!(ty, DataType::Nullable(_)));
                    if !may_be_missing {
                        required.push(Value::from(name.as_ref()));
                    }
                }
                let mut schema = json!({ "type": "object", "properties": properties });
                if !required.is_empty() {
                    schema["required"] = Value::Array(required);
                }
                schema
            }
        }
    }
}

/// Replace bound generics inside `dt` (for arguments that pass a generic on).
fn substitute(dt: &DataType, generics: &[(Generic, DataType)]) -> DataType {
    match dt {
        DataType::Generic(g) => generics
            .iter()
            .find(|(name, _)| name == g)
            .map(|(_, bound)| bound.clone())
            .unwrap_or_else(|| dt.clone()),
        other => other.clone(),
    }
}

/// Attach a doc comment as `description`, when there is one.
fn describe(schema: &mut Value, docs: &str) {
    // A doc comment keeps the space after each `///`; drop it per line.
    let docs = docs
        .lines()
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let docs = docs.trim();
    if docs.is_empty() {
        return;
    }
    if let Some(object) = schema.as_object_mut() {
        if !object.contains_key("$ref") {
            object.insert("description".into(), Value::from(docs));
            return;
        }
    }
    // `$ref` siblings are allowed in 2020-12, but wrap to stay unambiguous.
    *schema = json!({ "allOf": [schema.clone()], "description": docs });
}

fn primitive(p: &Primitive) -> Value {
    match p {
        Primitive::bool => json!({ "type": "boolean" }),
        Primitive::str => json!({ "type": "string" }),
        Primitive::char => json!({ "type": "string", "minLength": 1, "maxLength": 1 }),
        Primitive::f16 | Primitive::f32 | Primitive::f64 | Primitive::f128 => {
            json!({ "type": "number" })
        }
        Primitive::u8
        | Primitive::u16
        | Primitive::u32
        | Primitive::u64
        | Primitive::u128
        | Primitive::usize => json!({ "type": "integer", "minimum": 0 }),
        _ => json!({ "type": "integer" }),
    }
}

/// The schema of a set of named arguments — a tool's `inputSchema`: one
/// property per argument, required unless it's an `Option` (a call that
/// leaves it out passes `null`).
pub(crate) fn arguments(builder: &mut SchemaBuilder, args: &[(&str, DataType)]) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for (name, dt) in args {
        properties.insert((*name).to_owned(), builder.schema(dt));
        if !matches!(dt, DataType::Nullable(_)) {
            required.push(Value::from(*name));
        }
    }
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = Value::Array(required);
    }
    schema
}

/// Attach the gathered `$defs` to a root schema.
pub(crate) fn with_defs(mut schema: Value, defs: &BTreeMap<String, Value>) -> Value {
    if !defs.is_empty() {
        if let Some(object) = schema.as_object_mut() {
            object.insert(
                "$defs".into(),
                Value::Object(defs.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
            );
        }
    }
    schema
}
