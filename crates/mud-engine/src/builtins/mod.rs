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
use mud_core::{Direction, Effect, EntityId, PlaceId, RoleName, StyledText, World};
use mud_i18n::{Locale, t};

use crate::dispatch::{
    Broadcast, CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher,
};
use crate::objects::{Resolution, resolve_among};
use crate::text::sanitize;

mod look;
mod movement;

use look::Look;
use movement::Move;

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
        ("who", &[], Arc::new(Who)),
        ("get", &["take"], Arc::new(Get)),
        ("drop", &[], Arc::new(Drop)),
        ("north", &["n"], Arc::new(Move(Direction::North))),
        ("east", &["e"], Arc::new(Move(Direction::East))),
        ("south", &["s"], Arc::new(Move(Direction::South))),
        ("west", &["w"], Arc::new(Move(Direction::West))),
        ("up", &["u"], Arc::new(Move(Direction::Up))),
        ("down", &["d"], Arc::new(Move(Direction::Down))),
        ("quit", &[], Arc::new(Quit)),
    ]
}

/// `say`: speak to the room, echoing to the caller and broadcasting to every
/// other co-located session (§3.6.3, M1-19a).
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
        let name = ctx.caller_name().as_str().to_owned();
        // The caller hears "You say, …"; everyone else in the room hears
        // "<name> says, …". Sanitized player text is plain, so any markup renders
        // literally (§3.20.7).
        let heard = StyledText::new().role(
            t!(
                locale,
                "say.broadcast",
                name = name,
                message = message.clone()
            ),
            RoleName::SAY,
        );
        CommandReply::to_caller(
            StyledText::new().role(t!(locale, "say.speech", message = message), RoleName::SAY),
        )
        .with_broadcast(Broadcast::to_place(ctx.location(), ctx.caller(), heard))
    }
}

/// `who`: list the players currently connected and in-world (§3.19).
struct Who;

impl CommandHandler for Who {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        // Sort by name so the listing is stable regardless of registry iteration
        // order (the roster is backed by a HashMap).
        let mut names: Vec<String> = ctx
            .roster()
            .connected()
            .into_iter()
            .map(|presence| presence.name.as_str().to_owned())
            .collect();
        names.sort();
        CommandReply::to_caller(system(t!(locale, "who.online", names = names.join(", "))))
    }
}

/// `quit`: leave the game. Signals the driver to close the session (§3.19); the
/// socket teardown is the gateway's job (M1-21/22).
struct Quit;

impl CommandHandler for Quit {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        CommandReply::to_caller(system(t!(locale, "quit.goodbye"))).closing()
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
