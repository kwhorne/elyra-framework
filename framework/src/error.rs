//! Framework error type.

/// Errors raised while decoding, dispatching, or encoding a command.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to decode command arguments: {0}")]
    Decode(String),

    #[error("failed to encode command result: {0}")]
    Encode(String),

    #[error("unknown command: {0}")]
    UnknownCommand(String),

    #[error("command failed: {0}")]
    Command(String),

    #[error("codegen failed: {0}")]
    Codegen(String),

    #[error("io error: {0}")]
    Io(String),
}

impl Error {
    /// Wrap a decode failure (msgpack -> args tuple).
    pub fn decode(e: impl std::fmt::Display) -> Self {
        Error::Decode(e.to_string())
    }

    /// Wrap an encode failure (result -> msgpack).
    pub fn encode(e: impl std::fmt::Display) -> Self {
        Error::Encode(e.to_string())
    }

    /// Wrap a command's own error (the `Err` of a `Result`-returning command).
    pub fn command(e: impl std::fmt::Display) -> Self {
        Error::Command(e.to_string())
    }
}

/// Convenience alias used throughout the framework and generated code.
pub type Result<T, E = Error> = std::result::Result<T, E>;
