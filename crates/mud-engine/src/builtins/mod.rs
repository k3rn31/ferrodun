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
use mud_core::{Direction, EntityId, RoleName, StyledText, World};
use mud_i18n::t;

use crate::dispatch::{
    Broadcast, CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher,
};
use crate::text::sanitize;

mod items;
mod look;
mod movement;

use items::{Drop, Get, ShowInventory};
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
