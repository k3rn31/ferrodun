//! The place-lookup seam: resolving a [`PlaceId`] to its [`Place`] (§2.2).

use mud_core::{Place, PlaceId};

/// Read access to the tenant's places by id (§2.2).
///
/// `look` and movement need a [`Place`]'s title, description, and exits, but
/// [`mud_core::World`] stores only entity location/inventory — not the authored
/// room registry. That registry is `mud_world::Rooms`; this trait is the seam
/// the pipeline depends on so `mud-engine` need not depend on `mud-world`.
/// `Rooms` implements it at wiring time (M1-22); tests fake it.
pub trait Places {
    /// The [`Place`] with `id`, or `None` if no such place exists.
    fn get(&self, id: PlaceId) -> Option<&Place>;
}
