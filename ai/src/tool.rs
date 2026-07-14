use async_trait::async_trait;

use crate::error::Result;

/// A tool the model can call during a prompt.
///
/// Mirrors Laravel's `Tool` contract: a `name`, a `description`, a JSON-Schema
/// for the parameters, and a `call` that runs the tool and returns a string the
/// model reads back.
#[async_trait]
pub trait Tool: Send + Sync {
    /// The tool name the model uses to call it (snake_case recommended).
    fn name(&self) -> String;

    /// What the tool does — shown to the model.
    fn description(&self) -> String;

    /// JSON Schema (an `object` schema) describing the call arguments.
    fn parameters(&self) -> serde_json::Value;

    /// Run the tool with the model-provided `args`.
    async fn call(&self, args: serde_json::Value) -> Result<String>;
}
