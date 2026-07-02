//! End-to-end command dispatch through the public surface, against a real
//! `mud-core` World and a fake session (§2.7 steps 3–8). Mirrors
//! `mud-cmd/tests/command_pipeline.rs` and `mud-core/tests/locks_pipeline.rs`.
//!
//! The puppet and location command layers are exercised for real; the account
//! and channel layers are empty, as in M1. The session→caller resolution that
//! M1-18/19 will own is supplied here by a hand-built [`SessionResolver`].
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use mud_cmd::{Command, CommandName};
use mud_core::{EntityId, Lock, LockContext, PlaceId, StyledText, TenantTag, World};
use mud_engine::{
    CallerContext, CommandBinding, CommandContext, CommandHandler, CommandReply, Dispatcher,
    LayerCommands, Pipeline, PipelineError, Places, Presence, ResolvedSession, Roster,
    SessionResolver,
};
use mud_i18n::Locale;
use mud_schema::{InputLine, SessionId, SessionInput};

const HALL: u64 = 10;

fn session(value: u64) -> SessionId {
    SessionId::new(NonZeroU64::new(value).expect("session id non-zero"))
}

fn place(value: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(value).expect("place id non-zero"))
}

fn name(value: &str) -> CommandName {
    CommandName::parse(value).expect("valid command name")
}

fn command(canonical: &str, aliases: &[&str]) -> Command {
    aliases
        .iter()
        .fold(Command::new(name(canonical)), |cmd, alias| {
            cmd.with_alias(name(alias))
        })
}

fn admin_lock() -> Lock {
    mud_core::resolve(mud_core::parse("cmd:perm(admin)").expect("parse")).expect("resolve")
}

fn input(line: &str) -> SessionInput {
    SessionInput {
        session_id: session(1),
        line: InputLine::new(line),
    }
}

/// A handler that records its invocation count and replies with a fixed line.
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

    fn runs(&self) -> usize {
        self.runs.load(Ordering::Relaxed)
    }
}

impl CommandHandler for Recording {
    fn run(&self, _ctx: &CommandContext<'_>) -> CommandReply {
        self.runs.fetch_add(1, Ordering::Relaxed);
        CommandReply::to_caller(StyledText::new().plain(self.reply.clone()))
    }
}

/// Records the `switches`/`args` its run received, to prove the parse output is
/// threaded through `CommandContext` to the bound handler.
struct Capturing {
    seen: Mutex<Option<(Vec<String>, String)>>,
}

impl Capturing {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            seen: Mutex::new(None),
        })
    }

    fn captured(&self) -> (Vec<String>, String) {
        self.seen
            .lock()
            .expect("lock not poisoned")
            .clone()
            .expect("handler ran")
    }
}

impl CommandHandler for Capturing {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        let switches = ctx
            .switches()
            .iter()
            .map(|switch| switch.as_str().to_owned())
            .collect();
        *self.seen.lock().expect("lock not poisoned") = Some((switches, ctx.args().to_owned()));
        CommandReply::to_caller(StyledText::new().plain("ok"))
    }
}

/// Resolves the one fake session to a fixed caller + layers; everything else is
/// unknown (returns `None`).
struct FakeResolver {
    caller: EntityId,
    access: LockContext,
}

impl SessionResolver for FakeResolver {
    fn resolve(&self, session_id: SessionId, _world: &World) -> Option<ResolvedSession> {
        if session_id != session(1) {
            return None;
        }
        let layers = LayerCommands {
            puppet: vec![command("look", &["p"]), command("smite", &[])],
            location: vec![
                command("look", &["q"]),
                command("say", &[]),
                command("score", &[]),
            ],
            ..LayerCommands::default()
        };
        Some(ResolvedSession {
            caller: CallerContext::new(
                session_id,
                self.caller,
                place(HALL),
                mud_account::PuppetName::parse("hero").expect("name"),
                Locale::EN,
                self.access.clone(),
            ),
            layers,
        })
    }
}

impl Roster for FakeResolver {
    fn session_of(&self, entity: EntityId) -> Option<SessionId> {
        (entity == self.caller).then(|| session(1))
    }

    fn connected(&self) -> Vec<Presence> {
        Vec::new()
    }
}

/// A places registry with no rooms: the commands these tests bind never read a
/// `Place` (look/movement land in PR-B with their own room fixture).
struct NoPlaces;

impl Places for NoPlaces {
    fn get(&self, _id: PlaceId) -> Option<&mud_core::Place> {
        None
    }
}

/// Builds a world with a puppet placed in HALL, plus a resolver for it.
fn fixture(access: LockContext) -> (World, FakeResolver) {
    let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
    let puppet = world.create().expect("create puppet");
    world.move_to(puppet, place(HALL)).expect("place puppet");
    (
        world,
        FakeResolver {
            caller: puppet,
            access,
        },
    )
}

/// The single text line of a one-element output, for terse assertions.
fn only_line(outputs: &[mud_schema::SessionOutput]) -> &str {
    assert_eq!(outputs.len(), 1, "expected exactly one output");
    outputs.first().expect("one output").text.as_str()
}

#[test]
fn a_bound_command_runs_and_renders_its_reply() {
    let (mut world, resolver) = fixture(LockContext::new());
    let look = Recording::new("You look around.");
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("look"),
        CommandBinding::new(Arc::clone(&look) as Arc<_>),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("look"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(outputs.len(), 1);
    let reply = outputs.first().expect("one output");
    assert_eq!(reply.session_id, session(1));
    assert_eq!(reply.text.as_str(), "You look around.");
    assert_eq!(look.runs(), 1);
}

#[test]
fn the_handler_receives_the_parsed_switches_and_args() {
    let (mut world, resolver) = fixture(LockContext::new());
    let look = Capturing::new();
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("look"),
        CommandBinding::new(Arc::clone(&look) as Arc<_>),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    let _ = pipeline
        .dispatch(
            &mut world,
            &NoPlaces,
            &resolver,
            &input("look/quiet at the door"),
        )
        .expect("dispatch succeeds");

    let (switches, args) = look.captured();
    assert_eq!(switches, vec!["quiet".to_owned()]);
    assert_eq!(args, "at the door");
}

#[test]
fn the_puppet_alias_survives_the_merge_but_the_locations_does_not() {
    let (mut world, resolver) = fixture(LockContext::new());
    let look = Recording::new("looked");
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("look"),
        CommandBinding::new(Arc::clone(&look) as Arc<_>),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    // The puppet's `look` wins the collision, so its alias `p` resolves to it...
    let via_alias = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("p"))
        .expect("dispatch succeeds")
        .outputs;
    assert_eq!(only_line(&via_alias), "looked");

    // ...while the losing location binding's alias `q` is gone.
    let dropped = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("q"))
        .expect("dispatch succeeds")
        .outputs;
    assert_eq!(only_line(&dropped), "command.not-found");
    assert_eq!(look.runs(), 1, "only the alias hit runs the handler");
}

#[test]
fn an_unknown_command_reports_not_found_and_runs_nothing() {
    let (mut world, resolver) = fixture(LockContext::new());
    let look = Recording::new("looked");
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("look"),
        CommandBinding::new(Arc::clone(&look) as Arc<_>),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("dance"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "command.not-found");
    assert_eq!(look.runs(), 0);
}

#[test]
fn an_ambiguous_prefix_reports_ambiguity() {
    let (mut world, resolver) = fixture(LockContext::new());
    let mut pipeline = Pipeline::new(Dispatcher::new());

    // `s` prefixes both `say` and `score`. The candidate list is threaded to the
    // `t!` seam as `options`, but `command.ambiguous` is not in the builtin
    // catalog (only the M1-17 command bodies' keys are), so the message renders
    // as its literal key; surfacing the candidates is a M2 catalog concern.
    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("s"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "command.ambiguous");
}

#[test]
fn a_malformed_switch_reports_a_bad_switch() {
    let (mut world, resolver) = fixture(LockContext::new());
    let mut pipeline = Pipeline::new(Dispatcher::new());

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("look/"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "command.bad-switch");
}

#[test]
fn a_blank_line_produces_no_output() {
    let (mut world, resolver) = fixture(LockContext::new());
    let mut pipeline = Pipeline::new(Dispatcher::new());

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("   "))
        .expect("dispatch succeeds")
        .outputs;

    assert!(outputs.is_empty());
}

#[test]
fn a_matched_but_unbound_command_reports_generically() {
    let (mut world, resolver) = fixture(LockContext::new());
    // `score` is in the location layer but no handler is bound to it.
    let mut pipeline = Pipeline::new(Dispatcher::new());

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("score"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "command.unbound");
}

#[test]
fn a_lock_denies_a_caller_without_permission() {
    let (mut world, resolver) = fixture(LockContext::new()); // no `admin` perm
    let smite = Recording::new("smitten");
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("smite"),
        CommandBinding::new(Arc::clone(&smite) as Arc<_>).gated_by(admin_lock()),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("smite"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "command.denied");
    assert_eq!(smite.runs(), 0, "the gated handler must not run");
}

#[test]
fn a_lock_grants_a_caller_with_permission() {
    let (mut world, resolver) = fixture(LockContext::new().with_perm("admin"));
    let smite = Recording::new("smitten");
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("smite"),
        CommandBinding::new(Arc::clone(&smite) as Arc<_>).gated_by(admin_lock()),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("smite"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "smitten");
    assert_eq!(smite.runs(), 1);
}

#[test]
fn an_unresolvable_session_is_an_error() {
    let (mut world, resolver) = fixture(LockContext::new());
    let mut pipeline = Pipeline::new(Dispatcher::new());
    let unknown = SessionInput {
        session_id: session(999),
        line: InputLine::new("look"),
    };

    let result = pipeline.dispatch(&mut world, &NoPlaces, &resolver, &unknown);

    // `DispatchOutcome` (the `Ok` payload) does not derive `PartialEq` — it wraps
    // a `Vec<SessionOutput>` that has no meaningful notion of test equality here
    // — so this matches on the error variant directly instead of `assert_eq!`.
    assert!(matches!(
        result,
        Err(PipelineError::UnknownSession(id)) if id == session(999)
    ));
}

#[test]
fn each_run_mints_a_distinct_command_id() {
    // Trace-correlation (§2.7.1) is asserted as a unit test in `pipeline.rs`,
    // where the events originate: tracing-test's default filter scopes capture to
    // the test binary's own crate, so a library crate's events are invisible from
    // an integration test. Here we assert the observable proxy: each run is a
    // fresh dispatch and both succeed independently.
    let (mut world, resolver) = fixture(LockContext::new());
    let mut pipeline = Pipeline::new(Dispatcher::new());

    let first = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("score"))
        .expect("first dispatch")
        .outputs;
    let second = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("score"))
        .expect("second dispatch")
        .outputs;

    assert_eq!(only_line(&first), "command.unbound");
    assert_eq!(only_line(&second), "command.unbound");
}

/// A handler that returns a `MoveTo` effect for the caller, to prove the
/// pipeline applies a [`CommandReply`]'s effects against the `&mut World`.
struct Teleport {
    to: PlaceId,
}

impl CommandHandler for Teleport {
    fn run(&self, ctx: &CommandContext<'_>) -> CommandReply {
        CommandReply::to_caller(StyledText::new().plain("whoosh")).with_effect(
            mud_core::Effect::MoveTo {
                entity: ctx.caller(),
                place: self.to,
            },
        )
    }
}

#[test]
fn a_replys_effects_are_applied_to_the_world() {
    const STUDY: u64 = 11;
    let (mut world, resolver) = fixture(LockContext::new());
    let mut dispatcher = Dispatcher::new();
    dispatcher.bind(
        name("look"),
        CommandBinding::new(Arc::new(Teleport { to: place(STUDY) })),
    );
    let mut pipeline = Pipeline::new(dispatcher);

    // The puppet starts in HALL (see `fixture`).
    let outputs = pipeline
        .dispatch(&mut world, &NoPlaces, &resolver, &input("look"))
        .expect("dispatch succeeds")
        .outputs;

    assert_eq!(only_line(&outputs), "whoosh");
    assert!(
        world.is_located_in(resolver.caller, place(STUDY)),
        "the MoveTo effect must have moved the caller to the study"
    );
}
