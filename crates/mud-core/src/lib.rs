//! Core domain primitives for the Ferrodun engine.

mod arena;
mod entity_id;

pub use arena::{ArenaError, EntityArena};
pub use entity_id::{EntityId, EntityIdError, Generation, SlotIndex, TenantTag};
