//! Dense hot-component side-tables (§2.3.2.2).
//!
//! Hot components are touched every tick / combat round, so §2.3.2.2 keeps them
//! in dense, slot-indexed arrays rather than the dynamic component bag. This
//! module holds the tables in use: [`LocationOf`] (which [`Place`] each
//! entity occupies, plus a reverse occupant index), [`Inventory`] (which
//! entities a container holds), and [`Naming`] (the keywords each entity answers
//! to, for command-argument matching). The other hot components §2.3.2.2 lists —
//! `Position`, `Health`, `Initiative` — are not represented here.
//!
//! These tables are **pure storage keyed by [`SlotIndex`]** (the slot half of an
//! [`EntityId`]); they are deliberately *not* the liveness authority. The arena
//! ([`crate::EntityArena`]) owns liveness: a caller resolves a handle through
//! [`EntityArena::resolve`](crate::EntityArena::resolve) — which rejects stale
//! and cross-tenant handles — and only then indexes a side-table. Keeping the
//! tables ignorant of liveness is the §2.3.2 separation, not missing validation.
//!
//! [`Place`]: crate::Place
//! [`SlotIndex`]: crate::SlotIndex
//! [`EntityId`]: crate::EntityId

mod inventory;
mod location;
mod naming;

pub use inventory::Inventory;
pub use location::LocationOf;
pub use naming::{Keyword, Naming};
