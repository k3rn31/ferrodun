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
mod write_model;

pub use entity::{
    ArenaError, EntityArena, EntityId, EntityIdError, EntityKey, Generation, SlotIndex, TenantTag,
};
pub use locks::{
    AccessType, Lock, LockArg, LockContext, LockFn, ParseError, ParsedLock, ResolveError,
    ResolvedExpr, SyntaxExpr, parse, resolve,
};
pub use place::{
    Description, Direction, ParseDirectionError, Place, PlaceId, PlaceKey, PlaceKeyError, RoomData,
    Title,
};
pub use region::{RegionId, RegionKey, RegionKeyError};
pub use scheduler::{Scheduler, TICK_HZ, TICK_PERIOD};
pub use side_tables::{Inventory, Keyword, LocationOf, Naming};
pub use text::{
    Attributes, Color, ColorParseError, CompiledMarkup, FieldStyle, MarkupDiagnostic, Palette,
    RoleName, Span, SpanStyle, Style, StyledText, compile_markup,
};
pub use world::World;
pub use write_model::{Effect, MutationCommand, Precondition, TickEvent};
