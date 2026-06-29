//! Command behavior: the lock-check and dispatch registry (§2.7 steps 6–7).
//!
//! `mud-cmd` commands are pure metadata (name, aliases, switches) so they stay a
//! host-free leaf. The *executable* concerns — the lock that gates a command and
//! the `run` that performs it — live here, in a [`Dispatcher`] keyed by the
//! canonical [`CommandName`] the parser resolves. A [`CommandBinding`] pairs an
//! optional [`Lock`] with a [`CommandHandler`]; the [`Pipeline`](crate::Pipeline)
//! looks the binding up by name, checks the lock against the caller, then runs
//! the handler.

use std::collections::HashMap;
use std::sync::Arc;

use mud_cmd::{CommandName, Switch};
use mud_core::{EntityId, Lock, PlaceId, StyledText, World};
use mud_i18n::Locale;
use mud_schema::SessionId;

use crate::CommandId;
use crate::caller::CallerContext;

/// Everything a command handler may read about one run (§2.7 step 7).
///
/// Borrows the live [`World`] read-only and the parsed `switches`/`args`. The
/// `command_id` is exposed so a handler that spawns asynchronous work (an M2 Lua
/// script, an M3 LLM dialogue) can tag it with the originating run for log
/// correlation (§2.7.1).
#[must_use]
pub struct CommandContext<'a> {
    command_id: CommandId,
    caller: &'a CallerContext,
    switches: &'a [Switch],
    args: &'a str,
    world: &'a World,
}

impl<'a> CommandContext<'a> {
    /// Assembles the context for one handler invocation.
    ///
    /// Borrows the resolved [`CallerContext`] (session, caller entity, location,
    /// locale) rather than restating its fields, so adding a caller fact does not
    /// widen this signature.
    pub(crate) fn new(
        command_id: CommandId,
        caller: &'a CallerContext,
        switches: &'a [Switch],
        args: &'a str,
        world: &'a World,
    ) -> Self {
        Self {
            command_id,
            caller,
            switches,
            args,
            world,
        }
    }

    /// The trace-correlation id for this run (§2.7.1).
    pub fn command_id(&self) -> CommandId {
        self.command_id
    }

    /// The session the command was issued through.
    pub fn session_id(&self) -> SessionId {
        self.caller.session_id()
    }

    /// The entity issuing the command (player or NPC).
    pub fn caller(&self) -> EntityId {
        self.caller.caller()
    }

    /// The caller's current location.
    pub fn location(&self) -> PlaceId {
        self.caller.location()
    }

    /// The locale engine messages resolve against.
    pub fn locale(&self) -> &Locale {
        self.caller.locale()
    }

    /// The switches given after the command (e.g. `quiet` in `look/quiet`).
    pub fn switches(&self) -> &[Switch] {
        self.switches
    }

    /// The raw argument remainder, trimmed but otherwise verbatim.
    pub fn args(&self) -> &str {
        self.args
    }

    /// The live world, read-only.
    pub fn world(&self) -> &World {
        self.world
    }
}

/// What a command run produces (§2.7 step 8).
///
/// M1 carries only the styled reply to the caller. The type is a struct, not a
/// bare [`StyledText`], because it is the chosen seam for everything a command
/// affects beyond the caller. Handlers are intentionally pure over a read-only
/// [`World`]: a command that mutates the world (movement, `get`/`drop`) or
/// addresses other sessions (`say`, arrival/departure) does **not** get `&mut
/// World`; it returns the change as data here, and the [`Pipeline`](crate::Pipeline)
/// applies it against the `&mut World` it holds and fans out the extra outputs.
///
/// CONTRACT (unproven until M1-17): this keeps [`CommandHandler`] stable as those
/// commands land — *if* world effects and broadcast targets express cleanly as
/// return data. If they do not, reshaping `CommandReply` (not the handler taking
/// `&mut World`) is the intended escape hatch. The `&mut World` on
/// [`Pipeline::dispatch`](crate::Pipeline::dispatch) is the application point for
/// these effects; it is held now, ahead of the first effect, on purpose.
#[must_use]
pub struct CommandReply {
    output: StyledText,
}

impl CommandReply {
    /// A reply that sends `output` to the caller.
    pub fn to_caller(output: StyledText) -> Self {
        Self { output }
    }

    /// The styled text to present to the caller.
    pub fn output(&self) -> &StyledText {
        &self.output
    }
}

/// A command's `run` behavior (§2.7 step 7).
///
/// Rust-native for M1; Lua-defined handlers are an M2 implementation of this
/// same trait. `Send + Sync` so a [`Dispatcher`] can be shared across the async
/// runtime the server adds in M1-22.
pub trait CommandHandler: Send + Sync {
    /// Runs the command against `ctx`, producing the caller's reply.
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply;
}

/// A command's executable side: the lock that gates it and the handler that runs
/// it.
#[must_use]
pub struct CommandBinding {
    lock: Option<Lock>,
    handler: Arc<dyn CommandHandler>,
}

impl CommandBinding {
    /// An ungated binding: the handler runs for any caller.
    pub fn new(handler: Arc<dyn CommandHandler>) -> Self {
        Self {
            lock: None,
            handler,
        }
    }

    /// Gates this binding behind `lock` (§2.7 step 6): the handler runs only when
    /// the lock grants the caller access.
    pub fn gated_by(mut self, lock: Lock) -> Self {
        self.lock = Some(lock);
        self
    }

    /// The lock gating this binding, if any.
    pub(crate) fn lock(&self) -> Option<&Lock> {
        self.lock.as_ref()
    }

    /// The handler to run once the lock check passes.
    pub(crate) fn handler(&self) -> &dyn CommandHandler {
        self.handler.as_ref()
    }
}

/// Maps a resolved [`CommandName`] to its [`CommandBinding`].
///
/// Built once at startup by binding the built-in (M1-17) and, later,
/// script-defined commands. Lookup is by the canonical name the parser resolves,
/// so aliases and prefixes all route to the same binding.
#[derive(Default)]
#[must_use]
pub struct Dispatcher {
    bindings: HashMap<CommandName, CommandBinding>,
}

impl Dispatcher {
    /// An empty dispatcher.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `name` to `binding`, replacing any previous binding for that name.
    pub fn bind(&mut self, name: CommandName, binding: CommandBinding) {
        self.bindings.insert(name, binding);
    }

    /// The binding for `name`, or `None` if the command has no behavior bound.
    pub(crate) fn binding(&self, name: &CommandName) -> Option<&CommandBinding> {
        self.bindings.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A handler that counts its invocations and replies with a fixed message.
    struct Recording {
        runs: AtomicUsize,
        reply: String,
    }

    impl Recording {
        fn new(reply: &str) -> Arc<Self> {
            Arc::new(Self {
                runs: AtomicUsize::new(0),
                reply: reply.to_string(),
            })
        }
    }

    impl CommandHandler for Recording {
        fn run(&self, _ctx: &CommandContext<'_>) -> CommandReply {
            self.runs.fetch_add(1, Ordering::Relaxed);
            CommandReply::to_caller(StyledText::new().plain(self.reply.clone()))
        }
    }

    fn name(value: &str) -> CommandName {
        CommandName::parse(value).expect("valid command name")
    }

    #[test]
    fn a_bound_command_is_found_by_its_name() {
        let mut dispatcher = Dispatcher::new();
        dispatcher.bind(name("look"), CommandBinding::new(Recording::new("ok")));

        assert!(dispatcher.binding(&name("look")).is_some());
        assert!(dispatcher.binding(&name("smite")).is_none());
    }

    #[test]
    fn gating_a_binding_attaches_the_lock() {
        let lock =
            mud_core::resolve(mud_core::parse("x:perm(admin)").expect("parse")).expect("resolve");
        let binding = CommandBinding::new(Recording::new("ok")).gated_by(lock);

        assert!(binding.lock().is_some());
    }
}
