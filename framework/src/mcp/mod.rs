//! An Elyra app as an MCP server (RFC 0003): a chosen set of its commands,
//! exposed as tools to an AI agent over the Model Context Protocol.
//!
//! Nothing is exposed by default. A command is a tool when the ability its
//! `#[command(can = "…")]` declares is granted to the agent — a grant separate
//! from the frontend's:
//!
//! ```ignore
//! App::new().mcp(Mcp::new()
//!     .allow_abilities(["customers.view", "customers.create"])
//!     .confirm("customers.delete"))
//! ```
//!
//! Each tool's definition is a projection of its command: the name, the doc
//! comment as its description, and JSON Schemas for its arguments and result
//! built from the same specta types codegen exports.

mod confirm;
pub(crate) mod endpoint;
mod schema;
pub mod server;

pub use server::{Connection, McpServer};

use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};
use specta::{Format, Types};

use crate::command::CommandRegistry;
use crate::security::ability_matches;

/// How many calls a tool takes by default: 60 a minute — plenty for an agent
/// at work, and a stop for one caught in a loop.
pub const DEFAULT_RATE_LIMIT: (u32, Duration) = (60, Duration::from_secs(60));

/// What an AI agent may reach over MCP. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Mcp {
    abilities: Vec<String>,
    confirm: Vec<String>,
    limits: Vec<(String, u32, Duration)>,
}

impl Mcp {
    /// Expose nothing yet — grant abilities to expose the commands behind them.
    pub fn new() -> Self {
        Self::default()
    }

    /// Expose the commands whose `can` ability this covers — an exact name, or
    /// a namespace ending in `*` (`customers.*`). Commands without an ability
    /// are never exposed.
    pub fn allow_ability(mut self, ability: impl Into<String>) -> Self {
        self.abilities.push(ability.into());
        self
    }

    /// [`allow_ability`](Self::allow_ability) for several at once.
    pub fn allow_abilities<I, S>(mut self, abilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.abilities.extend(abilities.into_iter().map(Into::into));
        self
    }

    /// Ask the user before an agent runs a command with this ability (or
    /// namespace): the MCP client is asked to confirm first, and a client that
    /// can't ask gets an error rather than a silent run. The ability must also
    /// be allowed for the command to be a tool at all.
    pub fn confirm(mut self, ability: impl Into<String>) -> Self {
        self.confirm.push(ability.into());
        self
    }

    /// Let the tools with this ability (or namespace, or `*` for all) run at
    /// most `max` times `per` window — counted per tool, across connections.
    /// The last limit that matches wins; without one it's
    /// [`DEFAULT_RATE_LIMIT`]. Reads of a resource count, re-runs for a
    /// subscription don't.
    pub fn rate_limit(mut self, ability: impl Into<String>, max: u32, per: Duration) -> Self {
        self.limits.push((ability.into(), max, per));
        self
    }

    /// The limit for a tool with `ability`.
    pub fn limit_for(&self, ability: &str) -> (u32, Duration) {
        self.limits
            .iter()
            .rev()
            .find(|(g, _, _)| ability_matches(g, ability))
            .map_or(DEFAULT_RATE_LIMIT, |(_, max, per)| (*max, *per))
    }

    /// Whether the agent may call a command that requires `ability`.
    pub fn grants(&self, ability: &str) -> bool {
        self.abilities.iter().any(|g| ability_matches(g, ability))
    }

    /// Whether a call needing `ability` must be confirmed first.
    pub fn needs_confirmation(&self, ability: &str) -> bool {
        self.confirm.iter().any(|g| ability_matches(g, ability))
    }
}

/// An MCP agent ran a tool, or read a resource — a domain event, for an audit
/// log: `App::listen(|e: AgentCalled, ctx| …)`. Dispatched in the
/// background once the call is done. It carries no arguments: they may be
/// personal data.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AgentCalled {
    /// The command.
    pub tool: String,
    /// The client's name, as it gave it.
    pub client: String,
    /// Whether it succeeded.
    pub ok: bool,
    /// How long it took, in milliseconds.
    pub millis: u64,
}

/// What the `elyra:mcp` channel carries to every window — a `started`, then a
/// `finished` with `ok`, for an "Claude is adding a customer…" indicator.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct AgentActivity {
    pub tool: String,
    pub client: String,
    /// `"started"` or `"finished"`.
    pub phase: &'static str,
    /// On `finished`: whether it succeeded.
    pub ok: Option<bool>,
}

/// The channel [`AgentActivity`] arrives on.
pub const CHANNEL: &str = "elyra:mcp";

/// One tool, as MCP's `tools/list` describes it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
    pub annotations: ToolAnnotations,
    /// Not part of MCP: the ability that exposed it (for `rata mcp inspect`).
    #[serde(skip)]
    pub ability: String,
    /// The argument names, in the order the command takes them.
    #[serde(skip)]
    pub args: Vec<String>,
    /// For a live command that can run with no arguments given — an MCP
    /// resource, `app://<name>` — what to pass for each.
    #[serde(skip)]
    pub defaults: Option<Vec<Value>>,
}

impl Tool {
    /// Its resource URI, when it is one.
    pub fn resource_uri(&self) -> Option<String> {
        self.defaults
            .as_ref()
            .map(|_| format!("app://{}", self.name))
    }
}

/// MCP's behavior hints. Clients treat them as untrusted; the server enforces
/// what matters (a live command can't write; a confirmed one waits).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    /// A `#[command(live)]` — it only reads.
    pub read_only_hint: bool,
    /// Its ability is one `Mcp::confirm` names.
    pub destructive_hint: bool,
}

/// The tools `mcp` exposes from `registry`, in name order — deterministic, as
/// MCP asks, so a client's cache and the model's prompt stay stable.
pub fn catalog(registry: &CommandRegistry, mcp: &Mcp) -> Result<Vec<Tool>, String> {
    let mut types = Types::default();
    let mut exposed: Vec<(&dyn crate::command::Command, &'static str)> = registry
        .commands()
        .filter_map(|cmd| {
            let ability = cmd.ability()?;
            mcp.grants(ability).then_some((cmd, ability))
        })
        .collect();
    exposed.sort_by_key(|(cmd, _)| cmd.name());

    let sigs: Vec<_> = exposed
        .iter()
        .map(|(cmd, _)| cmd.signature(&mut types))
        .collect();
    // The wire shape serde produces — renames, tagging, flattening applied.
    let format = specta_serde::Format;
    let types = format
        .map_types(&types)
        .map_err(|e| format!("mcp: {e}"))?
        .into_owned();

    let mut tools = Vec::with_capacity(exposed.len());
    for ((cmd, ability), sig) in exposed.iter().zip(sigs) {
        let args: Vec<(&str, specta::datatype::DataType)> = sig
            .args
            .iter()
            .map(|(name, dt)| {
                format
                    .map_type(&types, dt)
                    .map(|dt| (*name, dt.into_owned()))
                    .map_err(|e| format!("mcp: `{}`: {e}", cmd.name()))
            })
            .collect::<Result<_, _>>()?;
        let ret = format
            .map_type(&types, &sig.ret)
            .map_err(|e| format!("mcp: `{}`: {e}", cmd.name()))?
            .into_owned();

        let mut inputs = schema::SchemaBuilder::new(&types, schema::Direction::Input);
        let input_schema = schema::arguments(&mut inputs, &args);
        let input_schema = schema::with_defs(input_schema, inputs.defs());
        // A resource: live, with no question to ask before it runs.
        let defaults = (cmd.live() && !mcp.needs_confirmation(ability))
            .then(|| defaults(&input_schema, &args))
            .flatten();
        let mut outputs = schema::SchemaBuilder::new(&types, schema::Direction::Output);
        let output_schema = outputs.schema(&ret);
        let output_schema = schema::with_defs(output_schema, outputs.defs());

        tools.push(Tool {
            name: cmd.name().to_owned(),
            description: cmd.description().to_owned(),
            input_schema,
            output_schema,
            annotations: ToolAnnotations {
                read_only_hint: cmd.live(),
                destructive_hint: mcp.needs_confirmation(ability),
            },
            ability: (*ability).to_owned(),
            args: args.iter().map(|(name, _)| (*name).to_owned()).collect(),
            defaults,
        });
    }
    Ok(tools)
}

/// What to pass for each argument when none is given, if every one can be
/// left out: `null` for an `Option`, `{}` for a struct whose fields all are.
fn defaults(input: &Value, args: &[(&str, specta::datatype::DataType)]) -> Option<Vec<Value>> {
    let required = |schema: &Value, name: &str| {
        schema["required"]
            .as_array()
            .is_some_and(|r| r.iter().any(|n| n == name))
    };
    args.iter()
        .map(|(name, _)| {
            if !required(input, name) {
                return Some(Value::Null);
            }
            let mut schema = &input["properties"][*name];
            if let Some(name) = schema["$ref"]
                .as_str()
                .and_then(|r| r.strip_prefix("#/$defs/"))
            {
                schema = &input["$defs"][name];
            }
            let empty = schema["required"].as_array().is_none_or(Vec::is_empty);
            (schema["type"] == "object" && empty).then(|| json!({}))
        })
        .collect()
}

/// The catalog as `rata mcp inspect` reads it: the tools, plus each one's
/// ability (which the MCP wire format doesn't carry).
pub(crate) fn inspect_json(tools: &[Tool]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|t| {
                let mut v = serde_json::to_value(t).unwrap_or(Value::Null);
                v["ability"] = json!(t.ability);
                v
            })
            .collect(),
    )
}
