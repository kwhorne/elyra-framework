/// A supported AI provider (the "lab").
///
/// Anthropic has its own driver; every other provider speaks OpenAI's Chat
/// Completions shape and runs on the OpenAI driver (RFC 0048): text, tools,
/// structured output and streaming. Images, speech, transcription and
/// embeddings are OpenAI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Provider {
    Anthropic,
    OpenAI,
    /// Google's Gemini, through its OpenAI-compatible endpoint.
    Gemini,
    Groq,
    Mistral,
    DeepSeek,
    /// xAI (Grok).
    XAi,
    OpenRouter,
    /// A local Ollama; no key.
    Ollama,
    /// Any other endpoint of OpenAI's Chat Completions shape: set its base URL
    /// (up to, not including, `/chat/completions`); a key is optional.
    Compatible,
}

impl Provider {
    /// Every provider, in declaration order.
    pub const ALL: [Provider; 10] = [
        Provider::Anthropic,
        Provider::OpenAI,
        Provider::Gemini,
        Provider::Groq,
        Provider::Mistral,
        Provider::DeepSeek,
        Provider::XAi,
        Provider::OpenRouter,
        Provider::Ollama,
        Provider::Compatible,
    ];

    /// Human/config name.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAI => "openai",
            Provider::Gemini => "gemini",
            Provider::Groq => "groq",
            Provider::Mistral => "mistral",
            Provider::DeepSeek => "deepseek",
            Provider::XAi => "xai",
            Provider::OpenRouter => "openrouter",
            Provider::Ollama => "ollama",
            Provider::Compatible => "compatible",
        }
    }

    /// The environment variable holding this provider's API key.
    pub fn env_key(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Gemini => "GEMINI_API_KEY",
            Provider::Groq => "GROQ_API_KEY",
            Provider::Mistral => "MISTRAL_API_KEY",
            Provider::DeepSeek => "DEEPSEEK_API_KEY",
            Provider::XAi => "XAI_API_KEY",
            Provider::OpenRouter => "OPENROUTER_API_KEY",
            Provider::Ollama => "OLLAMA_API_KEY",
            Provider::Compatible => "ELYRA_AI_COMPATIBLE_KEY",
        }
    }

    /// Optional base-URL override env var (for proxies/gateways).
    pub fn env_base_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_BASE_URL",
            Provider::OpenAI => "OPENAI_BASE_URL",
            Provider::Gemini => "GEMINI_BASE_URL",
            Provider::Groq => "GROQ_BASE_URL",
            Provider::Mistral => "MISTRAL_BASE_URL",
            Provider::DeepSeek => "DEEPSEEK_BASE_URL",
            Provider::XAi => "XAI_BASE_URL",
            Provider::OpenRouter => "OPENROUTER_BASE_URL",
            Provider::Ollama => "OLLAMA_BASE_URL",
            Provider::Compatible => "ELYRA_AI_COMPATIBLE_BASE_URL",
        }
    }

    /// Whether a request needs a key: `false` for a local Ollama and for a
    /// compatible endpoint, which send one only when it's set.
    pub fn needs_key(self) -> bool {
        !matches!(self, Provider::Ollama | Provider::Compatible)
    }

    /// The base URL without an override. Anthropic's and OpenAI's are the
    /// host (the driver appends `/v1/…`); the others include their version,
    /// with `/chat/completions` after it. `None` for `Compatible`.
    pub(crate) fn default_base_url(self) -> Option<&'static str> {
        Some(match self {
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::OpenAI => "https://api.openai.com",
            Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            Provider::Groq => "https://api.groq.com/openai/v1",
            Provider::Mistral => "https://api.mistral.ai/v1",
            Provider::DeepSeek => "https://api.deepseek.com/v1",
            Provider::XAi => "https://api.x.ai/v1",
            Provider::OpenRouter => "https://openrouter.ai/api/v1",
            Provider::Ollama => "http://localhost:11434/v1",
            Provider::Compatible => return None,
        })
    }

    /// The path after the base URL for a chat request.
    pub(crate) fn chat_path(self) -> &'static str {
        match self {
            Provider::Anthropic => "/v1/messages",
            Provider::OpenAI => "/v1/chat/completions",
            _ => "/chat/completions",
        }
    }

    /// The default text model: Anthropic's and OpenAI's. The other providers
    /// have none, since their model names change faster than a release; set
    /// one with `.model(…)` or `Ai::builder().text_model_for(…)`.
    pub fn default_text_model(self) -> Option<&'static str> {
        match self {
            Provider::Anthropic => Some("claude-sonnet-5-5"),
            Provider::OpenAI => Some("gpt-4o"),
            _ => None,
        }
    }
}
