//! Completions an app computes: what to suggest for a prompt's argument or a
//! resource template's variable, from a live command the caller may run.
//!
//! ```ignore
//! let summary = Prompt::new("customer_summary", "Summarize a customer", "Summarize {customer}.")
//!     .argument("customer", "The customer's name")
//!     .complete("customer", Completion::new("customers_index", "name"));
//!
//! Mcp::new()
//!     .allow_ability("customers.view")
//!     .prompt(&summary)
//!     .complete("customers_show", "id", Completion::new("customers_index", "id"))
//! ```
//!
//! What's typed goes into the command's `query.search` (as `make:resource`'s
//! `_index` takes it, 20 to a page), or wherever [`search`](Completion::search)
//! says; each row's `field` is a suggestion. The command runs as any call
//! does: through the caller's grant, the user's abilities, the window's
//! limits and the rate limit. So a completion shows only what the caller
//! could read anyway, and a source must be a live command (it only reads)
//! that needs no confirmation.

use serde_json::{Map, Value};

use super::{Mcp, Tool};

/// How many suggestions at most (MCP's limit).
pub const MAX_VALUES: usize = 100;

/// The longest text passed on to the command, in characters.
const MAX_TYPED: usize = 100;

/// How many rows a `make:resource` index is asked for.
const PER_PAGE: u64 = 20;

/// Where suggestions come from: a live command and the field of each row it
/// returns. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    command: String,
    field: String,
    /// The argument path the typed text goes in.
    search: Vec<String>,
    /// Whether to ask for `query.per_page` (the default path only).
    paged: bool,
}

impl Completion {
    /// Suggestions from live command `command`: each row's `field` (rows are
    /// a page's `data`, or the list it returns). What's typed is its
    /// `query.search`, as `make:resource`'s `_index` takes it.
    pub fn new(command: impl Into<String>, field: impl Into<String>) -> Self {
        Self {
            command: command.into(),
            field: field.into(),
            search: vec!["query".into(), "search".into()],
            paged: true,
        }
    }

    /// Put what's typed in this argument instead: a name (`term`), or a
    /// field of one (`filter.name`).
    pub fn search(mut self, path: &str) -> Self {
        self.search = path.split('.').map(str::to_owned).collect();
        self.paged = false;
        self
    }

    /// The command it runs.
    pub fn command(&self) -> &str {
        &self.command
    }

    /// The command's arguments, by name, for `typed`.
    pub(crate) fn arguments(&self, typed: &str) -> Map<String, Value> {
        let typed: String = typed.trim().chars().take(MAX_TYPED).collect();
        let mut arguments = Map::new();
        set(&mut arguments, &self.search, Value::from(typed));
        if self.paged {
            set(
                &mut arguments,
                &["query".to_owned(), "per_page".to_owned()],
                Value::from(PER_PAGE),
            );
        }
        arguments
    }

    /// The suggestions in what the command returned: each row's field, as
    /// text, once each, at most [`MAX_VALUES`].
    pub(crate) fn values(&self, output: &Value) -> Vec<String> {
        let rows = output
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| output.as_array());
        let mut values: Vec<String> = Vec::new();
        for row in rows.into_iter().flatten() {
            let value = match row {
                Value::Object(map) => map.get(&self.field),
                other => Some(other),
            };
            let text = match value {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Number(n)) => n.to_string(),
                _ => continue,
            };
            if !text.is_empty() && !values.contains(&text) {
                values.push(text);
            }
            if values.len() == MAX_VALUES {
                break;
            }
        }
        values
    }

    /// Whether `tools` (the caller's) has this source: a live command whose
    /// ability needs no confirmation. Why not, when it doesn't.
    pub(crate) fn source<'a>(
        &self,
        tools: &'a [Tool],
        needs_confirmation: impl Fn(&str) -> bool,
    ) -> Result<&'a Tool, String> {
        let command = &self.command;
        let tool = tools
            .iter()
            .find(|t| &t.name == command)
            .ok_or_else(|| format!("`{command}` isn't a command this grant offers"))?;
        if !tool.annotations.read_only_hint {
            return Err(format!(
                "`{command}` isn't a live command (one that only reads)"
            ));
        }
        if needs_confirmation(&tool.ability) {
            return Err(format!("`{command}` needs the user's confirmation"));
        }
        if self.field.is_empty() || self.search.iter().any(String::is_empty) {
            return Err(format!(
                "its field and search path from `{command}` can't be empty"
            ));
        }
        Ok(tool)
    }
}

/// Check at start that each completion `mcp` gives comes from a source it
/// offers in `tools`: its offered prompts' arguments, and (with `templates`,
/// for an MCP server) its resource templates' variables.
pub(crate) fn check(mcp: &Mcp, tools: &[Tool], templates: bool) -> Result<(), String> {
    let confirm = |ability: &str| mcp.needs_confirmation(ability);
    for prompt in mcp.offered_prompts() {
        for argument in prompt.arguments() {
            if let Some(completion) = &argument.complete {
                completion.source(tools, confirm).map_err(|why| {
                    format!(
                        "prompt `{}`, argument `{}`: {why}",
                        prompt.name(),
                        argument.name
                    )
                })?;
            }
        }
    }
    if !templates {
        return Ok(());
    }
    for (tool, variable, completion) in mcp.template_completions() {
        let template = tools
            .iter()
            .find(|t| &t.name == tool)
            .and_then(|t| t.template.as_ref())
            .ok_or_else(|| format!("`{tool}` isn't a resource template this grant offers"))?;
        if !template.vars.iter().any(|v| &v.name == variable) {
            return Err(format!(
                "`{}` has no variable `{variable}`",
                template.uri_template
            ));
        }
        completion
            .source(tools, confirm)
            .map_err(|why| format!("`{tool}`'s `{{{variable}}}`: {why}"))?;
    }
    Ok(())
}

/// Set `path` in `map` to `value`, making objects on the way.
fn set(map: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut at = map;
    for key in parents {
        let entry = at
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        at = entry.as_object_mut().expect("an object");
    }
    at.insert(last.clone(), value);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whats_typed_is_an_index_search_by_default() {
        let c = Completion::new("customers_index", "name");
        assert_eq!(
            Value::Object(c.arguments("  Ada ")),
            json!({ "query": { "search": "Ada", "per_page": 20 } })
        );
        let long = "x".repeat(150);
        assert_eq!(
            c.arguments(&long)["query"]["search"]
                .as_str()
                .unwrap()
                .len(),
            100
        );
        let own = Completion::new("people", "name").search("filter.name");
        assert_eq!(
            Value::Object(own.arguments("Ada")),
            json!({ "filter": { "name": "Ada" } })
        );
        let flat = Completion::new("people", "name").search("term");
        assert_eq!(
            Value::Object(flat.arguments("Ada")),
            json!({ "term": "Ada" })
        );
    }

    #[test]
    fn suggestions_are_each_rows_field_once() {
        let c = Completion::new("customers_index", "name");
        let page = json!({ "data": [
            { "id": 1, "name": "Ada" }, { "id": 2, "name": "Grace" },
            { "id": 3, "name": "Ada" }, { "id": 4, "name": null }, { "id": 5 },
            { "id": 6, "name": "" },
        ], "total": 6 });
        assert_eq!(c.values(&page), ["Ada", "Grace"]);
        // A list, numbers as text, and plain values.
        let ids = Completion::new("customers_index", "id");
        assert_eq!(ids.values(&json!([{ "id": 7 }, { "id": 42 }])), ["7", "42"]);
        assert_eq!(c.values(&json!(["Oslo", "Bergen"])), ["Oslo", "Bergen"]);
        assert!(c.values(&json!({ "name": "Ada" })).is_empty());
        let many: Vec<Value> = (0..150)
            .map(|i| json!({ "name": format!("n{i}") }))
            .collect();
        assert_eq!(c.values(&json!(many)).len(), MAX_VALUES);
    }

    #[test]
    fn set_makes_the_objects_on_the_way() {
        let mut map = Map::new();
        map.insert("query".into(), json!(3));
        set(&mut map, &["query".into(), "search".into()], json!("x"));
        assert_eq!(Value::Object(map), json!({ "query": { "search": "x" } }));
    }
}
