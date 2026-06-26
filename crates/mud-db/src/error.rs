//! Persistence error type, shared across database backends.

/// Errors raised by the persistence layer.
///
/// Backend-agnostic: both the SQLite backend and the future PostgreSQL backend
/// surface failures through this single type.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// A query or connection failure from the underlying driver.
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),

    /// A schema migration failed to apply.
    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
}
