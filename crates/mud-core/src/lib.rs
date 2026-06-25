//! Core domain primitives for the Ferrodun engine.

mod arena;
mod entity_id;
mod entity_key;
mod place;

pub use arena::{ArenaError, EntityArena};
pub use entity_id::{EntityId, EntityIdError, Generation, SlotIndex, TenantTag};
pub use entity_key::EntityKey;
pub use place::{Description, Direction, Place, PlaceId, RegionId, RoomData};
