//! Framework error type.

/// Errors raised while decoding, dispatching, or encoding a command.
#[derive(thiserror::Error)]
pub enum Error {
    #[error("failed to decode command arguments: {0}")]
    Decode(String),

    #[error("failed to encode command result: {0}")]
    Encode(String),

    #[error("unknown command: {0}")]
    UnknownCommand(String),

    /// A command's own error, surfaced to the caller **verbatim**.
    ///
    /// No prefix: the message is the contract. A `"command failed: "` prefix used
    /// to be prepended here, which silently broke structured errors — a
    /// `ValidationErrors` bag arrived as `command failed: {"email":[…]}` and could
    /// not be parsed as JSON by the frontend.
    #[error("{0}")]
    Command(String),

    /// A command's error with a kind the frontend can tell apart, carried as
    /// `x-elyra-error-kind`: `offline` (no answer from a server),
    /// `unauthenticated`, `denied` (a server's policy said no), `not-found`,
    /// `too-many-requests`, `server` — see [`Error::with_kind`]. The message is verbatim, as for
    /// [`Error::Command`].
    #[error("{message}")]
    Kind { kind: &'static str, message: String },

    #[error("codegen failed: {0}")]
    Codegen(String),

    #[error("io error: {0}")]
    Io(String),

    /// The app's configuration has problems (RFC 0025): all of them, from
    /// [`App::problems`](crate::App::problems). Returned by
    /// [`App::run`](crate::App::run) before anything starts.
    #[error("{0}")]
    Config(crate::app::Problems),
}

/// As derived, except [`Error::Config`]: `fn main() -> elyra::Result<()>`
/// prints `Debug`, and the configuration's problems read as their list.
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Decode(s) => f.debug_tuple("Decode").field(s).finish(),
            Error::Encode(s) => f.debug_tuple("Encode").field(s).finish(),
            Error::UnknownCommand(s) => f.debug_tuple("UnknownCommand").field(s).finish(),
            Error::Command(s) => f.debug_tuple("Command").field(s).finish(),
            Error::Kind { kind, message } => f
                .debug_struct("Kind")
                .field("kind", kind)
                .field("message", message)
                .finish(),
            Error::Codegen(s) => f.debug_tuple("Codegen").field(s).finish(),
            Error::Io(s) => f.debug_tuple("Io").field(s).finish(),
            Error::Config(problems) => std::fmt::Debug::fmt(problems, f),
        }
    }
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

    /// A command error of `kind`, which the frontend reads as
    /// `CommandError.kind`: `Error::with_kind("offline", "No connection")`.
    pub fn with_kind(kind: &'static str, message: impl std::fmt::Display) -> Self {
        Error::Kind {
            kind,
            message: message.to_string(),
        }
    }

    /// The kind the frontend sees, when it's more than a plain command error.
    pub fn kind(&self) -> Option<&'static str> {
        match self {
            Error::Kind { kind, .. } => Some(kind),
            _ => None,
        }
    }
}

/// `?` on a validation result inside an `elyra::Result` command. The bag stays
/// JSON (its `Display`), so the frontend still gets a `ValidationError` with
/// per-field messages.
impl From<crate::validation::ValidationErrors> for Error {
    fn from(errors: crate::validation::ValidationErrors) -> Self {
        Error::command(errors)
    }
}

/// `?` on a query inside an `elyra::Result` command.
#[cfg(feature = "database")]
impl From<elyra_db::Error> for Error {
    fn from(e: elyra_db::Error) -> Self {
        Error::command(e)
    }
}

/// Convenience alias used throughout the framework and generated code.
pub type Result<T, E = Error> = std::result::Result<T, E>;
