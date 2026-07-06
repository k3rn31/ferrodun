//! Item commands: `inventory`, `get`, `drop`, and their effect/reply helpers
//! (§2.7 step 5, step 7).

use mud_core::{Effect, EntityId, PlaceId, RoleName, StyledText, World};
use mud_i18n::{Locale, t};

use super::{display_name, system};
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};
use crate::objects::{Resolution, resolve_among};

/// `inventory`: list what the caller is carrying.
pub(super) struct ShowInventory;

impl CommandHandler for ShowInventory {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let items: Vec<String> = ctx
            .world()
            .inventory_of(ctx.caller())
            .filter_map(|item| display_name(ctx.world(), item))
            .collect();

        if items.is_empty() {
            return CommandReply::to_caller(system(t!(locale, "inventory.empty")));
        }
        let mut out = StyledText::new().role(t!(locale, "inventory.header"), RoleName::SYSTEM);
        for item in items {
            out = out.plain(format!("\n  {item}"));
        }
        CommandReply::to_caller(out)
    }
}

/// `get`: take an item off the floor into the caller's inventory (§2.7 step 5).
pub(super) struct Get;

impl CommandHandler for Get {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let caller = ctx.caller();
        // Scope: items on the floor here, not the caller and not what they carry.
        let floor: Vec<EntityId> = ctx
            .world()
            .occupants_of(ctx.location())
            .filter(|&entity| entity != caller)
            .collect();

        match resolve_among(ctx.world(), &floor, ctx.args()) {
            Resolution::NoMatch => CommandReply::to_caller(system(t!(locale, "object.not-here"))),
            Resolution::Ambiguous(options) => CommandReply::to_caller(system(t!(
                locale,
                "object.ambiguous",
                options = numbered(ctx.world(), &options)
            ))),
            Resolution::One(item) => take_all(ctx.world(), caller, &[item], &locale),
            Resolution::All(items) => take_all(ctx.world(), caller, &items, &locale),
        }
    }
}

/// `drop`: put a carried item down in the caller's current room (§2.7 step 5).
pub(super) struct Drop;

impl CommandHandler for Drop {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let caller = ctx.caller();
        let location = ctx.location();
        let carried: Vec<EntityId> = ctx.world().inventory_of(caller).collect();

        match resolve_among(ctx.world(), &carried, ctx.args()) {
            Resolution::NoMatch => {
                CommandReply::to_caller(system(t!(locale, "object.not-carried")))
            }
            Resolution::Ambiguous(options) => CommandReply::to_caller(system(t!(
                locale,
                "object.ambiguous",
                options = numbered(ctx.world(), &options)
            ))),
            Resolution::One(item) => drop_all(ctx.world(), caller, location, &[item], &locale),
            Resolution::All(items) => drop_all(ctx.world(), caller, location, &items, &locale),
        }
    }
}

/// Takes the matched floor items into `caller`'s inventory: one reply line and
/// one effect pair (lift off the ground, then add) per item (§2.7 step 7). The
/// single-item `get` is just the one-element case.
fn take_all(world: &World, caller: EntityId, items: &[EntityId], locale: &Locale) -> CommandReply {
    let lines: Vec<String> = items
        .iter()
        .map(|&item| {
            t!(
                locale,
                "get.taken",
                item = display_name(world, item).unwrap_or_default()
            )
        })
        .collect();
    items.iter().fold(
        CommandReply::to_caller(system(lines.join("\n"))),
        |reply, &item| {
            reply
                .with_effect(Effect::ClearLocation { entity: item })
                .with_effect(Effect::InventoryAdd {
                    container: caller,
                    item,
                })
        },
    )
}

/// Drops the matched carried items into `location`: one reply line and one
/// effect pair (remove from inventory, then place) per item. The single-item
/// `drop` is just the one-element case.
fn drop_all(
    world: &World,
    caller: EntityId,
    location: PlaceId,
    items: &[EntityId],
    locale: &Locale,
) -> CommandReply {
    let lines: Vec<String> = items
        .iter()
        .map(|&item| drop_line(world, item, locale))
        .collect();
    items.iter().fold(
        CommandReply::to_caller(system(lines.join("\n"))),
        |reply, &item| {
            reply
                .with_effect(Effect::InventoryRemove {
                    container: caller,
                    item,
                })
                .with_effect(Effect::MoveTo {
                    entity: item,
                    place: location,
                })
        },
    )
}

/// The `you drop X` line for one item.
fn drop_line(world: &World, item: EntityId, locale: &Locale) -> String {
    t!(
        locale,
        "drop.dropped",
        item = display_name(world, item).unwrap_or_default()
    )
}

/// A numbered candidate list for a disambiguation prompt: `1: sword, 2: shield`.
fn numbered(world: &World, candidates: &[EntityId]) -> String {
    candidates
        .iter()
        .enumerate()
        .map(|(index, &entity)| {
            let name = display_name(world, entity).unwrap_or_default();
            format!("{}: {name}", index + 1)
        })
        .collect::<Vec<_>>()
        .join(", ")
}
