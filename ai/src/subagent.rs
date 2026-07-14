use async_trait::async_trait;
use serde_json::{json, Value};

use crate::{agent::Agent, client::Ai, error::Result, tool::Tool};

/// Wraps an [`Agent`] so a parent agent can call it as a tool — a **sub-agent**.
///
/// Like Laravel's sub-agents, the delegate runs in isolation: it does **not**
/// receive the parent's conversation history, only the task the parent hands it.
/// The tool name and description default to the agent's
/// [`name`](Agent::name)/[`description`](Agent::description); override them with
/// [`with_name`](AgentTool::with_name) / [`with_description`](AgentTool::with_description).
pub struct AgentTool {
    ai: Ai,
    agent: Box<dyn Agent>,
    name: String,
    description: String,
}

impl AgentTool {
    /// Wrap `agent` as a tool bound to `ai`.
    pub fn new(ai: &Ai, agent: impl Agent + 'static) -> Self {
        let name = agent.name();
        let description = agent.description();
        Self {
            ai: ai.clone(),
            agent: Box::new(agent),
            name,
            description,
        }
    }

    /// Override the tool name the parent model uses.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Override the tool description shown to the parent model.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn description(&self) -> String {
        self.description.clone()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task": {
                    "type": "string",
                    "description": "The task or question to delegate to this sub-agent."
                }
            },
            "required": ["task"]
        })
    }

    async fn call(&self, args: Value) -> Result<String> {
        let task = args["task"].as_str().unwrap_or_default().to_string();
        let response = self.ai.prompt(self.agent.as_ref(), task).await?;
        Ok(response.text().to_string())
    }
}
