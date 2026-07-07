//! Session-facing builtins: `who` (§3.19) and `quit` (§3.19).

use mud_i18n::t;

use super::system;
use crate::dispatch::{CommandContext, CommandHandler, CommandReply};

/// `who`: list the players currently connected and in-world (§3.19).
pub(super) struct Who;

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
pub(super) struct Quit;

impl CommandHandler for Quit {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let locale = ctx.locale().clone();
        CommandReply::to_caller(system(t!(locale, "quit.goodbye"))).closing()
    }
}
