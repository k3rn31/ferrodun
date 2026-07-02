//! Cross-player broadcast (§3.6.3): `say` and movement reach other co-located
//! players' sessions via the pipeline's `Roster` fan-out.
#![allow(clippy::expect_used)] // integration test

use std::collections::HashMap;
use std::num::NonZeroU64;

use mud_account::PuppetName;
use mud_core::{
    Description, Direction, EntityId, LockContext, Place, PlaceId, RegionId, RoomData, TenantTag,
    Title, World,
};
use mud_engine::{
    CallerContext, Dispatcher, LayerCommands, Pipeline, Places, Presence, ResolvedSession, Roster,
    SessionResolver,
};
use mud_i18n::Locale;
use mud_schema::{InputLine, SessionId, SessionInput, SessionOutput};

const HALL: u64 = 10;
const STUDY: u64 = 11;

fn place(v: u64) -> PlaceId {
    PlaceId::new(NonZeroU64::new(v).expect("place nz"))
}
fn sid(v: u64) -> SessionId {
    SessionId::new(NonZeroU64::new(v).expect("session nz"))
}
fn region() -> RegionId {
    RegionId::new(NonZeroU64::new(1).expect("region nz"))
}

/// Two rooms: HALL ↔ STUDY (north/south).
struct Rooms(HashMap<PlaceId, Place>);
impl Rooms {
    fn two() -> Self {
        let hall = Place::Room(
            RoomData::new(place(HALL), region(), Description::new("A hall."))
                .with_title(Title::new("The Hall"))
                .with_exit(Direction::North, place(STUDY)),
        );
        let study = Place::Room(
            RoomData::new(place(STUDY), region(), Description::new("A study."))
                .with_title(Title::new("The Study"))
                .with_exit(Direction::South, place(HALL)),
        );
        let mut m = HashMap::new();
        m.insert(place(HALL), hall);
        m.insert(place(STUDY), study);
        Self(m)
    }
}
impl Places for Rooms {
    fn get(&self, id: PlaceId) -> Option<&Place> {
        self.0.get(&id)
    }
}

/// A fixed set of in-world players: (session, entity, name), each with a location
/// read live from the world so movement is reflected on the next command.
struct Players {
    players: Vec<(SessionId, EntityId, PuppetName)>,
    builtins: Vec<mud_cmd::Command>,
}
impl SessionResolver for Players {
    fn resolve(&self, session: SessionId, world: &World) -> Option<ResolvedSession> {
        let (s, entity, name) = self.players.iter().find(|(s, ..)| *s == session).cloned()?;
        let location = world.location_of(entity)?;
        Some(ResolvedSession {
            caller: CallerContext::new(s, entity, location, name, Locale::EN, LockContext::new()),
            layers: LayerCommands {
                builtins: self.builtins.clone(),
                ..LayerCommands::default()
            },
        })
    }
}
impl Roster for Players {
    fn session_of(&self, entity: EntityId) -> Option<SessionId> {
        self.players
            .iter()
            .find(|(_, e, _)| *e == entity)
            .map(|(s, ..)| *s)
    }
    fn connected(&self) -> Vec<Presence> {
        self.players
            .iter()
            .map(|(_, _, name)| Presence { name: name.clone() })
            .collect()
    }
}

fn text_for(outputs: &[SessionOutput], session: SessionId) -> String {
    outputs
        .iter()
        .filter(|o| o.session_id == session)
        .map(|o| o.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn say_echoes_to_the_speaker_and_broadcasts_to_the_room() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    let borel = world.create().expect("borel");
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("say hello"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    let speaker = text_for(&outcome.outputs, sid(1));
    let listener = text_for(&outcome.outputs, sid(2));
    assert!(
        speaker.contains("You say") && speaker.contains("hello"),
        "speaker: {speaker}"
    );
    assert!(
        listener.contains("arden") && listener.contains("hello"),
        "listener: {listener}"
    );
    assert!(
        !listener.contains("You say"),
        "listener must not see the echo: {listener}"
    );
}

#[test]
fn moving_announces_departure_and_arrival_to_the_two_rooms() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden"); // mover
    let borel = world.create().expect("borel"); // stays in HALL
    let cade = world.create().expect("cade"); // waits in STUDY
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");
    world.move_to(cade, place(STUDY)).expect("seat cade");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
            (sid(3), cade, PuppetName::parse("cade").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("north"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    // Borel (left behind in HALL) sees the departure; Cade (in STUDY) sees the
    // arrival from the south (the opposite of travelling north).
    let left_behind = text_for(&outcome.outputs, sid(2));
    let destination = text_for(&outcome.outputs, sid(3));
    assert!(
        left_behind.contains("arden") && left_behind.contains("leaves"),
        "depart: {left_behind}"
    );
    assert!(
        destination.contains("arden")
            && destination.contains("arrives")
            && destination.contains("south"),
        "arrive: {destination}"
    );
}

#[test]
fn who_lists_the_connected_players() {
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    let borel = world.create().expect("borel");
    world.move_to(arden, place(HALL)).expect("seat arden");
    world.move_to(borel, place(HALL)).expect("seat borel");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![
            (sid(1), arden, PuppetName::parse("arden").expect("name")),
            (sid(2), borel, PuppetName::parse("borel").expect("name")),
        ],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("who"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    let listed = text_for(&outcome.outputs, sid(1));
    assert!(
        listed.contains("arden") && listed.contains("borel"),
        "who: {listed}"
    );
}

#[test]
fn quit_signals_a_close_with_a_goodbye() {
    use mud_engine::SessionDisposition;
    let mut world = World::new(TenantTag::new(1).expect("tenant"));
    let arden = world.create().expect("arden");
    world.move_to(arden, place(HALL)).expect("seat arden");

    let mut dispatcher = Dispatcher::new();
    let builtins = mud_engine::register(&mut dispatcher);
    let resolver = Players {
        players: vec![(sid(1), arden, PuppetName::parse("arden").expect("name"))],
        builtins,
    };
    let mut pipeline = Pipeline::new(dispatcher);

    let input = SessionInput {
        session_id: sid(1),
        line: InputLine::new("quit"),
    };
    let outcome = pipeline
        .dispatch(&mut world, &Rooms::two(), &resolver, &input)
        .expect("dispatch");

    assert_eq!(outcome.disposition, SessionDisposition::Close);
    assert!(
        text_for(&outcome.outputs, sid(1)).contains("Goodbye"),
        "goodbye shown"
    );
}
