use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::{
    client::Ai,
    error::Result,
    message::Message,
    provider::Provider,
    request::{StructuredTool, TextRequest},
    response::Response,
    tool::Tool,
    {anthropic, openai},
};

/// A one-off ("anonymous") agent: configure instructions, tools, and model,
/// then [`prompt`](Chat::prompt) it. The Rust analogue of Laravel's `agent(...)`
/// helper.
pub struct Chat<'a> {
    ai: &'a Ai,
    provider: Option<Provider>,
    model: Option<String>,
    system: Option<String>,
    messages: Vec<Message>,
    temperature: Option<f32>,
    max_tokens: u32,
    tools: Vec<Box<dyn Tool>>,
    max_steps: u32,
}

impl<'a> Chat<'a> {
    pub(crate) fn new(ai: &'a Ai) -> Self {
        Self {
            ai,
            provider: None,
            model: None,
            system: None,
            messages: Vec::new(),
            temperature: None,
            max_tokens: 4096,
            tools: Vec::new(),
            max_steps: 8,
        }
    }

    /// Set the system prompt (the agent's instructions).
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.system = Some(instructions.into());
        self
    }

    /// Choose the provider (defaults to the client's default).
    pub fn provider(mut self, provider: Provider) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Choose the model (defaults to the provider's default text model).
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Sampling temperature.
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Max tokens to generate (default 4096).
    pub fn max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Maximum tool-use steps (default 8).
    pub fn max_steps(mut self, max_steps: u32) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Append a message (e.g. prior conversation).
    pub fn message(mut self, message: Message) -> Self {
        self.messages.push(message);
        self
    }

    /// Add a tool the model may call.
    pub fn tool(mut self, tool: impl Tool + 'static) -> Self {
        self.tools.push(Box::new(tool));
        self
    }

    pub(crate) fn tool_boxed(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Prompt the agent with `input` and return the text response.
    pub async fn prompt(mut self, input: impl Into<String>) -> Result<Response> {
        self.messages.push(Message::user(input));
        self.execute(None).await
    }

    /// Prompt for **structured output**: the model is forced to return JSON
    /// matching `T`'s schema, which is then deserialized into `T`.
    pub async fn prompt_as<T>(mut self, input: impl Into<String>) -> Result<T>
    where
        T: DeserializeOwned + JsonSchema,
    {
        self.messages.push(Message::user(input));
        let force = StructuredTool {
            name: "respond".into(),
            schema: json_schema_for::<T>(),
        };
        let response = self.execute(Some(force)).await?;
        response.parse::<T>()
    }

    async fn execute(self, force: Option<StructuredTool>) -> Result<Response> {
        let provider = self.provider.unwrap_or_else(|| self.ai.default_provider());
        let model = self
            .model
            .clone()
            .unwrap_or_else(|| self.ai.model_for(provider));
        let req = TextRequest {
            provider,
            model,
            system: self.system,
            messages: self.messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            force,
            max_steps: self.max_steps,
        };
        match provider {
            Provider::Anthropic => anthropic::run(self.ai, req, &self.tools).await,
            Provider::OpenAI => openai::run(self.ai, req, &self.tools).await,
        }
    }
}

/// Build a JSON-Schema `Value` for `T` (inlined root schema).
fn json_schema_for<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| serde_json::json!({"type": "object"}))
}
