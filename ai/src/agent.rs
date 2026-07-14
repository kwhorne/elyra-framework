use crate::{message::Message, provider::Provider, tool::Tool};

/// A named, reusable agent — the Rust analogue of a Laravel agent class.
///
/// Only [`instructions`](Agent::instructions) is required; override the rest to
/// add conversation history, tools, or per-agent model configuration. Prompt it
/// with [`Ai::prompt`](crate::Ai::prompt).
pub trait Agent: Send + Sync {
    /// The system prompt describing the agent's role.
    fn instructions(&self) -> String;

    /// Prior conversation messages (for context / memory).
    fn messages(&self) -> Vec<Message> {
        Vec::new()
    }

    /// Tools (and sub-agents) the agent may call.
    fn tools(&self) -> Vec<Box<dyn Tool>> {
        Vec::new()
    }

    /// Override the provider (defaults to the client's default).
    fn provider(&self) -> Option<Provider> {
        None
    }

    /// Override the model.
    fn model(&self) -> Option<String> {
        None
    }

    /// Sampling temperature (0.0–1.0).
    fn temperature(&self) -> Option<f32> {
        None
    }

    /// Max tokens to generate.
    fn max_tokens(&self) -> Option<u32> {
        None
    }

    /// Maximum tool-use steps before giving up.
    fn max_steps(&self) -> u32 {
        8
    }

    /// Tool name when this agent is used as a sub-agent (override for distinct
    /// names when a parent has several sub-agents).
    fn name(&self) -> String {
        "sub_agent".into()
    }

    /// Tool description when used as a sub-agent (defaults to the instructions).
    fn description(&self) -> String {
        self.instructions()
    }
}
