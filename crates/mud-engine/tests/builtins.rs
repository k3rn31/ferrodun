//! End-to-end tests for the M1-17 built-in commands (§2.7, §3.2, §3.6).
//!
//! Each test drives a command through the public [`Pipeline`] against a real
//! `mud-core` World and a small two-room map, asserting the rendered reply and
//! any world mutation the command's effects produced.
#![allow(clippy::expect_used)] // test helpers; mirrors `allow-expect-in-tests`

use std::collections::HashMap;
use std::num::NonZeroU64;

use mud_core::{
    Description, Direction, EntityId, Keyword, LockContext, Place, PlaceId, RegionId, RoomData,
    TenantTag, Title, World,
};
use mud_engine::{
    CallerContext, Dispatcher, LayerCommands, Pipeline, Places, ResolvedSession, SessionResolver,
};
use mud_i18n::Locale;
use mud_schema::{InputLine, SessionId, SessionInput, SessionOutput};

const HALL: u64 = 10;
const STUDY: u64 = 11;
const REGION: u64 = 1;
/// A place id wired as an exit target but never registered — a dangling exit.
const DANGLING: u64 = 999;

fn place(value: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(value).expect("place id non-zero"))
}

fn region() -> RegionId {
    RegionId::new(NonZeroU64::new(REGION).expect("region id non-zero"))
}

fn session() -> SessionId {
    SessionId::new(NonZeroU64::new(1).expect("session id non-zero"))
}

/// A places registry backed by a map, implementing the engine's [`Places`] seam.
struct MapPlaces {
    rooms: HashMap<PlaceId, Place>,
}

impl MapPlaces {
    /// A hall with a north exit to a study; the study has a south exit back.
    fn two_rooms() -> Self {
        let hall = Place::Room(
            RoomData::new(
                place(HALL),
                region(),
                Description::new("A long stone hall."),
            )
            .with_title(Title::new("The Great Hall"))
            .with_exit(Direction::North, place(STUDY)),
        );
        let study = Place::Room(
            RoomData::new(place(STUDY), region(), Description::new("A cramped study."))
                .with_title(Title::new("The Study"))
                .with_exit(Direction::South, place(HALL)),
        );
        let mut rooms = HashMap::new();
        rooms.insert(place(HALL), hall);
        rooms.insert(place(STUDY), study);
        Self { rooms }
    }

    /// A hall whose `up` exit is wired to a place that is **not** registered
    /// here — modelling stale/partial world data (a dangling exit).
    fn hall_with_dangling_exit() -> Self {
        let hall = Place::Room(
            RoomData::new(
                place(HALL),
                region(),
                Description::new("A long stone hall."),
            )
            .with_title(Title::new("The Great Hall"))
            .with_exit(Direction::Up, place(DANGLING)),
        );
        let mut rooms = HashMap::new();
        rooms.insert(place(HALL), hall);
        Self { rooms }
    }
}

impl Places for MapPlaces {
    fn get(&self, id: PlaceId) -> Option<&Place> {
        self.rooms.get(&id)
    }
}

/// Resolves the one session to a fixed caller in HALL, with every built-in bound.
struct FakeResolver {
    caller: EntityId,
    builtins: Vec<mud_cmd::Command>,
}

impl SessionResolver for FakeResolver {
    fn resolve(&self, session_id: SessionId, world: &World) -> Option<ResolvedSession> {
        if session_id != session() {
            return None;
        }
        let location = world.location_of(self.caller).unwrap_or(place(HALL));
        Some(ResolvedSession {
            caller: CallerContext::new(
                session_id,
                self.caller,
                location,
                Locale::EN,
                LockContext::new(),
            ),
            layers: LayerCommands {
                builtins: self.builtins.clone(),
                ..LayerCommands::default()
            },
        })
    }
}

/// A pipeline with all built-ins bound, a two-room map, a caller placed in HALL,
/// and a resolver that re-reads the caller's live location each command (so a
/// movement command is reflected in the next command's context).
struct Harness {
    world: World,
    places: MapPlaces,
    resolver: FakeResolver,
    pipeline: Pipeline,
    caller: EntityId,
}

impl Harness {
    fn new() -> Self {
        Self::with_places(MapPlaces::two_rooms())
    }

    /// Builds the harness over an explicit places registry, so a test can wire
    /// a map with, say, an exit to an unregistered place.
    fn with_places(places: MapPlaces) -> Self {
        let mut world = World::new(TenantTag::new(1).expect("tenant in range"));
        let caller = world.create().expect("create caller");
        world.move_to(caller, place(HALL)).expect("place caller");

        let mut dispatcher = Dispatcher::new();
        let builtins = mud_engine::register(&mut dispatcher);
        Self {
            world,
            places,
            resolver: FakeResolver { caller, builtins },
            pipeline: Pipeline::new(dispatcher),
            caller,
        }
    }

    /// Creates a named item on the floor of `at`.
    fn item_on_floor(&mut self, at: PlaceId, keywords: &[&str]) -> EntityId {
        let item = self.world.create().expect("create item");
        self.world.move_to(item, at).expect("place item");
        self.name(item, keywords);
        item
    }

    fn name(&mut self, item: EntityId, keywords: &[&str]) {
        let keywords = keywords.iter().map(Keyword::new).collect();
        self.world.name_entity(item, keywords).expect("name item");
    }

    fn run(&mut self, line: &str) -> Vec<SessionOutput> {
        let input = SessionInput {
            session_id: session(),
            line: InputLine::new(line),
        };
        self.pipeline
            .dispatch(&mut self.world, &self.places, &self.resolver, &input)
            .expect("dispatch succeeds")
    }

    /// The single text line of a one-element output.
    fn line(&mut self, command: &str) -> String {
        let outputs = self.run(command);
        assert_eq!(outputs.len(), 1, "expected exactly one output");
        outputs
            .first()
            .expect("one output")
            .text
            .as_str()
            .to_owned()
    }
}

#[test]
fn look_renders_the_room_title_description_and_exits() {
    let mut h = Harness::new();

    let view = h.line("look");

    assert!(view.contains("The Great Hall"), "title: {view}");
    assert!(view.contains("A long stone hall."), "description: {view}");
    assert!(view.contains("Exits: north"), "exits: {view}");
}

#[test]
fn look_lists_other_entities_present() {
    let mut h = Harness::new();
    let _ = h.item_on_floor(place(HALL), &["goblin"]);

    let view = h.line("look");

    assert!(view.contains("Also here: goblin"), "occupants: {view}");
}

#[test]
fn moving_north_applies_the_move_and_shows_the_destination() {
    let mut h = Harness::new();

    let arrival = h.line("north");

    assert!(
        arrival.contains("The Study"),
        "destination shown: {arrival}"
    );
    assert!(
        h.world.is_located_in(h.caller, place(STUDY)),
        "the caller moved to the study"
    );
}

#[test]
fn moving_through_an_unwired_exit_is_refused() {
    let mut h = Harness::new();

    // The hall has no east exit.
    assert_eq!(h.line("east"), "You can't go that way.");
    assert!(
        h.world.is_located_in(h.caller, place(HALL)),
        "a refused move leaves the caller in place"
    );
}

#[test]
fn moving_through_an_exit_to_a_missing_place_is_refused() {
    // The hall's `up` exit points at a place absent from the registry (stale
    // world data). The move must be refused outright rather than stranding the
    // caller in a place the engine can't render.
    let mut h = Harness::with_places(MapPlaces::hall_with_dangling_exit());

    assert_eq!(h.line("up"), "You can't go that way.");
    assert!(
        h.world.is_located_in(h.caller, place(HALL)),
        "an exit to a missing place leaves the caller in place"
    );
}

#[test]
fn say_echoes_the_spoken_text_to_the_caller() {
    let mut h = Harness::new();

    assert_eq!(h.line("say hello there"), "You say, \"hello there\"");
}

#[test]
fn say_with_no_text_prompts_for_some() {
    let mut h = Harness::new();

    assert_eq!(h.line("say   "), "Say what?");
}

#[test]
fn say_strips_ansi_and_renders_markup_literally() {
    let mut h = Harness::new();

    // A player trying to inject colour: the raw ANSI is stripped, and the colour
    // *markup* braces survive only as literal text — never compiled to styling
    // (§3.20.7).
    let echoed = h.line("say \u{1b}[31m{error}red{/}");

    assert_eq!(echoed, "You say, \"{error}red{/}\"");
}

#[test]
fn an_over_cap_say_is_rejected() {
    let mut h = Harness::new();
    let long = "x".repeat(5000);

    assert_eq!(h.line(&format!("say {long}")), "Your message is too long.");
}

#[test]
fn inventory_is_empty_then_lists_a_taken_item() {
    let mut h = Harness::new();
    assert_eq!(h.line("inventory"), "You are carrying nothing.");

    let _ = h.item_on_floor(place(HALL), &["sword"]);
    h.run("get sword");

    let inv = h.line("inventory");
    assert!(inv.contains("You are carrying:"), "header: {inv}");
    assert!(inv.contains("sword"), "item listed: {inv}");
}

#[test]
fn get_then_drop_round_trips_an_item_between_floor_and_inventory() {
    let mut h = Harness::new();
    let sword = h.item_on_floor(place(HALL), &["sword"]);

    assert_eq!(h.line("get sword"), "You take sword.");
    assert!(h.world.contains(h.caller, sword), "sword is now carried");
    assert!(
        !h.world.is_located_in(sword, place(HALL)),
        "sword left the floor"
    );

    assert_eq!(h.line("drop sword"), "You drop sword.");
    assert!(
        !h.world.contains(h.caller, sword),
        "sword no longer carried"
    );
    assert!(
        h.world.is_located_in(sword, place(HALL)),
        "sword is back on the floor"
    );
}

#[test]
fn getting_something_absent_reports_it_is_not_here() {
    let mut h = Harness::new();

    assert_eq!(h.line("get shield"), "You don't see that here.");
}

#[test]
fn dropping_something_not_carried_reports_it() {
    let mut h = Harness::new();
    // A sword on the floor is not in the caller's inventory.
    let _ = h.item_on_floor(place(HALL), &["sword"]);

    assert_eq!(h.line("drop sword"), "You aren't carrying that.");
}

#[test]
fn an_ambiguous_get_prompts_with_a_numbered_list() {
    let mut h = Harness::new();
    let _ = h.item_on_floor(place(HALL), &["sword"]);
    let _ = h.item_on_floor(place(HALL), &["sword"]);

    let prompt = h.line("get sword");

    assert_eq!(prompt, "Which do you mean? 1: sword, 2: sword");
}

#[test]
fn an_ordinal_disambiguates_a_get() {
    let mut h = Harness::new();
    let _first = h.item_on_floor(place(HALL), &["sword"]);
    let second = h.item_on_floor(place(HALL), &["sword"]);

    assert_eq!(h.line("get sword.2"), "You take sword.");
    assert!(
        h.world.contains(h.caller, second),
        "the second sword was taken"
    );
}

#[test]
fn get_all_takes_every_match() {
    let mut h = Harness::new();
    let a = h.item_on_floor(place(HALL), &["coin"]);
    let b = h.item_on_floor(place(HALL), &["coin"]);

    let reply = h.line("get all coin");

    assert!(
        reply.matches("You take coin.").count() == 2,
        "two lines: {reply}"
    );
    assert!(h.world.contains(h.caller, a) && h.world.contains(h.caller, b));
}
