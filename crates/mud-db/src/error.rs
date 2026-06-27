//! Persistence error type, shared across database backends.

use mud_core::{ArenaError, EntityKey};

/// Errors raised by the persistence layer.
///
/// Backend-agnostic: both the SQLite backend and the future PostgreSQL backend
/// surface failures through this single type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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

    /// Boot load found a `location`/`inventory` row referencing an `EntityKey`
    /// with no matching entity. Foreign keys make this unreachable in a
    /// consistent database, so it signals corruption; surfaced rather than
    /// panicked on so a corrupt file fails loudly.
    #[error("dangling entity reference during load: entity_key {0}")]
    DanglingReference(i64),

    /// Boot load could not mint an arena handle for a persisted `entities` row
    /// because the tenant's arena is exhausted. Surfaced rather than panicked on
    /// so the failure is recoverable by the caller.
    #[error("arena exhausted while loading entity_key {entity_key}")]
    LoadArenaExhausted {
        /// The persisted key whose handle could not be minted.
        entity_key: EntityKey,
        /// The arena failure that prevented minting.
        #[source]
        source: ArenaError,
    },

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
