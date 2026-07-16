//! Room-presence announcements (§2.7 step 8).
//!
//! [`announce`] is the single audience-resolution implementation shared by
//! every room broadcast — `say`, movement, and (later) session-lifecycle
//! (spawn/quit/disconnect) fan-out all resolve their audience here, so there is
//! exactly one place that decides who hears a room message.

use mud_core::{EntityId, PlaceId, RoleName, StyledText, World};
use mud_i18n::{Locale, t};
use mud_schema::{OutputKind, OutputText, SessionOutput};

use crate::roster::Roster;

/// Fans `message` out to every session co-located with `place`, except `except`.
///
/// Resolves the audience as the place's occupants minus `except` (typically
/// the acting entity), skipping any occupant with no connected session (an
/// NPC, an item, or a puppet with nobody controlling it) — those have nowhere
/// to receive the message.
pub fn announce(
    world: &World,
    roster: &dyn Roster,
    place: PlaceId,
    except: EntityId,
    message: &StyledText,
) -> Vec<SessionOutput> {
    world
        .occupants_of(place)
        .filter(|&occupant| occupant != except)
        .filter_map(|occupant| roster.session_of(occupant))
        .map(|session_id| SessionOutput {
            session_id,
            text: OutputText::new(message.clone()),
            kind: OutputKind::Line,
        })
        .collect()
}

/// The room message announcing `name` entering the world (`presence.enter`).
pub fn entered(locale: Locale, name: &str) -> StyledText {
    StyledText::new().role(t!(locale, "presence.enter", name = name), RoleName::SYSTEM)
}

/// The room message announcing `name` leaving the world (`presence.leave`).
pub fn left(locale: Locale, name: &str) -> StyledText {
    StyledText::new().role(t!(locale, "presence.leave", name = name), RoleName::SYSTEM)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::num::NonZeroU64;

    use mud_core::TenantTag;
    use mud_schema::SessionId;

    use super::*;
    use crate::roster::Presence;

    /// A roster over a fixed entity→session map.
    struct FakeRoster(HashMap<EntityId, SessionId>);

    impl Roster for FakeRoster {
        fn session_of(&self, entity: EntityId) -> Option<SessionId> {
            self.0.get(&entity).copied()
        }
        fn connected(&self) -> Vec<Presence> {
            Vec::new()
        }
        fn name_of(&self, _entity: EntityId) -> Option<mud_account::PuppetName> {
            None
        }
    }

    fn sid(n: u64) -> SessionId {
        SessionId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    fn place(n: u64) -> PlaceId {
        PlaceId::new(NonZeroU64::new(n).expect("nonzero"))
    }

    #[test]
    fn announce_reaches_co_located_sessions_except_the_actor() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let actor = world.create().expect("actor");
        let witness = world.create().expect("witness");
        let prop = world.create().expect("prop"); // session-less: skipped
        for entity in [actor, witness, prop] {
            world.move_to(entity, place(10)).expect("seat entity");
        }
        let roster = FakeRoster(HashMap::from([(actor, sid(1)), (witness, sid(2))]));

        let message = StyledText::new().plain("Bob appears from nowhere.");
        let outputs = announce(&world, &roster, place(10), actor, &message);

        assert_eq!(outputs.len(), 1, "only the witness has a session");
        let output = outputs.first().expect("one output");
        assert_eq!(output.session_id, sid(2));
        assert_eq!(output.text.to_plain_string(), "Bob appears from nowhere.");
    }

    #[test]
    fn announce_excludes_the_actor_even_when_the_actor_has_a_session() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let actor = world.create().expect("actor");
        world.move_to(actor, place(10)).expect("seat actor");
        let roster = FakeRoster(HashMap::from([(actor, sid(1))]));

        let message = StyledText::new().plain("gone");
        let outputs = announce(&world, &roster, place(10), actor, &message);

        assert!(
            outputs.is_empty(),
            "the actor must not hear their own announcement"
        );
    }

    #[test]
    fn entered_renders_the_presence_enter_template() {
        assert_eq!(
            entered(Locale::EN, "Bob").to_plain_string(),
            "Bob appears from nowhere."
        );
    }

    #[test]
    fn left_renders_the_presence_leave_template() {
        assert_eq!(left(Locale::EN, "Bob").to_plain_string(), "Bob disappears.");
    }
}
