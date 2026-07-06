//! `say`: speak to the room (§3.6.3, M1-19a).

use mud_core::{RoleName, StyledText};
use mud_i18n::t;

use super::system;
use crate::dispatch::{Broadcast, CommandContext, CommandHandler, CommandReply};
use crate::text::sanitize;

/// `say`: speak to the room, echoing to the caller and broadcasting to every
/// other co-located session (§3.6.3, M1-19a).
pub(super) struct Say;

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
