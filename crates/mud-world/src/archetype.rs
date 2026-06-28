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
