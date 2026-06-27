//! Persistence error type, shared across database backends.

/// Errors raised by the persistence layer.
///
/// Backend-agnostic: both the SQLite backend and the future PostgreSQL backend
/// surface failures through this single type.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// A query or connection failure from the underlying driver.
    ///
    /// The driver error is boxed so the `sqlx` dependency does not leak into the
    /// public API; this type stays backend-agnostic across SQLite and Postgres.
    #[error("database error: {0}")]
    Sqlx(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A schema migration failed to apply.
    #[error("migration error: {0}")]
    Migrate(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A persisted integer id read from the database was not a valid id —
    /// negative or zero where a positive `AUTOINCREMENT` key was expected.
    /// Defensive: signals DB corruption rather than a normal outcome.
    #[error("invalid persisted id: {0}")]
    InvalidId(i64),

    /// An in-memory id exceeded the signed range its column stores, so it could
    /// not be written back. Defensive: rowids never approach `i64::MAX`.
    #[error("entity key out of range for storage: {0}")]
    KeyOutOfRange(u64),

    /// A live arena handle had no `EntityKey` in the in-process map. Every minted
    /// handle is mapped, so this is an internal invariant violation surfaced
    /// rather than panicked on.
    #[error("internal map inconsistency: a live entity has no persisted key")]
    EntityNotMapped,

    /// Boot load could not resolve a persisted `EntityKey` to a live arena
    /// entity: either a `location`/`inventory` row referenced a key with no
    /// matching entity (foreign keys make this unreachable in a consistent
    /// database), or minting a handle for an `entities` row failed (arena
    /// exhaustion). Surfaced rather than panicked on so a corrupt file fails
    /// loudly.
    #[error("dangling entity reference during load: entity_key {0}")]
    DanglingReference(i64),

    /// A `MutationCommand` carried an `Effect` variant this backend cannot
    /// persist. Unreachable today (every variant is handled); the arm exists
    /// only because `mud_core::Effect` is `#[non_exhaustive]`, so a future
    /// variant surfaces here at runtime rather than as a compile error.
    #[error("unsupported effect variant")]
    UnsupportedEffect,
}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(err))
    }
}
