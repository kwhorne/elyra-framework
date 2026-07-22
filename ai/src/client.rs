use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

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
    retries: u32,
    retry_backoff: Duration,
    #[allow(clippy::type_complexity)]
    cache: Option<Arc<Mutex<HashMap<u64, (String, Instant)>>>>,
    cache_ttl: Option<Duration>,
    rate_limit: u32,
    rate_window: Arc<Mutex<VecDeque<Instant>>>,
    budget: Option<u64>,
    used: Arc<AtomicU64>,
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

    /// Send a request with the configured retry policy. Retries transient
    /// failures (timeouts, connection errors) and retryable statuses (429, 5xx,
    /// 529) with exponential backoff. Non-cloneable bodies (e.g. multipart) are
    /// sent once.
    pub(crate) async fn send(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        self.throttle().await;
        let mut attempt = 0u32;
        loop {
            let this = builder.try_clone();
            let send = match this {
                Some(clone) => clone.send().await,
                None => return builder.send().await.map_err(Into::into),
            };
            match send {
                Ok(resp) => {
                    if is_retryable_status(resp.status().as_u16()) && attempt < self.retries {
                        attempt += 1;
                        tokio::time::sleep(self.backoff(attempt)).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(e) => {
                    if is_transient(&e) && attempt < self.retries {
                        attempt += 1;
                        tokio::time::sleep(self.backoff(attempt)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }

    fn backoff(&self, attempt: u32) -> Duration {
        let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
        (self.retry_backoff * factor).min(Duration::from_secs(8))
    }

    pub(crate) fn cache_enabled(&self) -> bool {
        self.cache.is_some()
    }

    pub(crate) fn cache_get(&self, key: u64) -> Option<String> {
        let cache = self.cache.as_ref()?;
        let mut map = cache.lock();
        let (text, at) = map.get(&key)?.clone();
        if let Some(ttl) = self.cache_ttl {
            if at.elapsed() > ttl {
                map.remove(&key);
                return None;
            }
        }
        Some(text)
    }

    pub(crate) fn cache_put(&self, key: u64, text: String) {
        if let Some(cache) = &self.cache {
            cache.lock().insert(key, (text, Instant::now()));
        }
    }

    /// Wait (don't error) until under the configured requests-per-minute limit.
    async fn throttle(&self) {
        if self.rate_limit == 0 {
            return;
        }
        loop {
            let wait = {
                let mut win = self.rate_window.lock();
                let now = Instant::now();
                let cutoff = now - Duration::from_secs(60);
                while win.front().map(|&t| t < cutoff).unwrap_or(false) {
                    win.pop_front();
                }
                if (win.len() as u32) < self.rate_limit {
                    win.push_back(now);
                    return;
                }
                let oldest = *win.front().unwrap();
                (oldest + Duration::from_secs(60)).saturating_duration_since(now)
            };
            tokio::time::sleep(wait).await;
        }
    }

    /// Error if the cumulative token budget is exhausted.
    pub(crate) fn check_budget(&self) -> Result<()> {
        match self.budget {
            Some(max) if self.used.load(Ordering::Relaxed) >= max => Err(Error::Budget),
            _ => Ok(()),
        }
    }

    /// Count tokens against the budget.
    pub(crate) fn add_usage(&self, tokens: u64) {
        self.used.fetch_add(tokens, Ordering::Relaxed);
    }

    /// Total tokens counted against the budget so far.
    pub fn tokens_used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }

    /// Empty the response cache.
    pub fn clear_cache(&self) {
        if let Some(cache) = &self.cache {
            cache.lock().clear();
        }
    }
}

fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500 | 502 | 503 | 504 | 529)
}

fn is_transient(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect() || e.is_request()
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
    timeout: Duration,
    retries: u32,
    retry_backoff: Duration,
    cache: bool,
    cache_ttl: Option<Duration>,
    rate_limit: u32,
    budget: Option<u64>,
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
            timeout: Duration::from_secs(120),
            retries: 2,
            retry_backoff: Duration::from_millis(500),
            cache: false,
            cache_ttl: None,
            rate_limit: 0,
            budget: None,
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
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Max retries for transient failures (default 2; 0 disables).
    pub fn retries(mut self, retries: u32) -> Self {
        self.retries = retries;
        self
    }

    /// Base backoff between retries (doubles each attempt, capped at 8s).
    pub fn retry_backoff(mut self, backoff: Duration) -> Self {
        self.retry_backoff = backoff;
        self
    }

    /// Enable in-memory response caching for plain prompts (no tools).
    pub fn cache(mut self, enabled: bool) -> Self {
        self.cache = enabled;
        self
    }

    /// Enable caching with a time-to-live.
    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache = true;
        self.cache_ttl = Some(ttl);
        self
    }

    /// Throttle to at most `per_minute` requests (waits rather than erroring;
    /// 0 disables). Applies to every provider call.
    pub fn rate_limit(mut self, per_minute: u32) -> Self {
        self.rate_limit = per_minute;
        self
    }

    /// Refuse new prompts once cumulative tokens reach `max` (`Error::Budget`).
    pub fn token_budget(mut self, max: u64) -> Self {
        self.budget = Some(max);
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
            retries: self.retries,
            retry_backoff: self.retry_backoff,
            cache: if self.cache {
                Some(Arc::new(Mutex::new(HashMap::new())))
            } else {
                None
            },
            cache_ttl: self.cache_ttl,
            rate_limit: self.rate_limit,
            rate_window: Arc::new(Mutex::new(VecDeque::new())),
            budget: self.budget,
            used: Arc::new(AtomicU64::new(0)),
        }
    }
}

/// Convenience: a one-shot chat seeded with `messages`.
impl Ai {
    pub fn with_messages(&self, messages: Vec<Message>) -> Chat<'_> {
        let mut chat = Chat::new(self);
        for m in messages {
            chat = chat.message(m);
        }
        chat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_status_classification() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(529));
        assert!(!is_retryable_status(200));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(401));
    }

    #[test]
    fn cache_roundtrip_when_enabled() {
        let ai = Ai::builder().cache(true).build();
        assert!(ai.cache_enabled());
        assert_eq!(ai.cache_get(42), None);
        ai.cache_put(42, "hello".into());
        assert_eq!(ai.cache_get(42), Some("hello".to_string()));
        ai.clear_cache();
        assert_eq!(ai.cache_get(42), None);
    }

    #[test]
    fn cache_disabled_by_default() {
        let ai = Ai::builder().build();
        assert!(!ai.cache_enabled());
        ai.cache_put(1, "x".into());
        assert_eq!(ai.cache_get(1), None);
    }
}
