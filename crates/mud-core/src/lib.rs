//! Core domain primitives for the Ferrodun engine.

mod entity;
mod locks;
mod place;
mod region;
mod scheduler;
mod side_tables;
mod slug;
mod text;
mod world;

pub use entity::{
    ArenaError, EntityArena, EntityId, EntityIdError, EntityKey, Generation, SlotIndex, TenantTag,
};
pub use locks::{
    AccessType, Lock, LockArg, LockContext, LockFn, ParseError, ParsedLock, ResolveError,
    ResolvedExpr, SyntaxExpr, parse, resolve,
};
pub use place::{Description, Direction, Place, PlaceId, PlaceKey, PlaceKeyError, RoomData, Title};
pub use region::{RegionId, RegionKey, RegionKeyError};
pub use scheduler::{
    Effect, MutationCommand, Precondition, Scheduler, TICK_HZ, TICK_PERIOD, TickEvent,
};
pub use side_tables::{Inventory, LocationOf};
pub use text::{
    Attributes, Color, ColorParseError, CompiledMarkup, FieldStyle, MarkupDiagnostic, Palette,
    RoleName, Span, SpanStyle, Style, StyledText, compile_markup,
};
pub use world::World;
