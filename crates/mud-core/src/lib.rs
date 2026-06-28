//! Core domain primitives for the Ferrodun engine.

mod entity;
mod locks;
mod place;
mod scheduler;
mod side_tables;
mod world;

pub use entity::{
    ArenaError, EntityArena, EntityId, EntityIdError, EntityKey, Generation, SlotIndex, TenantTag,
};
pub use locks::{
    AccessType, Lock, LockArg, LockContext, LockFn, ParseError, ParsedLock, ResolveError,
    ResolvedExpr, SyntaxExpr, parse, resolve,
};
pub use place::{
    Description, Direction, Place, PlaceId, PlaceKey, PlaceKeyError, RegionId, RoomData, Title,
};
pub use scheduler::{
    Effect, MutationCommand, Precondition, Scheduler, TICK_HZ, TICK_PERIOD, TickEvent,
};
pub use side_tables::{Inventory, LocationOf};
pub use world::World;
