//! `mud-engine`'s [`Places`] seam over the loaded room registry (PLAN M1-22).

use mud_core::{Place, PlaceId};
use mud_engine::Places;
use mud_world::Rooms;

/// Adapts the tenant's loaded [`Rooms`] registry to the pipeline's [`Places`] seam.
pub struct WorldPlaces(Rooms);

impl WorldPlaces {
    /// Wraps a tenant's loaded room registry for the pipeline.
    pub fn new(rooms: Rooms) -> Self {
        Self(rooms)
    }
}

impl Places for WorldPlaces {
    fn get(&self, id: PlaceId) -> Option<&Place> {
        self.0.get(id)
    }
}
