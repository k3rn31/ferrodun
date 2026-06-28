//! The translation between a room's durable [`PlaceKey`] slug and its ephemeral
//! in-process [`PlaceId`] handle.
//!
//! The database persists a location by its durable slug ([`PlaceKey`]); the
//! in-memory [`World`](mud_core::World) addresses places by the ephemeral
//! [`PlaceId`] minted at world load. [`PersistentWorld`](super::PersistentWorld)
//! bridges the two with a `PlaceMap` supplied at construction — built by the
//! world loader, which is the single authority on both identities. The map holds
//! only `mud-core` types, so the persistence layer never depends on the world
//! loader.

use std::collections::HashMap;

use mud_core::{PlaceId, PlaceKey};

/// A bidirectional map between durable [`PlaceKey`] slugs and ephemeral
/// [`PlaceId`] handles for one loaded world.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct PlaceMap {
    by_id: HashMap<PlaceId, PlaceKey>,
    by_key: HashMap<PlaceKey, PlaceId>,
}

impl PlaceMap {
    /// Builds a map from `(PlaceId, PlaceKey)` pairs.
    ///
    /// A `PlaceId` and a `PlaceKey` each identify the same room, so the pairs are
    /// expected to be one-to-one; a repeated id or slug keeps the last pair seen.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (PlaceId, PlaceKey)>) -> Self {
        let mut by_id = HashMap::new();
        let mut by_key = HashMap::new();
        for (id, key) in pairs {
            by_id.insert(id, key.clone());
            by_key.insert(key, id);
        }
        Self { by_id, by_key }
    }

    /// The durable slug of an ephemeral handle, if the handle names a known room.
    #[must_use]
    pub(super) fn key_of(&self, id: PlaceId) -> Option<&PlaceKey> {
        self.by_id.get(&id)
    }

    /// The ephemeral handle of a durable slug, if the slug names a known room.
    #[must_use]
    pub(super) fn id_of(&self, key: &PlaceKey) -> Option<PlaceId> {
        self.by_key.get(key).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU64;

    fn place(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("test place id must be non-zero"))
    }

    fn key(slug: &str) -> PlaceKey {
        PlaceKey::parse(slug).expect("test slug must be valid")
    }

    #[test]
    fn translates_both_directions() {
        let map = PlaceMap::from_pairs([(place(1), key("hall")), (place(2), key("study"))]);

        assert_eq!(map.id_of(&key("study")), Some(place(2)));
        assert_eq!(map.key_of(place(1)), Some(&key("hall")));
    }

    #[test]
    fn misses_an_unknown_id_or_slug() {
        let map = PlaceMap::from_pairs([(place(1), key("hall"))]);

        assert_eq!(map.id_of(&key("nowhere")), None);
        assert_eq!(map.key_of(place(9)), None);
    }
}
