//! Entity identity and liveness (§2.3.1–2.3.2).
//!
//! An entity carries two identifiers with distinct lifetimes. [`EntityId`] is
//! the ephemeral, tenant-scoped packed handle minted by the [`EntityArena`];
//! [`EntityKey`] is the durable identity that may cross the disk, wire, or IPC
//! boundary. The arena is the liveness authority: it mints handles and rejects
//! stale or cross-tenant ones, so a handle is validated here before any
//! component table is indexed.

mod arena;
mod id;
mod key;

pub use arena::{ArenaError, EntityArena};
pub use id::{EntityId, EntityIdError, Generation, SlotIndex, TenantTag};
pub use key::EntityKey;
