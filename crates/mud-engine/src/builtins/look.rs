//! `look` and the room-rendering helpers that show a place to a viewer (§3.2).

use mud_core::{Direction, EntityId, Place, PlaceId, RoleName, Span, StyledText, World};
use mud_i18n::{Locale, t};

use super::{display_name, system};
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};

/// `look`: render the caller's current room (§3.2).
pub(super) struct Look;

impl CommandHandler for Look {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        match ctx.places().get(ctx.location()) {
            Some(place) => {
                CommandReply::to_caller(render_room(place, ctx.world(), ctx.caller(), &locale))
            }
            None => CommandReply::to_caller(system(t!(locale, "look.void"))),
        }
    }
}

/// Renders a room as the caller sees it: title, description, exits, and the
/// other entities present (§3.2).
pub(super) fn render_room(
    place: &Place,
    world: &World,
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

    let others = occupant_names(world, place.id(), viewer);
    if !others.is_empty() {
        out.push(Span::role(
            format!(
                "\n{}",
                t!(locale, "look.also-here", names = others.join(", "))
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

/// The display names of the entities in `place` other than `viewer`, skipping
/// any without a keyword to show.
fn occupant_names(world: &World, place: PlaceId, viewer: EntityId) -> Vec<String> {
    world
        .occupants_of(place)
        .filter(|&entity| entity != viewer)
        .filter_map(|entity| display_name(world, entity))
        .collect()
}
