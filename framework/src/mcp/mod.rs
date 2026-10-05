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

mod schema;

use serde::Serialize;
use serde_json::{json, Value};
use specta::{Format, Types};

use crate::command::CommandRegistry;
use crate::security::ability_matches;

/// What an AI agent may reach over MCP. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Mcp {
    abilities: Vec<String>,
    confirm: Vec<String>,
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

    /// Whether the agent may call a command that requires `ability`.
    pub fn grants(&self, ability: &str) -> bool {
        self.abilities.iter().any(|g| ability_matches(g, ability))
    }

    /// Whether a call needing `ability` must be confirmed first.
    pub fn needs_confirmation(&self, ability: &str) -> bool {
        self.confirm.iter().any(|g| ability_matches(g, ability))
    }
}

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
        });
    }
    Ok(tools)
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
