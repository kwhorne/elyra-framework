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
//!
//! Fields may be **paths** — `address.city`, `items.*.name` — and errors are
//! keyed by the concrete path (`items.1.name`), like Laravel. **An unknown rule
//! panics**, naming the rule and the field: a misspelt `"requried"` must not
//! quietly let invalid input through. `unique` / `exists` check the database and
//! run through [`Validator::validate_with`].

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

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

/// Every rule name the validator understands.
const KNOWN: &[&str] = &[
    "required",
    "required_if",
    "required_with",
    "required_without",
    "accepted",
    "filled",
    "nullable",
    "sometimes",
    "string",
    "integer",
    "numeric",
    "boolean",
    "array",
    "email",
    "url",
    "uuid",
    "ip",
    "alpha",
    "alpha_num",
    "alpha_dash",
    "digits",
    "digits_between",
    "min",
    "max",
    "size",
    "between",
    "gt",
    "gte",
    "lt",
    "lte",
    "in",
    "not_in",
    "starts_with",
    "ends_with",
    "regex",
    "date",
    "before",
    "before_or_equal",
    "after",
    "after_or_equal",
    "same",
    "confirmed",
    "distinct",
    "unique",
    "exists",
];

/// Rules that run even when the field is absent or null.
const IMPLICIT: &[&str] = &[
    "required",
    "required_if",
    "required_with",
    "required_without",
    "accepted",
    "filled",
];

/// Rules that need the database ([`Validator::validate_with`]).
const DB_RULES: &[&str] = &["unique", "exists"];

/// A rule that needs the database, collected by the synchronous pass.
#[cfg_attr(not(feature = "database"), allow(dead_code))]
struct DbCheck {
    path: String,
    field: String,
    rule: String,
    arg: Option<String>,
    value: Value,
}

/// Validates a JSON value against a set of field rules.
pub struct Validator<'a> {
    data: &'a Value,
    rules: Vec<(String, Vec<String>)>,
    translator: Option<&'a crate::i18n::Translator>,
}

impl<'a> Validator<'a> {
    /// Validate `data` (typically a command's input object).
    pub fn new(data: &'a Value) -> Self {
        Self {
            data,
            rules: Vec::new(),
            translator: None,
        }
    }

    /// Render messages through the app's translations: `validation.<key>`
    /// (`validation.min.string`) for the text and `validation.attributes.<field>`
    /// for field names, in the translator's current locale. Keys it lacks fall
    /// back to the built-in English.
    ///
    /// ```ignore
    /// Validator::new(&input)
    ///     .translator(&ctx.get::<Translator>())
    ///     .rules(&[("email", "required|email")])
    ///     .validate()?;
    /// ```
    pub fn translator(mut self, translator: &'a crate::i18n::Translator) -> Self {
        self.translator = Some(translator);
        self
    }

    /// Add a rule string for one field, e.g. `("email", "required|email")`.
    /// The field may be a path: `"address.city"`, `"items.*.name"`.
    pub fn rule(mut self, field: &str, rules: &str) -> Self {
        let list = rules
            .split('|')
            .filter(|r| !r.is_empty())
            .map(str::to_owned)
            .collect();
        self.rules.push((field.to_string(), list));
        self
    }

    /// Add several field rules at once.
    pub fn rules(mut self, rules: &[(&str, &str)]) -> Self {
        for (field, r) in rules {
            self = self.rule(field, r);
        }
        self
    }

    /// Add rules as a list rather than a `|`-separated string — needed when a
    /// rule's argument contains a `|`, as a `regex` alternation does:
    /// `.rule_list("code", &["required", "regex:^(ab|cd)[0-9]+$"])`.
    pub fn rule_list(mut self, field: &str, rules: &[&str]) -> Self {
        self.rules.push((
            field.to_string(),
            rules.iter().map(|r| (*r).to_owned()).collect(),
        ));
        self
    }

    /// Run the rules and collect every error.
    ///
    /// # Panics
    /// On an unknown rule, and on `unique` / `exists` — those need the database;
    /// use [`validate_with`](Validator::validate_with).
    pub fn errors(&self) -> ValidationErrors {
        let (errors, db) = self.run();
        if let Some(check) = db.first() {
            panic!(
                "validation rule `{}` on `{}` needs the database: use `validate_with(&db).await`",
                check.rule, check.field
            );
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

    /// Run every rule, including `unique` / `exists` against `db`.
    #[cfg(feature = "database")]
    pub async fn errors_with(&self, db: &elyra_db::Database) -> ValidationErrors {
        let (mut errors, checks) = self.run();
        for check in checks {
            if let Some(msg) = db_check(db, &check).await {
                let text = render(&msg, &check.path, &check.field, self.translator);
                errors.add(&check.path, text);
            }
        }
        errors
    }

    /// [`validate`](Validator::validate), with `unique` / `exists` checked
    /// against `db`:
    ///
    /// ```ignore
    /// Validator::new(&input)
    ///     .rules(&[("email", "required|email|unique:users"), ("role_id", "exists:roles,id")])
    ///     .validate_with(&db)
    ///     .await?;
    /// ```
    #[cfg(feature = "database")]
    pub async fn validate_with(self, db: &elyra_db::Database) -> Result<(), ValidationErrors> {
        let errors = self.errors_with(db).await;
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// The synchronous pass: every rule but the database ones, which are
    /// returned for the caller to run (or refuse).
    fn run(&self) -> (ValidationErrors, Vec<DbCheck>) {
        let mut errors = ValidationErrors::new();
        let mut db = Vec::new();
        for (pattern, list) in &self.rules {
            let rules: Vec<(&str, Option<&str>)> = list
                .iter()
                .map(|r| match r.split_once(':') {
                    Some((name, arg)) => (name, Some(arg)),
                    None => (r.as_str(), None),
                })
                .collect();
            for (name, _) in &rules {
                if !KNOWN.contains(name) {
                    panic!("unknown validation rule `{name}` on `{pattern}`");
                }
            }
            let targets = expand(self.data, pattern);
            let duplicates = if rules.iter().any(|(n, _)| *n == "distinct") {
                duplicate_values(&targets)
            } else {
                Vec::new()
            };

            for (path, value) in &targets {
                let present = value.is_some_and(|v| !v.is_null());
                for (name, arg) in &rules {
                    // Absent/null fields only meet the implicit rules; the rest
                    // are skipped (Laravel's implicit "sometimes" behaviour).
                    if !present && !IMPLICIT.contains(name) {
                        continue;
                    }
                    if DB_RULES.contains(name) {
                        db.push(DbCheck {
                            path: path.clone(),
                            field: pattern.clone(),
                            rule: (*name).to_owned(),
                            arg: arg.map(str::to_owned),
                            value: value.cloned().unwrap_or(Value::Null),
                        });
                        continue;
                    }
                    if *name == "distinct" {
                        if duplicates.contains(path) {
                            let failed = msg("distinct").expect("a message");
                            errors.add(path, render(&failed, path, pattern, self.translator));
                        }
                        continue;
                    }
                    let cx = Cx {
                        data: self.data,
                        pattern,
                        path,
                    };
                    if let Some(failed) = check(name, *arg, *value, &cx) {
                        errors.add(path, render(&failed, path, pattern, self.translator));
                    }
                }
            }
        }
        (errors, db)
    }
}

/// Where a rule is being applied.
struct Cx<'a> {
    data: &'a Value,
    pattern: &'a str,
    path: &'a str,
}

impl Cx<'_> {
    /// Resolve another field, filling its `*`s from this field's concrete path —
    /// so `required_if:items.*.type,…` on `items.3.qty` reads `items.3.type`.
    fn other(&self, other: &str) -> Option<&Value> {
        let resolved = if other.contains('*') {
            let pattern: Vec<&str> = self.pattern.split('.').collect();
            let concrete: Vec<&str> = self.path.split('.').collect();
            other
                .split('.')
                .enumerate()
                .map(|(i, seg)| {
                    if seg == "*" && pattern.get(i) == Some(&"*") {
                        concrete.get(i).copied().unwrap_or(seg)
                    } else {
                        seg
                    }
                })
                .collect::<Vec<_>>()
                .join(".")
        } else {
            other.to_owned()
        };
        expand(self.data, &resolved)
            .into_iter()
            .next()
            .and_then(|(_, v)| v)
    }
}

/// Expand a field pattern against `data` into `(concrete path, value)` pairs.
/// A plain path always yields one pair (the value may be missing, so
/// `required` can fire); a `*` yields one pair per array element or object key
/// that exists.
fn expand<'v>(data: &'v Value, pattern: &str) -> Vec<(String, Option<&'v Value>)> {
    fn walk<'v>(
        value: Option<&'v Value>,
        segs: &[&str],
        prefix: &str,
        out: &mut Vec<(String, Option<&'v Value>)>,
    ) {
        let join = |seg: &str| {
            if prefix.is_empty() {
                seg.to_owned()
            } else {
                format!("{prefix}.{seg}")
            }
        };
        let Some((seg, rest)) = segs.split_first() else {
            out.push((prefix.to_owned(), value));
            return;
        };
        if *seg == "*" {
            match value {
                Some(Value::Array(items)) => {
                    for (i, item) in items.iter().enumerate() {
                        walk(Some(item), rest, &join(&i.to_string()), out);
                    }
                }
                Some(Value::Object(map)) => {
                    for (key, item) in map {
                        walk(Some(item), rest, &join(key), out);
                    }
                }
                _ => {}
            }
            return;
        }
        let child = match value {
            Some(Value::Object(map)) => map.get(*seg),
            Some(Value::Array(items)) => seg.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        };
        walk(child, rest, &join(seg), out);
    }
    let segs: Vec<&str> = pattern.split('.').collect();
    let mut out = Vec::new();
    walk(Some(data), &segs, "", &mut out);
    out
}

/// Paths whose value appears more than once among `targets` (for `distinct`).
fn duplicate_values(targets: &[(String, Option<&Value>)]) -> Vec<String> {
    // A single array field: duplicates within the array.
    if let [(path, Some(Value::Array(items)))] = targets {
        let mut seen = Vec::new();
        for item in items {
            if seen.contains(&item) {
                return vec![path.clone()];
            }
            seen.push(item);
        }
        return Vec::new();
    }
    // A wildcard: duplicates across the matched values; each repeat is flagged.
    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, value) in targets {
        if let Some(v) = value.filter(|v| !v.is_null()) {
            *counts.entry(v.to_string()).or_default() += 1;
        }
    }
    targets
        .iter()
        .filter(|(_, v)| v.is_some_and(|v| counts.get(&v.to_string()).copied().unwrap_or(0) > 1))
        .map(|(path, _)| path.clone())
        .collect()
}

fn humanize(field: &str) -> String {
    field.replace(['_', '-'], " ")
}

/// The "size" of a value: string length, array length, or the number itself.
fn size(value: &Value) -> f64 {
    match value {
        Value::String(s) => s.chars().count() as f64,
        Value::Array(a) => a.len() as f64,
        Value::Object(o) => o.len() as f64,
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        Some(Value::Array(a)) => a.is_empty(),
        Some(Value::Object(o)) => o.is_empty(),
        _ => false,
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

fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// A value as text, for the rules that compare strings (`in`, `starts_with`).
fn text(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}

fn list(arg: Option<&str>) -> Vec<&str> {
    arg.map(|a| a.split(',').map(str::trim).collect())
        .unwrap_or_default()
}

/// Compiled `regex:` patterns, reused across calls.
fn regex(pattern: &str) -> Arc<regex_lite::Regex> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<regex_lite::Regex>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(re) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(pattern) {
        return re.clone();
    }
    // Laravel writes patterns as `/…/`; accept that and the bare form.
    let bare = pattern
        .strip_prefix('/')
        .and_then(|p| p.strip_suffix('/'))
        .unwrap_or(pattern);
    let re = Arc::new(
        regex_lite::Regex::new(bare)
            .unwrap_or_else(|e| panic!("invalid `regex:` pattern `{pattern}`: {e}")),
    );
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(pattern.to_owned(), re.clone());
    re
}

/// A point in time for `date` / `before` / `after`.
#[derive(PartialEq, PartialOrd)]
enum Moment {
    Date(jiff::civil::Date),
    Time(jiff::Timestamp),
}

impl Moment {
    fn parse(s: &str) -> Option<Self> {
        let today = || jiff::Zoned::now().date();
        match s {
            "today" => return Some(Moment::Date(today())),
            "tomorrow" => return today().tomorrow().ok().map(Moment::Date),
            "yesterday" => return today().yesterday().ok().map(Moment::Date),
            _ => {}
        }
        if let Ok(date) = s.parse::<jiff::civil::Date>() {
            return Some(Moment::Date(date));
        }
        s.parse::<jiff::Timestamp>().ok().map(Moment::Time)
    }

    /// Compare across kinds by the UTC date of a timestamp.
    fn cmp_with(&self, other: &Moment) -> Option<std::cmp::Ordering> {
        let as_date = |m: &Moment| match m {
            Moment::Date(d) => *d,
            Moment::Time(t) => t.to_zoned(jiff::tz::TimeZone::UTC).date(),
        };
        match (self, other) {
            (Moment::Time(a), Moment::Time(b)) => a.partial_cmp(b),
            _ => as_date(self).partial_cmp(&as_date(other)),
        }
    }
}

/// A failed rule: a message key (`min.string`) and its placeholder values,
/// rendered into text by [`render`] — from the app's translations when it has
/// them, else from the built-in English.
struct Msg {
    key: &'static str,
    params: Vec<(&'static str, String)>,
}

fn msg(key: &'static str) -> Option<Msg> {
    Some(Msg {
        key,
        params: Vec::new(),
    })
}

impl Msg {
    fn with(mut self, name: &'static str, value: impl ToString) -> Self {
        self.params.push((name, value.to_string()));
        self
    }
}

/// The key for a size-flavoured message, by the value's type.
fn flavour(base: &'static str, value: &Value) -> &'static str {
    match (base, value) {
        ("min", Value::String(_)) => "min.string",
        ("min", Value::Array(_)) => "min.array",
        ("min", _) => "min.numeric",
        ("max", Value::String(_)) => "max.string",
        ("max", Value::Array(_)) => "max.array",
        ("max", _) => "max.numeric",
        ("size", Value::String(_)) => "size.string",
        ("size", Value::Array(_)) => "size.array",
        ("size", _) => "size.numeric",
        ("between", Value::String(_)) => "between.string",
        ("between", Value::Array(_)) => "between.array",
        ("between", _) => "between.numeric",
        ("gt", Value::String(_)) => "gt.string",
        ("gt", Value::Array(_)) => "gt.array",
        ("gt", _) => "gt.numeric",
        ("gte", Value::String(_)) => "gte.string",
        ("gte", Value::Array(_)) => "gte.array",
        ("gte", _) => "gte.numeric",
        ("lt", Value::String(_)) => "lt.string",
        ("lt", Value::Array(_)) => "lt.array",
        ("lt", _) => "lt.numeric",
        ("lte", Value::String(_)) => "lte.string",
        ("lte", Value::Array(_)) => "lte.array",
        (_, _) => "lte.numeric",
    }
}

/// The built-in English for each message key. An app overrides any of them —
/// and translates them — with `validation.<key>` in its translation files.
fn english(key: &str) -> &'static str {
    match key {
        "required" => "The :attribute field is required.",
        "required_if" => "The :attribute field is required when :other is :value.",
        "required_with" => "The :attribute field is required when :values is present.",
        "required_without" => "The :attribute field is required when :values is not present.",
        "accepted" => "The :attribute must be accepted.",
        "filled" => "The :attribute field must have a value.",
        "string" => "The :attribute must be a string.",
        "integer" => "The :attribute must be an integer.",
        "numeric" => "The :attribute must be a number.",
        "boolean" => "The :attribute must be true or false.",
        "array" => "The :attribute must be an array.",
        "email" => "The :attribute must be a valid email address.",
        "url" => "The :attribute must be a valid URL.",
        "uuid" => "The :attribute must be a valid UUID.",
        "ip" => "The :attribute must be a valid IP address.",
        "alpha" => "The :attribute must only contain letters.",
        "alpha_num" => "The :attribute must only contain letters and numbers.",
        "alpha_dash" => {
            "The :attribute must only contain letters, numbers, dashes and underscores."
        }
        "digits" => "The :attribute must be :digits digits.",
        "digits_between" => "The :attribute must be between :min and :max digits.",
        "min.string" => "The :attribute must be at least :min characters.",
        "min.array" => "The :attribute must have at least :min items.",
        "min.numeric" => "The :attribute must be at least :min.",
        "max.string" => "The :attribute must not be greater than :max characters.",
        "max.array" => "The :attribute must not have more than :max items.",
        "max.numeric" => "The :attribute must not be greater than :max.",
        "size.string" => "The :attribute must be :size characters.",
        "size.array" => "The :attribute must contain :size items.",
        "size.numeric" => "The :attribute must be :size.",
        "between.string" => "The :attribute must be between :min and :max characters.",
        "between.array" => "The :attribute must have between :min and :max items.",
        "between.numeric" => "The :attribute must be between :min and :max.",
        "gt.string" => "The :attribute must be greater than :value characters.",
        "gt.array" => "The :attribute must have greater than :value items.",
        "gt.numeric" => "The :attribute must be greater than :value.",
        "gte.string" => "The :attribute must be greater than or equal to :value characters.",
        "gte.array" => "The :attribute must have greater than or equal to :value items.",
        "gte.numeric" => "The :attribute must be greater than or equal to :value.",
        "lt.string" => "The :attribute must be less than :value characters.",
        "lt.array" => "The :attribute must have less than :value items.",
        "lt.numeric" => "The :attribute must be less than :value.",
        "lte.string" => "The :attribute must be less than or equal to :value characters.",
        "lte.array" => "The :attribute must have less than or equal to :value items.",
        "lte.numeric" => "The :attribute must be less than or equal to :value.",
        "in" | "not_in" | "exists" => "The selected :attribute is invalid.",
        "starts_with" => "The :attribute must start with one of the following: :values.",
        "ends_with" => "The :attribute must end with one of the following: :values.",
        "regex" => "The :attribute format is invalid.",
        "date" => "The :attribute is not a valid date.",
        "before" => "The :attribute must be a date before :date.",
        "before_or_equal" => "The :attribute must be a date before or equal to :date.",
        "after" => "The :attribute must be a date after :date.",
        "after_or_equal" => "The :attribute must be a date after or equal to :date.",
        "same" => "The :attribute and :other must match.",
        "confirmed" => "The :attribute confirmation does not match.",
        "distinct" => "The :attribute field has a duplicate value.",
        "unique" => "The :attribute has already been taken.",
        _ => "The :attribute is invalid.",
    }
}

/// Every message key, for a translator to cover (see [`message_keys`]).
const MESSAGE_KEYS: &[&str] = &[
    "required",
    "required_if",
    "required_with",
    "required_without",
    "accepted",
    "filled",
    "string",
    "integer",
    "numeric",
    "boolean",
    "array",
    "email",
    "url",
    "uuid",
    "ip",
    "alpha",
    "alpha_num",
    "alpha_dash",
    "digits",
    "digits_between",
    "min.string",
    "min.array",
    "min.numeric",
    "max.string",
    "max.array",
    "max.numeric",
    "size.string",
    "size.array",
    "size.numeric",
    "between.string",
    "between.array",
    "between.numeric",
    "gt.string",
    "gt.array",
    "gt.numeric",
    "gte.string",
    "gte.array",
    "gte.numeric",
    "lt.string",
    "lt.array",
    "lt.numeric",
    "lte.string",
    "lte.array",
    "lte.numeric",
    "in",
    "not_in",
    "exists",
    "starts_with",
    "ends_with",
    "regex",
    "date",
    "before",
    "before_or_equal",
    "after",
    "after_or_equal",
    "same",
    "confirmed",
    "distinct",
    "unique",
];

/// Every message key the validator can produce, as `(key, built-in English)`
/// — the list a translation file needs to cover under `validation.<key>`.
pub fn message_keys() -> impl Iterator<Item = (&'static str, &'static str)> {
    MESSAGE_KEYS.iter().map(|k| (*k, english(k)))
}

/// Render a message: the app's `validation.<key>` if it has one, else English,
/// with `:attribute` (and `:other`) named through `validation.attributes.*`.
fn render(msg: &Msg, path: &str, pattern: &str, t: Option<&crate::i18n::Translator>) -> String {
    let template = t
        .and_then(|t| t.raw_message(&format!("validation.{}", msg.key)))
        .unwrap_or_else(|| english(msg.key).to_owned());
    let mut params: Vec<(&str, String)> = vec![("attribute", attribute(path, Some(pattern), t))];
    for (name, value) in &msg.params {
        let value = if *name == "other" {
            attribute(value, None, t)
        } else {
            value.clone()
        };
        params.push((name, value));
    }
    fill(&template, &params)
}

/// A field's display name: `validation.attributes.<path>`, then the pattern it
/// came from (`items.*.name`), then the path with `_` / `-` as spaces.
fn attribute(path: &str, pattern: Option<&str>, t: Option<&crate::i18n::Translator>) -> String {
    let lookup = |key: &str| t.and_then(|t| t.raw_message(&format!("validation.attributes.{key}")));
    lookup(path)
        .or_else(|| pattern.and_then(lookup))
        .unwrap_or_else(|| humanize(path))
}

/// Fill `:name` placeholders, longest names first.
fn fill(template: &str, params: &[(&str, String)]) -> String {
    let mut params: Vec<&(&str, String)> = params.iter().collect();
    params.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut out = template.to_owned();
    for (name, value) in params {
        out = out.replace(&format!(":{name}"), value);
    }
    out
}

/// Format a rule's numeric argument the way it was written (`18`, not `18.0`).
fn n(v: f64) -> String {
    v.to_string()
}

/// Apply one rule; `Some(msg)` if it fails.
fn check(name: &str, arg: Option<&str>, value: Option<&Value>, cx: &Cx) -> Option<Msg> {
    let null = Value::Null;
    let v = value.unwrap_or(&null);
    let num = |a: &str| a.trim().parse::<f64>().ok();
    let num_arg = || arg.and_then(num);
    // A numeric literal, or another field's size (`gt:min_price`).
    let bound = |a: &str| num(a).or_else(|| cx.other(a).map(size));
    let fail = |failed: bool, key: &'static str| if failed { msg(key) } else { None };
    match name {
        "required" => fail(is_empty(value), "required"),
        "required_if" => {
            let args = list(arg);
            let (other, wanted) = args.split_first()?;
            let actual = cx.other(other).map(text);
            (actual.is_some_and(|a| wanted.contains(&a.as_str())) && is_empty(value)).then(|| {
                Msg {
                    key: "required_if",
                    params: Vec::new(),
                }
                .with("other", other)
                .with("value", wanted.join(", "))
            })
        }
        "required_with" | "required_without" => {
            let others = list(arg);
            let triggered = if name == "required_with" {
                others.iter().any(|o| !is_empty(cx.other(o)))
            } else {
                others.iter().any(|o| is_empty(cx.other(o)))
            };
            let key = if name == "required_with" {
                "required_with"
            } else {
                "required_without"
            };
            (triggered && is_empty(value)).then(|| {
                Msg {
                    key,
                    params: Vec::new(),
                }
                .with("values", others.join(" / "))
            })
        }
        "accepted" => {
            let yes = matches!(v, Value::Bool(true))
                || v.as_i64() == Some(1)
                || v.as_str()
                    .is_some_and(|s| ["1", "yes", "on", "true"].contains(&s));
            fail(!yes, "accepted")
        }
        "filled" => fail(value.is_some() && is_empty(value), "filled"),
        "nullable" | "sometimes" => None,
        "string" => fail(!v.is_string(), "string"),
        "integer" => fail(!(v.is_i64() || v.is_u64()), "integer"),
        "numeric" => fail(!v.is_number(), "numeric"),
        "boolean" => fail(!v.is_boolean(), "boolean"),
        "array" => fail(!v.is_array(), "array"),
        "email" => fail(!v.as_str().is_some_and(is_email), "email"),
        "url" => fail(
            !v.as_str()
                .is_some_and(|s| s.starts_with("http://") || s.starts_with("https://")),
            "url",
        ),
        "uuid" => fail(!v.as_str().is_some_and(is_uuid), "uuid"),
        "ip" => fail(
            !v.as_str()
                .is_some_and(|s| s.parse::<std::net::IpAddr>().is_ok()),
            "ip",
        ),
        "alpha" => fail(
            !v.as_str()
                .is_some_and(|s| !s.is_empty() && s.chars().all(char::is_alphabetic)),
            "alpha",
        ),
        "alpha_num" => fail(
            !v.as_str()
                .is_some_and(|s| !s.is_empty() && s.chars().all(char::is_alphanumeric)),
            "alpha_num",
        ),
        "alpha_dash" => fail(
            !v.as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            }),
            "alpha_dash",
        ),
        "digits" | "digits_between" => {
            let digits = match v {
                Value::String(s) => Some(s.clone()),
                Value::Number(num) if num.is_u64() => Some(num.to_string()),
                _ => None,
            }
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
            let len = digits.as_ref().map(|s| s.len() as f64);
            if name == "digits" {
                let want = num_arg()?;
                (len != Some(want)).then(|| msg("digits").unwrap().with("digits", n(want)))
            } else {
                let args = list(arg);
                let (lo, hi) = (num(args.first()?)?, num(args.get(1)?)?);
                (!len.is_some_and(|l| l >= lo && l <= hi)).then(|| {
                    msg("digits_between")
                        .unwrap()
                        .with("min", n(lo))
                        .with("max", n(hi))
                })
            }
        }
        "min" => num_arg().and_then(|min| {
            (size(v) < min).then(|| msg(flavour("min", v)).unwrap().with("min", n(min)))
        }),
        "max" => num_arg().and_then(|max| {
            (size(v) > max).then(|| msg(flavour("max", v)).unwrap().with("max", n(max)))
        }),
        "size" => num_arg().and_then(|want| {
            (size(v) != want).then(|| msg(flavour("size", v)).unwrap().with("size", n(want)))
        }),
        "between" => {
            let args = list(arg);
            let (lo, hi) = (num(args.first()?)?, num(args.get(1)?)?);
            let sz = size(v);
            (sz < lo || sz > hi).then(|| {
                msg(flavour("between", v))
                    .unwrap()
                    .with("min", n(lo))
                    .with("max", n(hi))
            })
        }
        "gt" | "gte" | "lt" | "lte" => {
            let b = bound(arg?)?;
            let sz = size(v);
            let ok = match name {
                "gt" => sz > b,
                "gte" => sz >= b,
                "lt" => sz < b,
                _ => sz <= b,
            };
            let base = match name {
                "gt" => "gt",
                "gte" => "gte",
                "lt" => "lt",
                _ => "lte",
            };
            (!ok).then(|| msg(flavour(base, v)).unwrap().with("value", n(b)))
        }
        "in" => fail(!list(arg).contains(&text(v).as_str()), "in"),
        "not_in" => fail(list(arg).contains(&text(v).as_str()), "not_in"),
        "starts_with" | "ends_with" => {
            let affixes = list(arg);
            let ok = v.as_str().is_some_and(|s| {
                affixes.iter().any(|a| {
                    if name == "starts_with" {
                        s.starts_with(a)
                    } else {
                        s.ends_with(a)
                    }
                })
            });
            let key = if name == "starts_with" {
                "starts_with"
            } else {
                "ends_with"
            };
            (!ok).then(|| {
                Msg {
                    key,
                    params: Vec::new(),
                }
                .with("values", affixes.join(", "))
            })
        }
        "regex" => {
            let re = regex(arg?);
            fail(!v.as_str().is_some_and(|s| re.is_match(s)), "regex")
        }
        "date" => fail(
            !v.as_str().is_some_and(|s| Moment::parse(s).is_some()),
            "date",
        ),
        "before" | "before_or_equal" | "after" | "after_or_equal" => {
            let reference = arg?;
            let against = Moment::parse(reference).or_else(|| {
                cx.other(reference)
                    .and_then(Value::as_str)
                    .and_then(Moment::parse)
            })?;
            let ord = v
                .as_str()
                .and_then(Moment::parse)
                .and_then(|m| m.cmp_with(&against));
            use std::cmp::Ordering::*;
            let ok = matches!(
                (name, ord),
                ("before", Some(Less))
                    | ("before_or_equal", Some(Less | Equal))
                    | ("after", Some(Greater))
                    | ("after_or_equal", Some(Greater | Equal))
            );
            let key = match name {
                "before" => "before",
                "before_or_equal" => "before_or_equal",
                "after" => "after",
                _ => "after_or_equal",
            };
            (!ok).then(|| msg(key).unwrap().with("date", reference))
        }
        "same" => arg.and_then(|other| {
            (cx.other(other) != value).then(|| msg("same").unwrap().with("other", other))
        }),
        "confirmed" => {
            let confirmation = format!("{}_confirmation", cx.path);
            fail(
                expand(cx.data, &confirmation)
                    .into_iter()
                    .next()
                    .and_then(|(_, v)| v)
                    != value,
                "confirmed",
            )
        }
        _ => None,
    }
}

/// Run one `unique` / `exists` check. Identifiers come from rule literals and
/// are spliced into SQL, so they're checked; a malformed one is wiring and panics.
#[cfg(feature = "database")]
async fn db_check(db: &elyra_db::Database, check: &DbCheck) -> Option<Msg> {
    use elyra_db::model::{bind_value, placeholder};
    use elyra_db::sqlx::{self, Row};

    let ident = |s: &str| {
        let ok = !s.is_empty()
            && s.split('.').count() <= 2
            && s.split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if !ok {
            panic!(
                "invalid identifier `{s}` in `{}` on `{}`",
                check.rule, check.field
            );
        }
        s.to_owned()
    };
    let args = list(check.arg.as_deref());
    let table = ident(args.first().copied().unwrap_or_else(|| {
        panic!(
            "`{}` on `{}` needs a table: `{}:table,column`",
            check.rule, check.field, check.rule
        )
    }));
    // The column defaults to the field's own name (its last path segment).
    let column = ident(
        args.get(1)
            .copied()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| check.field.rsplit('.').next().unwrap_or(&check.field)),
    );
    let bound = |v: &Value| -> Option<elyra_db::Value> {
        Some(match v {
            Value::String(s) => elyra_db::Value::Text(s.clone()),
            Value::Bool(b) => elyra_db::Value::Bool(*b),
            Value::Number(n) => match n.as_i64() {
                Some(i) => elyra_db::Value::Int(i),
                None => elyra_db::Value::Real(n.as_f64()?),
            },
            _ => return None,
        })
    };
    let Some(value) = bound(&check.value) else {
        return msg("exists");
    };

    let driver = db.driver();
    let mut sql_args = sqlx::any::AnyArguments::default();
    bind_value(&mut sql_args, &value).ok()?;
    let mut sql = format!(
        "SELECT COUNT(*) AS n FROM {table} WHERE {column} = {}",
        placeholder(driver, 1)
    );
    // `unique:users,email,<id>,<id_column>` ignores the row being updated.
    if check.rule == "unique" {
        if let Some(except) = args.get(2).filter(|e| !e.is_empty() && **e != "NULL") {
            let id_column = ident(args.get(3).copied().unwrap_or("id"));
            let except = except
                .parse::<i64>()
                .map(elyra_db::Value::Int)
                .unwrap_or_else(|_| elyra_db::Value::Text((*except).to_owned()));
            bind_value(&mut sql_args, &except).ok()?;
            sql.push_str(&format!(" AND {id_column} <> {}", placeholder(driver, 2)));
        }
    }
    let count: i64 = match sqlx::query_with(sqlx::AssertSqlSafe(sql), sql_args)
        .fetch_one(db.pool())
        .await
    {
        Ok(row) => row.try_get("n").unwrap_or(0),
        Err(e) => panic!(
            "`{}` on `{}` could not query `{table}`: {e}",
            check.rule, check.field
        ),
    };
    match (check.rule.as_str(), count) {
        ("unique", n) if n > 0 => msg("unique"),
        ("exists", 0) => msg("exists"),
        _ => None,
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
    fn errs(input: Value, field: &str, rules: &str) -> ValidationErrors {
        Validator::new(&input).rule(field, rules).errors()
    }

    /// `rules` passes for `good` and fails for `bad`, with a message containing `says`.
    fn both(rules: &str, good: Value, bad: Value, says: &str) {
        let ok = errs(json!({ "f": good.clone() }), "f", rules);
        assert!(ok.is_empty(), "`{rules}` rejected {good}: {ok}");
        let e = errs(json!({ "f": bad.clone() }), "f", rules);
        let msg = e
            .first("f")
            .unwrap_or_else(|| panic!("`{rules}` accepted {bad}"));
        assert!(msg.contains(says), "`{rules}` on {bad}: {msg}");
    }

    #[test]
    #[should_panic(expected = "unknown validation rule `requried` on `email`")]
    fn an_unknown_rule_panics_instead_of_passing_everything() {
        errs(json!({}), "email", "requried|email");
    }

    #[test]
    fn type_and_format_rules() {
        both("array", json!([1]), json!("x"), "must be an array");
        both(
            "uuid",
            json!("67e55044-10b1-426f-9247-bb680e5fe0c8"),
            json!("67e55044-10b1"),
            "valid UUID",
        );
        both("ip", json!("192.168.0.1"), json!("999.1.1.1"), "valid IP");
        both("ip", json!("::1"), json!("nope"), "valid IP");
        both(
            "alpha",
            json!("Øyvind"),
            json!("abc1"),
            "only contain letters.",
        );
        both(
            "alpha_num",
            json!("abc123"),
            json!("abc-1"),
            "letters and numbers",
        );
        both(
            "alpha_dash",
            json!("a-b_c1"),
            json!("a b"),
            "dashes and underscores",
        );
        both("digits:4", json!("0123"), json!("123"), "must be 4 digits");
        both("digits:4", json!(1234), json!("12a4"), "must be 4 digits");
        both(
            "digits_between:2,4",
            json!("123"),
            json!("12345"),
            "between 2 and 4 digits",
        );
    }

    #[test]
    fn comparison_rules() {
        both(
            "between:3,5",
            json!("abcd"),
            json!("ab"),
            "between 3 and 5 characters",
        );
        both("between:1,10", json!(5), json!(11), "between 1 and 10.");
        both("gt:10", json!(11), json!(10), "greater than 10");
        both("gte:10", json!(10), json!(9), "greater than or equal to 10");
        both("lt:3", json!("ab"), json!("abc"), "less than 3 characters");
        both(
            "lte:2",
            json!([1, 2]),
            json!([1, 2, 3]),
            "less than or equal to 2 items",
        );
        // Against another field.
        let input = json!({ "min": 5, "max": 3 });
        assert!(Validator::new(&input)
            .rule("max", "gt:min")
            .errors()
            .has("max"));
        assert!(Validator::new(&input)
            .rule("min", "gt:max")
            .errors()
            .is_empty());
    }

    #[test]
    fn membership_and_string_shape() {
        both(
            "not_in:admin,root",
            json!("ada"),
            json!("root"),
            "selected f is invalid",
        );
        both(
            "starts_with:http,ftp",
            json!("ftp://x"),
            json!("ssh://x"),
            "start with one of the following: http, ftp",
        );
        both(
            "ends_with:.csv",
            json!("a.csv"),
            json!("a.txt"),
            "end with one of the following: .csv",
        );
        both(
            "regex:^[A-Z]{3}$",
            json!("ABC"),
            json!("ABCD"),
            "format is invalid",
        );
        both(
            "regex:/^\\d+$/",
            json!("123"),
            json!("12a"),
            "format is invalid",
        );
    }

    #[test]
    fn a_regex_alternation_goes_through_rule_list() {
        // `|` would split a rule string; the list form keeps the pattern whole.
        let ok = json!({ "code": "cd42" });
        let bad = json!({ "code": "ef42" });
        let v = |input: &Value| {
            Validator::new(input)
                .rule_list("code", &["required", "regex:^(ab|cd)[0-9]+$"])
                .errors()
        };
        assert!(v(&ok).is_empty());
        assert!(v(&bad).has("code"));
    }

    #[test]
    #[should_panic(expected = "invalid `regex:` pattern")]
    fn an_invalid_regex_panics() {
        errs(json!({ "f": "x" }), "f", "regex:([");
    }

    #[test]
    fn date_rules() {
        both(
            "date",
            json!("2026-09-23"),
            json!("2026-02-30"),
            "not a valid date",
        );
        both(
            "date",
            json!("2026-09-23T10:00:00Z"),
            json!("next week"),
            "not a valid date",
        );
        both(
            "after:2026-01-01",
            json!("2026-06-01"),
            json!("2025-12-31"),
            "date after 2026-01-01",
        );
        both(
            "before:2026-01-01",
            json!("2025-12-31"),
            json!("2026-01-01"),
            "date before 2026-01-01",
        );
        both(
            "before_or_equal:2026-01-01",
            json!("2026-01-01"),
            json!("2026-01-02"),
            "before or equal to",
        );
        both(
            "after:today",
            json!("2999-01-01"),
            json!("2000-01-01"),
            "date after today",
        );
        // Against another field.
        let input = json!({ "start": "2026-05-01", "end": "2026-04-01" });
        let e = Validator::new(&input).rule("end", "after:start").errors();
        assert_eq!(e.first("end"), Some("The end must be a date after start."));
    }

    #[test]
    fn implicit_rules_run_on_absent_fields() {
        both("accepted", json!("yes"), json!("no"), "must be accepted");
        assert!(
            errs(json!({}), "terms", "accepted").has("terms"),
            "absent is not accepted"
        );

        // `filled`: may be absent, but not present-and-empty.
        assert!(errs(json!({}), "nick", "filled").is_empty());
        assert!(errs(json!({ "nick": "" }), "nick", "filled").has("nick"));

        let business = json!({ "type": "business" });
        let e = Validator::new(&business)
            .rule("vat", "required_if:type,business,org")
            .errors();
        assert_eq!(
            e.first("vat"),
            Some("The vat field is required when type is business, org.")
        );
        assert!(Validator::new(&json!({ "type": "person" }))
            .rule("vat", "required_if:type,business")
            .errors()
            .is_empty());

        let with = json!({ "street": "Storgata 1" });
        assert!(Validator::new(&with)
            .rule("city", "required_with:street")
            .errors()
            .has("city"));
        assert!(Validator::new(&json!({}))
            .rule("city", "required_with:street")
            .errors()
            .is_empty());

        assert!(Validator::new(&json!({}))
            .rule("email", "required_without:phone")
            .errors()
            .has("email"));
        assert!(Validator::new(&json!({ "phone": "123" }))
            .rule("email", "required_without:phone")
            .errors()
            .is_empty());
    }

    #[test]
    fn nested_paths_are_validated_and_keyed_by_path() {
        let input = json!({ "address": { "city": "", "zip": "0150" } });
        let e = Validator::new(&input)
            .rules(&[
                ("address.city", "required"),
                ("address.zip", "digits:4"),
                ("address.country", "required"),
            ])
            .errors();
        assert_eq!(
            e.first("address.city"),
            Some("The address.city field is required.")
        );
        assert!(!e.has("address.zip"));
        assert!(
            e.has("address.country"),
            "a missing nested field still meets `required`"
        );
    }

    #[test]
    fn wildcards_expand_over_every_element() {
        let input = json!({ "items": [
            { "name": "Pen", "qty": 2 },
            { "qty": 0 },
            { "name": "", "qty": 5 },
        ]});
        let e = Validator::new(&input)
            .rules(&[
                ("items.*.name", "required|string"),
                ("items.*.qty", "integer|min:1"),
            ])
            .errors();
        assert!(!e.has("items.0.name"));
        assert_eq!(
            e.first("items.1.name"),
            Some("The items.1.name field is required.")
        );
        assert!(e.has("items.2.name"));
        assert!(e.has("items.1.qty"));
        assert!(!e.has("items.0.qty") && !e.has("items.2.qty"));
        // No array, nothing to expand: a wildcard rule has no targets.
        assert!(errs(json!({}), "items.*.name", "required").is_empty());
    }

    #[test]
    fn a_sibling_reference_follows_the_wildcard_index() {
        let input = json!({ "items": [
            { "kind": "digital" },
            { "kind": "physical" },
        ]});
        let e = Validator::new(&input)
            .rule("items.*.weight", "required_if:items.*.kind,physical")
            .errors();
        assert!(!e.has("items.0.weight"));
        assert!(e.has("items.1.weight"));
    }

    #[test]
    fn distinct_within_an_array_and_across_a_wildcard() {
        assert!(errs(json!({ "tags": ["a", "b"] }), "tags", "distinct").is_empty());
        assert!(errs(json!({ "tags": ["a", "a"] }), "tags", "distinct").has("tags"));

        let input = json!({ "rows": [{ "sku": "A" }, { "sku": "B" }, { "sku": "A" }] });
        let e = Validator::new(&input)
            .rule("rows.*.sku", "distinct")
            .errors();
        assert!(e.has("rows.0.sku") && e.has("rows.2.sku"));
        assert!(!e.has("rows.1.sku"));
    }

    #[test]
    #[should_panic(
        expected = "rule `unique` on `email` needs the database: use `validate_with(&db).await`"
    )]
    fn database_rules_refuse_the_synchronous_path() {
        errs(
            json!({ "email": "a@b.co" }),
            "email",
            "required|email|unique:users",
        );
    }
    fn norwegian() -> crate::i18n::Translator {
        let t = crate::i18n::Translator::new("en").add_json(
            "nb",
            r#"{ "validation": {
                "required": ":attribute må fylles ut.",
                "min": { "string": ":attribute må ha minst :min tegn." },
                "same": ":attribute og :other må være like.",
                "attributes": { "email": "e-postadressen", "password_confirmation": "bekreftelsen",
                                "items.*.name": "varenavnet" } } }"#,
        );
        t.set_locale("nb");
        t
    }

    #[test]
    fn messages_and_field_names_come_from_the_translator() {
        let t = norwegian();
        let input = json!({ "email": "", "name": "ab" });
        let e = Validator::new(&input)
            .translator(&t)
            .rules(&[("email", "required"), ("name", "min:3")])
            .errors();
        assert_eq!(e.first("email"), Some("e-postadressen må fylles ut."));
        // No attribute translation for `name`: the humanised field name.
        assert_eq!(e.first("name"), Some("name må ha minst 3 tegn."));
    }

    #[test]
    fn a_missing_translation_falls_back_to_english() {
        let t = norwegian();
        let e = Validator::new(&json!({ "email": "nope" }))
            .translator(&t)
            .rule("email", "email")
            .errors();
        // `validation.email` isn't translated, but the attribute still is.
        assert_eq!(
            e.first("email"),
            Some("The e-postadressen must be a valid email address.")
        );
    }

    #[test]
    fn other_fields_and_wildcard_patterns_are_named_too() {
        let t = norwegian();
        let input = json!({ "password": "a", "password_confirmation": "b",
                            "items": [{ "name": "" }] });
        let e = Validator::new(&input)
            .translator(&t)
            .rules(&[
                ("password", "same:password_confirmation"),
                ("items.*.name", "required"),
            ])
            .errors();
        assert_eq!(
            e.first("password"),
            Some("password og bekreftelsen må være like.")
        );
        assert_eq!(
            e.first("items.0.name"),
            Some("varenavnet må fylles ut."),
            "named via its pattern"
        );
    }

    #[test]
    fn without_a_translator_nothing_changes() {
        let e = errs(json!({ "email": "" }), "email", "required");
        assert_eq!(e.first("email"), Some("The email field is required."));
    }

    #[test]
    fn every_message_key_has_english_and_uses_its_placeholders() {
        let keys: Vec<_> = message_keys().collect();
        assert_eq!(keys.len(), 59, "a message key was added or removed");
        for (key, english) in keys {
            assert!(
                english.contains(":attribute"),
                "`{key}` has no :attribute: {english}"
            );
            assert_ne!(
                english, "The :attribute is invalid.",
                "`{key}` fell through to the default"
            );
        }
    }
}
