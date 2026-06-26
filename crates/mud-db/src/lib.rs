//! Persistence backend for the Ferrodun engine.
//!
//! Each tenant owns a physically distinct database; there is no shared database
//! and no tenant column, so cross-tenant queries are impossible by construction
//! (SPEC §2.5.1.4). The layer is namespaced by backend: [`sqlite`] is the
//! development/embedded backend, and a PostgreSQL backend will join it as a
//! sibling module when production is exercised.

mod error;
mod sqlite;

pub use error::DbError;
pub use sqlite::{PersistentWorld, TenantDb};
