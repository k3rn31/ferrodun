//! The command pipeline (§2.7 steps 3–8).
//!
//! [`Pipeline::dispatch`] turns one [`SessionInput`] line into the
//! [`SessionOutput`]s the caller should receive, running the ordered steps:
//! resolve the session (3), merge the caller's command layers (4), parse the
//! line (5), lock-check the caller (6), dispatch the bound handler (7), and
//! render the reply per session (8). Every run is wrapped in a `tracing` span
//! carrying its [`CommandId`](crate::CommandId) so everything it emits shares one
//! correlation id (§2.7.1).

use mud_cmd::ParseOutcome;
use mud_core::{TickEvent, World};
use mud_i18n::t;
use mud_schema::{OutputText, SessionInput, SessionOutput};

use crate::CommandId;
use crate::caller::{CallerContext, SessionResolver};
use crate::command_id::CommandIdGen;
use crate::dispatch::{CommandContext, Dispatcher};
use crate::places::Places;

/// Runs player input through §2.7's command pipeline.
///
/// Owns the command-behavior [`Dispatcher`] and the per-run id generator. One
/// pipeline serves one World; the server (M1-22) drives it from the gateway's
/// input frames.
#[must_use]
pub struct Pipeline {
    dispatcher: Dispatcher,
    ids: CommandIdGen,
}

impl Pipeline {
    /// A pipeline dispatching to `dispatcher`'s bound commands.
    pub fn new(dispatcher: Dispatcher) -> Self {
        Self {
            dispatcher,
            ids: CommandIdGen::new(),
        }
    }

    /// Runs one input line through §2.7 steps 3–8.
    ///
    /// Returns the rendered output for the caller. Player-visible outcomes — an
    /// unknown command, an ambiguous prefix, a malformed switch, or a lock
    /// denial — are reported as ordinary [`SessionOutput`] messages, not errors.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError::UnknownSession`] when the session resolves to no
    /// caller (an unknown or not-yet-logged-in session), and
    /// [`PipelineError::CommandIdExhausted`] if the per-run id space is spent.
    pub fn dispatch(
        &mut self,
        world: &mut World,
        places: &dyn Places,
        resolver: &impl SessionResolver,
        input: &SessionInput,
    ) -> Result<Vec<SessionOutput>, PipelineError> {
        let command_id = self.ids.next()?;
        let session_id = input.session_id;
        let span = tracing::info_span!(
            "command",
            command_id = %command_id,
            session_id = %session_id,
        );
        let _entered = span.enter();
        // One observable record per run so every command is traced (§2.7.1), even
        // on the happy path where no warning fires. Everything emitted from here
        // on inherits the span's `command_id`.
        tracing::debug!("dispatching command");

        // Step 3: resolve session → account → puppet → location stack.
        let resolved = resolver
            .resolve(session_id, world)
            .ok_or(PipelineError::UnknownSession(session_id))?;
        let caller = resolved.caller;
        let locale = caller.locale().clone();

        // Step 4: merge the caller's command layers into one table.
        let table = resolved.layers.merge();

        // Step 5: parse the line against the merged table.
        let outputs = match table.parse(input.line.as_str()) {
            ParseOutcome::Empty => Vec::new(),
            ParseOutcome::NotFound => message(session_id, t!(locale, "command.not-found")),
            ParseOutcome::Ambiguous(names) => {
                let options = names
                    .iter()
                    .map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                message(
                    session_id,
                    t!(locale, "command.ambiguous", options = options),
                )
            }
            ParseOutcome::BadSwitch(error) => {
                message(session_id, t!(locale, "command.bad-switch", reason = error))
            }
            ParseOutcome::Matched {
                command,
                switches,
                args,
            } => self.run_matched(
                world,
                places,
                command_id,
                &caller,
                Parsed {
                    command,
                    switches: &switches,
                    args,
                },
            ),
        };

        Ok(outputs)
    }

    /// Steps 6–8 for a parsed command: lock-check, dispatch, apply effects,
    /// render.
    fn run_matched(
        &self,
        world: &mut World,
        places: &dyn Places,
        command_id: CommandId,
        caller: &CallerContext,
        parsed: Parsed<'_>,
    ) -> Vec<SessionOutput> {
        let Parsed {
            command,
            switches,
            args,
        } = parsed;
        let session_id = caller.session_id();
        let locale = caller.locale();

        let Some(binding) = self.dispatcher.binding(command.name()) else {
            // Parsed to a name with no behavior bound: a content/registry gap, not
            // player error. Log it; tell the player generically without leaking
            // the unbound name.
            tracing::warn!(command = %command.name().as_str(), "matched command has no bound handler");
            return message(session_id, t!(locale.clone(), "command.unbound"));
        };

        // Step 6: lock-check the caller. An ungated command is always permitted.
        if let Some(lock) = binding.lock()
            && !lock.evaluate(caller.access())
        {
            tracing::warn!(command = %command.name().as_str(), "lock denied command");
            return message(session_id, t!(locale.clone(), "command.denied"));
        }

        // Step 7: dispatch the handler over a read-only world, then apply the
        // effects it returned against the &mut World held here. The immutable
        // borrow in `ctx` ends before the mutation, so a handler cannot both read
        // and write the world in one run — mutation is data it requests.
        let reply = {
            let ctx = CommandContext::new(command_id, caller, switches, args, &*world, places);
            binding.handler().run(&ctx)
        };
        for &effect in reply.effects() {
            if let Some(TickEvent::Rejected { effect, error }) = world.apply_effect(effect) {
                tracing::warn!(
                    command = %command.name().as_str(),
                    ?effect,
                    ?error,
                    "command effect rejected",
                );
            }
        }

        // Step 8: render per session. The styled-text-over-IPC swap and ANSI
        // rendering land in M1-21/22; for now flatten to plain text.
        message(session_id, reply.output().to_plain_string())
    }
}

/// The §2.7-step-5 parse output for a matched command, grouped so the
/// lock-check/dispatch step takes one argument rather than three.
struct Parsed<'a> {
    command: &'a mud_cmd::Command,
    switches: &'a [mud_cmd::Switch],
    args: &'a str,
}

/// Wraps one engine message as a single-element output for `session_id`.
fn message(session_id: mud_schema::SessionId, text: String) -> Vec<SessionOutput> {
    vec![SessionOutput {
        session_id,
        text: OutputText::new(text),
    }]
}

/// A fault that prevents a command from running at all (as opposed to a
/// player-visible outcome, which is reported as output).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum PipelineError {
    /// The session resolved to no caller (unknown or not yet logged in).
    #[error("no caller is bound to session {0}")]
    UnknownSession(mud_schema::SessionId),
    /// The per-run command-id space is exhausted.
    #[error("command-id space exhausted")]
    CommandIdExhausted,
}

#[cfg(test)]
mod tests {
    //! Trace-correlation (§2.7.1) is exercised here, in the crate the events
    //! originate from, so tracing-test's crate-scoped capture sees them — an
    //! integration test would not (mirrors `mud-i18n`'s in-crate traced tests).

    use std::num::NonZeroU64;

    use mud_account::PuppetName;
    use mud_core::{EntityId, LockContext, PlaceId, TenantTag, World};
    use mud_i18n::Locale;
    use mud_schema::InputLine;

    use super::*;
    use crate::caller::ResolvedSession;
    use crate::layers::LayerCommands;

    struct FakeResolver {
        caller: EntityId,
    }

    impl SessionResolver for FakeResolver {
        fn resolve(
            &self,
            session_id: mud_schema::SessionId,
            _world: &World,
        ) -> Option<ResolvedSession> {
            let place = PlaceId::new(NonZeroU64::new(10).expect("non-zero"));
            Some(ResolvedSession {
                caller: CallerContext::new(
                    session_id,
                    self.caller,
                    place,
                    PuppetName::parse("hero").expect("name"),
                    Locale::EN,
                    LockContext::new(),
                ),
                // An empty table: the line parses to NotFound, but the run is still
                // traced — which is what this test observes.
                layers: LayerCommands::default(),
            })
        }
    }

    fn input(value: u64, line: &str) -> SessionInput {
        SessionInput {
            session_id: mud_schema::SessionId::new(NonZeroU64::new(value).expect("non-zero")),
            line: InputLine::new(line),
        }
    }

    /// A places registry with no rooms: these traced-dispatch tests parse to
    /// `NotFound`, so no handler reads a `Place`.
    struct NoPlaces;

    impl Places for NoPlaces {
        fn get(&self, _id: PlaceId) -> Option<&mud_core::Place> {
            None
        }
    }

    #[tracing_test::traced_test]
    #[test]
    fn each_run_is_traced_with_a_distinct_command_id() {
        let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
        let caller = world.create().expect("create caller");
        let resolver = FakeResolver { caller };
        let mut pipeline = Pipeline::new(Dispatcher::new());

        pipeline
            .dispatch(&mut world, &NoPlaces, &resolver, &input(1, "look"))
            .expect("first dispatch");
        pipeline
            .dispatch(&mut world, &NoPlaces, &resolver, &input(1, "look"))
            .expect("second dispatch");

        // Each run logs under its own command span; the ids increase per run.
        assert!(logs_contain("command_id=1"));
        assert!(logs_contain("command_id=2"));
    }
}
