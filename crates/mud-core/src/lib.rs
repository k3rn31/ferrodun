//! Core domain primitives for the Ferrodun engine.

mod arena;
mod entity_id;
mod entity_key;
mod place;
mod scheduler;
mod side_tables;
mod world;

pub use arena::{ArenaError, EntityArena};
pub use entity_id::{EntityId, EntityIdError, Generation, SlotIndex, TenantTag};
pub use entity_key::EntityKey;
pub use place::{Description, Direction, Place, PlaceId, RegionId, RoomData};
pub use scheduler::{
    Effect, MutationCommand, Precondition, Scheduler, TICK_HZ, TICK_PERIOD, TickEvent,
};
pub use side_tables::{Inventory, LocationOf};
pub use world::World;
