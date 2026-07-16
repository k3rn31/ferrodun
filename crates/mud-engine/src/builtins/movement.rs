//! The six movement commands and the canonical direction names they render
//! (§3.2.2, §3.14.5.1).

use mud_core::{Direction, Effect, RoleName, StyledText};
use mud_i18n::t;

use super::look::render_room;
use super::system;
use crate::dispatch::{Broadcast, CommandContext, CommandHandler, CommandReply};

/// One of the six movement commands, carrying the direction it travels (§3.2.2).
pub(super) struct Move(pub(super) Direction);

impl CommandHandler for Move {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let destination = ctx
            .places()
            .get(ctx.location())
            .and_then(|place| place.neighbor(self.0));
        let Some(to) = destination else {
            return CommandReply::to_caller(system(t!(locale, "move.no-exit")));
        };
        // An exit wired to a place the registry can't resolve is no passage:
        // refuse rather than strand the caller in a place that can't be
        // rendered (and from which no exit resolves either). A wired exit to a
        // missing place is a world-data fault, not player error, so log it for
        // operators while the caller sees the ordinary "no way" refusal.
        let Some(place) = ctx.places().get(to) else {
            tracing::warn!(
                from = ?ctx.location(),
                to = ?to,
                direction = ?self.0,
                "exit wired to a place absent from the registry; refusing the move",
            );
            return CommandReply::to_caller(system(t!(locale, "move.no-exit")));
        };
        // Show the destination room as the caller arrives; the MoveTo effect is
        // applied by the pipeline after this handler returns.
        let arrival = render_room(place, ctx.world(), ctx.roster(), ctx.caller(), &locale);
        let name = ctx.caller_name().as_str().to_owned();
        // Both broadcasts are resolved against the pre-effect world: the room
        // left still has the caller present, and the destination room doesn't
        // yet — so the audiences match the departure/arrival semantics exactly.
        let depart = StyledText::new().role(
            t!(
                locale,
                "move.depart",
                name = name.clone(),
                direction = self.0.name()
            ),
            RoleName::SYSTEM,
        );
        let arrive = StyledText::new().role(
            t!(
                locale,
                "move.arrive-from",
                name = name,
                direction = self.0.opposite().name()
            ),
            RoleName::SYSTEM,
        );
        CommandReply::to_caller(arrival)
            .with_broadcast(Broadcast::to_place(ctx.location(), ctx.caller(), depart))
            .with_broadcast(Broadcast::to_place(to, ctx.caller(), arrive))
            .with_effect(Effect::MoveTo {
                entity: ctx.caller(),
                place: to,
            })
    }
}
