//! The session-registry read seam.
//!
//! [`Roster::session_of`] is the entity→session reverse map the pipeline uses to
//! fan a broadcast out to an audience's sessions; [`Roster::connected`] backs the
//! `who` command. The `SessionService`'s resolver implements it over the live
//! in-world bindings, so `mud-engine`'s command layer never touches the registry
//! storage directly (mirroring the `Places` / `SessionResolver` seams).

use mud_account::PuppetName;
use mud_core::EntityId;
use mud_schema::SessionId;

/// A connected in-world player, for the `who` listing.
#[derive(Debug, Clone)]
#[must_use]
pub struct Presence {
    /// The player's puppet display name.
    pub name: PuppetName,
}

/// Reads the in-world session registry without exposing its storage.
pub trait Roster {
    /// The session controlling `entity`, or `None` if no in-world session does
    /// (an NPC, or an entity nobody is puppeting).
    fn session_of(&self, entity: EntityId) -> Option<SessionId>;

    /// Every connected in-world player, in no guaranteed order.
    fn connected(&self) -> Vec<Presence>;
}
