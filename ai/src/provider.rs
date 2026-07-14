/// A supported AI provider (the "lab").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    Anthropic,
    OpenAI,
}

impl Provider {
    /// Human/config name.
    pub fn as_str(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic",
            Provider::OpenAI => "openai",
        }
    }

    /// The environment variable holding this provider's API key.
    pub fn env_key(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
        }
    }

    /// Optional base-URL override env var (for proxies/gateways).
    pub fn env_base_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "ANTHROPIC_BASE_URL",
            Provider::OpenAI => "OPENAI_BASE_URL",
        }
    }

    pub(crate) fn default_base_url(self) -> &'static str {
        match self {
            Provider::Anthropic => "https://api.anthropic.com",
            Provider::OpenAI => "https://api.openai.com",
        }
    }

    /// The default text model for this provider.
    pub fn default_text_model(self) -> &'static str {
        match self {
            Provider::Anthropic => "claude-sonnet-5",
            Provider::OpenAI => "gpt-4o",
        }
    }
}
