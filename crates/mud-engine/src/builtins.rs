//! The M1 set of Rust-native built-in commands (§2.7 step 7, M1-17).
//!
//! Each command is a [`CommandHandler`]; [`register`] binds them all into a
//! [`Dispatcher`] and returns the matching [`Command`] metadata for the
//! session's built-in command layer (§2.7 step 4). The set is `look`, the six
//! movement commands, `say`, `inventory`, `get`, and `drop`. Commands that
//! mutate the world return [`Effect`]s on their [`CommandReply`]; the pipeline
//! applies them (§2.7 step 7).
//!
//! Player-facing strings resolve through the `t!` seam (§3.14.4); player-authored
//! text is sanitized (§3.6.4) and emitted as plain spans so embedded markup
//! renders literally (§3.20.7).

use std::sync::Arc;

use mud_cmd::{Command, CommandName};
use mud_core::{Direction, Effect, EntityId, Place, PlaceId, RoleName, Span, StyledText, World};
use mud_i18n::{Locale, t};

use crate::dispatch::{CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher};
use crate::objects::{Resolution, resolve_among};
use crate::text::sanitize;

/// Binds every built-in command into `dispatcher` and returns the command
/// metadata for the session's built-in layer (§2.7 step 4).
///
/// The returned [`Command`]s carry the canonical names and aliases the parser
/// resolves; the bindings carry the behavior. The two are built from one table
/// so a command can never appear on one side only.
pub fn register(dispatcher: &mut Dispatcher) -> Vec<Command> {
    let mut commands = Vec::new();
    for (canonical, aliases, handler) in table() {
        let Ok(name) = CommandName::parse(canonical) else {
            // The names are 'static and known-valid; a parse failure means a typo
            // in the table, not runtime input. Skip rather than abort startup.
            tracing::error!(command = canonical, "built-in command name failed to parse");
            continue;
        };
        let command =
            aliases.iter().fold(
                Command::new(name.clone()),
                |cmd, alias| match CommandName::parse(alias) {
                    Ok(alias) => cmd.with_alias(alias),
                    Err(_) => {
                        tracing::error!(alias, "built-in command alias failed to parse");
                        cmd
                    }
                },
            );
        dispatcher.bind(name, CommandBinding::new(handler));
        commands.push(command);
    }
    commands
}

/// The built-in command table: canonical name, aliases, and handler.
fn table() -> Vec<(
    &'static str,
    &'static [&'static str],
    Arc<dyn CommandHandler>,
)> {
    vec![
        ("look", &["l"], Arc::new(Look)),
        ("inventory", &["i", "inv"], Arc::new(ShowInventory)),
        ("say", &[], Arc::new(Say)),
        ("get", &["take"], Arc::new(Get)),
        ("drop", &[], Arc::new(Drop)),
        ("north", &["n"], Arc::new(Move(Direction::North))),
        ("east", &["e"], Arc::new(Move(Direction::East))),
        ("south", &["s"], Arc::new(Move(Direction::South))),
        ("west", &["w"], Arc::new(Move(Direction::West))),
        ("up", &["u"], Arc::new(Move(Direction::Up))),
        ("down", &["d"], Arc::new(Move(Direction::Down))),
    ]
}

/// `look`: render the caller's current room (§3.2).
struct Look;

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

/// One of the six movement commands, carrying the direction it travels (§3.2.2).
struct Move(Direction);

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
        let arrival = render_room(place, ctx.world(), ctx.caller(), &locale);
        CommandReply::to_caller(arrival).with_effect(Effect::MoveTo {
            entity: ctx.caller(),
            place: to,
        })
    }
}

/// `say`: speak to the room (§3.6.3). Caller echo only in M1; the room broadcast
/// lands with the session→entity map (M1-19).
struct Say;

impl CommandHandler for Say {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        let message = match sanitize(ctx.args()) {
            Ok(message) => message,
            Err(_) => return CommandReply::to_caller(system(t!(locale, "content.too-long"))),
        };
        if message.trim().is_empty() {
            return CommandReply::to_caller(system(t!(locale, "say.nothing")));
        }
        // The whole line carries the `say` role; the sanitized body is plain text
        // inside it, so any markup the player typed renders literally (§3.20.7).
        CommandReply::to_caller(
            StyledText::new().role(t!(locale, "say.speech", message = message), RoleName::SAY),
        )
    }
}

/// `inventory`: list what the caller is carrying.
struct ShowInventory;

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
struct Get;

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
struct Drop;

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

/// Renders a room as the caller sees it: title, description, exits, and the
/// other entities present (§3.2).
fn render_room(place: &Place, world: &World, viewer: EntityId, locale: &Locale) -> StyledText {
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
    DIRECTIONS
        .iter()
        .filter(|&&dir| place.neighbor(dir).is_some())
        .map(|&dir| direction_name(dir))
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

/// An entity's display name: its first keyword, or `None` if it has none.
fn display_name(world: &World, entity: EntityId) -> Option<String> {
    world
        .keywords_of(entity)
        .first()
        .map(|keyword| keyword.as_str().to_owned())
}

/// Wraps engine-authored text as a single `system`-role line.
fn system(text: String) -> StyledText {
    StyledText::new().role(text, RoleName::SYSTEM)
}

/// The canonical English name of a direction (§3.14.5.1: built-in command names
/// are invariant across locales).
fn direction_name(dir: Direction) -> &'static str {
    match dir {
        Direction::North => "north",
        Direction::East => "east",
        Direction::South => "south",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// The six directions in display order.
const DIRECTIONS: [Direction; 6] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
    Direction::Up,
    Direction::Down,
];
