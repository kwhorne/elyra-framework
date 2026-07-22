use std::hash::{Hash, Hasher};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::{
    client::Ai,
    error::{Error, Result},
    message::Message,
    provider::Provider,
    provider_tool::{ProviderTool, WebFetch, WebSearch},
    request::{StructuredTool, TextRequest},
    response::Response,
    stream::{StreamChunk, TextStream},
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
    provider_tools: Vec<ProviderTool>,
    fallback: Vec<Provider>,
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
            provider_tools: Vec::new(),
            fallback: Vec::new(),
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

    /// Add the native **web search** provider tool (Anthropic).
    pub fn web_search(mut self, config: WebSearch) -> Self {
        self.provider_tools.push(ProviderTool::WebSearch(config));
        self
    }

    /// Add the native **web fetch** provider tool (Anthropic).
    pub fn web_fetch(mut self, config: WebFetch) -> Self {
        self.provider_tools.push(ProviderTool::WebFetch(config));
        self
    }

    /// Providers to fall back to (in order) if the primary fails after retries.
    /// Each fallback uses its own default text model.
    pub fn failover<I: IntoIterator<Item = Provider>>(mut self, providers: I) -> Self {
        self.fallback.extend(providers);
        self
    }

    pub(crate) fn tool_boxed(mut self, tool: Box<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }

    /// Add an [`Agent`](crate::Agent) as a sub-agent tool. The delegate runs in
    /// isolation (no parent history). Override the agent's
    /// [`name`](crate::Agent::name)/[`description`](crate::Agent::description)
    /// for distinct tool names.
    pub fn sub_agent(self, agent: impl crate::Agent + 'static) -> Self {
        let tool = crate::subagent::AgentTool::new(self.ai, agent);
        self.tool_boxed(Box::new(tool))
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

    /// Stream a plain-text response token-by-token. Tools and structured output
    /// are not used in streaming mode. Requires a tokio runtime (Elyra has one).
    pub fn stream(mut self, input: impl Into<String>) -> TextStream {
        self.messages.push(Message::user(input));
        let ai = self.ai.clone();
        let provider = self.provider.unwrap_or_else(|| ai.default_provider());
        let model = self.model.clone().unwrap_or_else(|| ai.model_for(provider));
        let req = TextRequest {
            provider,
            model,
            system: self.system,
            messages: self.messages,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            force: None,
            max_steps: self.max_steps,
            provider_tools: Vec::new(),
        };
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<StreamChunk>>();
        if let Err(e) = ai.check_budget() {
            let _ = tx.send(Err(e));
            return TextStream { rx };
        }
        tokio::spawn(async move {
            match provider {
                Provider::Anthropic => anthropic::stream(&ai, req, tx).await,
                Provider::OpenAI => openai::stream(&ai, req, tx).await,
            }
        });
        TextStream { rx }
    }

    async fn execute(self, force: Option<StructuredTool>) -> Result<Response> {
        self.ai.check_budget()?;
        let primary = self.provider.unwrap_or_else(|| self.ai.default_provider());
        let primary_model = self
            .model
            .clone()
            .unwrap_or_else(|| self.ai.model_for(primary));

        // Cache plain prompts only (tools/provider-tools imply side effects).
        let cacheable =
            self.tools.is_empty() && self.provider_tools.is_empty() && self.ai.cache_enabled();
        let key = cacheable.then(|| {
            cache_key(
                primary,
                &primary_model,
                &self.system,
                &self.messages,
                self.temperature,
                self.max_tokens,
                force.as_ref(),
            )
        });
        if let Some(k) = key {
            if let Some(text) = self.ai.cache_get(k) {
                return Ok(Response {
                    text,
                    usage: crate::Usage::default(),
                    steps: 0,
                });
            }
        }

        // Primary provider first, then each distinct fallback (with its default model).
        let mut attempts: Vec<(Provider, String)> = vec![(primary, primary_model)];
        for p in &self.fallback {
            if !attempts.iter().any(|(ap, _)| ap == p) {
                attempts.push((*p, self.ai.model_for(*p)));
            }
        }

        let mut last_err = Error::Empty;
        for (provider, model) in attempts {
            let req = TextRequest {
                provider,
                model,
                system: self.system.clone(),
                messages: self.messages.clone(),
                temperature: self.temperature,
                max_tokens: self.max_tokens,
                force: force.clone(),
                max_steps: self.max_steps,
                provider_tools: self.provider_tools.clone(),
            };
            let result = match provider {
                Provider::Anthropic => anthropic::run(self.ai, req, &self.tools).await,
                Provider::OpenAI => openai::run(self.ai, req, &self.tools).await,
            };
            match result {
                Ok(resp) => {
                    let usage = resp.usage();
                    self.ai
                        .add_usage((usage.input_tokens + usage.output_tokens) as u64);
                    if let Some(k) = key {
                        self.ai.cache_put(k, resp.text().to_string());
                    }
                    return Ok(resp);
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }
}

/// A stable cache key for a plain prompt (provider/model + full conversation).
#[allow(clippy::too_many_arguments)]
fn cache_key(
    provider: Provider,
    model: &str,
    system: &Option<String>,
    messages: &[Message],
    temperature: Option<f32>,
    max_tokens: u32,
    force: Option<&StructuredTool>,
) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    provider.as_str().hash(&mut h);
    model.hash(&mut h);
    system.hash(&mut h);
    for m in messages {
        m.role.as_str().hash(&mut h);
        m.content.hash(&mut h);
    }
    temperature.map(|t| t.to_bits()).hash(&mut h);
    max_tokens.hash(&mut h);
    if let Some(f) = force {
        f.name.hash(&mut h);
        f.schema.to_string().hash(&mut h);
    }
    h.finish()
}

/// Build a JSON-Schema `Value` for `T` (inlined root schema).
fn json_schema_for<T: JsonSchema>() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(T))
        .unwrap_or_else(|_| serde_json::json!({"type": "object"}))
}
