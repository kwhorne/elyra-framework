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

    /// The batch being rolled back holds a migration that's neither a file in
    /// the migrations directory nor a registered Rust migration, so its `down`
    /// can't be run. Nothing was rolled back.
    #[error(
        "migration `{0}` in the last batch is neither a `.sql` file nor a registered Rust \
         migration, so it can't be rolled back; nothing was"
    )]
    UnknownMigration(String),
}

pub type Result<T> = std::result::Result<T, Error>;
