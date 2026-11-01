//! Prompts: tasks an app offers by name — listed to an MCP client
//! (`prompts/list`, `prompts/get`) and shown as suggestions in the
//! assistant's panel before the first message.
//!
//! ```ignore
//! let overdue = Prompt::new("overdue_invoices", "Overdue invoices",
//!         "Find the invoices that are overdue and draft a reminder for each.")
//!     .needs("invoices.view");
//! let summary = Prompt::new("customer_summary", "Summarize a customer",
//!         "Summarize {customer}: open invoices, last contact, notes.")
//!     .argument("customer", "The customer's name or number")
//!     .needs("customers.view");
//!
//! App::new()
//!     .mcp(Mcp::new().allow_abilities(["invoices.*", "customers.*"]).prompts([&overdue, &summary]))
//!     .assistant(Assistant::new().allow_abilities(["invoices.*", "customers.*"]).prompts([&overdue, &summary]))
//! ```
//!
//! A prompt is offered only where every ability it `needs` is granted: to
//! the agent by its MCP grant; in the assistant by its grant, the user's
//! abilities and the window's limits.

use serde::Serialize;
use serde_json::{Map, Value};

/// A task an app offers by name. See the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prompt {
    name: String,
    title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    text: String,
    arguments: Vec<PromptArgument>,
    #[serde(skip)]
    needs: Vec<String>,
}

/// Something a prompt asks for: `{customer}` in its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PromptArgument {
    pub name: String,
    pub description: String,
    pub required: bool,
}

impl From<&Prompt> for Prompt {
    fn from(prompt: &Prompt) -> Self {
        prompt.clone()
    }
}

/// The longest an argument's value may be, in characters.
const MAX_VALUE: usize = 2_000;

impl Prompt {
    /// A prompt called `name` (letters, digits, `_` and `-`; what an MCP
    /// client asks for), shown as `title`, that asks `text`. `{name}` in
    /// the text is an [argument](Self::argument).
    pub fn new(name: impl Into<String>, title: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            description: None,
            text: text.into(),
            arguments: Vec::new(),
            needs: Vec::new(),
        }
    }

    /// A longer description, for an MCP client's list and the
    /// suggestion's tooltip.
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// An argument the prompt can't do without: `{name}` in its text.
    pub fn argument(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.arguments.push(PromptArgument {
            name: name.into(),
            description: description.into(),
            required: true,
        });
        self
    }

    /// An argument that may be left out: `{name}` is then empty.
    pub fn optional_argument(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.arguments.push(PromptArgument {
            name: name.into(),
            description: description.into(),
            required: false,
        });
        self
    }

    /// An ability the task takes (`invoices.view`, or a namespace): the
    /// prompt is offered only where it's granted. Call it once per ability.
    pub fn needs(mut self, ability: impl Into<String>) -> Self {
        self.needs.push(ability.into());
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn arguments(&self) -> &[PromptArgument] {
        &self.arguments
    }

    /// The abilities it takes.
    pub fn needed(&self) -> &[String] {
        &self.needs
    }

    /// Whether it's offered where `allows` says yes to each ability it takes.
    pub(crate) fn offered(&self, allows: impl Fn(&str) -> bool) -> bool {
        self.needs.iter().all(|ability| allows(ability))
    }

    /// Whether the prompt is well-formed: a valid name, and each `{…}` in
    /// its text an argument, each argument in its text.
    pub(crate) fn check(&self) -> Result<(), String> {
        let valid = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        };
        if !valid(&self.name) {
            return Err(format!(
                "prompt `{}`: a name is letters, digits, `_` and `-`",
                self.name
            ));
        }
        let used = placeholders(&self.text);
        for name in &used {
            if !self.arguments.iter().any(|a| &a.name == name) {
                return Err(format!(
                    "prompt `{}`: `{{{name}}}` is in its text but isn't an argument",
                    self.name
                ));
            }
        }
        for argument in &self.arguments {
            if !valid(&argument.name) {
                return Err(format!(
                    "prompt `{}`: argument `{}` is letters, digits, `_` and `-`",
                    self.name, argument.name
                ));
            }
            if !used.contains(&argument.name) {
                return Err(format!(
                    "prompt `{}`: argument `{}` isn't in its text",
                    self.name, argument.name
                ));
            }
        }
        Ok(())
    }

    /// The text with `arguments` filled in. A required one missing, or a
    /// value that isn't a string or is too long, is an error.
    pub fn render(&self, arguments: &Map<String, Value>) -> Result<String, String> {
        let mut values = Vec::with_capacity(self.arguments.len());
        for argument in &self.arguments {
            let value = match arguments.get(&argument.name) {
                None | Some(Value::Null) if argument.required => {
                    return Err(format!("`{}` is required", argument.name));
                }
                None | Some(Value::Null) => String::new(),
                Some(Value::String(s)) if s.chars().count() <= MAX_VALUE => s.clone(),
                Some(Value::String(_)) => {
                    return Err(format!(
                        "`{}` is longer than {MAX_VALUE} characters",
                        argument.name
                    ));
                }
                Some(_) => return Err(format!("`{}` must be a string", argument.name)),
            };
            if argument.required && value.trim().is_empty() {
                return Err(format!("`{}` is required", argument.name));
            }
            values.push((argument.name.as_str(), value));
        }
        // One pass over the text, so a value that holds `{other}` stays as
        // the user wrote it.
        let mut out = String::with_capacity(self.text.len());
        let mut rest = self.text.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('}').map(|end| (&after[..end], end)) {
                Some((name, end)) if values.iter().any(|(n, _)| *n == name) => {
                    let value = &values.iter().find(|(n, _)| *n == name).unwrap().1;
                    out.push_str(value);
                    rest = &after[end + 1..];
                }
                _ => {
                    out.push('{');
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        Ok(out)
    }
}

/// The `{name}`s in `text` whose name could be an argument's.
fn placeholders(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end)
                if end > 0
                    && after[..end]
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
            {
                let name = after[..end].to_owned();
                if !found.contains(&name) {
                    found.push(name);
                }
                rest = &after[end + 1..];
            }
            _ => rest = after,
        }
    }
    found
}

/// Check every prompt, and that no two share a name.
pub(crate) fn check_all(prompts: &[Prompt]) -> Result<(), String> {
    for (i, prompt) in prompts.iter().enumerate() {
        prompt.check()?;
        if prompts[..i].iter().any(|p| p.name == prompt.name) {
            return Err(format!("two prompts are called `{}`", prompt.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    fn summary() -> Prompt {
        Prompt::new(
            "customer_summary",
            "Summarize a customer",
            "Summarize {customer}{period}.",
        )
        .argument("customer", "The customer")
        .optional_argument("period", "A period, like ` this year`")
    }

    #[test]
    fn arguments_are_filled_in() {
        let p = summary();
        assert_eq!(
            p.render(&args(json!({ "customer": "Ada", "period": " this year" }))),
            Ok("Summarize Ada this year.".into())
        );
        // An optional one left out is empty.
        assert_eq!(
            p.render(&args(json!({ "customer": "Ada" }))),
            Ok("Summarize Ada.".into())
        );
    }

    #[test]
    fn a_required_argument_must_be_given_as_text() {
        let p = summary();
        assert_eq!(
            p.render(&args(json!({}))),
            Err("`customer` is required".into())
        );
        assert_eq!(
            p.render(&args(json!({ "customer": "  " }))),
            Err("`customer` is required".into())
        );
        assert_eq!(
            p.render(&args(json!({ "customer": 42 }))),
            Err("`customer` must be a string".into())
        );
        let long = "x".repeat(MAX_VALUE + 1);
        assert!(p.render(&args(json!({ "customer": long }))).is_err());
        assert!(p
            .render(&args(json!({ "customer": "x".repeat(MAX_VALUE) })))
            .is_ok());
    }

    #[test]
    fn a_value_holding_braces_is_left_as_written() {
        let p = summary();
        assert_eq!(
            p.render(&args(json!({ "customer": "{period}" }))),
            Ok("Summarize {period}.".into())
        );
        // Braces that aren't an argument stay in the text.
        let json = Prompt::new("j", "J", "Answer as {\"ok\": true} for {who}").argument("who", "");
        assert_eq!(json.check(), Ok(()));
        assert_eq!(
            json.render(&args(json!({ "who": "Ada" }))),
            Ok("Answer as {\"ok\": true} for Ada".into())
        );
    }

    #[test]
    fn a_prompt_is_checked_at_start() {
        assert_eq!(summary().check(), Ok(()));
        assert!(Prompt::new("bad name", "T", "x").check().is_err());
        assert!(Prompt::new("", "T", "x").check().is_err());
        let undeclared = Prompt::new("p", "T", "About {who}");
        assert!(undeclared.check().unwrap_err().contains("`{who}`"));
        let unused = Prompt::new("p", "T", "Hello").argument("who", "");
        assert!(unused.check().unwrap_err().contains("isn't in its text"));
        let twice = [Prompt::new("p", "A", "a"), Prompt::new("p", "B", "b")];
        assert!(check_all(&twice).unwrap_err().contains("two prompts"));
        assert_eq!(check_all(&[summary(), Prompt::new("q", "Q", "q")]), Ok(()));
    }

    #[test]
    fn it_is_offered_where_every_ability_it_needs_is_allowed() {
        let p = Prompt::new("p", "P", "p")
            .needs("invoices.view")
            .needs("customers.view");
        assert!(p.offered(|_| true));
        assert!(!p.offered(|a| a == "invoices.view"));
        assert!(Prompt::new("free", "F", "f").offered(|_| false));
    }
}
