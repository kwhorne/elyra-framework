use std::collections::HashMap;

use crate::{
    agent::Agent,
    audio::{SpeechRequest, TranscriptionRequest},
    chat::Chat,
    embeddings::EmbeddingRequest,
    error::{Error, Result},
    image::ImageRequest,
    message::Message,
    provider::Provider,
    response::Response,
};

/// The AI client — holds provider credentials, default models, and the HTTP
/// client. Cheap to clone (the inner `reqwest::Client` is `Arc`-backed) and
/// safe to bind in the Elyra container.
#[derive(Clone)]
pub struct Ai {
    pub(crate) http: reqwest::Client,
    keys: HashMap<Provider, String>,
    base_urls: HashMap<Provider, String>,
    default_provider: Provider,
    text_model: Option<String>,
    image_model: String,
    embed_model: String,
    tts_model: String,
    transcribe_model: String,
}

impl Ai {
    /// Build a client from the environment: reads each provider's API key
    /// (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`) and optional base-URL overrides.
    pub fn from_env() -> Ai {
        let mut b = Ai::builder();
        for provider in [Provider::Anthropic, Provider::OpenAI] {
            if let Ok(key) = std::env::var(provider.env_key()) {
                if !key.is_empty() {
                    b = b.provider_key(provider, key);
                }
            }
            if let Ok(url) = std::env::var(provider.env_base_url()) {
                if !url.is_empty() {
                    b = b.base_url(provider, url);
                }
            }
        }
        b.build()
    }

    /// A builder for explicit configuration.
    pub fn builder() -> AiBuilder {
        AiBuilder::default()
    }

    /// The provider used when an agent/chat doesn't specify one.
    pub fn default_provider(&self) -> Provider {
        self.default_provider
    }

    /// Start an anonymous agent (a one-off chat).
    pub fn chat(&self) -> Chat<'_> {
        Chat::new(self)
    }

    /// Prompt a named [`Agent`] with `input`.
    pub async fn prompt(&self, agent: &dyn Agent, input: impl Into<String>) -> Result<Response> {
        let mut chat = Chat::new(self)
            .instructions(agent.instructions())
            .max_steps(agent.max_steps());
        for m in agent.messages() {
            chat = chat.message(m);
        }
        for t in agent.tools() {
            chat = chat.tool_boxed(t);
        }
        if let Some(p) = agent.provider() {
            chat = chat.provider(p);
        }
        if let Some(m) = agent.model() {
            chat = chat.model(m);
        }
        if let Some(t) = agent.temperature() {
            chat = chat.temperature(t);
        }
        if let Some(mt) = agent.max_tokens() {
            chat = chat.max_tokens(mt);
        }
        chat.prompt(input).await
    }

    /// Generate an image from a text prompt (OpenAI).
    pub fn image(&self, prompt: impl Into<String>) -> ImageRequest<'_> {
        ImageRequest::new(self, prompt.into())
    }

    /// Generate embeddings for one or more inputs (OpenAI).
    pub fn embeddings<I, S>(&self, inputs: I) -> EmbeddingRequest<'_>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        EmbeddingRequest::new(self, inputs.into_iter().map(Into::into).collect())
    }

    /// Synthesize speech from text (OpenAI text-to-speech).
    pub fn speech(&self, input: impl Into<String>) -> SpeechRequest<'_> {
        SpeechRequest::new(self, input.into())
    }

    /// Transcribe audio bytes to text (OpenAI speech-to-text). `filename`'s
    /// extension helps the API detect the format (e.g. `"audio.mp3"`).
    pub fn transcribe(
        &self,
        bytes: Vec<u8>,
        filename: impl Into<String>,
    ) -> TranscriptionRequest<'_> {
        TranscriptionRequest::new(self, bytes, filename.into())
    }

    // --- crate-internal accessors -----------------------------------------

    pub(crate) fn key(&self, provider: Provider) -> Result<&str> {
        self.keys
            .get(&provider)
            .map(String::as_str)
            .ok_or(Error::MissingKey(provider.env_key()))
    }

    pub(crate) fn base_url(&self, provider: Provider) -> &str {
        self.base_urls
            .get(&provider)
            .map(String::as_str)
            .unwrap_or_else(|| provider.default_base_url())
    }

    pub(crate) fn model_for(&self, provider: Provider) -> String {
        self.text_model
            .clone()
            .unwrap_or_else(|| provider.default_text_model().to_string())
    }

    pub(crate) fn image_model(&self) -> &str {
        &self.image_model
    }

    pub(crate) fn embed_model(&self) -> &str {
        &self.embed_model
    }

    pub(crate) fn tts_model(&self) -> &str {
        &self.tts_model
    }

    pub(crate) fn transcribe_model(&self) -> &str {
        &self.transcribe_model
    }
}

/// Builder for [`Ai`].
pub struct AiBuilder {
    keys: HashMap<Provider, String>,
    base_urls: HashMap<Provider, String>,
    default_provider: Provider,
    text_model: Option<String>,
    image_model: String,
    embed_model: String,
    tts_model: String,
    transcribe_model: String,
    timeout: std::time::Duration,
}

impl Default for AiBuilder {
    fn default() -> Self {
        Self {
            keys: HashMap::new(),
            base_urls: HashMap::new(),
            default_provider: Provider::Anthropic,
            text_model: None,
            image_model: "gpt-image-1".into(),
            embed_model: "text-embedding-3-small".into(),
            tts_model: "gpt-4o-mini-tts".into(),
            transcribe_model: "whisper-1".into(),
            timeout: std::time::Duration::from_secs(120),
        }
    }
}

impl AiBuilder {
    /// Set a provider's API key.
    pub fn provider_key(mut self, provider: Provider, key: impl Into<String>) -> Self {
        self.keys.insert(provider, key.into());
        self
    }

    /// Override a provider's base URL (proxy/gateway).
    pub fn base_url(mut self, provider: Provider, url: impl Into<String>) -> Self {
        self.base_urls.insert(provider, url.into());
        self
    }

    /// The provider used when none is specified.
    pub fn default_provider(mut self, provider: Provider) -> Self {
        self.default_provider = provider;
        self
    }

    /// Override the default text model (otherwise the provider's default).
    pub fn text_model(mut self, model: impl Into<String>) -> Self {
        self.text_model = Some(model.into());
        self
    }

    /// Override the image model (default `gpt-image-1`).
    pub fn image_model(mut self, model: impl Into<String>) -> Self {
        self.image_model = model.into();
        self
    }

    /// Override the embedding model (default `text-embedding-3-small`).
    pub fn embed_model(mut self, model: impl Into<String>) -> Self {
        self.embed_model = model.into();
        self
    }

    /// Override the text-to-speech model (default `gpt-4o-mini-tts`).
    pub fn tts_model(mut self, model: impl Into<String>) -> Self {
        self.tts_model = model.into();
        self
    }

    /// Override the transcription model (default `whisper-1`).
    pub fn transcribe_model(mut self, model: impl Into<String>) -> Self {
        self.transcribe_model = model.into();
        self
    }

    /// HTTP timeout (default 120s).
    pub fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Finish building.
    pub fn build(self) -> Ai {
        let http = reqwest::Client::builder()
            .timeout(self.timeout)
            .build()
            .unwrap_or_default();
        Ai {
            http,
            keys: self.keys,
            base_urls: self.base_urls,
            default_provider: self.default_provider,
            text_model: self.text_model,
            image_model: self.image_model,
            embed_model: self.embed_model,
            tts_model: self.tts_model,
            transcribe_model: self.transcribe_model,
        }
    }
}

/// Build a client from a single message list — for quick messages helper.
impl Ai {
    /// Convenience: a one-shot chat seeded with `messages`.
    pub fn with_messages(&self, messages: Vec<Message>) -> Chat<'_> {
        let mut chat = Chat::new(self);
        for m in messages {
            chat = chat.message(m);
        }
        chat
    }
}
