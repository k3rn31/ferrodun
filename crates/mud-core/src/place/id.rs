//! Place identifiers (§2.2.2).
//!
//! A place has two identities with distinct lifetimes, mirroring entities
//! (§2.3.1.4). [`PlaceId`] is the ephemeral in-process handle defined here;
//! [`PlaceKey`](super::key::PlaceKey) is the durable authored slug. [`RegionId`]
//! names the region a place belongs to.

use std::num::NonZeroU64;

/// The **ephemeral** in-process handle of a [`Place`](super::Place) (§2.2.2).
///
/// `PlaceId` is to a place what [`EntityId`](crate::EntityId) is to an entity: a
/// dense handle minted fresh when the world is loaded into memory, valid only for
/// that process lifetime, used on the hot path. The **durable** identity authored
/// by builders and persisted for an entity's location is the
/// [`PlaceKey`](super::key::PlaceKey) slug; the two are never confused at compile
/// time.
///
/// Backed by `NonZeroU64`: ids are 1-based, so an absent neighbour or exit is
/// representable as `Option::None` (which takes the niche for free), never as a
/// meaningless id `0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct PlaceId(NonZeroU64);

impl PlaceId {
    /// Wraps a place identifier value.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the underlying identifier value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// The identifier of the region a [`Place`](super::Place) belongs to (§2.2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[must_use]
pub struct RegionId(NonZeroU64);

impl RegionId {
    /// Wraps a region identifier value.
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// Returns the underlying identifier value.
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The non-zero niche encodes "no exit" as None for free, keeping an optional
    // neighbour the same width as a PlaceId.
    #[test]
    fn option_place_id_is_niche_optimized() {
        assert_eq!(size_of::<Option<PlaceId>>(), 8);
    }

    #[test]
    fn ids_round_trip_through_new_and_get() {
        let value = NonZeroU64::new(7).expect("non-zero literal");
        assert_eq!(PlaceId::new(value).get(), value);
        assert_eq!(RegionId::new(value).get(), value);
    }
}
