//! The minimal player puppet archetype (§2.3.5, M1 subset).
//!
//! M1 has no component-default or hook system yet (that is M2). The player
//! archetype therefore carries only what a freshly created puppet needs: the
//! room it starts in.

use mud_core::PlaceId;

/// The shape of a player puppet: for M1, just its starting room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub struct PlayerArchetype {
    start_room: PlaceId,
}

impl PlayerArchetype {
    /// Creates a player archetype that spawns puppets in `start_room`.
    pub fn new(start_room: PlaceId) -> Self {
        Self { start_room }
    }

    /// The room a new puppet starts in.
    pub fn start_room(&self) -> PlaceId {
        self.start_room
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;

    use super::*;

    fn place_id(value: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(value).expect("non-zero test id"))
    }

    #[test]
    fn start_room_returns_the_room_it_was_built_with() {
        let archetype = PlayerArchetype::new(place_id(42));
        assert_eq!(archetype.start_room(), place_id(42));
    }

    #[test]
    fn archetypes_with_the_same_start_room_are_equal() {
        assert_eq!(
            PlayerArchetype::new(place_id(1)),
            PlayerArchetype::new(place_id(1))
        );
        assert_ne!(
            PlayerArchetype::new(place_id(1)),
            PlayerArchetype::new(place_id(2))
        );
    }
}
