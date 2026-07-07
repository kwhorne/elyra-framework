//! Database errors.

/// Errors from connecting, querying, or migrating.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unknown database driver for url (expected sqlite:, mysql:, or postgres:): {0}")]
    UnknownDriver(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error("io error: {0}")]
    Io(String),

    #[error("invalid migration filename `{0}` (expected `<version>_<name>.sql`)")]
    InvalidMigration(String),

    #[error("query error: {0}")]
    Query(String),
}

pub type Result<T> = std::result::Result<T, Error>;
