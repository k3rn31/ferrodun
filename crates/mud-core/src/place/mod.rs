//! The `Place` spatial surface (§2.2).
//!
//! Every spatial location in Ferrodun is a [`Place`]. Movement, perception,
//! line-of-sight, and NPC awareness all operate against the surface [`Place`]
//! exposes — `id`, `region`, `title`, `describe`, `neighbor`, `occupants`,
//! `visible_places` (§2.2.2) — with no special cases per variant (§2.2.3), and via
//! static dispatch (an enum, never a trait object, §2.2.5).
//!
//! A place carries two identities with distinct lifetimes (§2.2.6), mirroring
//! entities: [`PlaceId`] is the ephemeral in-process handle ([`id`]); [`PlaceKey`]
//! is the durable authored slug ([`key`]). The [`Place`] enum and its room content
//! live in [`room`]. The region a place belongs to ([`RegionId`](crate::RegionId))
//! lives in its own [`region`](crate::region) module, since a region is not a place.

mod id;
mod key;
mod room;

pub use id::PlaceId;
pub use key::{PlaceKey, PlaceKeyError};
pub use room::{Description, Direction, Place, RoomData, Title};
