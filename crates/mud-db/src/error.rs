//! Persistence error type, shared across database backends.

use mud_core::{ArenaError, EntityKey};

/// Errors raised by the persistence layer, backend-agnostic across the SQLite
/// backend and any future PostgreSQL one.
///
/// Two failure axes share this type. **Infrastructure faults** — the driver, a
/// migration, or an offloaded blocking task failed (`Sqlx`, `Migrate`,
/// `BlockingTask`) — are transient/operational. **Consistency violations** —
/// a persisted value is out of range, unparseable, dangling, or otherwise
/// impossible in a well-formed database (`InvalidId`, `CorruptValue`,
/// `KeyOutOfRange`, `EntityNotMapped`, `UnknownPlaceKey`, `PlaceNotMapped`,
/// `DanglingReference`, `LoadArenaExhausted`, `UnsupportedEffect`) — are
/// surfaced rather than panicked on, so a corrupt or newer-schema row fails
/// loudly instead of silently. Callers propagate both; no caller branches on
/// the axis today, so the two live in one enum.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DbError {
    // --- Infrastructure faults ---
    /// A query or connection failure from the underlying driver.
    ///
    /// The driver error is boxed so the `sqlx` dependency does not leak into the
    /// public API; this type stays backend-agnostic across SQLite and Postgres.
    #[error("database error: {0}")]
    Sqlx(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A schema migration failed to apply.
    #[error("migration error: {0}")]
    Migrate(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A blocking task offloaded with `spawn_blocking` (e.g. argon2 password
    /// verification, kept off the async runtime) failed to complete — it
    /// panicked or was cancelled. An internal fault, surfaced rather than
    /// unwrapped. The `tokio` join error is boxed so it does not leak into the
    /// public API.
    #[error("background task failed: {0}")]
    BlockingTask(#[source] Box<dyn std::error::Error + Send + Sync>),

    // --- Consistency violations ---
    /// A persisted integer id read from the database was not a valid id —
    /// negative or zero where a positive `AUTOINCREMENT` key was expected.
    /// Defensive: signals DB corruption rather than a normal outcome.
    #[error("invalid persisted id: {0}")]
    InvalidId(i64),

    /// A persisted text value failed domain validation on load — a manual edit,
    /// corruption, or a row written by a newer schema (e.g. an unknown account
    /// `state` token or a puppet name outside the allowed alphabet). Surfaced
    /// rather than coerced, so a bad row fails loudly instead of silently.
    #[error("corrupt persisted value: {0}")]
    CorruptValue(String),

    /// An in-memory id exceeded the signed range its column stores, so it could
    /// not be written back. Defensive: rowids never approach `i64::MAX`. Shared
    /// by every typed id that narrows to an `i64` column (entity key, account id).
    #[error("id out of range for storage: {0}")]
    KeyOutOfRange(u64),

    /// A live arena handle had no `EntityKey` in the in-process map. Every minted
    /// handle is mapped, so this is an internal invariant violation surfaced
    /// rather than panicked on.
    #[error("internal map inconsistency: a live entity has no persisted key")]
    EntityNotMapped,

    /// Boot load found a `location` row whose persisted slug names no room in the
    /// loaded world — the room was removed since the location was written. Content
    /// drift, surfaced rather than panicked on so a stale reference fails loudly.
    #[error("location references unknown room slug: {0}")]
    UnknownPlaceKey(String),

    /// A `MoveTo` targeted a `PlaceId` absent from the world's place map. Every
    /// in-memory handle that reaches persistence came from a loaded room, so this
    /// is an internal invariant violation surfaced rather than panicked on.
    #[error("internal map inconsistency: a place handle has no durable slug")]
    PlaceNotMapped,

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

impl DbError {
    /// Wraps a driver failure. Crate-internal on purpose: the only sanctioned
    /// path from a `sqlx` error into [`DbError`], so `sqlx` never appears in
    /// the public API (no public `From` impl).
    pub(crate) fn from_sqlx(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }

    /// Wraps a migration failure. Crate-internal for the same reason as
    /// [`DbError::from_sqlx`].
    pub(crate) fn from_migrate(err: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(Box::new(err))
    }

    /// Wraps a failed `spawn_blocking` join. Crate-internal for the same
    /// reason as [`DbError::from_sqlx`].
    pub(crate) fn from_join(err: tokio::task::JoinError) -> Self {
        Self::BlockingTask(Box::new(err))
    }
}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        Self::Sqlx(Box::new(err))
    }
}
