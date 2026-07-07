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

use crate::dispatch::{CommandBinding, CommandHandler, Dispatcher};

mod items;
mod look;
mod movement;
mod say;
mod session;

use items::{Drop, Get, ShowInventory};
use look::Look;
use movement::Move;
use say::Say;
use session::{Quit, Who};

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

/// An entity's display name: its first keyword, or `None` if it has none.
pub(super) fn display_name(world: &World, entity: EntityId) -> Option<String> {
    world
        .keywords_of(entity)
        .first()
        .map(|keyword| keyword.as_str().to_owned())
}

/// Wraps engine-authored text as a single `system`-role line.
pub(super) fn system(text: String) -> StyledText {
    StyledText::new().role(text, RoleName::SYSTEM)
}
