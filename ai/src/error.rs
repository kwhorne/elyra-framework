use thiserror::Error;

/// Errors from the AI SDK.
#[derive(Error, Debug)]
pub enum Error {
    /// Transport/HTTP failure (connection, timeout, TLS).
    #[error("http error: {0}")]
    Http(String),
    /// The provider returned a non-success status with a message.
    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },
    /// No API key configured for the given provider.
    #[error("missing API key for {0} (set its env var)")]
    MissingKey(&'static str),
    /// A response body could not be decoded as expected.
    #[error("decode error: {0}")]
    Decode(String),
    /// The response contained no usable content.
    #[error("empty response from provider")]
    Empty,
    /// A tool invocation failed.
    #[error("tool `{0}` failed: {1}")]
    Tool(String, String),
    /// The agent exceeded its tool-use step budget.
    #[error("exceeded max steps ({0})")]
    MaxSteps(u32),
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Error::Http(e.to_string())
    }
}

/// AI SDK result type.
pub type Result<T> = std::result::Result<T, Error>;
