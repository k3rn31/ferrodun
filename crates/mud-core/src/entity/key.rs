//! `EntityKey`, the durable entity identity (§2.3.1.5).
//!
//! An entity has two identifiers with distinct lifetimes. `EntityId`
//! (§2.3.1.1, see [`id`](super::id)) is the ephemeral packed arena handle, valid
//! only within one arena instance. `EntityKey` is the durable identity and
//! database primary key: it is the only entity reference that may cross the
//! disk, wire, or IPC boundary (§2.3.1.4).
//!
//! An `EntityKey`:
//! - is unique within a tenant and is never reused for the lifetime of the
//!   database, even after the entity is destroyed;
//! - is stable across cache eviction (§2.5.3.2), World restart, and engine
//!   upgrade;
//! - is a per-tenant monotonic 64-bit value. Tenant scoping comes from the
//!   per-tenant database (§2.5.1.4) and the routing layer, not from bits in the
//!   key.
//!
//! Per-tenant monotonic minting and the `EntityKey`↔`EntityId` mapping are out
//! of scope here; this type carries only the identity.

use std::fmt;
use std::num::NonZeroU64;

/// The durable, per-tenant identity and database primary key of an entity
/// (§2.3.1.5). Distinct from the ephemeral [`EntityId`](crate::EntityId) so the
/// two identities cannot be confused at compile time.
///
/// Backed by `NonZeroU64`: keys are 1-based, so an unassigned reference is
/// representable only as `Option::None` (which gets the niche for free), never
/// as a meaningless key `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct EntityKey(NonZeroU64);

impl EntityKey {
    /// Wraps a monotonic key value.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the underlying monotonic value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl fmt::Display for EntityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: u64) -> EntityKey {
        EntityKey::new(NonZeroU64::new(value).expect("test key must be non-zero"))
    }

    #[test]
    fn entity_key_is_eight_bytes() {
        assert_eq!(size_of::<EntityKey>(), 8);
    }

    // The non-zero niche encodes "unassigned" as `None` for free: an optional
    // key stays the same width as a key, with no sentinel value to reserve.
    #[test]
    fn option_entity_key_is_niche_optimized() {
        assert_eq!(size_of::<Option<EntityKey>>(), 8);
    }

    #[test]
    fn round_trips_through_new_and_get() {
        let value = NonZeroU64::new(42).expect("non-zero literal");
        assert_eq!(EntityKey::new(value).get(), value);
    }

    // Monotonic keys order by creation: a lower-numbered key sorts first.
    #[test]
    fn orders_by_monotonic_value() {
        assert!(key(1) < key(2));
    }
}
