//! `look` and the room-rendering helpers that show a place to a viewer (§3.2).

use mud_core::{Direction, EntityId, Place, PlaceId, RoleName, Span, StyledText, World};
use mud_i18n::{Locale, t};

use super::{display_name, system};
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};
use crate::roster::Roster;

/// `look`: render the caller's current room (§3.2).
pub(super) struct Look;

impl CommandHandler for Look {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        match ctx.places().get(ctx.location()) {
            Some(place) => CommandReply::to_caller(render_room(
                place,
                ctx.world(),
                ctx.roster(),
                ctx.caller(),
                &locale,
            )),
            None => CommandReply::to_caller(system(t!(locale, "look.void"))),
        }
    }
}

/// Renders a room as the caller sees it: title, description, exits, the
/// connected players present, and the other entities present (§3.2).
pub(super) fn render_room(
    place: &Place,
    world: &World,
    roster: &dyn Roster,
    viewer: EntityId,
    locale: &Locale,
) -> StyledText {
    let mut out = StyledText::new();
    if let Some(title) = place.title() {
        append(&mut out, title.styled());
        out.push(Span::plain("\n"));
    }
    append(&mut out, place.describe(viewer).styled());

    let exits = exit_names(place);
    if !exits.is_empty() {
        out.push(Span::role(
            format!("\n{}", t!(locale, "look.exits", exits = exits.join(", "))),
            RoleName::SYSTEM,
        ));
    }

    let (players, things) = occupants(world, roster, place.id(), viewer);
    if let Some(line) = players_line(&players, locale) {
        out.push(Span::role(format!("\n{line}"), RoleName::SYSTEM));
    }
    if !things.is_empty() {
        out.push(Span::role(
            format!(
                "\n{}",
                t!(locale, "look.also-here", names = things.join(", "))
            ),
            RoleName::SYSTEM,
        ));
    }
    out
}

/// Appends every span of `text` onto `out`.
fn append(out: &mut StyledText, text: &StyledText) {
    for span in text.spans() {
        out.push(span.clone());
    }
}

/// The names of the wired exits of `place`, in N/E/S/W/U/D order.
fn exit_names(place: &Place) -> Vec<&'static str> {
    Direction::ALL
        .into_iter()
        .filter(|&dir| place.neighbor(dir).is_some())
        .map(Direction::name)
        .collect()
}

/// Splits the occupants of `place` other than `viewer` into connected players
/// (roster names, sorted for a stable sentence) and keyword-named things,
/// skipping entities with neither name source.
fn occupants(
    world: &World,
    roster: &dyn Roster,
    place: PlaceId,
    viewer: EntityId,
) -> (Vec<String>, Vec<String>) {
    let mut players = Vec::new();
    let mut things = Vec::new();
    for entity in world.occupants_of(place).filter(|&entity| entity != viewer) {
        if let Some(name) = roster.name_of(entity) {
            players.push(name.as_str().to_owned());
        } else if let Some(name) = display_name(world, entity) {
            things.push(name);
        }
    }
    players.sort();
    (players, things)
}

/// One Diku-voice sentence for the players present, or `None` for an empty
/// list: singular `look.player-here`, plural `look.players-here` over an
/// English and-join (locale-aware list formatting is the M2-I rework's job).
fn players_line(players: &[String], locale: &Locale) -> Option<String> {
    match players {
        [] => None,
        [name] => Some(t!(locale, "look.player-here", name = name.clone())),
        many => Some(t!(locale, "look.players-here", names = and_join(many))),
    }
}

/// Joins names as `a`, `a and b`, or `a, b and c`.
fn and_join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [head @ .., last] => format!("{} and {}", head.join(", "), last),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mud_account::PuppetName;
    use mud_core::{Description, Keyword, RegionId, RoomData, TenantTag, Title};
    use mud_i18n::Locale;
    use std::collections::HashMap;
    use std::num::NonZeroU64;

    use crate::roster::{Presence, Roster};
    use mud_schema::SessionId;

    /// A roster naming a fixed set of player entities.
    struct FakeRoster(HashMap<EntityId, PuppetName>);

    impl Roster for FakeRoster {
        fn session_of(&self, entity: EntityId) -> Option<SessionId> {
            self.0
                .contains_key(&entity)
                .then(|| SessionId::new(NonZeroU64::new(1).expect("nonzero")))
        }
        fn connected(&self) -> Vec<Presence> {
            Vec::new()
        }
        fn name_of(&self, entity: EntityId) -> Option<PuppetName> {
            self.0.get(&entity).cloned()
        }
    }

    fn room(id: u64) -> Place {
        Place::Room(
            RoomData::new(
                PlaceId::new(NonZeroU64::new(id).expect("nonzero")),
                RegionId::new(NonZeroU64::new(1).expect("nonzero")),
                Description::new("A taproom."),
            )
            .with_title(Title::new("The Tavern")),
        )
    }

    fn puppet_name(name: &str) -> PuppetName {
        PuppetName::parse(name).expect("valid puppet name")
    }

    /// Seats `count` entities in the room, returning them in creation order.
    fn seated(world: &mut World, place: PlaceId, count: usize) -> Vec<EntityId> {
        (0..count)
            .map(|_| {
                let entity = world.create().expect("create entity");
                world.move_to(entity, place).expect("seat entity");
                entity
            })
            .collect()
    }

    /// The nth seated entity (clippy denies slice indexing workspace-wide).
    fn nth(entities: &[EntityId], index: usize) -> EntityId {
        *entities.get(index).expect("seated entity")
    }

    #[test]
    fn one_player_renders_the_singular_sentence() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 2);
        let (viewer, alice) = (nth(&entities, 0), nth(&entities, 1));
        let roster = FakeRoster(HashMap::from([(alice, puppet_name("Alice"))]));

        let text = render_room(&place, &world, &roster, viewer, &Locale::EN).to_plain_string();
        assert!(text.contains("Alice is here."), "got: {text}");
        assert!(!text.contains("are here"), "got: {text}");
    }

    #[test]
    fn many_players_collapse_into_one_and_joined_sentence() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 4);
        let roster = FakeRoster(HashMap::from([
            (nth(&entities, 1), puppet_name("Carol")),
            (nth(&entities, 2), puppet_name("Alice")),
            (nth(&entities, 3), puppet_name("Bob")),
        ]));

        let text =
            render_room(&place, &world, &roster, nth(&entities, 0), &Locale::EN).to_plain_string();
        // Sorted, comma-joined with a final "and" (design: Diku voice, one line).
        assert!(
            text.contains("Alice, Bob and Carol are here."),
            "got: {text}"
        );
    }

    #[test]
    fn objects_stay_in_the_also_here_list_and_players_leave_it() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 3);
        let (viewer, alice, sword) = (nth(&entities, 0), nth(&entities, 1), nth(&entities, 2));
        world
            .name_entity(sword, vec![Keyword::new("sword")])
            .expect("name the sword");
        let roster = FakeRoster(HashMap::from([(alice, puppet_name("Alice"))]));

        let text = render_room(&place, &world, &roster, viewer, &Locale::EN).to_plain_string();
        assert!(text.contains("Alice is here."), "got: {text}");
        assert!(text.contains("Also here: sword"), "got: {text}");
        assert!(!text.contains("Also here: Alice"), "got: {text}");
    }

    #[test]
    fn a_nameless_session_less_occupant_is_still_skipped() {
        let mut world = World::new(TenantTag::new(1).expect("tenant"));
        let place = room(10);
        let entities = seated(&mut world, place.id(), 2);
        let roster = FakeRoster(HashMap::new());

        let text =
            render_room(&place, &world, &roster, nth(&entities, 0), &Locale::EN).to_plain_string();
        assert!(!text.contains("here"), "got: {text}");
    }
}
