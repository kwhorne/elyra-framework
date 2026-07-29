//! Laravel-style input validation for command inputs.
//!
//! Commands receive untrusted data from the frontend; [`Validator`] checks it
//! against a familiar rule string (`"required|email|min:3"`) and produces a
//! per-field [`ValidationErrors`] bag. Return it via `?` from a command and the
//! frontend receives the structured errors (see `docs/validation.md`).
//!
//! ```
//! use elyra::validation::Validator;
//! use serde_json::json;
//!
//! let input = json!({ "email": "not-an-email", "age": 15 });
//! let errors = Validator::new(&input)
//!     .rules(&[("email", "required|email"), ("age", "integer|min:18")])
//!     .errors();
//! assert!(errors.has("email"));
//! assert!(errors.has("age"));
//! ```

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// A per-field error bag. Serializes to a Laravel-style object of message
/// arrays (`{"email": ["The email must be a valid email address."]}`); its
/// `Display` is that JSON, so returning it as a command error surfaces the
/// structure to the frontend.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ValidationErrors(pub BTreeMap<String, Vec<String>>);

impl ValidationErrors {
    /// An empty bag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a message for `field`.
    pub fn add(&mut self, field: &str, message: impl Into<String>) {
        self.0
            .entry(field.to_string())
            .or_default()
            .push(message.into());
    }

    /// Whether there are no errors.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Whether `field` has any error.
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    /// The first message for `field`, if any.
    pub fn first(&self, field: &str) -> Option<&str> {
        self.0
            .get(field)
            .and_then(|v| v.first())
            .map(String::as_str)
    }
}

impl std::fmt::Display for ValidationErrors {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".into()))
    }
}

impl std::error::Error for ValidationErrors {}

/// Validates a JSON value against a set of field rules.
pub struct Validator<'a> {
    data: &'a Value,
    rules: Vec<(String, String)>,
}

impl<'a> Validator<'a> {
    /// Validate `data` (typically a command's input object).
    pub fn new(data: &'a Value) -> Self {
        Self {
            data,
            rules: Vec::new(),
        }
    }

    /// Add a rule string for one field, e.g. `("email", "required|email")`.
    pub fn rule(mut self, field: &str, rules: &str) -> Self {
        self.rules.push((field.to_string(), rules.to_string()));
        self
    }

    /// Add several field rules at once.
    pub fn rules(mut self, rules: &[(&str, &str)]) -> Self {
        for (field, r) in rules {
            self.rules.push((field.to_string(), r.to_string()));
        }
        self
    }

    /// Run the rules and collect every error.
    pub fn errors(&self) -> ValidationErrors {
        let mut errors = ValidationErrors::new();
        for (field, rule_str) in &self.rules {
            let value = self.data.get(field);
            let present = value.map(|v| !v.is_null()).unwrap_or(false);
            let parsed: Vec<(&str, Option<&str>)> = rule_str
                .split('|')
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let mut it = r.splitn(2, ':');
                    (it.next().unwrap_or(""), it.next())
                })
                .collect();

            for (name, arg) in parsed {
                // Absent/null fields only fail `required`; other rules are skipped
                // (Laravel's implicit "sometimes" behaviour).
                if !present && name != "required" {
                    continue;
                }
                if let Some(message) =
                    check(name, arg, field, value.unwrap_or(&Value::Null), self.data)
                {
                    errors.add(field, message);
                }
            }
        }
        errors
    }

    /// Run the rules; `Ok(())` if valid, else the error bag.
    pub fn validate(self) -> Result<(), ValidationErrors> {
        let errors = self.errors();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn humanize(field: &str) -> String {
    field.replace(['_', '-'], " ")
}

/// The "size" of a value: string length, array length, or the number itself.
fn size(value: &Value) -> f64 {
    match value {
        Value::String(s) => s.chars().count() as f64,
        Value::Array(a) => a.len() as f64,
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn is_email(s: &str) -> bool {
    let mut parts = s.splitn(2, '@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !s.chars().any(char::is_whitespace)
}

/// Apply one rule; return an error message if it fails.
fn check(
    name: &str,
    arg: Option<&str>,
    field: &str,
    value: &Value,
    data: &Value,
) -> Option<String> {
    let human = humanize(field);
    let num_arg = || arg.and_then(|a| a.parse::<f64>().ok());
    match name {
        "required" => {
            let empty = match value {
                Value::Null => true,
                Value::String(s) => s.is_empty(),
                Value::Array(a) => a.is_empty(),
                _ => false,
            };
            empty.then(|| format!("The {human} field is required."))
        }
        "nullable" | "sometimes" => None,
        "string" => (!value.is_string()).then(|| format!("The {human} must be a string.")),
        "integer" => (!(value.is_i64() || value.is_u64()))
            .then(|| format!("The {human} must be an integer.")),
        "numeric" => (!value.is_number()).then(|| format!("The {human} must be a number.")),
        "boolean" => (!value.is_boolean()).then(|| format!("The {human} must be true or false.")),
        "email" => value
            .as_str()
            .map(|s| !is_email(s))
            .unwrap_or(true)
            .then(|| format!("The {human} must be a valid email address.")),
        "url" => value
            .as_str()
            .map(|s| !(s.starts_with("http://") || s.starts_with("https://")))
            .unwrap_or(true)
            .then(|| format!("The {human} must be a valid URL.")),
        "min" => {
            num_arg().and_then(|min| (size(value) < min).then(|| min_message(&human, value, min)))
        }
        "max" => {
            num_arg().and_then(|max| (size(value) > max).then(|| max_message(&human, value, max)))
        }
        "size" => num_arg()
            .and_then(|want| (size(value) != want).then(|| size_message(&human, value, want))),
        "in" => {
            let allowed: Vec<&str> = arg.map(|a| a.split(',').collect()).unwrap_or_default();
            let as_str = value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string());
            (!allowed.contains(&as_str.as_str()))
                .then(|| format!("The selected {human} is invalid."))
        }
        "same" => arg.and_then(|other| {
            (data.get(other) != Some(value))
                .then(|| format!("The {human} and {} must match.", humanize(other)))
        }),
        "confirmed" => {
            let confirmation = format!("{field}_confirmation");
            (data.get(&confirmation) != Some(value))
                .then(|| format!("The {human} confirmation does not match."))
        }
        _ => None, // unknown rule: ignore rather than fail closed
    }
}

fn min_message(human: &str, value: &Value, min: f64) -> String {
    match value {
        Value::String(_) => format!("The {human} must be at least {min} characters."),
        Value::Array(_) => format!("The {human} must have at least {min} items."),
        _ => format!("The {human} must be at least {min}."),
    }
}

fn max_message(human: &str, value: &Value, max: f64) -> String {
    match value {
        Value::String(_) => format!("The {human} must not be greater than {max} characters."),
        Value::Array(_) => format!("The {human} must not have more than {max} items."),
        _ => format!("The {human} must not be greater than {max}."),
    }
}

fn size_message(human: &str, value: &Value, want: f64) -> String {
    match value {
        Value::String(_) => format!("The {human} must be {want} characters."),
        Value::Array(_) => format!("The {human} must contain {want} items."),
        _ => format!("The {human} must be {want}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn passes_valid_input() {
        let input = json!({ "email": "a@b.com", "age": 21, "name": "Ada" });
        assert!(Validator::new(&input)
            .rules(&[
                ("email", "required|email"),
                ("age", "integer|min:18"),
                ("name", "required|string")
            ])
            .validate()
            .is_ok());
    }

    #[test]
    fn collects_field_errors() {
        let input = json!({ "email": "nope", "age": 15 });
        let e = Validator::new(&input)
            .rules(&[
                ("email", "required|email"),
                ("age", "integer|min:18"),
                ("name", "required"),
            ])
            .errors();
        assert!(e.has("email"));
        assert!(e.has("age"));
        assert!(e.has("name")); // absent -> required fails
        assert_eq!(e.first("name"), Some("The name field is required."));
    }

    #[test]
    fn absent_optional_field_passes() {
        let input = json!({ "email": "a@b.com" });
        assert!(Validator::new(&input)
            .rules(&[("email", "required|email"), ("bio", "string|max:200")])
            .validate()
            .is_ok());
    }

    #[test]
    fn confirmed_and_same() {
        let ok = json!({ "password": "secret", "password_confirmation": "secret" });
        assert!(Validator::new(&ok)
            .rule("password", "confirmed")
            .validate()
            .is_ok());
        let bad = json!({ "password": "secret", "password_confirmation": "nope" });
        assert!(Validator::new(&bad)
            .rule("password", "confirmed")
            .errors()
            .has("password"));
    }

    #[test]
    fn display_is_json_bag() {
        let input = json!({ "age": 15 });
        let e = Validator::new(&input).rule("age", "min:18").errors();
        let json = e.to_string();
        assert!(json.contains("\"age\""));
        assert!(json.starts_with('{'));
    }
}
